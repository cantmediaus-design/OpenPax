use std::ops::Range;

use wgpu::util::DeviceExt;

use crate::Camera;

/// Format of the texture the scene is rendered into.
///
/// Everything works on sRGB-encoded values with no conversion, like the game's own block
/// shading and like egui, which samples the result as-is.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;
const MESH_FLOATS: usize = 12;
const LINE_FLOATS: usize = 7;

const SHADER: &str = r"
struct Globals { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var color_texture: texture_2d<f32>;
@group(1) @binding(1) var color_sampler: sampler;

struct MeshIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tint: vec4<f32>,
};
struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs_mesh(in: MeshIn) -> MeshOut {
    var out: MeshOut;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    // Fixed per-side shading in the style of the game: top bright, bottom dark.
    let weight = in.normal * in.normal;
    let vertical = select(0.5, 1.0, in.normal.y > 0.0);
    let shade = weight.x * 0.6 + weight.y * vertical + weight.z * 0.8;
    out.uv = in.uv;
    out.color = in.tint.rgb * shade;
    return out;
}

@fragment
fn fs_mesh(in: MeshOut) -> @location(0) vec4<f32> {
    let texel = textureSample(color_texture, color_sampler, in.uv);
    if (texel.a < 0.1) {
        discard;
    }
    return vec4<f32>(texel.rgb * in.color, 1.0);
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> LineOut {
    var out: LineOut;
    out.clip = globals.view_proj * vec4<f32>(position, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}
";

/// RGBA8 pixels (sRGB, straight alpha) to upload as a texture.
pub struct TextureImage<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Colour the texture is multiplied by.
    pub tint: [f32; 4],
}

/// A textured quad; vertices in counter-clockwise order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    /// Index into the images passed to [`Renderer::set_textures`].
    pub texture: usize,
    pub vertices: [MeshVertex; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineVertex {
    pub position: [f32; 3],
    /// sRGB-encoded RGBA.
    pub color: [f32; 4],
}

struct Target {
    size: [u32; 2],
    resolved: wgpu::TextureView,
    multisampled: wgpu::TextureView,
    depth: wgpu::TextureView,
}

struct Batch {
    texture: usize,
    indices: Range<u32>,
}

pub struct Renderer {
    mesh_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: Vec<wgpu::BindGroup>,
    mesh: Option<(wgpu::Buffer, wgpu::Buffer, Vec<Batch>)>,
    lines: Option<(wgpu::Buffer, u32)>,
    target: Option<Target>,
}

fn floats_to_bytes(floats: &[f32]) -> Vec<u8> {
    floats.iter().flat_map(|f| f.to_le_bytes()).collect()
}

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("openpax viewport shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("openpax globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("openpax texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("openpax globals"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("openpax globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        // Pixel art: nearest filtering, and repeat so UVs outside 0..1 tile.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("openpax sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });

        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("openpax mesh layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("openpax line layout"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });

        let mesh_attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4
        ];
        let line_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let pipeline = |label: &str,
                        layout: &wgpu::PipelineLayout,
                        entry_points: (&str, &str),
                        stride: usize,
                        attributes: &[wgpu::VertexAttribute],
                        topology: wgpu::PrimitiveTopology,
                        blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry_points.0),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: (stride * 4) as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_points.1),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_pipeline = pipeline(
            "openpax mesh pipeline",
            &mesh_layout,
            ("vs_mesh", "fs_mesh"),
            MESH_FLOATS,
            &mesh_attributes,
            wgpu::PrimitiveTopology::TriangleList,
            wgpu::BlendState::REPLACE,
        );
        let line_pipeline = pipeline(
            "openpax line pipeline",
            &line_layout,
            ("vs_line", "fs_line"),
            LINE_FLOATS,
            &line_attributes,
            wgpu::PrimitiveTopology::LineList,
            wgpu::BlendState::ALPHA_BLENDING,
        );

        Self {
            mesh_pipeline,
            line_pipeline,
            globals,
            globals_bind_group,
            texture_layout,
            sampler,
            textures: Vec::new(),
            mesh: None,
            lines: None,
            target: None,
        }
    }

    /// Makes the render target match `size` (in pixels). Returns `true` if it was recreated,
    /// in which case [`Self::view`] is a new texture view.
    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) -> bool {
        let size = [size[0].max(1), size[1].max(1)];
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return false;
        }
        let texture = |label, format, sample_count, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        self.target = Some(Target {
            size,
            resolved: texture(
                "openpax viewport",
                COLOR_FORMAT,
                1,
                attachment | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            multisampled: texture("openpax viewport msaa", COLOR_FORMAT, SAMPLES, attachment),
            depth: texture("openpax viewport depth", DEPTH_FORMAT, SAMPLES, attachment),
        });
        true
    }

    /// The rendered image. Only valid after [`Self::resize`].
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|t| &t.resolved)
    }

    /// Replaces all textures; [`Quad::texture`] indexes into `images`.
    pub fn set_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        images: &[TextureImage<'_>],
    ) {
        self.textures = images
            .iter()
            .map(|image| {
                let size = wgpu::Extent3d {
                    width: image.width.max(1),
                    height: image.height.max(1),
                    depth_or_array_layers: 1,
                };
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("openpax model texture"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: COLOR_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    image.rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * size.width),
                        rows_per_image: Some(size.height),
                    },
                    size,
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("openpax model texture"),
                    layout: &self.texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                })
            })
            .collect();
    }

    /// Replaces the textured geometry.
    pub fn set_quads(&mut self, device: &wgpu::Device, quads: &[Quad]) {
        if quads.is_empty() {
            self.mesh = None;
            return;
        }
        let mut order: Vec<usize> = (0..quads.len()).collect();
        order.sort_by_key(|i| quads[*i].texture);

        let mut vertices = Vec::with_capacity(quads.len() * 4 * MESH_FLOATS);
        let mut indices = Vec::with_capacity(quads.len() * 6);
        let mut batches: Vec<Batch> = Vec::new();
        for (n, quad) in order.into_iter().map(|i| &quads[i]).enumerate() {
            for v in &quad.vertices {
                vertices.extend_from_slice(&v.position);
                vertices.extend_from_slice(&v.normal);
                vertices.extend_from_slice(&v.uv);
                vertices.extend_from_slice(&v.tint);
            }
            let base = n as u32 * 4;
            let start = indices.len() as u32;
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            match batches.last_mut() {
                Some(batch) if batch.texture == quad.texture => batch.indices.end = start + 6,
                _ => batches.push(Batch {
                    texture: quad.texture,
                    indices: start..start + 6,
                }),
            }
        }
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("openpax mesh vertices"),
            contents: &floats_to_bytes(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_bytes: Vec<u8> = indices.iter().flat_map(|i: &u32| i.to_le_bytes()).collect();
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("openpax mesh indices"),
            contents: &index_bytes,
            usage: wgpu::BufferUsages::INDEX,
        });
        self.mesh = Some((vertex_buffer, index_buffer, batches));
    }

    /// Replaces the line overlay; vertices are consumed in pairs.
    pub fn set_lines(&mut self, device: &wgpu::Device, lines: &[LineVertex]) {
        if lines.len() < 2 {
            self.lines = None;
            return;
        }
        let mut floats = Vec::with_capacity(lines.len() * LINE_FLOATS);
        for v in lines {
            floats.extend_from_slice(&v.position);
            floats.extend_from_slice(&v.color);
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("openpax line vertices"),
            contents: &floats_to_bytes(&floats),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.lines = Some((buffer, lines.len() as u32 / 2 * 2));
    }

    /// Draws the current geometry into the render target. `clear` is an sRGB-encoded colour.
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        clear: [f32; 3],
    ) {
        let Some(target) = &self.target else { return };
        let aspect = target.size[0] as f32 / target.size[1] as f32;
        let view_proj = camera.view_proj(aspect).to_cols_array();
        queue.write_buffer(&self.globals, 0, &floats_to_bytes(&view_proj));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("openpax viewport"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("openpax viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.multisampled,
                    depth_slice: None,
                    resolve_target: Some(&target.resolved),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(clear[0]),
                            g: f64::from(clear[1]),
                            b: f64::from(clear[2]),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bind_group, &[]);

            if let Some((vertices, indices, batches)) = &self.mesh {
                pass.set_pipeline(&self.mesh_pipeline);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                for batch in batches {
                    if let Some(texture) = self.textures.get(batch.texture) {
                        pass.set_bind_group(1, texture, &[]);
                        pass.draw_indexed(batch.indices.clone(), 0, 0..1);
                    }
                }
            }
            if let Some((vertices, count)) = &self.lines {
                pass.set_pipeline(&self.line_pipeline);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.draw(0..*count, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
    }
}

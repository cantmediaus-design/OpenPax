//! The 3D model editor tab: an orbit viewport rendered with wgpu.

use eframe::egui::{self, Color32, PointerButton, Rect, Sense, pos2};
use eframe::egui_wgpu::{RenderState, wgpu};
use openpax_core::{Pack, ResourceLocation};
use openpax_model::{Direction, ModelDocument, ModelFile, build_mesh, element_corners, pick};
use openpax_paint::Bitmap;
use openpax_render::{Camera, LineVertex, MeshVertex, Quad, Renderer, TextureImage};

/// Viewport colours (sRGB-encoded).
const CLEAR: [f32; 3] = [0.105, 0.105, 0.125];
const GRID: [f32; 4] = [1.0, 1.0, 1.0, 0.10];
const BLOCK_OUTLINE: [f32; 4] = [1.0, 1.0, 1.0, 0.30];
const SELECTION: [f32; 4] = [1.0, 0.55, 0.05, 1.0];
const BLOCK: f32 = 16.0;

/// GPU resources for one viewport. Frees its egui texture when the tab goes away.
struct Gpu {
    render_state: RenderState,
    renderer: Renderer,
    texture_id: Option<egui::TextureId>,
}

impl Drop for Gpu {
    fn drop(&mut self) {
        if let Some(id) = self.texture_id {
            self.render_state.renderer.write().free_texture(&id);
        }
    }
}

/// What the GPU buffers were last built from.
#[derive(Clone, Copy, PartialEq)]
struct Uploaded {
    revision: u64,
    selected: Option<usize>,
    assets: u64,
    grid: bool,
}

pub struct ModelEditor {
    pub rel_path: String,
    pub doc: ModelDocument,
    saved_revision: u64,
    camera: Camera,
    gpu: Option<Gpu>,
    uploaded: Option<Uploaded>,
    /// Face textures that could not be loaded from this pack (shown as a checkerboard).
    pub missing_textures: Vec<String>,
    pub show_grid: bool,
}

impl ModelEditor {
    pub fn open(pack: &Pack, rel_path: &str) -> Result<Self, String> {
        let bytes = pack.read(rel_path).map_err(|e| e.to_string())?;
        let text = String::from_utf8(bytes).map_err(|_| "file is not valid UTF-8".to_owned())?;
        let model = ModelFile::from_json(&text).map_err(|e| e.to_string())?;
        Ok(Self {
            rel_path: rel_path.to_owned(),
            doc: ModelDocument::new(model),
            saved_revision: 0,
            camera: Camera::default(),
            gpu: None,
            uploaded: None,
            missing_textures: Vec::new(),
            show_grid: true,
        })
    }

    pub fn dirty(&self) -> bool {
        self.doc.revision() != self.saved_revision
    }

    pub fn save(&mut self, pack: &Pack) -> Result<(), String> {
        pack.write(&self.rel_path, self.doc.model().to_json().as_bytes())
            .map_err(|e| e.to_string())?;
        self.saved_revision = self.doc.revision();
        Ok(())
    }

    pub fn reset_view(&mut self) {
        self.camera = Camera::default();
    }

    /// Draws the viewport. `assets` changes whenever any file in the pack is saved, so
    /// textures edited in another tab show up here.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        pack: &Pack,
        render_state: Option<&RenderState>,
        assets: u64,
    ) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let Some(render_state) = render_state else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "The 3D viewport needs the wgpu renderer.",
                egui::FontId::default(),
                ui.visuals().weak_text_color(),
            );
            return;
        };
        self.handle_input(ui, &response, rect);

        let scale = ui.ctx().pixels_per_point();
        let size = [
            (rect.width() * scale).round() as u32,
            (rect.height() * scale).round() as u32,
        ];
        let gpu = self.gpu.get_or_insert_with(|| Gpu {
            render_state: render_state.clone(),
            renderer: Renderer::new(&render_state.device),
            texture_id: None,
        });
        let resized = gpu.renderer.resize(&render_state.device, size);

        let wanted = Uploaded {
            revision: self.doc.revision(),
            selected: self.doc.selected(),
            assets,
            grid: self.show_grid,
        };
        if self.uploaded != Some(wanted) {
            let scene = Scene::build(self.doc.model(), self.doc.selected(), self.show_grid, pack);
            let images: Vec<TextureImage<'_>> = scene
                .textures
                .iter()
                .map(|b| TextureImage {
                    width: b.width(),
                    height: b.height(),
                    rgba: b.data(),
                })
                .collect();
            let (device, queue) = (&render_state.device, &render_state.queue);
            gpu.renderer.set_textures(device, queue, &images);
            gpu.renderer.set_quads(device, &scene.quads);
            gpu.renderer.set_lines(device, &scene.lines);
            self.missing_textures = scene.missing;
            self.uploaded = Some(wanted);
        }
        gpu.renderer.render(
            &render_state.device,
            &render_state.queue,
            &self.camera,
            CLEAR,
        );

        // Hand the rendered texture to egui (again whenever the target was recreated).
        if (resized || gpu.texture_id.is_none())
            && let Some(view) = gpu.renderer.view()
        {
            let mut egui_renderer = render_state.renderer.write();
            let filter = wgpu::FilterMode::Linear;
            match gpu.texture_id {
                Some(id) => egui_renderer.update_egui_texture_from_wgpu_texture(
                    &render_state.device,
                    view,
                    filter,
                    id,
                ),
                None => {
                    gpu.texture_id = Some(egui_renderer.register_native_texture(
                        &render_state.device,
                        view,
                        filter,
                    ));
                }
            }
        }
        let painter = ui.painter_at(rect);
        if let Some(id) = gpu.texture_id {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(id, rect, uv, Color32::WHITE);
        }
        self.overlay(ui, &painter, rect);
    }

    fn handle_input(&mut self, ui: &egui::Ui, response: &egui::Response, rect: Rect) {
        let delta = response.drag_delta();
        if response.dragged_by(PointerButton::Primary) {
            self.camera.orbit(-delta.x * 0.01, delta.y * 0.01);
        }
        if response.dragged_by(PointerButton::Secondary)
            || response.dragged_by(PointerButton::Middle)
        {
            self.camera
                .pan(-delta.x / rect.height(), delta.y / rect.height());
        }
        if response.contains_pointer() {
            let (scroll, pinch) = ui
                .ctx()
                .input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 || pinch != 1.0 {
                self.camera.zoom((-scroll * 0.004).exp() / pinch);
            }
        }
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let ndc = [
                (pos.x - rect.left()) / rect.width() * 2.0 - 1.0,
                1.0 - (pos.y - rect.top()) / rect.height() * 2.0,
            ];
            let (origin, direction) = self.camera.ray(ndc, rect.width() / rect.height());
            let hit = pick(self.doc.model(), origin, direction);
            self.doc.select(hit.map(|h| h.element));
        }
    }

    fn overlay(&self, ui: &egui::Ui, painter: &egui::Painter, rect: Rect) {
        let mut lines = Vec::new();
        let count = self.doc.model().elements().len();
        if count == 0 {
            match &self.doc.model().parent {
                Some(parent) => lines.push(format!(
                    "No elements of its own: the shape comes from the parent \"{parent}\"."
                )),
                None => lines.push("No elements yet. Use Add cube in the inspector.".to_owned()),
            }
        }
        if !self.missing_textures.is_empty() {
            lines.push(format!(
                "Not found in this pack: {}",
                self.missing_textures.join(", ")
            ));
        }
        lines.push("Drag: orbit   Right-drag: pan   Scroll: zoom   Click: select".to_owned());

        let font = egui::FontId::proportional(12.0);
        let mut y = rect.top() + 8.0;
        for line in lines {
            let pos = pos2(rect.left() + 10.0, y);
            let color = ui.visuals().weak_text_color();
            let drawn = painter.text(pos, egui::Align2::LEFT_TOP, line, font.clone(), color);
            y = drawn.bottom() + 2.0;
        }
    }
}

/// CPU-side geometry and textures for one frame of the viewport.
struct Scene {
    textures: Vec<Bitmap>,
    quads: Vec<Quad>,
    lines: Vec<LineVertex>,
    missing: Vec<String>,
}

impl Scene {
    fn build(model: &ModelFile, selected: Option<usize>, grid: bool, pack: &Pack) -> Self {
        // Texture 0 stands in for anything that cannot be loaded.
        let mut textures = vec![missing_texture()];
        let mut names: Vec<String> = vec![String::new()];
        let mut missing: Vec<String> = Vec::new();
        let mut quads = Vec::new();

        for face in build_mesh(model) {
            let index = match &face.texture {
                Some(location) => match names.iter().position(|n| n == location) {
                    Some(index) => index,
                    None => match load_texture(pack, location) {
                        Some(bitmap) => {
                            textures.push(bitmap);
                            names.push(location.clone());
                            textures.len() - 1
                        }
                        None => {
                            note_missing(&mut missing, location.clone());
                            0
                        }
                    },
                },
                None => {
                    let raw = &model.elements()[face.element].faces[&face.direction].texture;
                    note_missing(&mut missing, format!("{raw} (variable not set)"));
                    0
                }
            };
            quads.push(Quad {
                texture: index,
                vertices: face.vertices.map(|v| MeshVertex {
                    position: v.position,
                    normal: v.normal,
                    uv: v.uv,
                    tint: [1.0; 4],
                }),
            });
        }

        let mut lines = Vec::new();
        if grid {
            floor_grid(&mut lines);
        }
        if let Some(element) = selected.and_then(|i| model.elements().get(i)) {
            // Push the outline out a touch so it does not z-fight with the faces.
            let centre = centre_of(element);
            for side in Direction::ALL {
                let corners = element_corners(element, side).map(|p| {
                    std::array::from_fn(|axis| centre[axis] + (p[axis] - centre[axis]) * 1.01)
                });
                for i in 0..4 {
                    line(&mut lines, corners[i], corners[(i + 1) % 4], SELECTION);
                }
            }
        }
        Self {
            textures,
            quads,
            lines,
            missing,
        }
    }
}

fn note_missing(missing: &mut Vec<String>, name: String) {
    if !missing.contains(&name) {
        missing.push(name);
    }
}

fn centre_of(element: &openpax_model::Element) -> [f32; 3] {
    // Average of two opposite corners after rotation.
    let a = element_corners(element, Direction::Down)[0];
    let b = element_corners(element, Direction::Up)[2];
    std::array::from_fn(|i| (a[i] + b[i]) / 2.0)
}

fn line(lines: &mut Vec<LineVertex>, a: [f32; 3], b: [f32; 3], color: [f32; 4]) {
    lines.push(LineVertex { position: a, color });
    lines.push(LineVertex { position: b, color });
}

/// A grid on the floor of the block, the block's bounding box, and the three axes.
fn floor_grid(lines: &mut Vec<LineVertex>) {
    for i in 0..=16 {
        let t = i as f32;
        line(lines, [t, 0.0, 0.0], [t, 0.0, BLOCK], GRID);
        line(lines, [0.0, 0.0, t], [BLOCK, 0.0, t], GRID);
    }
    for (y0, y1) in [(0.0, 0.0), (BLOCK, BLOCK)] {
        line(lines, [0.0, y0, 0.0], [BLOCK, y1, 0.0], BLOCK_OUTLINE);
        line(lines, [BLOCK, y0, 0.0], [BLOCK, y1, BLOCK], BLOCK_OUTLINE);
        line(lines, [BLOCK, y0, BLOCK], [0.0, y1, BLOCK], BLOCK_OUTLINE);
        line(lines, [0.0, y0, BLOCK], [0.0, y1, 0.0], BLOCK_OUTLINE);
    }
    for (x, z) in [(0.0, 0.0), (BLOCK, 0.0), (BLOCK, BLOCK), (0.0, BLOCK)] {
        line(lines, [x, 0.0, z], [x, BLOCK, z], BLOCK_OUTLINE);
    }
    // Axes from the block's origin corner: x red, y green, z blue.
    let o = [-1.0, 0.0, -1.0];
    line(lines, o, [5.0, 0.0, -1.0], [1.0, 0.1, 0.1, 1.0]);
    line(lines, o, [-1.0, 6.0, -1.0], [0.1, 0.8, 0.1, 1.0]);
    line(lines, o, [-1.0, 0.0, 5.0], [0.15, 0.3, 1.0, 1.0]);
}

/// The classic magenta/black checkerboard.
fn missing_texture() -> Bitmap {
    let mut bitmap = Bitmap::filled(2, 2, [0, 0, 0, 255]);
    bitmap.set(0, 0, [248, 0, 248, 255]);
    bitmap.set(1, 1, [248, 0, 248, 255]);
    bitmap
}

/// Loads `namespace:path` from the pack's textures; animated textures give their first frame.
fn load_texture(pack: &Pack, location: &str) -> Option<Bitmap> {
    let location: ResourceLocation = location.parse().ok()?;
    let rel_path = format!(
        "assets/{}/textures/{}.png",
        location.namespace(),
        location.path()
    );
    let bitmap = openpax_paint::decode_png(&pack.read(&rel_path).ok()?).ok()?;
    match pack
        .texture_meta(&rel_path)
        .ok()
        .flatten()
        .and_then(|m| m.animation)
    {
        Some(animation) => {
            let (width, height) = animation.frame_size(bitmap.width(), bitmap.height());
            Some(bitmap.resized(width, height))
        }
        None => Some(bitmap),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpax_model::Element;

    fn pack() -> (tempfile::TempDir, Pack) {
        let dir = tempfile::tempdir().unwrap();
        let pack = Pack::create(dir.path().join("pack"), "test", 46).unwrap();
        let strip = openpax_paint::encode_png(&Bitmap::filled(4, 12, [9, 9, 9, 255])).unwrap();
        pack.write("assets/minecraft/textures/block/lava.png", &strip)
            .unwrap();
        pack.write(
            "assets/minecraft/textures/block/lava.png.mcmeta",
            br#"{"animation":{}}"#,
        )
        .unwrap();
        (dir, pack)
    }

    #[test]
    fn scene_loads_textures_and_reports_missing_ones() {
        let (_dir, pack) = pack();
        let mut model = ModelFile::default();
        model
            .textures
            .insert("a".into(), "minecraft:block/lava".into());
        model.textures.insert("b".into(), "block/nope".into());
        let mut cube = Element::cube([0.0; 3], [16.0; 3], "#a");
        cube.faces.get_mut(&Direction::Up).unwrap().texture = "#b".into();
        cube.faces.get_mut(&Direction::Down).unwrap().texture = "#c".into();
        model.elements = Some(vec![cube]);

        let scene = Scene::build(&model, Some(0), true, &pack);
        assert_eq!(scene.quads.len(), 6);
        assert_eq!(
            scene.textures.len(),
            2,
            "placeholder plus lava, loaded once"
        );
        let lava = &scene.textures[1];
        assert_eq!(
            (lava.width(), lava.height()),
            (4, 4),
            "first animation frame only"
        );
        assert_eq!(scene.missing, ["#c (variable not set)", "block/nope"]);
        assert_eq!(scene.quads.iter().filter(|q| q.texture == 0).count(), 2);
        assert!(scene.lines.len() > 24 && scene.lines.len().is_multiple_of(2));

        let bare = Scene::build(&model, None, false, &pack);
        assert!(bare.lines.is_empty());
    }

    #[test]
    fn open_edit_save_reopen() {
        let (_dir, pack) = pack();
        let rel = "assets/minecraft/models/block/thing.json";
        pack.write(
            rel,
            br#"{"parent":"block/block","display":{"gui":{"scale":[1,1,1]}}}"#,
        )
        .unwrap();

        let mut editor = ModelEditor::open(&pack, rel).unwrap();
        assert!(!editor.dirty());
        editor
            .doc
            .add_element(Element::cube([0.0; 3], [8.0; 3], "#0"));
        assert!(editor.dirty());
        editor.save(&pack).unwrap();
        assert!(!editor.dirty());

        let reopened = ModelEditor::open(&pack, rel).unwrap();
        let model = reopened.doc.model();
        assert_eq!(model.elements().len(), 1);
        assert_eq!(model.parent.as_deref(), Some("block/block"));
        assert!(
            model.extra.contains_key("display"),
            "unknown keys survive a save"
        );
    }

    #[test]
    fn invalid_json_is_rejected() {
        let (_dir, pack) = pack();
        pack.write("assets/minecraft/models/bad.json", b"{ not json")
            .unwrap();
        assert!(ModelEditor::open(&pack, "assets/minecraft/models/bad.json").is_err());
    }
}

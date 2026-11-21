//! The 3D model editor tab: an orbit viewport rendered with wgpu.

use eframe::egui::{
    self, Align, Color32, Layout, PointerButton, Rect, RichText, Sense, UiBuilder, pos2, vec2,
};
use eframe::egui_wgpu::{RenderState, wgpu};
use openpax_core::{Pack, ResourceLocation};
use openpax_model::{Direction, ModelDocument, ModelFile, build_mesh, element_corners, pick};
use openpax_paint::Bitmap;
use openpax_render::{Camera, LineVertex, MeshVertex, Quad, Renderer, TextureImage};

use crate::commands::Command;
use crate::theme::{self, SPACE_M, SPACE_S, Tokens};
use crate::toasts::Level;
use crate::{icons, widgets};

const BLOCK: f32 = 16.0;
/// Camera presets: label, tooltip, yaw and pitch in degrees.
const VIEWS: [(&str, &str, f32, f32); 6] = [
    ("Front", "Look at the south face", 0.0, 0.0),
    ("Back", "Look at the north face", 180.0, 0.0),
    ("Left", "Look at the west face", -90.0, 0.0),
    ("Right", "Look at the east face", 90.0, 0.0),
    ("Top", "Look straight down", 0.0, 88.0),
    ("3D", "Three-quarter view", 35.0, 25.0),
];
const HINT: &str = "Drag to orbit  ·  Right-drag to pan  ·  Scroll to zoom  ·  Click to select";

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
    /// The viewport's line colours depend on the theme.
    dark: bool,
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
    /// The missing textures the user has already been told about.
    missing_reported: Vec<String>,
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
            missing_reported: Vec::new(),
            show_grid: true,
        })
    }

    pub fn dirty(&self) -> bool {
        self.doc.revision() != self.saved_revision
    }

    /// Treats the current contents as what is on disk.
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.doc.revision();
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

    /// Turns the camera to the given angles (degrees), keeping its target and distance.
    fn set_view(&mut self, yaw: f32, pitch: f32) {
        self.camera.yaw = yaw.to_radians();
        self.camera.pitch = pitch.to_radians();
    }

    /// A warning about textures this pack lacks, returned once each time the list changes.
    pub fn take_missing_notice(&mut self) -> Option<String> {
        if self.missing_textures == self.missing_reported {
            return None;
        }
        self.missing_reported.clone_from(&self.missing_textures);
        if self.missing_textures.is_empty() {
            return None;
        }
        let name = self.rel_path.rsplit('/').next().unwrap_or_default();
        Some(format!(
            "{name} uses textures that are not in this pack:\n{}",
            self.missing_textures.join("\n")
        ))
    }

    /// Draws the viewport. `assets` changes whenever any file in the pack is saved, so
    /// textures edited in another tab show up here.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        pack: &Pack,
        render_state: Option<&RenderState>,
        assets: u64,
        show_hint: &mut bool,
    ) {
        let tokens = theme::of(ui);
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
            dark: ui.visuals().dark_mode,
        };
        if self.uploaded != Some(wanted) {
            let scene = Scene::build(
                self.doc.model(),
                self.doc.selected(),
                self.show_grid,
                &tokens,
                pack,
            );
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
            tokens.viewport_clear,
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
        self.overlay(ui, rect, show_hint);
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

    /// Controls and notices drawn over the viewport.
    fn overlay(&mut self, ui: &mut egui::Ui, rect: Rect, show_hint: &mut bool) {
        let ctx = ui.ctx().clone();
        let inner = rect.shrink(SPACE_M);

        // View controls, top-left.
        let mut bar = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::left_to_right(Align::Min)),
        );
        theme::floating_frame(&bar).show(&mut bar, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (label, tooltip, yaw, pitch) in VIEWS {
                    let button = egui::Button::new(label)
                        .frame_when_inactive(false)
                        .min_size(vec2(0.0, theme::ICON_BUTTON));
                    if ui.add(button).on_hover_text(tooltip).clicked() {
                        self.set_view(yaw, pitch);
                    }
                }
                ui.separator();
                let grid = Command::ToggleGrid.tooltip(&ctx);
                if widgets::icon_toggle(ui, self.show_grid, icons::GRID, &grid).clicked() {
                    self.show_grid = !self.show_grid;
                }
                let reset = Command::ResetView.tooltip(&ctx);
                if widgets::icon_button(ui, icons::HOME, &reset).clicked() {
                    self.reset_view();
                }
                if widgets::icon_toggle(ui, *show_hint, icons::HELP, "Show navigation help")
                    .clicked()
                {
                    *show_hint = !*show_hint;
                }
            });
        });

        // Notices, stacked up from the bottom-left corner. The navigation hint can be closed.
        let mut notices: Vec<(Level, String, bool)> = Vec::new();
        if *show_hint {
            notices.push((Level::Info, HINT.to_owned(), true));
        }
        if !self.missing_textures.is_empty() {
            let missing = self.missing_textures.join(", ");
            notices.push((
                Level::Warning,
                format!("Not found in this pack: {missing}"),
                false,
            ));
        }
        if self.doc.model().elements().is_empty() {
            let text = match &self.doc.model().parent {
                Some(parent) => {
                    format!("No elements of its own: the shape comes from the parent \"{parent}\".")
                }
                None => "No elements yet. Use Add Cube in the inspector.".to_owned(),
            };
            notices.push((Level::Info, text, false));
        }
        let mut column = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::bottom_up(Align::Min)),
        );
        column.spacing_mut().item_spacing.y = SPACE_S;
        for (level, text, closable) in notices {
            let color = level.color(&column);
            theme::floating_frame(&column).show(&mut column, |ui| {
                ui.horizontal(|ui| {
                    ui.label(widgets::icon(level.icon()).size(14.0).color(color));
                    ui.add(egui::Label::new(RichText::new(text).small()).selectable(false));
                    let close = egui::Button::new(RichText::new(icons::CLOSE).size(11.0))
                        .frame_when_inactive(false);
                    if closable
                        && ui
                            .add(close)
                            .on_hover_text("Hide (the ? button brings it back)")
                            .clicked()
                    {
                        *show_hint = false;
                    }
                });
            });
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
    fn build(
        model: &ModelFile,
        selected: Option<usize>,
        grid: bool,
        tokens: &Tokens,
        pack: &Pack,
    ) -> Self {
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
            floor_grid(&mut lines, tokens);
        }
        if let Some(element) = selected.and_then(|i| model.elements().get(i)) {
            // Push the outline out a touch so it does not z-fight with the faces.
            let centre = centre_of(element);
            for side in Direction::ALL {
                let corners = element_corners(element, side).map(|p| {
                    std::array::from_fn(|axis| centre[axis] + (p[axis] - centre[axis]) * 1.01)
                });
                for i in 0..4 {
                    let colour = tokens.viewport_selection;
                    line(&mut lines, corners[i], corners[(i + 1) % 4], colour);
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
fn floor_grid(lines: &mut Vec<LineVertex>, tokens: &Tokens) {
    let (grid, outline) = (tokens.viewport_grid, tokens.viewport_outline);
    for i in 0..=16 {
        let t = i as f32;
        line(lines, [t, 0.0, 0.0], [t, 0.0, BLOCK], grid);
        line(lines, [0.0, 0.0, t], [BLOCK, 0.0, t], grid);
    }
    for (y0, y1) in [(0.0, 0.0), (BLOCK, BLOCK)] {
        line(lines, [0.0, y0, 0.0], [BLOCK, y1, 0.0], outline);
        line(lines, [BLOCK, y0, 0.0], [BLOCK, y1, BLOCK], outline);
        line(lines, [BLOCK, y0, BLOCK], [0.0, y1, BLOCK], outline);
        line(lines, [0.0, y0, BLOCK], [0.0, y1, 0.0], outline);
    }
    for (x, z) in [(0.0, 0.0), (BLOCK, 0.0), (BLOCK, BLOCK), (0.0, BLOCK)] {
        line(lines, [x, 0.0, z], [x, BLOCK, z], outline);
    }
    // Axes from the block's origin corner: x red, y green, z blue.
    let o = [-1.0, 0.0, -1.0];
    let [x, y, z] = tokens
        .axis
        .map(|c| c.to_array().map(|channel| f32::from(channel) / 255.0));
    line(lines, o, [5.0, 0.0, -1.0], x);
    line(lines, o, [-1.0, 6.0, -1.0], y);
    line(lines, o, [-1.0, 0.0, 5.0], z);
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

        let tokens = theme::tokens(true);
        let scene = Scene::build(&model, Some(0), true, &tokens, &pack);
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

        let bare = Scene::build(&model, None, false, &tokens, &pack);
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

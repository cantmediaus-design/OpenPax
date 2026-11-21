//! Editor tabs: the pixel canvas for textures and a plain text editor for everything else.

use eframe::egui::{
    self, Color32, CursorIcon, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Vec2, pos2, vec2,
};
use openpax_core::{Pack, TextureMeta};
use openpax_paint::{Document, Palette, ToolKind, ToolSettings};

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 128.0;
const LAYERS_JSON: &str = ".layers.json";
const LAYERS_PNG: &str = ".layers.png";

/// Tool selection shared by all texture editors.
pub struct ToolState {
    pub tool: ToolKind,
    pub settings: ToolSettings,
    pub palette: Palette,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            tool: ToolKind::Pencil,
            settings: ToolSettings::default(),
            palette: Palette::default(),
        }
    }
}

pub enum Editor {
    Texture(Box<TextureEditor>),
    Text(TextEditor),
}

impl Editor {
    pub fn rel_path(&self) -> &str {
        match self {
            Self::Texture(e) => &e.rel_path,
            Self::Text(e) => &e.rel_path,
        }
    }

    pub fn dirty(&self) -> bool {
        match self {
            Self::Texture(e) => e.dirty(),
            Self::Text(e) => e.dirty(),
        }
    }

    pub fn title(&self) -> String {
        let name = self.rel_path().rsplit('/').next().unwrap_or_default();
        if self.dirty() {
            format!("{name} *")
        } else {
            name.to_owned()
        }
    }

    pub fn save(&mut self, pack: &Pack) -> Result<(), String> {
        match self {
            Self::Texture(e) => e.save(pack),
            Self::Text(e) => e.save(pack),
        }
    }
}

pub struct TextureEditor {
    pub rel_path: String,
    pub doc: Document,
    saved_revision: u64,
    pub meta: TextureMeta,
    saved_meta: TextureMeta,
    texture: Option<TextureHandle>,
    texture_revision: u64,
    pub zoom: f32,
    pan: Vec2,
    pub fit_pending: bool,
    pub show_grid: bool,
    pub tile_preview: bool,
    /// Pixel under the cursor, for the status bar.
    pub hover: Option<(i32, i32)>,
}

impl TextureEditor {
    pub fn open(pack: &Pack, rel_path: &str) -> Result<Self, String> {
        let bytes = pack.read(rel_path).map_err(|e| e.to_string())?;
        let bitmap = openpax_paint::decode_png(&bytes).map_err(|e| e.to_string())?;
        let meta = pack
            .texture_meta(rel_path)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();

        // Layers live in a sidecar; use them only if they still match the PNG on disk.
        let layered = pack
            .read(&Pack::sidecar_path(rel_path, LAYERS_JSON))
            .ok()
            .zip(pack.read(&Pack::sidecar_path(rel_path, LAYERS_PNG)).ok())
            .and_then(|(json, png)| {
                openpax_paint::decode_layered(&String::from_utf8_lossy(&json), &png).ok()
            })
            .filter(|doc| doc.flatten() == bitmap);
        let doc = layered.unwrap_or_else(|| Document::from_bitmap(bitmap));

        Ok(Self {
            rel_path: rel_path.to_owned(),
            saved_revision: doc.revision(),
            doc,
            saved_meta: meta.clone(),
            meta,
            texture: None,
            texture_revision: u64::MAX,
            zoom: 8.0,
            pan: Vec2::ZERO,
            fit_pending: true,
            show_grid: true,
            tile_preview: false,
            hover: None,
        })
    }

    pub fn dirty(&self) -> bool {
        self.doc.revision() != self.saved_revision || self.meta != self.saved_meta
    }

    pub fn save(&mut self, pack: &Pack) -> Result<(), String> {
        self.doc.stroke_end();
        let png = openpax_paint::encode_png(&self.doc.flatten()).map_err(|e| e.to_string())?;
        pack.write(&self.rel_path, &png)
            .map_err(|e| e.to_string())?;

        let json_path = Pack::sidecar_path(&self.rel_path, LAYERS_JSON);
        let png_path = Pack::sidecar_path(&self.rel_path, LAYERS_PNG);
        if self.doc.layers().len() > 1 {
            let (json, strip) =
                openpax_paint::encode_layered(&self.doc).map_err(|e| e.to_string())?;
            pack.write(&json_path, json.as_bytes())
                .map_err(|e| e.to_string())?;
            pack.write(&png_path, &strip).map_err(|e| e.to_string())?;
        } else {
            pack.remove(&json_path).map_err(|e| e.to_string())?;
            pack.remove(&png_path).map_err(|e| e.to_string())?;
        }

        if self.meta != self.saved_meta {
            pack.write_texture_meta(&self.rel_path, &self.meta)
                .map_err(|e| e.to_string())?;
            self.saved_meta = self.meta.clone();
        }
        self.saved_revision = self.doc.revision();
        Ok(())
    }

    /// The flattened document as a GPU texture, refreshed when the document changes.
    pub fn texture(&mut self, ctx: &egui::Context) -> &TextureHandle {
        let revision = self.doc.revision();
        if self.texture.is_none() || self.texture_revision != revision {
            let flat = self.doc.flatten();
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [flat.width() as usize, flat.height() as usize],
                flat.data(),
            );
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => {
                    let name = format!("doc:{}", self.rel_path);
                    self.texture = Some(ctx.load_texture(name, image, TextureOptions::NEAREST));
                }
            }
            self.texture_revision = revision;
        }
        self.texture.as_ref().expect("texture was just created")
    }

    fn size(&self) -> Vec2 {
        vec2(self.doc.width() as f32, self.doc.height() as f32)
    }

    /// Screen position of the image's top-left corner.
    fn origin(&self, view: Rect) -> Pos2 {
        view.center() + self.pan - self.size() * self.zoom / 2.0
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, tools: &mut ToolState, checker: &TextureHandle) {
        let view = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(view, Sense::drag());
        let painter = ui.painter_at(view);
        painter.rect_filled(view, 0.0, ui.visuals().extreme_bg_color);

        let size = self.size();
        if self.fit_pending {
            let fit = ((view.width() - 32.0) / size.x).min((view.height() - 32.0) / size.y);
            self.zoom = if fit >= 1.0 { fit.floor() } else { fit }.clamp(MIN_ZOOM, MAX_ZOOM);
            self.pan = Vec2::ZERO;
            self.fit_pending = false;
        }

        let input = ui.ctx().input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                i.pointer.latest_pos(),
                i.key_down(egui::Key::Space),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.secondary_pressed(),
                i.pointer.middle_down(),
            )
        });
        let (scroll, pinch, pointer, space, pressed, down, secondary, middle) = input;
        let over = response.contains_pointer();

        // Zoom towards the cursor.
        let factor = (scroll * 0.01).exp() * pinch;
        if over
            && factor != 1.0
            && let Some(pos) = pointer
        {
            let pixel = (pos - self.origin(view)) / self.zoom;
            self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
            let centered = view.center() - size * self.zoom / 2.0;
            self.pan = pos - pixel * self.zoom - centered;
        }

        // Pan with the middle button or space + drag.
        let panning = middle || space;
        if response.dragged_by(PointerButton::Middle)
            || (space && response.dragged_by(PointerButton::Primary))
        {
            self.pan += response.drag_delta();
        }

        let origin = self.origin(view);
        let zoom = self.zoom;
        let image_rect = Rect::from_min_size(origin, size * zoom);
        let to_screen = |x: f32, y: f32| origin + vec2(x, y) * zoom;

        // Transparency checkerboard (one cell per pixel), then the image.
        let checker_uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(size.x / 2.0, size.y / 2.0));
        painter.image(checker.id(), image_rect, checker_uv, Color32::WHITE);
        let full_uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let texture_id = self.texture(ui.ctx()).id();
        if self.tile_preview {
            for (dx, dy) in (-1..=1).flat_map(|y| (-1..=1).map(move |x| (x, y))) {
                if (dx, dy) != (0, 0) {
                    let offset = vec2(dx as f32 * size.x, dy as f32 * size.y) * zoom;
                    let tint = Color32::from_white_alpha(110);
                    painter.image(texture_id, image_rect.translate(offset), full_uv, tint);
                }
            }
        }
        painter.image(texture_id, image_rect, full_uv, Color32::WHITE);

        // Pixel grid.
        if self.show_grid && zoom >= 6.0 && size.x + size.y <= 1024.0 {
            let stroke = Stroke::new(1.0, Color32::from_black_alpha(48));
            for x in 1..self.doc.width() {
                let sx = to_screen(x as f32, 0.0).x;
                painter.vline(sx, image_rect.y_range(), stroke);
            }
            for y in 1..self.doc.height() {
                let sy = to_screen(0.0, y as f32).y;
                painter.hline(image_rect.x_range(), sy, stroke);
            }
        }

        // Animation frame boundaries.
        if let Some(animation) = &self.meta.animation {
            let (fw, fh) = animation.frame_size(self.doc.width(), self.doc.height());
            let stroke = Stroke::new(1.5, Color32::from_rgb(255, 196, 0));
            for x in (fw..self.doc.width()).step_by(fw as usize) {
                painter.vline(to_screen(x as f32, 0.0).x, image_rect.y_range(), stroke);
            }
            for y in (fh..self.doc.height()).step_by(fh as usize) {
                painter.hline(image_rect.x_range(), to_screen(0.0, y as f32).y, stroke);
            }
        }

        painter.rect_stroke(
            image_rect,
            0.0,
            Stroke::new(1.0, Color32::from_gray(128)),
            StrokeKind::Outside,
        );

        if let Some(sel) = self.doc.selection() {
            let rect = Rect::from_min_max(
                to_screen(sel.x as f32, sel.y as f32),
                to_screen((sel.x + sel.w) as f32, (sel.y + sel.h) as f32),
            );
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, Color32::BLACK),
                StrokeKind::Inside,
            );
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, Color32::WHITE),
                StrokeKind::Outside,
            );
        }

        // Tools.
        let pixel = pointer.map(|pos| {
            let p = (pos - origin) / zoom;
            (p.x.floor() as i32, p.y.floor() as i32)
        });
        self.hover = pixel.filter(|_| over);

        if over && !panning {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            if let Some((x, y)) = pixel {
                let brush = match tools.tool {
                    ToolKind::Pencil | ToolKind::Eraser | ToolKind::Line => {
                        tools.settings.size.max(1) as i32
                    }
                    _ => 1,
                };
                let (x0, y0) = (x - brush / 2, y - brush / 2);
                let rect = Rect::from_min_max(
                    to_screen(x0 as f32, y0 as f32),
                    to_screen((x0 + brush) as f32, (y0 + brush) as f32),
                );
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, Color32::WHITE),
                    StrokeKind::Outside,
                );
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, Color32::BLACK),
                    StrokeKind::Inside,
                );

                let pick = if secondary {
                    Some(ToolKind::Picker)
                } else {
                    None
                };
                let begin = pick.or((pressed).then_some(tools.tool));
                if let Some(tool) = begin
                    && let Some(color) = self.doc.stroke_begin(tool, tools.settings, (x, y))
                    && color[3] != 0
                {
                    tools.settings.color = color;
                }
            }
        }
        if self.doc.stroke_active() {
            match pixel {
                Some(p) if down => self.doc.stroke_move(p),
                _ if !down => self.doc.stroke_end(),
                _ => {}
            }
            ui.ctx().request_repaint();
        }
    }
}

pub struct TextEditor {
    pub rel_path: String,
    text: String,
    saved: String,
}

impl TextEditor {
    pub fn open(pack: &Pack, rel_path: &str) -> Result<Self, String> {
        let bytes = pack.read(rel_path).map_err(|e| e.to_string())?;
        let text =
            String::from_utf8(bytes).map_err(|_| "file is not valid UTF-8 text".to_owned())?;
        Ok(Self {
            rel_path: rel_path.to_owned(),
            saved: text.clone(),
            text,
        })
    }

    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }

    pub fn save(&mut self, pack: &Pack) -> Result<(), String> {
        pack.write(&self.rel_path, self.text.as_bytes())
            .map_err(|e| e.to_string())?;
        self.saved.clone_from(&self.text);
        Ok(())
    }

    fn is_json(&self) -> bool {
        self.rel_path.ends_with(".json") || self.rel_path.ends_with(".mcmeta")
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if self.is_json() {
            match serde_json::from_str::<serde_json::Value>(&self.text) {
                Ok(_) => ui.weak("Valid JSON"),
                Err(e) => ui.colored_label(ui.visuals().error_fg_color, format!("JSON error: {e}")),
            };
            ui.separator();
        }
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut self.text)
                    .code_editor()
                    .desired_width(f32::INFINITY),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpax_core::Animation;
    use openpax_paint::Bitmap;

    const REL: &str = "assets/minecraft/textures/block/test.png";

    fn pack_with_texture() -> (tempfile::TempDir, Pack) {
        let dir = tempfile::tempdir().unwrap();
        let pack = Pack::create(dir.path().join("pack"), "test", 46).unwrap();
        let png = openpax_paint::encode_png(&Bitmap::new(4, 4)).unwrap();
        pack.write(REL, &png).unwrap();
        (dir, pack)
    }

    fn paint(editor: &mut TextureEditor, at: (i32, i32), color: [u8; 4]) {
        let settings = ToolSettings {
            color,
            ..Default::default()
        };
        editor.doc.stroke_begin(ToolKind::Pencil, settings, at);
        editor.doc.stroke_end();
    }

    #[test]
    fn save_and_reopen_keeps_pixels_layers_and_animation() {
        let (_dir, pack) = pack_with_texture();
        let mut editor = TextureEditor::open(&pack, REL).unwrap();
        assert!(!editor.dirty());

        paint(&mut editor, (0, 0), [255, 0, 0, 255]);
        editor.doc.add_layer();
        paint(&mut editor, (3, 3), [0, 0, 255, 255]);
        editor.meta.animation = Some(Animation {
            frametime: Some(5),
            ..Default::default()
        });
        assert!(editor.dirty());
        editor.save(&pack).unwrap();
        assert!(!editor.dirty());

        // The PNG Minecraft reads is the flattened image.
        let on_disk = openpax_paint::decode_png(&pack.read(REL).unwrap()).unwrap();
        assert_eq!(on_disk.get(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(on_disk.get(3, 3), Some([0, 0, 255, 255]));

        let reopened = TextureEditor::open(&pack, REL).unwrap();
        assert_eq!(
            reopened.doc.layers().len(),
            2,
            "layers come back from the sidecar"
        );
        assert_eq!(reopened.doc.flatten(), on_disk);
        assert_eq!(reopened.meta.animation.unwrap().frametime, Some(5));
    }

    #[test]
    fn stale_layer_sidecar_is_ignored() {
        let (_dir, pack) = pack_with_texture();
        let mut editor = TextureEditor::open(&pack, REL).unwrap();
        editor.doc.add_layer();
        paint(&mut editor, (1, 1), [0, 255, 0, 255]);
        editor.save(&pack).unwrap();

        // Another program overwrites the PNG; the saved layers no longer describe it.
        let other = openpax_paint::encode_png(&Bitmap::filled(4, 4, [9, 9, 9, 255])).unwrap();
        pack.write(REL, &other).unwrap();
        let reopened = TextureEditor::open(&pack, REL).unwrap();
        assert_eq!(reopened.doc.layers().len(), 1);
        assert_eq!(reopened.doc.flatten().get(1, 1), Some([9, 9, 9, 255]));
    }

    #[test]
    fn single_layer_save_removes_sidecar() {
        let (_dir, pack) = pack_with_texture();
        let mut editor = TextureEditor::open(&pack, REL).unwrap();
        editor.doc.add_layer();
        editor.save(&pack).unwrap();
        assert!(pack.exists(&Pack::sidecar_path(REL, LAYERS_JSON)));
        editor.doc.merge_down();
        editor.save(&pack).unwrap();
        assert!(!pack.exists(&Pack::sidecar_path(REL, LAYERS_JSON)));
        assert!(!pack.exists(&Pack::sidecar_path(REL, LAYERS_PNG)));
    }

    #[test]
    fn text_editor_round_trip() {
        let (_dir, pack) = pack_with_texture();
        pack.write("assets/minecraft/models/item/x.json", b"{}")
            .unwrap();
        let mut editor = TextEditor::open(&pack, "assets/minecraft/models/item/x.json").unwrap();
        editor.text.push('\n');
        assert!(editor.dirty());
        editor.save(&pack).unwrap();
        assert!(!editor.dirty());
        assert_eq!(
            pack.read("assets/minecraft/models/item/x.json").unwrap(),
            b"{}\n"
        );
    }
}

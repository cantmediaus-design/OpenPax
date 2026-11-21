//! Editor tabs: the pixel canvas for textures, the 3D model editor and a plain text editor
//! for everything else.

use eframe::egui::{self, Align, Layout, Margin, Rect, RichText, Sense, UiBuilder, pos2, vec2};
use openpax_core::{Pack, TextureMeta};

use crate::commands::Active;
use crate::icons;
use crate::model_editor::ModelEditor;
use crate::texture_editor::{LAYERS_JSON, LAYERS_PNG, doc_from_image, export_image};
pub use crate::texture_editor::{MAX_BRUSH, TextureEditor, ToolState};
use crate::theme::{self, SPACE_M, TOOLBAR_HEIGHT};
use crate::widgets::{self, Edge};

pub enum Editor {
    Texture(Box<TextureEditor>),
    Model(Box<ModelEditor>),
    Text(TextEditor),
}

impl Editor {
    pub fn kind(&self) -> Active {
        match self {
            Self::Texture(_) => Active::Texture,
            Self::Model(_) => Active::Model,
            Self::Text(_) => Active::Text,
        }
    }

    pub fn rel_path(&self) -> &str {
        match self {
            Self::Texture(e) => &e.rel_path,
            Self::Model(e) => &e.rel_path,
            Self::Text(e) => &e.rel_path,
        }
    }

    pub fn dirty(&self) -> bool {
        match self {
            Self::Texture(e) => e.dirty(),
            Self::Model(e) => e.dirty(),
            Self::Text(e) => e.dirty(),
        }
    }

    pub fn title(&self) -> String {
        let name = self.rel_path().rsplit('/').next().unwrap_or_default();
        let icon = match self {
            Self::Texture(_) => icons::IMAGE,
            Self::Model(_) => icons::CUBE,
            Self::Text(_) => icons::FILE_CODE,
        };
        if self.dirty() {
            format!("{icon} {name} •")
        } else {
            format!("{icon} {name}")
        }
    }

    pub fn save(&mut self, pack: &Pack) -> Result<(), String> {
        match self {
            Self::Texture(e) => e.save(pack),
            Self::Model(e) => e.save(pack),
            Self::Text(e) => e.save(pack),
        }
    }

    /// What [`Self::save`] would write, as `(suffix after the asset path, bytes)` pairs, for
    /// crash recovery. The suffixes match [`crate::recovery`]'s file naming.
    pub fn snapshot(&self) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
        let mut files = Vec::new();
        match self {
            Self::Texture(e) => {
                let (texture_files, meta) = e.files()?;
                files.extend(
                    texture_files
                        .into_iter()
                        .filter(|(_, bytes)| !bytes.is_empty()),
                );
                if !meta.is_empty() {
                    files.push((".mcmeta", meta.to_json().into_bytes()));
                }
            }
            Self::Model(e) => files.push(("", e.doc.model().to_json().into_bytes())),
            Self::Text(e) => files.push(("", e.text().as_bytes().to_vec())),
        }
        Ok(files)
    }

    /// Loads a snapshot made by [`Self::snapshot`] into this editor as unsaved changes.
    pub fn restore_snapshot(
        &mut self,
        read: impl Fn(&str) -> Option<Vec<u8>>,
    ) -> Result<(), String> {
        let main = read("").ok_or("the recovered file is missing")?;
        match self {
            Self::Texture(e) => {
                let bitmap = openpax_paint::decode_png(&main).map_err(|e| e.to_string())?;
                let meta: TextureMeta = match read(".mcmeta") {
                    Some(json) => serde_json::from_slice(&json).map_err(|e| e.to_string())?,
                    None => TextureMeta::default(),
                };
                let layered = read(LAYERS_JSON)
                    .zip(read(LAYERS_PNG))
                    .and_then(|(json, png)| {
                        openpax_paint::decode_layered(&String::from_utf8_lossy(&json), &png).ok()
                    })
                    .filter(|doc| export_image(doc) == bitmap);
                let doc = layered.unwrap_or_else(|| doc_from_image(bitmap, &meta));
                e.restore(doc, meta);
            }
            Self::Model(e) => {
                let text = String::from_utf8(main).map_err(|e| e.to_string())?;
                let model =
                    openpax_model::ModelFile::from_json(&text).map_err(|e| e.to_string())?;
                e.doc.modify(|m| *m = model);
            }
            Self::Text(e) => {
                e.restore(String::from_utf8(main).map_err(|e| e.to_string())?);
            }
        }
        Ok(())
    }

    /// Whether the editor's own history can undo / redo (text tabs use the text box's).
    pub fn can_undo_redo(&self) -> (bool, bool) {
        match self {
            Self::Texture(e) => (e.doc.can_undo(), e.doc.can_redo()),
            Self::Model(e) => (e.doc.can_undo(), e.doc.can_redo()),
            Self::Text(_) => (false, false),
        }
    }

    pub fn undo(&mut self) {
        match self {
            Self::Texture(e) => drop(e.doc.undo()),
            Self::Model(e) => drop(e.doc.undo()),
            Self::Text(_) => {}
        }
    }

    pub fn redo(&mut self) {
        match self {
            Self::Texture(e) => drop(e.doc.redo()),
            Self::Model(e) => drop(e.doc.redo()),
            Self::Text(_) => {}
        }
    }
}
/// Result of checking a text tab's contents as JSON.
#[derive(Clone, Debug, PartialEq)]
enum JsonStatus {
    NotJson,
    Valid,
    /// Message plus 1-based line and column of the problem.
    Invalid(String, usize, usize),
}

pub struct TextEditor {
    pub rel_path: String,
    text: String,
    saved: String,
    status: JsonStatus,
    /// Right-aligned line numbers, rebuilt when the number of lines changes.
    gutter: String,
    gutter_lines: usize,
    /// Line (1-based) and character index to move the cursor to on the next frame.
    jump: Option<(usize, usize)>,
}

impl TextEditor {
    pub fn open(pack: &Pack, rel_path: &str) -> Result<Self, String> {
        let bytes = pack.read(rel_path).map_err(|e| e.to_string())?;
        let text =
            String::from_utf8(bytes).map_err(|_| "file is not valid UTF-8 text".to_owned())?;
        let mut editor = Self {
            rel_path: rel_path.to_owned(),
            saved: text.clone(),
            text,
            status: JsonStatus::NotJson,
            gutter: String::new(),
            gutter_lines: 0,
            jump: None,
        };
        editor.text_changed();
        Ok(editor)
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

    /// Replaces the text with recovered, unsaved text.
    pub fn restore(&mut self, text: String) {
        self.text = text;
        self.text_changed();
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn is_json(&self) -> bool {
        self.rel_path.ends_with(".json") || self.rel_path.ends_with(".mcmeta")
    }

    /// Re-validates the text and refreshes the line-number gutter.
    fn text_changed(&mut self) {
        self.status = if self.is_json() {
            match serde_json::from_str::<serde_json::Value>(&self.text) {
                Ok(_) => JsonStatus::Valid,
                Err(e) => {
                    // serde appends " at line L column C"; the toolbar shows that separately.
                    let message = e.to_string();
                    let message = message.split(" at line ").next().unwrap_or(&message);
                    JsonStatus::Invalid(message.to_owned(), e.line(), e.column())
                }
            }
        } else {
            JsonStatus::NotJson
        };

        let lines = self.text.split('\n').count();
        if lines != self.gutter_lines {
            let width = lines.to_string().len();
            self.gutter = (1..=lines)
                .map(|n| format!("{n:>width$}"))
                .collect::<Vec<_>>()
                .join("\n");
            self.gutter_lines = lines;
        }
    }

    /// Rewrites the text as consistently indented JSON. Returns false if it does not parse.
    fn format_json(&mut self) -> bool {
        match serde_json::from_str::<serde_json::Value>(&self.text) {
            Ok(value) => {
                self.text = openpax_model::to_pretty_json(&value);
                self.text_changed();
                true
            }
            Err(_) => false,
        }
    }

    /// Character index of a 1-based line and byte column, clamped to the text.
    fn char_index(&self, line: usize, column: usize) -> usize {
        let line_start: usize = self
            .text
            .split_inclusive('\n')
            .take(line.saturating_sub(1))
            .map(str::len)
            .sum();
        let mut byte = (line_start + column.saturating_sub(1)).min(self.text.len());
        while !self.text.is_char_boundary(byte) {
            byte -= 1;
        }
        self.text[..byte].chars().count()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        let bar = Rect::from_min_size(full.min, vec2(full.width(), TOOLBAR_HEIGHT));
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.allocate_rect(bar, Sense::hover());
        self.toolbar(ui, bar);

        let id = ui.scope_id().with("text");
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        let mut scroll_to_line = None;
        if let Some((line, index)) = self.jump.take() {
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            let cursor = egui::text::CCursor::new(index);
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
            egui::TextEdit::store_state(ui.ctx(), id, state);
            ui.ctx().memory_mut(|m| m.request_focus(id));
            scroll_to_line = Some(line);
        }

        let margin = Margin::symmetric(8, 6);
        let gutter_fill = theme::of(ui).toolbar;
        let mut changed = false;
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            let available = ui.available_size();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                egui::Frame::new()
                    .fill(gutter_fill)
                    .inner_margin(margin)
                    .show(ui, |ui| {
                        // A vertical layout, so the numbers start at the top like the text.
                        ui.vertical(|ui| {
                            ui.set_min_height(available.y - 2.0 * f32::from(margin.top));
                            let numbers = RichText::new(&self.gutter).font(font.clone()).weak();
                            ui.add(egui::Label::new(numbers).selectable(false).extend());
                        });
                    });
                let output = egui::TextEdit::multiline(&mut self.text)
                    .id(id)
                    .code_editor()
                    .frame(egui::Frame::NONE.inner_margin(margin))
                    .desired_width(f32::INFINITY)
                    .min_size(vec2(0.0, available.y))
                    .show(ui);
                changed = output.response.changed();
                if let Some(line) = scroll_to_line {
                    let top = output.response.rect.top()
                        + f32::from(margin.top)
                        + line.saturating_sub(1) as f32 * row_height;
                    let target = Rect::from_min_size(
                        pos2(output.response.rect.left(), top),
                        vec2(1.0, row_height),
                    );
                    ui.scroll_to_rect(target, Some(Align::Center));
                }
            });
        });
        if changed {
            self.text_changed();
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        widgets::toolbar_background(ui, rect, Edge::Bottom);
        let mut bar = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(SPACE_M, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let ui = &mut bar;
        let tokens = theme::of(ui);
        ui.label(widgets::icon(icons::FILE_CODE).color(tokens.accent));
        ui.label(RichText::new(if self.is_json() { "JSON" } else { "Text" }).strong());
        ui.separator();

        match self.status.clone() {
            JsonStatus::NotJson => {
                ui.weak(format!("{} lines", self.gutter_lines));
            }
            JsonStatus::Valid => {
                ui.label(widgets::icon(icons::SUCCESS).color(tokens.success));
                ui.weak("Valid JSON");
            }
            JsonStatus::Invalid(message, line, column) => {
                let color = ui.visuals().error_fg_color;
                ui.label(widgets::icon(icons::ERROR).color(color));
                let text =
                    RichText::new(format!("Line {line}, column {column}: {message}")).color(color);
                if ui
                    .add(egui::Button::new(text).frame_when_inactive(false))
                    .on_hover_text("Jump to the problem")
                    .clicked()
                {
                    self.jump = Some((line, self.char_index(line, column)));
                }
            }
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can_format = self.status == JsonStatus::Valid;
            let button = egui::Button::new(widgets::icon_label(icons::FORMAT, "Format"));
            if ui
                .add_enabled(can_format, button)
                .on_hover_text("Re-indent the JSON")
                .on_disabled_hover_text("Only valid JSON can be formatted")
                .clicked()
            {
                self.format_json();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpax_core::Animation;
    use openpax_paint::{Bitmap, ToolKind, ToolSettings};

    use crate::texture_editor::{MAX_RECENT_COLORS, MAX_ZOOM, MIN_ZOOM};

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
        editor.doc.stroke_begin(ToolKind::Pencil, &settings, at);
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

    #[test]
    fn json_errors_report_their_position_and_formatting_fixes_layout() {
        let (_dir, pack) = pack_with_texture();
        let rel = "assets/minecraft/models/item/y.json";
        pack.write(rel, b"{\n  \"a\": 1,\n  oops\n}").unwrap();
        let mut editor = TextEditor::open(&pack, rel).unwrap();
        let JsonStatus::Invalid(_, line, column) = editor.status.clone() else {
            panic!("expected a JSON error");
        };
        assert_eq!(line, 3);
        assert_eq!(editor.gutter, "1\n2\n3\n4");
        // The index points into the third line.
        let index = editor.char_index(line, column);
        assert!((12..=17).contains(&index), "{index}");
        assert!(!editor.format_json());

        editor.text = "{\"b\":[1,2],\"a\":{\"c\":true}}".to_owned();
        editor.text_changed();
        assert_eq!(editor.status, JsonStatus::Valid);
        assert!(editor.format_json());
        assert!(editor.text.contains('\n'));
        assert!(editor.text.find("\"b\"") < editor.text.find("\"a\""));
        assert!(editor.dirty());
    }

    #[test]
    fn zoom_steps_walk_the_ladder_and_stop_at_the_ends() {
        let (_dir, pack) = pack_with_texture();
        let mut editor = TextureEditor::open(&pack, REL).unwrap();
        editor.zoom = 7.0;
        editor.zoom_step(1);
        assert_eq!(editor.zoom, 8.0);
        editor.zoom_step(-1);
        assert_eq!(editor.zoom, 6.0);
        for _ in 0..40 {
            editor.zoom_step(1);
        }
        assert_eq!(editor.zoom, MAX_ZOOM);
        for _ in 0..40 {
            editor.zoom_step(-1);
        }
        assert_eq!(editor.zoom, MIN_ZOOM);
    }

    #[test]
    fn tool_state_tracks_recent_colours_and_swaps() {
        let mut tools = ToolState::default();
        let (primary, secondary) = (tools.settings.color, tools.secondary);
        tools.swap_colors();
        assert_eq!(
            (tools.settings.color, tools.secondary),
            (secondary, primary)
        );

        for i in 0..20u8 {
            tools.note_color([i, 0, 0, 255]);
        }
        tools.note_color([15, 0, 0, 255]);
        assert_eq!(tools.recent.len(), MAX_RECENT_COLORS);
        assert_eq!(tools.recent[0], [15, 0, 0, 255]);
        assert_eq!(tools.recent.iter().filter(|c| c[0] == 15).count(), 1);

        tools.resize_brush(-5);
        assert_eq!(tools.settings.size, 1);
        tools.resize_brush(100);
        assert_eq!(tools.settings.size, MAX_BRUSH);
    }
}

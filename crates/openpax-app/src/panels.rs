//! The right-hand inspector: tools, palette, layers and properties.

use std::time::Duration;

use eframe::egui::{self, Color32, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use openpax_core::{Animation, version_for_format};
use openpax_paint::Palette;

use crate::app::{App, TOOL_KEYS, texture_mut};
use crate::editor::Editor;

const SWATCH: f32 = 18.0;
const PREVIEW: f32 = 96.0;
const TICK: Duration = Duration::from_millis(50);

impl App {
    pub(crate) fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let active = self.active.and_then(|id| self.editors.get(&id));
                if matches!(active, Some(Editor::Model(_))) {
                    self.model_sections(ui);
                } else {
                    section(ui, "Tools", |ui| self.tools_section(ui));
                    section(ui, "Palette", |ui| self.palette_section(ui));
                    section(ui, "Layers", |ui| self.layers_section(ui));
                    section(ui, "Texture", |ui| self.texture_section(ui));
                }
                section(ui, "Pack", |ui| self.pack_section(ui));
            });
    }

    fn tools_section(&mut self, ui: &mut egui::Ui) {
        let tools = &mut self.tools;
        ui.horizontal_wrapped(|ui| {
            for (tool, _, key) in TOOL_KEYS {
                ui.selectable_value(&mut tools.tool, tool, tool.name())
                    .on_hover_text(format!("Shortcut: {key}"));
            }
        });
        ui.add_space(4.0);

        let settings = &mut tools.settings;
        ui.horizontal(|ui| {
            let [r, g, b, a] = settings.color;
            let mut color = Color32::from_rgba_unmultiplied(r, g, b, a);
            if ui.color_edit_button_srgba(&mut color).changed() {
                settings.color = color.to_srgba_unmultiplied();
            }
            ui.monospace(format!("#{r:02x}{g:02x}{b:02x}{a:02x}"));
        });
        ui.add(egui::Slider::new(&mut settings.size, 1..=16).text("Brush size"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut settings.mirror_x, "Mirror X");
            ui.checkbox(&mut settings.mirror_y, "Mirror Y");
        });
        ui.checkbox(&mut settings.fill_contiguous, "Fill connected area only");
        ui.checkbox(&mut settings.shape_filled, "Filled shapes");
        ui.weak("Right-click picks a colour. Middle-drag or Space-drag pans; scroll zooms.");
    }

    fn palette_section(&mut self, ui: &mut egui::Ui) {
        let tools = &mut self.tools;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            for color in &tools.palette.colors {
                let (rect, response) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
                let [r, g, b, a] = *color;
                ui.painter()
                    .rect_filled(rect, 2.0, Color32::from_rgba_unmultiplied(r, g, b, a));
                if *color == tools.settings.color {
                    let stroke = Stroke::new(2.0, ui.visuals().strong_text_color());
                    ui.painter()
                        .rect_stroke(rect, 2.0, stroke, StrokeKind::Inside);
                }
                if response
                    .on_hover_text(format!("#{r:02x}{g:02x}{b:02x}{a:02x}"))
                    .clicked()
                {
                    tools.settings.color = *color;
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Add current").clicked() {
                tools.palette.add(tools.settings.color);
            }
            if ui.button("Default").clicked() {
                tools.palette = Palette::default();
            }
            let editor = texture_mut(&mut self.editors, self.active);
            if ui
                .add_enabled(editor.is_some(), egui::Button::new("From texture"))
                .clicked()
                && let Some(editor) = editor
            {
                tools.palette = Palette::from_bitmap(&editor.doc.flatten(), 64);
            }
        });
    }

    fn layers_section(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            ui.weak("Open a texture to edit its layers.");
            return;
        };
        let doc = &mut editor.doc;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Add").clicked() {
                doc.add_layer();
            }
            if ui.button("Duplicate").clicked() {
                doc.duplicate_layer();
            }
            if ui
                .add_enabled(doc.layers().len() > 1, egui::Button::new("Delete"))
                .clicked()
            {
                doc.remove_layer();
            }
            if ui.button("Up").clicked() {
                doc.move_layer(true);
            }
            if ui.button("Down").clicked() {
                doc.move_layer(false);
            }
            if ui
                .add_enabled(doc.active() > 0, egui::Button::new("Merge down"))
                .clicked()
            {
                doc.merge_down();
            }
        });

        // Top layer first, like every other paint program.
        for index in (0..doc.layers().len()).rev() {
            let (name, mut visible) = {
                let layer = &doc.layers()[index];
                (layer.name.clone(), layer.visible)
            };
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut visible, "")
                    .on_hover_text("Visible")
                    .changed()
                {
                    doc.set_layer_visible(index, visible);
                }
                if ui.selectable_label(index == doc.active(), name).clicked() {
                    doc.set_active(index);
                }
            });
        }

        ui.separator();
        let active = doc.active();
        let mut name = doc.layers()[active].name.clone();
        let mut opacity = doc.layers()[active].opacity;
        ui.horizontal(|ui| {
            ui.label("Name");
            if ui.text_edit_singleline(&mut name).changed() {
                doc.set_layer_name(active, name);
            }
        });
        if ui
            .add(egui::Slider::new(&mut opacity, 0.0..=1.0).text("Opacity"))
            .changed()
        {
            doc.set_layer_opacity(active, opacity);
        }
        ui.weak("Layers are kept in the pack's .openpax folder; the PNG stays flattened.");
    }

    fn texture_section(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            ui.weak("No texture open.");
            return;
        };
        let (width, height) = (editor.doc.width(), editor.doc.height());
        ui.monospace(&editor.rel_path);
        ui.label(format!("{width} × {height} px"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut editor.show_grid, "Grid");
            ui.checkbox(&mut editor.tile_preview, "Tiling preview");
        });

        ui.separator();
        let mut animated = editor.meta.animation.is_some();
        if ui.checkbox(&mut animated, "Animated (.mcmeta)").changed() {
            editor.meta.animation = animated.then(Animation::default);
        }
        let Some(mut animation) = editor.meta.animation.clone() else {
            return;
        };

        let mut frametime = animation.frametime.unwrap_or(1);
        ui.horizontal(|ui| {
            ui.label("Frame time");
            if ui
                .add(
                    egui::DragValue::new(&mut frametime)
                        .range(1..=12000)
                        .suffix(" ticks"),
                )
                .changed()
            {
                animation.frametime = Some(frametime);
            }
        });
        let mut interpolate = animation.interpolate.unwrap_or(false);
        if ui
            .checkbox(&mut interpolate, "Interpolate between frames")
            .changed()
        {
            animation.interpolate = interpolate.then_some(true);
        }

        let (fw, fh) = animation.frame_size(width, height);
        let count = animation.frame_count(width, height);
        ui.label(format!("{count} frames of {fw} × {fh} px"));
        if let Some(frames) = &animation.frames {
            ui.weak(format!(
                "Custom frame order with {} entries (edit the .mcmeta to change).",
                frames.len()
            ));
        }
        if width == fw {
            ui.horizontal(|ui| {
                if ui.button("Add frame").clicked() {
                    editor.doc.resize_canvas(width, height + fh);
                    editor.fit_pending = true;
                }
                if ui
                    .add_enabled(height > fh, egui::Button::new("Remove last frame"))
                    .clicked()
                {
                    editor.doc.resize_canvas(width, height - fh);
                    editor.fit_pending = true;
                }
            });
        }
        if editor.meta.animation.as_ref() != Some(&animation) {
            editor.meta.animation = Some(animation.clone());
        }

        // Live preview at game speed (20 ticks per second).
        let tick = (ui.ctx().input(|i| i.time) * 20.0) as u64;
        let frame = animation.frame_at(width, height, tick).min(count - 1);
        let columns = (width / fw).max(1);
        let (col, row) = (frame % columns, frame / columns);
        let uv = Rect::from_min_max(
            pos2(
                (col * fw) as f32 / width as f32,
                (row * fh) as f32 / height as f32,
            ),
            pos2(
                ((col + 1) * fw) as f32 / width as f32,
                ((row + 1) * fh) as f32 / height as f32,
            ),
        );
        let size = vec2(PREVIEW, PREVIEW * fh as f32 / fw as f32);
        let texture_id = editor.texture(ui.ctx()).id();
        ui.add(egui::Image::from_texture((texture_id, size)).uv(uv));
        ui.weak(format!("Frame {frame}"));
        ui.ctx().request_repaint_after(TICK);
    }

    fn pack_section(&mut self, ui: &mut egui::Ui) {
        let Some(pack) = &mut self.pack else { return };
        ui.monospace(pack.root().display().to_string());

        match &mut pack.meta.pack.description {
            serde_json::Value::String(text) => {
                ui.label("Description");
                ui.text_edit_singleline(text);
            }
            _ => {
                ui.weak(
                    "The description is a text component; edit pack.mcmeta as text to change it.",
                );
            }
        }
        match pack.meta.format() {
            Some(mut format) => {
                ui.horizontal(|ui| {
                    ui.label("Format");
                    if ui
                        .add(egui::DragValue::new(&mut format).range(1..=9999))
                        .changed()
                    {
                        pack.meta.set_format(format);
                    }
                    ui.weak(version_for_format(format).unwrap_or("unknown version"));
                });
            }
            None => {
                ui.weak("No pack format declared.");
            }
        }
        if ui.button("Save pack.mcmeta").clicked() {
            match pack.save_meta() {
                Ok(()) => self.status = "Saved pack.mcmeta".to_owned(),
                Err(e) => self.message(format!("Could not save pack.mcmeta:\n{e}")),
            }
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(title)
        .default_open(true)
        .show(ui, add_contents);
}

//! The preview window: the active texture at actual size and enlarged, playing its
//! animation, against different backgrounds and as seen with colour-vision deficiencies.

use eframe::egui::{self, Color32, Rect, Sense, ViewportBuilder, ViewportId, pos2, vec2};
use openpax_paint::color::VisionSim;

use crate::app::{App, texture_mut};
use crate::theme::SPACE_S;

/// Preview backgrounds: name and colour (None is the checkerboard).
const BACKGROUNDS: [(&str, Option<Color32>); 6] = [
    ("Checker", None),
    ("Sky", Some(Color32::from_rgb(120, 167, 255))),
    ("Grass", Some(Color32::from_rgb(94, 157, 52))),
    ("Stone", Some(Color32::from_rgb(125, 125, 125))),
    ("Night", Some(Color32::from_rgb(12, 14, 30))),
    ("White", Some(Color32::WHITE)),
];

#[derive(Clone, Copy)]
struct PreviewState {
    background: usize,
    vision: VisionSim,
    /// Repeat the texture 3 × 3, like a block surface in the world.
    tiled: bool,
    scale: f32,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            background: 0,
            vision: VisionSim::Normal,
            tiled: false,
            scale: 4.0,
        }
    }
}

impl App {
    pub(crate) fn show_preview(&mut self, ctx: &egui::Context) {
        if !self.preview_open {
            return;
        }
        let mut open = true;
        let checker = self.checker.id();
        let state_id = egui::Id::unique("preview_state");
        let mut state: PreviewState = ctx.data(|d| d.get_temp(state_id)).unwrap_or_default();
        let view = self.tools.view.clone();
        let mut editor = texture_mut(&mut self.editors, self.active);
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("openpax_preview"),
            ViewportBuilder::default()
                .with_title("OpenPax Preview")
                .with_inner_size([420.0, 480.0]),
            |ui, _class| {
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    open = false;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    let Some(editor) = editor.as_deref_mut() else {
                        ui.weak("Open a texture to preview it.");
                        return;
                    };
                    // Keep the animation going (the main window drives playback).
                    editor.tick_playback(ui.ctx(), &view);
                    ui.horizontal_wrapped(|ui| {
                        egui::ComboBox::from_id_salt("bg")
                            .selected_text(BACKGROUNDS[state.background].0)
                            .show_ui(ui, |ui| {
                                for (i, (name, _)) in BACKGROUNDS.iter().enumerate() {
                                    ui.selectable_value(&mut state.background, i, *name);
                                }
                            });
                        egui::ComboBox::from_id_salt("vision")
                            .selected_text(state.vision.name())
                            .show_ui(ui, |ui| {
                                for v in VisionSim::ALL {
                                    ui.selectable_value(&mut state.vision, v, v.name());
                                }
                            });
                        ui.checkbox(&mut state.tiled, "Tiled 3×3");
                        ui.add(
                            egui::Slider::new(&mut state.scale, 1.0..=16.0)
                                .step_by(1.0)
                                .text("×"),
                        );
                        let label = if editor.playing { "Pause" } else { "Play" };
                        if editor.doc.frame_count() > 1 && ui.button(label).clicked() {
                            editor.toggle_play(ui.ctx(), &view);
                        }
                    });
                    ui.add_space(SPACE_S);
                    let ctx = ui.ctx().clone();
                    let frame = editor.doc.current_frame();
                    let id = if state.vision == VisionSim::Normal {
                        editor.frame_texture(&ctx, frame)
                    } else {
                        editor.texture(&ctx, state.vision)
                    };
                    let (w, h) = (editor.doc.width() as f32, editor.doc.height() as f32);
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    let draw = |ui: &mut egui::Ui, scale: f32| {
                        let reps = if state.tiled { 3.0 } else { 1.0 };
                        let size = vec2(w, h) * scale * reps;
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        match BACKGROUNDS[state.background].1 {
                            Some(c) => {
                                ui.painter().rect_filled(rect, 0.0, c);
                            }
                            None => {
                                let cells = Rect::from_min_max(
                                    pos2(0.0, 0.0),
                                    pos2(size.x / 16.0, size.y / 16.0),
                                );
                                ui.painter().image(checker, rect, cells, Color32::WHITE);
                            }
                        }
                        let tile = vec2(w, h) * scale;
                        for ty in 0..reps as i32 {
                            for tx in 0..reps as i32 {
                                let r = Rect::from_min_size(
                                    rect.min + vec2(tx as f32 * tile.x, ty as f32 * tile.y),
                                    tile,
                                );
                                ui.painter().image(id, r, uv, Color32::WHITE);
                            }
                        }
                    };
                    // Actual size and 2× next to each other, then the chosen scale.
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.weak("1×");
                            draw(ui, 1.0);
                        });
                        ui.vertical(|ui| {
                            ui.weak("2×");
                            draw(ui, 2.0);
                        });
                    });
                    ui.add_space(SPACE_S);
                    egui::ScrollArea::both().show(ui, |ui| draw(ui, state.scale));
                    if editor.playing {
                        ctx.request_repaint();
                    }
                });
            },
        );
        ctx.data_mut(|d| d.insert_temp(state_id, state));
        if !open {
            self.preview_open = false;
        }
    }
}

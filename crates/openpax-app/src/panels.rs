//! The right-hand inspector: colours, palette, layers and properties of the active tab.

use eframe::egui::{self, RichText};
use openpax_core::version_for_format;

use crate::app::App;
use crate::commands::Active;
use crate::dialogs::format_picker;
use crate::theme::{SPACE_M, SPACE_S};
use crate::{icons, widgets};

impl App {
    pub(crate) fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                // Keep controls clear of the scroll bar.
                ui.set_max_width(ui.available_width() - SPACE_M);
                ui.add_space(SPACE_S);
                match self.active_kind() {
                    Active::Texture => self.texture_sections(ui),

                    Active::Model => self.model_sections(ui),
                    Active::Text | Active::None => {}
                }
                widgets::section(ui, icons::PACKAGE, "Pack", |ui| self.pack_section(ui));
            });
    }

    fn pack_section(&mut self, ui: &mut egui::Ui) {
        let dirty = self.pack_dirty();
        let Some(pack) = &mut self.pack else { return };
        let root = pack.root().display().to_string();
        ui.label(RichText::new(pack.name()).strong());
        ui.add(egui::Label::new(RichText::new(&root).monospace().weak()).truncate())
            .on_hover_text(&root);
        ui.add_space(SPACE_S);

        match &mut pack.meta.pack.description {
            serde_json::Value::String(text) => {
                widgets::caption(ui, "DESCRIPTION");
                ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
            }
            _ => {
                ui.weak(
                    "The description is a text component; edit pack.mcmeta as text to change it.",
                );
            }
        }
        match pack.meta.format() {
            Some(mut format) => {
                widgets::caption(ui, "GAME VERSION");
                ui.horizontal(|ui| {
                    let before = format;
                    format_picker(ui, &mut format);
                    if format != before {
                        pack.meta.set_format(format);
                    }
                });
                if version_for_format(format).is_none() {
                    ui.weak("OpenPax does not know which game version this format belongs to.");
                }
            }
            None => {
                ui.weak("No pack format declared.");
            }
        }

        if dirty {
            ui.add_space(SPACE_M);
            ui.horizontal(|ui| {
                let save = widgets::primary_button(ui, true, "Save pack.mcmeta").clicked();
                let revert = widgets::secondary_button(ui, "Revert").clicked();
                if save {
                    self.save_pack_meta();
                } else if revert {
                    self.revert_pack_meta();
                }
            });
        }

        ui.add_space(SPACE_M);
        if let Some(action) = self.pack_icon_ui(ui) {
            self.run_icon_action(action);
        }
    }
}

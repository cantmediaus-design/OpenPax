//! Modal dialogs: new pack, new texture, canvas resize, unsaved-changes prompts, messages.

use eframe::egui;
use openpax_core::{Pack, RESOURCE_FORMATS, ResourceLocation, version_for_format};
use openpax_paint::Bitmap;

use crate::app::{App, texture_mut};

const MAX_TEXTURE_SIDE: u32 = 4096;

#[derive(Default)]
pub enum Dialog {
    #[default]
    None,
    Message(String),
    NewPack {
        name: String,
        description: String,
        format: u32,
    },
    NewTexture {
        namespace: String,
        path: String,
        width: u32,
        height: u32,
    },
    Resize {
        width: u32,
        height: u32,
    },
    /// Editors with unsaved changes that are about to be closed.
    ConfirmClose {
        ids: Vec<u64>,
        quit: bool,
    },
}

impl Dialog {
    pub fn new_pack() -> Self {
        Self::NewPack {
            name: "my_pack".to_owned(),
            description: "Made with OpenPax".to_owned(),
            format: RESOURCE_FORMATS[0].1,
        }
    }

    pub fn new_texture() -> Self {
        Self::NewTexture {
            namespace: "minecraft".to_owned(),
            path: "block/my_texture".to_owned(),
            width: 16,
            height: 16,
        }
    }
}

fn format_label(format: u32) -> String {
    match version_for_format(format) {
        Some(version) => format!("{version}  (format {format})"),
        None => format!("format {format}"),
    }
}

/// A combo box of known game versions plus a free-form format number.
pub(crate) fn format_picker(ui: &mut egui::Ui, format: &mut u32) {
    egui::ComboBox::from_id_salt("pack_format")
        .selected_text(format_label(*format))
        .show_ui(ui, |ui| {
            for (version, number) in RESOURCE_FORMATS {
                ui.selectable_value(format, *number, format!("{version}  (format {number})"));
            }
        });
    ui.add(egui::DragValue::new(format).range(1..=9999));
}

impl App {
    pub(crate) fn show_dialog(&mut self, ctx: &egui::Context) {
        let mut dialog = std::mem::take(&mut self.dialog);
        if matches!(dialog, Dialog::None) {
            return;
        }
        let mut done = false;
        let modal = egui::Modal::new(egui::Id::unique("openpax_dialog")).show(ctx, |ui| {
            ui.set_width(380.0);
            done = match &mut dialog {
                Dialog::None => true,
                Dialog::Message(text) => {
                    ui.label(text.as_str());
                    ui.add_space(8.0);
                    ui.button("OK").clicked()
                }
                Dialog::NewPack {
                    name,
                    description,
                    format,
                } => self.new_pack_dialog(ui, name, description, format),
                Dialog::NewTexture {
                    namespace,
                    path,
                    width,
                    height,
                } => self.new_texture_dialog(ui, namespace, path, width, height),
                Dialog::Resize { width, height } => self.resize_dialog(ui, width, height),
                Dialog::ConfirmClose { ids, quit } => self.confirm_close_dialog(ui, ids, *quit),
            };
        });
        // An action may have replaced the dialog (e.g. with an error message); that one wins.
        if !done && !modal.should_close() && matches!(self.dialog, Dialog::None) {
            self.dialog = dialog;
        }
    }

    fn new_pack_dialog(
        &mut self,
        ui: &mut egui::Ui,
        name: &mut String,
        description: &mut String,
        format: &mut u32,
    ) -> bool {
        ui.heading("New Resource Pack");
        egui::Grid::new("new_pack").num_columns(2).show(ui, |ui| {
            ui.label("Folder name");
            ui.text_edit_singleline(name);
            ui.end_row();
            ui.label("Description");
            ui.text_edit_singleline(description);
            ui.end_row();
            ui.label("Game version");
            ui.horizontal(|ui| format_picker(ui, format));
            ui.end_row();
        });
        let name_ok = !name.trim().is_empty()
            && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']);
        if !name_ok {
            ui.colored_label(ui.visuals().warn_fg_color, "Enter a valid folder name.");
        }
        ui.add_space(8.0);
        let mut done = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(name_ok, egui::Button::new("Choose Location and Create…"))
                .clicked()
                && let Some(parent) = rfd::FileDialog::new()
                    .set_title("Create the pack folder inside…")
                    .pick_folder()
            {
                let root = parent.join(name.trim());
                match Pack::create(&root, description, *format) {
                    Ok(_) => self.open_pack(root),
                    Err(e) => self.message(format!("Could not create pack:\n{e}")),
                }
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
        done
    }

    fn new_texture_dialog(
        &mut self,
        ui: &mut egui::Ui,
        namespace: &mut String,
        path: &mut String,
        width: &mut u32,
        height: &mut u32,
    ) -> bool {
        ui.heading("New Texture");
        egui::Grid::new("new_texture")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Namespace");
                ui.text_edit_singleline(namespace);
                ui.end_row();
                ui.label("Path");
                ui.text_edit_singleline(path);
                ui.end_row();
                ui.label("Size");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(width).range(1..=MAX_TEXTURE_SIDE));
                    ui.label("×");
                    ui.add(egui::DragValue::new(height).range(1..=MAX_TEXTURE_SIDE));
                });
                ui.end_row();
            });
        let location = ResourceLocation::new(namespace, path);
        let rel_path = format!("assets/{namespace}/textures/{path}.png");
        match &location {
            Ok(_) => ui.weak(&rel_path),
            Err(_) => ui.colored_label(
                ui.visuals().warn_fg_color,
                "Use lowercase letters, digits, _ - . and / only.",
            ),
        };
        ui.add_space(8.0);
        let mut done = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(location.is_ok(), egui::Button::new("Create"))
                .clicked()
            {
                self.create_texture(&rel_path, *width, *height);
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
        done
    }

    fn create_texture(&mut self, rel_path: &str, width: u32, height: u32) {
        let Some(pack) = &self.pack else { return };
        if pack.exists(rel_path) {
            self.message(format!("{rel_path} already exists."));
            return;
        }
        let result = openpax_paint::encode_png(&Bitmap::new(width, height))
            .map_err(|e| e.to_string())
            .and_then(|png| pack.write(rel_path, &png).map_err(|e| e.to_string()));
        match result {
            Ok(()) => {
                self.rescan();
                self.open_asset(rel_path);
            }
            Err(e) => self.message(format!("Could not create texture:\n{e}")),
        }
    }

    fn resize_dialog(&mut self, ui: &mut egui::Ui, width: &mut u32, height: &mut u32) -> bool {
        ui.heading("Resize Canvas");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(width).range(1..=MAX_TEXTURE_SIDE));
            ui.label("×");
            ui.add(egui::DragValue::new(height).range(1..=MAX_TEXTURE_SIDE));
        });
        ui.weak("The top-left corner stays in place; pixels outside the new size are cropped.");
        ui.add_space(8.0);
        let mut done = false;
        ui.horizontal(|ui| {
            if ui.button("Resize").clicked() {
                if let Some(editor) = texture_mut(&mut self.editors, self.active) {
                    editor.doc.resize_canvas(*width, *height);
                    editor.fit_pending = true;
                }
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
        done
    }

    fn confirm_close_dialog(&mut self, ui: &mut egui::Ui, ids: &[u64], quit: bool) -> bool {
        ui.heading("Unsaved Changes");
        ui.label("These files have unsaved changes:");
        for id in ids {
            if let Some(editor) = self.editors.get(id) {
                ui.monospace(editor.rel_path());
            }
        }
        ui.add_space(8.0);
        let mut done = false;
        ui.horizontal(|ui| {
            let save = ui.button("Save").clicked();
            let discard = ui.button("Discard").clicked();
            done = ui.button("Cancel").clicked();
            if save || discard {
                done = true;
                if save && !ids.iter().all(|id| self.save_editor(*id)) {
                    return; // the failure is shown in its own dialog; keep the tabs open
                }
                for id in ids {
                    self.close_editor(*id);
                }
                if quit {
                    self.allow_quit = true;
                    ui.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        });
        done
    }
}

//! Modal dialogs: new pack, new texture, new model, canvas resize and unsaved-changes prompts.
//! The default-pack and remix dialogs live in [`crate::remix_ui`].
//!
//! Dialogs are for decisions and forms. Errors and other notices go through
//! [`App::notify`](crate::app::App::notify) instead, so they never replace an open dialog.

use std::path::PathBuf;

use eframe::egui::{self, Key, RichText};
use openpax_core::{Pack, RESOURCE_FORMATS, ResourceLocation, version_for_format};
use openpax_model::{Element, ModelFile};
use openpax_paint::Bitmap;

use crate::app::{App, texture_mut};
use crate::remix_ui::{LoadDefaultState, RemixState};
use crate::theme::{self, DIALOG_WIDTH, SPACE_L, SPACE_M, SPACE_S};
use crate::{icons, widgets};

const WIDE_DIALOG_WIDTH: f32 = 680.0;
const MAX_TEXTURE_SIDE: u32 = 4096;
const LOCATION_RULE: &str = "Use lowercase letters, digits, _ - . and / only.";

pub enum Dialog {
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
        /// An existing PNG to import instead of starting from a blank canvas.
        source: Option<PathBuf>,
    },
    NewModel {
        namespace: String,
        path: String,
    },
    Resize {
        width: u32,
        height: u32,
    },
    /// Editors with unsaved changes, and what to do once they are saved or discarded.
    ConfirmClose {
        ids: Vec<u64>,
        then: Then,
    },
    LoadDefault(Box<LoadDefaultState>),
    Remix(Box<RemixState>),
    Shortcuts(crate::shortcuts_ui::ShortcutEditor),
    About,
    /// Rename or move an asset; `path` is the new path below its category, without extension.
    RenameAsset {
        from: String,
        path: String,
    },
    DeleteAsset {
        rel_path: String,
    },
    /// Unsaved work found after OpenPax did not close normally.
    Recover {
        assets: Vec<String>,
    },
    OptimizeModel(Box<crate::optimize_ui::OptimizeModelState>),
    /// Options for optimizing every model, before the analysis runs.
    OptimizePack,
    /// What optimizing every model would change, waiting for confirmation.
    OptimizeSummary(Box<crate::optimize::OptimizePlan>),
}

/// What an unsaved-changes prompt is guarding.
#[derive(Clone, Debug, PartialEq)]
pub enum Then {
    /// Close these tabs.
    CloseTabs,
    /// Close every tab and quit.
    Quit,
    /// Close every tab and open another pack.
    OpenPack(PathBuf),
    /// Export the pack as a zip; tabs stay open.
    Export,
}

impl Then {
    /// Whether `pack.mcmeta` edits are part of the prompt (everything but closing single tabs).
    fn includes_pack_meta(&self) -> bool {
        *self != Self::CloseTabs
    }
}

/// `folder/name`, or just `name` at the top of the category.
fn join_path(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

/// Turns a file name into something valid inside a resource location.
fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '_' | '-' | '.' => c,
            _ => '_',
        })
        .collect();
    if cleaned.is_empty() {
        "texture".to_owned()
    } else {
        cleaned
    }
}

impl Dialog {
    pub fn new_pack() -> Self {
        Self::NewPack {
            name: "my_pack".to_owned(),
            description: "Made with OpenPax".to_owned(),
            format: RESOURCE_FORMATS[0].1,
        }
    }

    pub fn new_model() -> Self {
        Self::new_model_in("minecraft", "block")
    }

    pub fn new_model_in(namespace: &str, folder: &str) -> Self {
        Self::NewModel {
            namespace: namespace.to_owned(),
            path: join_path(folder, "my_model"),
        }
    }

    pub fn new_texture() -> Self {
        Self::new_texture_in("minecraft", "block")
    }

    pub fn new_texture_in(namespace: &str, folder: &str) -> Self {
        Self::NewTexture {
            namespace: namespace.to_owned(),
            path: join_path(folder, "my_texture"),
            width: 16,
            height: 16,
            source: None,
        }
    }

    /// Adds an existing PNG file to the pack as a texture.
    pub fn import_texture(source: PathBuf) -> Self {
        let stem = source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::NewTexture {
            namespace: "minecraft".to_owned(),
            path: join_path("block", &sanitize_name(&stem)),
            width: 16,
            height: 16,
            source: Some(source),
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
    ui.add(egui::DragValue::new(format).range(1..=9999))
        .on_hover_text("Pack format number");
}

pub(crate) fn header(ui: &mut egui::Ui, glyph: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).size(22.0).color(theme::of(ui).accent));
        ui.heading(title);
    });
    ui.add_space(SPACE_M);
}

/// The button row of a dialog. Returns `(confirmed, cancelled)`; Enter confirms.
pub(crate) fn footer(ui: &mut egui::Ui, primary: &str, enabled: bool) -> (bool, bool) {
    ui.add_space(SPACE_L);
    let enter = enabled && ui.input(|i| i.key_pressed(Key::Enter));
    let (mut confirm, mut cancel) = (false, false);
    egui::Sides::new().show(
        ui,
        |_| {},
        |ui| {
            confirm = widgets::primary_button(ui, enabled, primary).clicked();
            cancel = widgets::secondary_button(ui, "Cancel").clicked();
        },
    );
    (confirm || enter, cancel)
}

pub(crate) fn form(ui: &mut egui::Ui, id: &str, add_rows: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([SPACE_L, SPACE_M])
        .show(ui, add_rows);
}

/// A full-width text field in a form row; focused when the dialog first appears.
pub(crate) fn text_row(ui: &mut egui::Ui, label: &str, text: &mut String, focus: bool) {
    ui.label(label);
    let response = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
    if focus {
        response.request_focus();
    }
    ui.end_row();
}

fn size_row(ui: &mut egui::Ui, width: &mut u32, height: &mut u32) {
    ui.label("Size");
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(width)
                .range(1..=MAX_TEXTURE_SIDE)
                .suffix(" px"),
        );
        ui.label("×");
        ui.add(
            egui::DragValue::new(height)
                .range(1..=MAX_TEXTURE_SIDE)
                .suffix(" px"),
        );
        for side in [16, 32, 64] {
            if ui.small_button(side.to_string()).clicked() {
                (*width, *height) = (side, side);
            }
        }
    });
    ui.end_row();
}

pub(crate) fn problem(ui: &mut egui::Ui, text: &str) {
    ui.add_space(SPACE_S);
    let color = ui.visuals().warn_fg_color;
    ui.horizontal(|ui| {
        ui.label(widgets::icon(icons::WARNING).color(color));
        ui.colored_label(color, text);
    });
}

impl App {
    pub(crate) fn show_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialogs.pop() else {
            return;
        };
        // Position in the stack; a dialog opened from this one goes on top of it.
        let depth = self.dialogs.len();
        let fresh = std::mem::take(&mut self.dialog_fresh);
        let mut done = false;
        let modal = egui::Modal::new(egui::Id::unique(("openpax_dialog", depth))).show(ctx, |ui| {
            ui.set_width(match dialog {
                Dialog::Remix(_) | Dialog::Shortcuts(_) | Dialog::OptimizeSummary(_) => {
                    WIDE_DIALOG_WIDTH
                }
                Dialog::OptimizeModel(_) | Dialog::OptimizePack => DIALOG_WIDTH + 80.0,
                Dialog::LoadDefault(_) => DIALOG_WIDTH + 80.0,
                _ => DIALOG_WIDTH,
            });
            done = match &mut dialog {
                Dialog::NewPack {
                    name,
                    description,
                    format,
                } => self.new_pack_dialog(ui, fresh, name, description, format),
                Dialog::NewTexture {
                    namespace,
                    path,
                    width,
                    height,
                    source,
                } => self.new_texture_dialog(ui, fresh, namespace, path, width, height, source),
                Dialog::NewModel { namespace, path } => {
                    self.new_model_dialog(ui, fresh, namespace, path)
                }
                Dialog::Resize { width, height } => self.resize_dialog(ui, width, height),
                Dialog::ConfirmClose { ids, then } => self.confirm_close_dialog(ui, ids, then),
                Dialog::LoadDefault(state) => self.load_default_dialog(ui, state),
                Dialog::Remix(state) => self.remix_dialog(ui, state),
                Dialog::Shortcuts(state) => self.shortcuts_dialog(ui, state),
                Dialog::About => about_dialog(ui),
                Dialog::RenameAsset { from, path } => {
                    let from = from.clone();
                    self.rename_dialog(ui, fresh, &from, path)
                }
                Dialog::DeleteAsset { rel_path } => {
                    let rel_path = rel_path.clone();
                    self.delete_dialog(ui, &rel_path)
                }
                Dialog::Recover { assets } => {
                    let assets = assets.clone();
                    self.recover_dialog(ui, &assets)
                }
                Dialog::OptimizeModel(state) => self.optimize_model_dialog(ui, state),
                Dialog::OptimizePack => self.optimize_pack_dialog(ui),
                Dialog::OptimizeSummary(plan) => self.optimize_summary_dialog(ui, plan),
            };
        });
        // Escape or a click outside cancels.
        if !done && !modal.should_close() {
            self.dialogs.insert(depth, dialog);
        }
    }

    fn new_pack_dialog(
        &mut self,
        ui: &mut egui::Ui,
        fresh: bool,
        name: &mut String,
        description: &mut String,
        format: &mut u32,
    ) -> bool {
        header(ui, icons::FOLDER_PLUS, "New Resource Pack");
        form(ui, "new_pack", |ui| {
            text_row(ui, "Folder name", name, fresh);
            text_row(ui, "Description", description, false);
            ui.label("Game version");
            ui.horizontal(|ui| format_picker(ui, format));
            ui.end_row();
        });
        let name_ok = !name.trim().is_empty()
            && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']);
        if !name_ok {
            problem(ui, "Enter a valid folder name.");
        }
        let (confirm, cancel) = footer(ui, "Choose Location…", name_ok);
        if confirm
            && let Some(parent) = rfd::FileDialog::new()
                .set_title("Create the pack folder inside…")
                .pick_folder()
        {
            let root = parent.join(name.trim());
            match Pack::create(&root, description, *format) {
                Ok(_) => self.open_new_pack(root),
                Err(e) => self.error(format!("Could not create pack:\n{e}")),
            }
            return true;
        }
        cancel
    }

    #[expect(clippy::too_many_arguments)]
    fn new_texture_dialog(
        &mut self,
        ui: &mut egui::Ui,
        fresh: bool,
        namespace: &mut String,
        path: &mut String,
        width: &mut u32,
        height: &mut u32,
        source: &Option<PathBuf>,
    ) -> bool {
        match source {
            Some(_) => header(ui, icons::IMAGE, "Import Texture"),
            None => header(ui, icons::IMAGE, "New Texture"),
        }
        if let Some(source) = source {
            ui.label(
                RichText::new(source.display().to_string())
                    .monospace()
                    .weak(),
            );
            ui.add_space(SPACE_S);
        }
        form(ui, "new_texture", |ui| {
            text_row(ui, "Namespace", namespace, false);
            text_row(ui, "Path", path, fresh);
            if source.is_none() {
                size_row(ui, width, height);
            }
        });
        let rel_path = format!("assets/{namespace}/textures/{path}.png");
        let valid = self.location_feedback(ui, namespace, path, &rel_path);
        let action = if source.is_some() { "Import" } else { "Create" };
        let (confirm, cancel) = footer(ui, action, valid);
        if confirm {
            self.create_texture(&rel_path, *width, *height, source.as_deref());
            return true;
        }
        cancel
    }

    /// Shows where a new asset will be written, or why it cannot be. Returns whether it can.
    fn location_feedback(
        &self,
        ui: &mut egui::Ui,
        namespace: &str,
        path: &str,
        rel_path: &str,
    ) -> bool {
        if ResourceLocation::new(namespace, path).is_err() {
            problem(ui, LOCATION_RULE);
            return false;
        }
        if self.pack.as_ref().is_some_and(|p| p.exists(rel_path)) {
            problem(ui, "A file with this name already exists.");
            return false;
        }
        ui.add_space(SPACE_S);
        ui.label(RichText::new(rel_path).monospace().weak());
        true
    }

    fn create_texture(
        &mut self,
        rel_path: &str,
        width: u32,
        height: u32,
        source: Option<&std::path::Path>,
    ) {
        let Some(pack) = &self.pack else { return };
        let png = match source {
            // Decode first so that only real PNG files end up in the pack.
            Some(source) => std::fs::read(source)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    openpax_paint::decode_png(&bytes)
                        .map(|_| bytes)
                        .map_err(|e| e.to_string())
                }),
            None => {
                openpax_paint::encode_png(&Bitmap::new(width, height)).map_err(|e| e.to_string())
            }
        };
        let result = png.and_then(|png| pack.write(rel_path, &png).map_err(|e| e.to_string()));
        match result {
            Ok(()) => {
                self.rescan();
                self.open_asset(rel_path);
            }
            Err(e) => self.error(format!("Could not create texture:\n{e}")),
        }
    }

    fn new_model_dialog(
        &mut self,
        ui: &mut egui::Ui,
        fresh: bool,
        namespace: &mut String,
        path: &mut String,
    ) -> bool {
        header(ui, icons::CUBE, "New Model");
        form(ui, "new_model", |ui| {
            text_row(ui, "Namespace", namespace, false);
            text_row(ui, "Path", path, fresh);
        });
        let rel_path = format!("assets/{namespace}/models/{path}.json");
        let valid = self.location_feedback(ui, namespace, path, &rel_path);
        ui.weak("Starts as one full-block cube using texture variable #0.");
        let (confirm, cancel) = footer(ui, "Create", valid);
        if confirm {
            self.create_model(&rel_path);
            return true;
        }
        cancel
    }

    fn create_model(&mut self, rel_path: &str) {
        let Some(pack) = &self.pack else { return };
        let mut model = ModelFile::default();
        model
            .textures
            .insert("0".into(), "minecraft:block/stone".into());
        model.textures.insert("particle".into(), "#0".into());
        model.elements = Some(vec![Element::cube([0.0; 3], [16.0; 3], "#0")]);
        match pack.write(rel_path, model.to_json().as_bytes()) {
            Ok(()) => {
                self.rescan();
                self.open_asset(rel_path);
            }
            Err(e) => self.error(format!("Could not create model:\n{e}")),
        }
    }

    fn resize_dialog(&mut self, ui: &mut egui::Ui, width: &mut u32, height: &mut u32) -> bool {
        header(ui, icons::RESIZE, "Resize Canvas");
        form(ui, "resize", |ui| size_row(ui, width, height));
        ui.add_space(SPACE_S);
        ui.weak("The top-left corner stays in place; pixels outside the new size are cropped.");
        let (confirm, cancel) = footer(ui, "Resize", true);
        if confirm {
            if let Some(editor) = texture_mut(&mut self.editors, self.active) {
                editor.doc.resize_canvas(*width, *height);
                editor.fit_pending = true;
            }
            return true;
        }
        cancel
    }

    fn confirm_close_dialog(&mut self, ui: &mut egui::Ui, ids: &[u64], then: &Then) -> bool {
        header(ui, icons::WARNING, "Unsaved Changes");
        let pack_meta = then.includes_pack_meta() && self.pack_dirty();
        let count = ids.len() + usize::from(pack_meta);
        let files = if count == 1 {
            "This file has".to_owned()
        } else {
            format!("These {count} files have")
        };
        ui.label(match then {
            Then::CloseTabs | Then::Quit => format!("{files} unsaved changes:"),
            Then::OpenPack(_) => format!("{files} unsaved changes. Save before switching packs?"),
            Then::Export => format!(
                "{files} unsaved changes. The zip is made from the files on disk, so save first \
                 to include them."
            ),
        });
        ui.add_space(SPACE_S);
        egui::ScrollArea::vertical()
            .max_height(160.0)
            .show(ui, |ui| {
                for id in ids {
                    if let Some(editor) = self.editors.get(id) {
                        ui.monospace(editor.rel_path());
                    }
                }
                if pack_meta {
                    ui.monospace("pack.mcmeta");
                }
            });

        ui.add_space(SPACE_L);
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let (mut save, mut discard, mut cancel) = (false, false, false);
        let (save_label, discard_label, discard_hint) = match then {
            Then::Export => (
                "Save and Export",
                "Export Without Them",
                "Export the files as they are on disk; your edits stay open",
            ),
            _ if count == 1 => ("Save", "Don't Save", "Close and lose the changes"),
            _ => ("Save All", "Don't Save", "Close and lose the changes"),
        };
        egui::Sides::new().show(
            ui,
            |ui| {
                discard = widgets::secondary_button(ui, discard_label)
                    .on_hover_text(discard_hint)
                    .clicked();
            },
            |ui| {
                save = widgets::primary_button(ui, true, save_label).clicked();
                cancel = widgets::secondary_button(ui, "Cancel").clicked();
            },
        );
        save |= enter;

        if !(save || discard) {
            return cancel;
        }
        if save {
            let saved =
                ids.iter().all(|id| self.save_editor(*id)) && (!pack_meta || self.save_pack_meta());
            if !saved {
                return true; // the failure is reported; keep the tabs open
            }
        }
        if *then == Then::Export {
            self.export_zip();
            return true;
        }
        if discard && pack_meta {
            self.revert_pack_meta();
        }
        for id in ids {
            self.close_editor(*id);
        }
        match then {
            Then::Quit => {
                self.allow_quit = true;
                ui.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Then::OpenPack(root) => self.open_pack(root.clone()),
            Then::CloseTabs | Then::Export => {}
        }
        true
    }
}

/// A dialog footer with a single Close button; Enter closes too. Returns true when closed.
pub(crate) fn footer_close(ui: &mut egui::Ui) -> bool {
    ui.add_space(SPACE_L);
    let enter = ui.input(|i| i.key_pressed(Key::Enter));
    let mut close = false;
    egui::Sides::new().show(
        ui,
        |_| {},
        |ui| close = widgets::primary_button(ui, true, "Close").clicked(),
    );
    close || enter
}

fn about_dialog(ui: &mut egui::Ui) -> bool {
    header(ui, icons::PACKAGE, "About OpenPax");
    ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).strong());
    ui.label("An open-source studio for making Minecraft: Java Edition resource packs.");
    ui.add_space(SPACE_S);
    ui.hyperlink_to(
        widgets::icon_label(icons::BOOK, "Project page and documentation"),
        env!("CARGO_PKG_REPOSITORY"),
    );
    ui.add_space(SPACE_S);
    ui.weak("Licensed under MIT or Apache-2.0. Not affiliated with Mojang or Microsoft.");
    footer_close(ui)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_file_names_become_valid_paths() {
        assert_eq!(sanitize_name("My Cool Block (2)"), "my_cool_block__2_");
        assert_eq!(sanitize_name(""), "texture");
        let Dialog::NewTexture { path, source, .. } =
            Dialog::import_texture(PathBuf::from("C:/art/Stone Bricks.PNG"))
        else {
            panic!("expected a texture dialog");
        };
        assert_eq!(path, "block/stone_bricks");
        assert!(source.is_some());
        assert!(ResourceLocation::new("minecraft", &path).is_ok());
    }

    #[test]
    fn new_asset_dialogs_start_in_the_given_folder() {
        let Dialog::NewTexture { path, .. } = Dialog::new_texture_in("ns", "") else {
            panic!();
        };
        assert_eq!(path, "my_texture");
        let Dialog::NewModel { namespace, path } = Dialog::new_model_in("ns", "item/tools") else {
            panic!();
        };
        assert_eq!(
            (namespace.as_str(), path.as_str()),
            ("ns", "item/tools/my_model")
        );
    }
}

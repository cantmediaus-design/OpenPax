//! What the texture-editor commands do.

use eframe::egui;
use openpax_paint::effects::{Effect, OutlinePlace};
use openpax_paint::{Palette, PaletteFormat, ScaleMethod, Scope, SelectMode, Transform};

use crate::app::{App, texture_mut};
use crate::clipboard;
use crate::dialogs::Dialog;
use crate::pixel_cmds::PixelCmd;
use crate::pixel_dialogs::PixelDialog;

impl App {
    pub(crate) fn open_pixel_dialog(&mut self, dialog: PixelDialog) {
        self.open_dialog(Dialog::Pixel(Box::new(dialog)));
    }

    pub(crate) fn run_pixel(&mut self, command: PixelCmd, ctx: &egui::Context) {
        // Commands that do not need an open texture.
        match command {
            PixelCmd::BatchExport => {
                let textures: Vec<String> = self
                    .pack
                    .as_ref()
                    .map(|p| {
                        p.index
                            .entries
                            .iter()
                            .filter(|e| e.kind == openpax_core::AssetKind::Texture)
                            .map(|e| e.rel_path.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                self.open_pixel_dialog(PixelDialog::batch(textures));
                return;
            }
            PixelCmd::ImportAnimation => {
                self.import_animation();
                return;
            }
            PixelCmd::ImportPalette => {
                self.import_palette();
                return;
            }
            PixelCmd::OpenScriptsFolder => {
                crate::user_files::reveal(&crate::user_files::scripts_dir());
                return;
            }
            _ => {}
        }
        let view = &mut self.tools.view;
        match command {
            PixelCmd::TogglePreview => {
                self.preview_open ^= true;
                return;
            }
            PixelCmd::ToggleRulers => {
                view.rulers ^= true;
                return;
            }
            PixelCmd::ToggleTimeline => {
                view.show_timeline ^= true;
                return;
            }
            PixelCmd::ToggleChecks => {
                view.show_issues ^= true;
                return;
            }
            PixelCmd::CycleTiling => {
                view.tiling = view.tiling.next();
                return;
            }
            PixelCmd::ExportPalette => {
                self.export_palette();
                return;
            }
            _ => {}
        }
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        let doc = &mut editor.doc;
        let primary = self.tools.settings.color;
        let mut message: Option<String> = None;
        let mut dialog: Option<PixelDialog> = None;
        match command {
            PixelCmd::ScaleImage => {
                dialog = Some(PixelDialog::scale(doc.width(), doc.height(), false))
            }
            PixelCmd::ScaleSelection => {
                let b = doc.selection_bounds().unwrap_or(doc.bounds());
                dialog = Some(PixelDialog::scale(b.w as u32, b.h as u32, true));
            }
            PixelCmd::CropToSelection => {
                doc.crop_to_selection();
                editor.fit_pending = true;
            }
            PixelCmd::Trim => {
                doc.trim();
                editor.fit_pending = true;
            }
            PixelCmd::RotateCanvasCw => {
                doc.transform_canvas(Transform::Rotate90 { clockwise: true });
                editor.fit_pending = true;
            }
            PixelCmd::RotateCanvasCcw => {
                doc.transform_canvas(Transform::Rotate90 { clockwise: false });
                editor.fit_pending = true;
            }
            PixelCmd::RotateCanvas180 => doc.transform_canvas(Transform::Rotate180),
            PixelCmd::IndexedMode => {
                let on = !doc.extras.indexed;
                let palette = if doc.extras.palette.is_empty() {
                    self.tools.palette.colors.clone()
                } else {
                    doc.extras.palette.clone()
                };
                doc.edit_extras("Indexed colour mode", |e| {
                    e.indexed = on;
                    if e.palette.is_empty() {
                        e.palette = palette;
                    }
                });
                message = Some(if on {
                    "Indexed mode: painting snaps to the document palette".into()
                } else {
                    "Indexed mode off".into()
                });
            }
            PixelCmd::InvertSelection => doc.invert_selection(),
            PixelCmd::SelectLayerAlpha => doc.select_layer_alpha(doc.active(), SelectMode::Replace),
            PixelCmd::SaveSelection => {
                if doc.selection().is_some() {
                    let n = doc.extras.saved_selections.len() + 1;
                    dialog = Some(PixelDialog::SaveSelection {
                        name: format!("Selection {n}"),
                    });
                } else {
                    message = Some("Select something first".into());
                }
            }
            PixelCmd::CopyMerged => {
                let image = doc.copy_merged();
                message = Some(match clipboard::copy_image(&image) {
                    Ok(()) => format!(
                        "Copied {} × {} merged pixels",
                        image.width(),
                        image.height()
                    ),
                    Err(e) => format!("Could not copy: {e}"),
                });
            }
            PixelCmd::PasteInPlace => match clipboard::paste_image() {
                Ok(image) => {
                    let at = doc.selection_bounds().map_or((0, 0), |r| (r.x, r.y));
                    doc.paste_floating(&image, at);
                    self.tools.tool = openpax_paint::ToolKind::Move;
                    message = Some("Pasted. Drag or nudge it into place, Enter applies.".into());
                }
                Err(e) => message = Some(e),
            },
            PixelCmd::FillSelection => doc.fill_selected(primary),
            PixelCmd::NudgeLeft => doc.nudge(-1, 0),
            PixelCmd::NudgeRight => doc.nudge(1, 0),
            PixelCmd::NudgeUp => doc.nudge(0, -1),
            PixelCmd::NudgeDown => doc.nudge(0, 1),
            PixelCmd::NudgeLeftFar => doc.nudge(-10, 0),
            PixelCmd::NudgeRightFar => doc.nudge(10, 0),
            PixelCmd::NudgeUpFar => doc.nudge(0, -10),
            PixelCmd::NudgeDownFar => doc.nudge(0, 10),
            PixelCmd::Commit => doc.commit_floating(),
            PixelCmd::FlipSelectionH => doc.transform_selection(Transform::FlipHorizontal),
            PixelCmd::FlipSelectionV => doc.transform_selection(Transform::FlipVertical),
            PixelCmd::RotateCw => doc.transform_selection(Transform::Rotate90 { clockwise: true }),
            PixelCmd::RotateCcw => {
                doc.transform_selection(Transform::Rotate90 { clockwise: false })
            }
            PixelCmd::Rotate180 => doc.transform_selection(Transform::Rotate180),
            PixelCmd::RotateBy => {
                dialog = Some(PixelDialog::Rotate {
                    degrees: 15.0,
                    smooth: true,
                })
            }
            PixelCmd::NewGroup => doc.add_group(),
            PixelCmd::GroupLayer => doc.group_active(),
            PixelCmd::Ungroup => doc.ungroup(),
            PixelCmd::MergeVisible => doc.merge_visible(),
            PixelCmd::FlattenLayers => doc.flatten_layers(),
            PixelCmd::LayerViaCopy => doc.layer_from_selection(false),
            PixelCmd::LayerViaCut => doc.layer_from_selection(true),
            PixelCmd::AddReference => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Reference image")
                    .add_filter(
                        "Images",
                        &["png", "gif", "webp", "jpg", "jpeg", "bmp", "tga"],
                    )
                    .pick_file()
                {
                    match std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|b| openpax_paint::decode_image(&b).map_err(|e| e.to_string()))
                    {
                        Ok(image) => {
                            let name = path
                                .file_stem()
                                .map_or("Reference".into(), |s| s.to_string_lossy().into_owned());
                            doc.add_reference_layer(&name, &image);
                        }
                        Err(e) => message = Some(format!("Could not open the image: {e}")),
                    }
                }
            }
            PixelCmd::ToggleLayerLock => {
                let i = doc.active();
                let locked = !doc.layers()[i].locked;
                doc.edit_layer(
                    i,
                    if locked { "Lock layer" } else { "Unlock layer" },
                    None,
                    |l| l.locked = locked,
                );
            }
            PixelCmd::ToggleAlphaLock => {
                let i = doc.active();
                let on = !doc.layers()[i].alpha_lock;
                doc.edit_layer(i, "Lock transparency", None, |l| l.alpha_lock = on);
            }
            PixelCmd::NewTilemap => dialog = Some(PixelDialog::new_tilemap(doc)),
            PixelCmd::NewFrame => doc.add_frame(false),
            PixelCmd::DuplicateFrame => doc.add_frame(true),
            PixelCmd::NewLinkedFrame => doc.add_linked_frame(),
            PixelCmd::DeleteFrame => doc.remove_frame(),
            PixelCmd::PrevFrame => {
                let n = doc.frame_count();
                doc.set_current_frame((doc.current_frame() + n - 1) % n);
            }
            PixelCmd::NextFrame => {
                let n = doc.frame_count();
                doc.set_current_frame((doc.current_frame() + 1) % n);
            }
            PixelCmd::FirstFrame => doc.set_current_frame(0),
            PixelCmd::LastFrame => doc.set_current_frame(doc.frame_count() - 1),
            PixelCmd::PlayAnimation => {
                // Enter first finishes what is in progress.
                if !editor.poly_points.is_empty() {
                    editor.finish_polygon(&self.tools);
                } else if editor.doc.floating().is_some() {
                    editor.doc.commit_floating();
                } else {
                    editor.toggle_play(ctx, &self.tools.view);
                }
            }
            PixelCmd::ToggleOnionSkin => editor.onion_skin ^= true,
            PixelCmd::AddTag => {
                let (from, to) = doc
                    .frame_range()
                    .unwrap_or((doc.current_frame(), doc.current_frame()));
                let n = doc.tags().len() + 1;
                doc.add_tag(openpax_paint::Tag {
                    name: format!("Tag {n}"),
                    from,
                    to,
                    direction: openpax_paint::PlayDirection::Forward,
                    color: [230, 90, 90, 255],
                });
            }
            PixelCmd::LinkCels => {
                if let Some((a, b)) = doc.frame_range() {
                    doc.link_cels(doc.active(), a, b);
                } else {
                    message =
                        Some("Shift-click frames in the timeline to choose which to link".into());
                }
            }
            PixelCmd::UnlinkCel => doc.unlink_cel(doc.active(), doc.current_frame()),
            PixelCmd::Filters => dialog = Some(PixelDialog::filter(Effect::presets()[2].clone())),
            PixelCmd::Outline => {
                dialog = Some(PixelDialog::filter(Effect::Outline {
                    color: primary,
                    place: OutlinePlace::Outside,
                    diagonal: false,
                }));
            }
            PixelCmd::ReplaceColor => {
                dialog = Some(PixelDialog::ReplaceColor {
                    from: primary,
                    to: self.tools.secondary,
                    tolerance: 0,
                    scope: Scope::Layer,
                });
            }
            PixelCmd::PaletteSwap => {
                let from = Palette::from_bitmap(&doc.flatten(), 64);
                dialog = Some(PixelDialog::PaletteSwap {
                    to: from.colors.clone(),
                    from: from.colors,
                    scope: Scope::All,
                });
            }
            PixelCmd::ReduceColors => {
                dialog = Some(PixelDialog::Reduce {
                    colors: 8,
                    use_palette: false,
                    dither: openpax_paint::quantize::Dither::None,
                    scope: Scope::Cel,
                });
            }
            PixelCmd::Invert => doc.apply_filter("Invert", Scope::Cel, |b| Effect::Invert.apply(b)),
            PixelCmd::Desaturate => {
                doc.apply_filter("Desaturate", Scope::Cel, |b| Effect::Desaturate.apply(b))
            }
            PixelCmd::ClearGuides => doc.edit_extras("Clear guides", |e| e.guides.clear()),
            PixelCmd::ExportImage => {
                let name = editor
                    .rel_path
                    .rsplit('/')
                    .next()
                    .unwrap_or("texture")
                    .trim_end_matches(".png")
                    .to_owned();
                dialog = Some(PixelDialog::export(name, doc));
            }
            PixelCmd::RunScript => dialog = Some(PixelDialog::scripts()),
            _ => {}
        }
        if let Some(reason) = editor.notice.take() {
            message = Some(reason.to_owned());
        }
        if let Some(d) = dialog {
            self.open_pixel_dialog(d);
        }
        if let Some(m) = message {
            self.info(m);
        }
    }

    /// Opens a GIF, APNG, WebP or a set of numbered PNGs as a new animated texture.
    fn import_animation(&mut self) {
        let Some(paths) = rfd::FileDialog::new()
            .set_title("Import animation or frames")
            .add_filter("Animations and images", &["gif", "png", "apng", "webp"])
            .pick_files()
        else {
            return;
        };
        let mut names: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        openpax_paint::export::sort_sequence(&mut names);
        let mut frames = Vec::new();
        for name in &names {
            match std::fs::read(name)
                .map_err(|e| e.to_string())
                .and_then(|b| {
                    openpax_paint::export::decode_animation(&b).map_err(|e| e.to_string())
                }) {
                Ok(f) => frames.extend(f),
                Err(e) => {
                    self.error(format!("Could not read {name}:\n{e}"));
                    return;
                }
            }
        }
        let stem =
            std::path::Path::new(&names[0])
                .file_stem()
                .map_or("animation".to_owned(), |s| {
                    s.to_string_lossy()
                        .trim_end_matches(|c: char| c.is_ascii_digit() || c == '_' || c == '-')
                        .to_lowercase()
                });
        self.open_pixel_dialog(PixelDialog::ImportAnimation {
            frames,
            path: format!(
                "block/{}",
                if stem.is_empty() { "animation" } else { &stem }
            ),
        });
    }

    fn import_palette(&mut self) {
        let extensions: Vec<&str> = PaletteFormat::ALL.iter().map(|f| f.extension()).collect();
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import palette")
            .add_filter("Palettes", &extensions)
            .pick_file()
        else {
            return;
        };
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let result = PaletteFormat::from_extension(&ext)
            .ok_or_else(|| format!("Unknown palette format .{ext}"))
            .and_then(|f| {
                std::fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|b| Palette::decode(&b, f))
            });
        match result {
            Ok(mut palette) => {
                if palette.name == "Imported" {
                    palette.name = path
                        .file_stem()
                        .map_or("Imported".into(), |s| s.to_string_lossy().into_owned());
                }
                let n = palette.colors.len();
                self.tools.palette = palette;
                self.success(format!("Imported {n} colours"));
            }
            Err(e) => self.error(format!("Could not import the palette:\n{e}")),
        }
    }

    fn export_palette(&mut self) {
        let extensions: Vec<&str> = PaletteFormat::ALL.iter().map(|f| f.extension()).collect();
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export palette")
            .set_file_name(format!("{}.gpl", self.tools.palette.name))
            .add_filter("Palettes", &extensions)
            .save_file()
        else {
            return;
        };
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let format = PaletteFormat::from_extension(&ext).unwrap_or(PaletteFormat::Gpl);
        match std::fs::write(&path, self.tools.palette.encode(format)) {
            Ok(()) => self.success(format!("Saved {}", path.display())),
            Err(e) => self.error(format!("Could not save the palette:\n{e}")),
        }
    }
}

/// Scale method names for combo boxes.
pub fn scale_method_ui(ui: &mut egui::Ui, method: &mut ScaleMethod) {
    egui::ComboBox::from_id_salt("scale_method")
        .selected_text(method.name())
        .show_ui(ui, |ui| {
            for m in ScaleMethod::ALL {
                ui.selectable_value(method, m, m.name());
            }
        });
}

impl App {
    /// File-menu entries of the pixel editor.
    pub(crate) fn pixel_file_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::Command;
        ui.separator();
        self.menu_item(ui, Command::Pixel(PixelCmd::ImportAnimation));
        self.menu_item(ui, Command::Pixel(PixelCmd::ExportImage));
        self.menu_item(ui, Command::Pixel(PixelCmd::BatchExport));
        ui.menu_button(
            crate::widgets::icon_label(crate::icons::PALETTE, "Palettes"),
            |ui| {
                self.menu_item(ui, Command::Pixel(PixelCmd::ImportPalette));
                self.menu_item(ui, Command::Pixel(PixelCmd::ExportPalette));
            },
        );
        ui.menu_button(
            crate::widgets::icon_label(crate::icons::SCRIPT, "Scripts"),
            |ui| {
                self.menu_item(ui, Command::Pixel(PixelCmd::RunScript));
                self.menu_item(ui, Command::Pixel(PixelCmd::OpenScriptsFolder));
                let scripts = openpax_script::list_scripts(&crate::user_files::scripts_dir());
                if !scripts.is_empty() {
                    ui.separator();
                }
                for s in scripts {
                    let enabled = self.active_kind() == crate::commands::Active::Texture;
                    let button = ui.add_enabled(
                        enabled,
                        egui::Button::new(crate::widgets::icon_label(crate::icons::PLAY, &s.name)),
                    );
                    let clicked = button.clicked();
                    if !s.description.is_empty() {
                        button.on_hover_text(&s.description);
                    }
                    if clicked {
                        self.run_script_file(&s);
                    }
                }
            },
        );
    }

    fn run_script_file(&mut self, script: &openpax_script::ScriptFile) {
        let source = match std::fs::read_to_string(&script.path) {
            Ok(s) => s,
            Err(e) => {
                self.error(format!("Could not read {}:\n{e}", script.path.display()));
                return;
            }
        };
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        let base = script
            .path
            .parent()
            .map(std::path::Path::to_owned)
            .unwrap_or_default();
        let out = openpax_script::run(&mut editor.doc, &source, &script.name, &base);
        let printed = out.printed.join("\n");
        match out.error {
            Some(e) => self.error(format!("{} failed:\n{e}", script.name)),
            None if printed.is_empty() => self.success(format!("Ran {}", script.name)),
            None => self.info(printed),
        }
    }

    /// The Image, Layer, Select, Frame and Filter menus, shown while a texture is open.
    pub(crate) fn pixel_menus(&mut self, ui: &mut egui::Ui) {
        use crate::commands::{Active, Command};
        if self.active_kind() != Active::Texture {
            return;
        }
        let indexed =
            texture_mut(&mut self.editors, self.active).is_some_and(|e| e.doc.extras.indexed);
        ui.menu_button("Image", |ui| {
            self.menu_item(ui, Command::ResizeCanvas);
            for c in PixelCmd::in_category("Image") {
                if c == PixelCmd::IndexedMode {
                    ui.separator();
                    self.menu_toggle(ui, Command::Pixel(c), indexed);
                } else {
                    self.menu_item(ui, Command::Pixel(c));
                }
            }
            ui.separator();
            self.menu_item(ui, Command::FlipHorizontal);
            self.menu_item(ui, Command::FlipVertical);
        });
        ui.menu_button("Select", |ui| {
            self.menu_item(ui, Command::SelectAll);
            self.menu_item(ui, Command::Deselect);
            for c in PixelCmd::in_category("Select") {
                if matches!(c, PixelCmd::FlipSelectionH | PixelCmd::CopyMerged) {
                    ui.separator();
                }
                self.menu_item(ui, Command::Pixel(c));
            }
            let saved: Vec<String> = texture_mut(&mut self.editors, self.active)
                .map(|e| {
                    e.doc
                        .extras
                        .saved_selections
                        .iter()
                        .map(|s| s.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            if !saved.is_empty() {
                ui.menu_button("Load Selection", |ui| {
                    for name in saved {
                        if ui.button(&name).clicked()
                            && let Some(e) = texture_mut(&mut self.editors, self.active)
                        {
                            e.doc.load_selection(&name, SelectMode::Replace);
                        }
                    }
                });
            }
        });
        ui.menu_button("Layer", |ui| {
            for c in PixelCmd::in_category("Layer") {
                self.menu_item(ui, Command::Pixel(c));
            }
        });
        ui.menu_button("Frame", |ui| {
            for c in PixelCmd::in_category("Frame") {
                self.menu_item(ui, Command::Pixel(c));
            }
        });
        ui.menu_button("Filter", |ui| {
            for c in PixelCmd::in_category("Filter") {
                self.menu_item(ui, Command::Pixel(c));
            }
        });
        ui.menu_button("Canvas", |ui| {
            let (rulers, timeline, checks) = (
                self.tools.view.rulers,
                self.tools.view.show_timeline,
                self.tools.view.show_issues,
            );
            self.menu_toggle(ui, Command::Pixel(PixelCmd::ToggleRulers), rulers);
            self.menu_toggle(ui, Command::Pixel(PixelCmd::ToggleTimeline), timeline);
            self.menu_toggle(ui, Command::Pixel(PixelCmd::ToggleChecks), checks);
            let preview = self.preview_open;
            self.menu_toggle(ui, Command::Pixel(PixelCmd::TogglePreview), preview);
            self.menu_item(ui, Command::Pixel(PixelCmd::ClearGuides));
            ui.separator();
            crate::pixel_panels::view_settings_ui(ui, &mut self.tools.view);
        });
    }
}

//! The pack icon (`pack.png`): preview, edit, and make one from a texture or an image file.

use eframe::egui::{self, RichText, vec2};
use openpax_paint::Bitmap;

use crate::app::{App, PACK_ICON, texture_mut};
use crate::theme::SPACE_S;
use crate::{icons, widgets};

/// Side Minecraft shows the icon at, more or less; small textures are scaled up to about this.
const ICON_SIDE: u32 = 128;
const PREVIEW: f32 = 64.0;

/// Turns a texture into a pack icon: the top square (the first frame of an animation strip),
/// scaled up by a whole factor so pixel art stays crisp.
pub fn icon_from(texture: &Bitmap) -> Bitmap {
    let side = texture.width().min(texture.height()).max(1);
    let factor = (ICON_SIDE / side).max(1);
    let out_side = side * factor;
    let mut out = Bitmap::new(out_side, out_side);
    for y in 0..out_side {
        for x in 0..out_side {
            if let Some(color) = texture.get((x / factor) as i32, (y / factor) as i32) {
                out.set(x as i32, y as i32, color);
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconAction {
    Edit,
    FromTexture,
    Import,
    CreateBlank,
}

impl App {
    /// The pack icon part of the Pack section. Returns what was clicked.
    pub(crate) fn pack_icon_ui(&mut self, ui: &mut egui::Ui) -> Option<IconAction> {
        let pack = self.pack.as_ref()?;
        let exists = pack.exists(PACK_ICON);
        let has_texture =
            texture_mut(&mut self.editors, self.active).is_some_and(|e| e.rel_path != PACK_ICON);
        let mut action = None;
        widgets::caption(ui, "ICON");
        ui.horizontal(|ui| {
            let (rect, response) =
                ui.allocate_exact_size(vec2(PREVIEW, PREVIEW), egui::Sense::click());
            let tokens = crate::theme::of(ui);
            ui.painter()
                .rect_filled(rect, crate::theme::RADIUS, tokens.toolbar);
            if exists {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                let Some(pack) = &self.pack else { return };
                self.thumbs
                    .show(&mut child, pack, PACK_ICON, PREVIEW, icons::IMAGE);
                if response.on_hover_text("Click to edit pack.png").clicked() {
                    action = Some(IconAction::Edit);
                }
            } else {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::IMAGE,
                    egui::FontId::proportional(28.0),
                    ui.visuals().weak_text_color(),
                );
                response.on_hover_text("This pack has no pack.png yet");
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = SPACE_S;
                if exists
                    && ui
                        .button(widgets::icon_label(icons::PENCIL, "Edit"))
                        .clicked()
                {
                    action = Some(IconAction::Edit);
                }
                if ui
                    .add_enabled(
                        has_texture,
                        egui::Button::new(widgets::icon_label(icons::IMAGE, "Use Current Texture")),
                    )
                    .on_hover_text("Make the icon from the texture you are editing")
                    .on_disabled_hover_text("Open a texture first")
                    .clicked()
                {
                    action = Some(IconAction::FromTexture);
                }
                if ui
                    .button(widgets::icon_label(icons::FOLDER_OPEN, "Import Image…"))
                    .clicked()
                {
                    action = Some(IconAction::Import);
                }
                if !exists
                    && ui
                        .button(widgets::icon_label(icons::PLUS, "Create Blank"))
                        .clicked()
                {
                    action = Some(IconAction::CreateBlank);
                }
            });
        });
        if !exists {
            ui.label(RichText::new("Minecraft shows a default icon until you add one.").weak());
        }
        action
    }

    pub(crate) fn run_icon_action(&mut self, action: IconAction) {
        let image = match action {
            IconAction::Edit => {
                self.open_asset(PACK_ICON);
                return;
            }
            IconAction::CreateBlank => Bitmap::new(ICON_SIDE / 2, ICON_SIDE / 2),
            IconAction::FromTexture => {
                let Some(editor) = texture_mut(&mut self.editors, self.active) else {
                    return;
                };
                icon_from(&editor.doc.flatten())
            }
            IconAction::Import => {
                let Some(path) = rfd::FileDialog::new()
                    .set_title("Choose an image for the pack icon")
                    .add_filter("PNG image", &["png"])
                    .pick_file()
                else {
                    return;
                };
                match std::fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| openpax_paint::decode_png(&bytes).map_err(|e| e.to_string()))
                {
                    Ok(bitmap) => bitmap,
                    Err(e) => {
                        self.error(format!("Could not read {}:\n{e}", path.display()));
                        return;
                    }
                }
            }
        };
        self.replace_pack_icon(&image);
    }

    /// Writes a new pack.png. An existing one goes to the trash first, so it can be restored.
    fn replace_pack_icon(&mut self, image: &Bitmap) {
        let open = self
            .editors
            .iter()
            .find(|(_, e)| e.rel_path() == PACK_ICON)
            .map(|(id, e)| (*id, e.dirty()));
        if let Some((_, true)) = open {
            self.warn("pack.png is open with unsaved changes. Save or close it first.");
            return;
        }
        let Some(pack) = &self.pack else { return };
        let png = match openpax_paint::encode_png(image) {
            Ok(png) => png,
            Err(e) => {
                self.error(format!("Could not encode the icon:\n{e}"));
                return;
            }
        };
        let mut replaced = false;
        if pack.exists(PACK_ICON) {
            match pack.trash_asset(PACK_ICON) {
                Ok(folder) => {
                    self.trashed.push((folder, PACK_ICON.to_owned()));
                    replaced = true;
                }
                Err(e) => {
                    self.error(format!("Could not replace pack.png:\n{e}"));
                    return;
                }
            }
        }
        let Some(pack) = &self.pack else { return };
        if let Err(e) = pack.write(PACK_ICON, &png) {
            self.error(format!("Could not write pack.png:\n{e}"));
            return;
        }
        if let Some((id, _)) = open {
            self.close_editor(id);
        }
        self.thumbs.invalidate(PACK_ICON);
        self.open_asset(PACK_ICON);
        self.success(if replaced {
            "Replaced pack.png. File → Restore Deleted File brings the old one back."
        } else {
            "Created pack.png"
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_textures_scale_up_by_a_whole_factor() {
        let mut texture = Bitmap::new(16, 32); // a two-frame animation strip
        texture.set(0, 0, [255, 0, 0, 255]);
        texture.set(0, 20, [0, 255, 0, 255]); // second frame, left out
        let icon = icon_from(&texture);
        assert_eq!((icon.width(), icon.height()), (128, 128));
        assert_eq!(icon.get(7, 7), Some([255, 0, 0, 255]));
        assert_eq!(icon.get(8, 8), Some([0, 0, 0, 0]));
    }

    #[test]
    fn large_images_keep_their_size() {
        let icon = icon_from(&Bitmap::new(300, 200));
        assert_eq!((icon.width(), icon.height()), (200, 200));
    }
}

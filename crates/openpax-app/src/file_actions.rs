//! Renaming, duplicating, deleting and restoring assets from the explorer and the File menu.

use eframe::egui::{self, RichText};
use openpax_core::{AssetEntry, ResourceLocation};

use crate::app::App;
use crate::dialogs::{self, Dialog};
use crate::icons;
use crate::theme::SPACE_S;

/// `path` without its extension, and the extension with its dot (`""` if there is none).
fn split_extension(path: &str) -> (&str, &str) {
    match path.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.contains('/') => (stem, &path[stem.len()..]),
        _ => (path, ""),
    }
}

/// The folder an asset's path is relative to: `assets/<namespace>/<category>/`.
fn base_folder(entry: &AssetEntry) -> String {
    if entry.category.is_empty() {
        format!("assets/{}/", entry.namespace)
    } else {
        format!("assets/{}/{}/", entry.namespace, entry.category)
    }
}

/// A free path next to `rel_path` for a copy: `name_copy.png`, `name_copy2.png`, ...
pub fn copy_path(rel_path: &str, exists: impl Fn(&str) -> bool) -> String {
    let (stem, ext) = split_extension(rel_path);
    (1..)
        .map(|n| {
            if n == 1 {
                format!("{stem}_copy{ext}")
            } else {
                format!("{stem}_copy{n}{ext}")
            }
        })
        .find(|candidate| !exists(candidate))
        .expect("some name is free")
}

impl App {
    fn entry(&self, rel_path: &str) -> Option<&AssetEntry> {
        self.pack
            .as_ref()?
            .index
            .entries
            .iter()
            .find(|e| e.rel_path == rel_path)
    }

    /// The tab showing `rel_path`, if it is open.
    fn editor_for(&self, rel_path: &str) -> Option<u64> {
        self.editors
            .iter()
            .find(|(_, e)| e.rel_path() == rel_path)
            .map(|(id, _)| *id)
    }

    /// The asset in the focused tab.
    pub(crate) fn active_asset(&self) -> Option<String> {
        let id = self.active?;
        Some(self.editors.get(&id)?.rel_path().to_owned())
    }

    pub(crate) fn start_rename(&mut self, rel_path: &str) {
        let Some(entry) = self.entry(rel_path) else {
            return;
        };
        if self
            .editor_for(rel_path)
            .and_then(|id| self.editors.get(&id))
            .is_some_and(crate::editor::Editor::dirty)
        {
            self.warn(format!("Save {} before renaming it.", entry.file_name()));
            return;
        }
        let (stem, _) = split_extension(&entry.path);
        let dialog = Dialog::RenameAsset {
            from: rel_path.to_owned(),
            path: stem.to_owned(),
        };
        self.open_dialog(dialog);
    }

    pub(crate) fn rename_dialog(
        &mut self,
        ui: &mut egui::Ui,
        fresh: bool,
        from: &str,
        path: &mut String,
    ) -> bool {
        let Some(entry) = self.entry(from).cloned() else {
            return true; // the file went away meanwhile
        };
        dialogs::header(ui, icons::RENAME, "Rename or Move");
        let base = base_folder(&entry);
        let (_, ext) = split_extension(&entry.path);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.label(RichText::new(&base).monospace().weak());
            let width = (ui.available_width() - 48.0).max(120.0);
            let field = ui.add(egui::TextEdit::singleline(path).desired_width(width));
            if fresh {
                field.request_focus();
            }
            ui.label(RichText::new(ext).monospace().weak());
        });
        ui.add_space(SPACE_S);
        ui.weak("Change the name, or the folders before it to move the file.");

        let target = format!("{base}{}{ext}", path.trim());
        let valid = if ResourceLocation::new(&entry.namespace, path.trim()).is_err() {
            dialogs::problem(ui, "Use lowercase letters, digits, _ - . and / only.");
            false
        } else if target == from {
            false
        } else if self.pack.as_ref().is_some_and(|p| p.exists(&target)) {
            dialogs::problem(ui, "A file with this name already exists.");
            false
        } else {
            true
        };
        if entry.kind == openpax_core::AssetKind::Texture || entry.kind.is_text() {
            ui.add_space(SPACE_S);
            ui.weak("Models and other files that refer to the old name are not updated.");
        }
        let (confirm, cancel) = dialogs::footer(ui, "Rename", valid);
        if confirm {
            self.rename_asset(from, &target);
            return true;
        }
        cancel
    }

    fn rename_asset(&mut self, from: &str, to: &str) {
        let Some(pack) = &self.pack else { return };
        let open = self.editor_for(from);
        if let Err(e) = pack.move_asset(from, to) {
            self.error(format!("Could not rename {from}:\n{e}"));
            return;
        }
        if let Some(id) = open {
            self.close_editor(id);
        }
        self.thumbs.invalidate(from);
        self.rescan();
        if open.is_some() {
            self.open_asset(to);
        }
        self.success(format!("Renamed to {to}"));
    }

    pub(crate) fn duplicate_asset(&mut self, rel_path: &str) {
        let Some(pack) = &self.pack else { return };
        let target = copy_path(rel_path, |p| pack.exists(p));
        if let Err(e) = pack.copy_asset(rel_path, &target) {
            self.error(format!("Could not duplicate {rel_path}:\n{e}"));
            return;
        }
        self.rescan();
        self.open_asset(&target);
        self.success(format!("Created {target}. Press F2 to rename it."));
    }

    pub(crate) fn start_delete(&mut self, rel_path: &str) {
        if self.entry(rel_path).is_some() {
            self.open_dialog(Dialog::DeleteAsset {
                rel_path: rel_path.to_owned(),
            });
        }
    }

    pub(crate) fn delete_dialog(&mut self, ui: &mut egui::Ui, rel_path: &str) -> bool {
        dialogs::header(ui, icons::TRASH, "Delete File");
        ui.label("Delete this file, along with its .mcmeta and layers?");
        ui.add_space(SPACE_S);
        ui.label(RichText::new(rel_path).monospace());
        let open = self.editor_for(rel_path);
        if open
            .and_then(|id| self.editors.get(&id))
            .is_some_and(crate::editor::Editor::dirty)
        {
            dialogs::problem(ui, "It is open with unsaved changes, which will be lost.");
        }
        ui.add_space(SPACE_S);
        ui.weak(
            "It moves to the pack's .openpax/trash folder. File → Restore Deleted File \
             brings it back.",
        );
        let (confirm, cancel) = dialogs::footer(ui, "Delete", true);
        if confirm {
            self.delete_asset(rel_path);
            return true;
        }
        cancel
    }

    fn delete_asset(&mut self, rel_path: &str) {
        let Some(pack) = &self.pack else { return };
        match pack.trash_asset(rel_path) {
            Ok(folder) => {
                if let Some(id) = self.editor_for(rel_path) {
                    self.close_editor(id);
                }
                self.trashed.push((folder, rel_path.to_owned()));
                self.thumbs.invalidate(rel_path);
                self.rescan();
                self.success(format!(
                    "Deleted {}. File → Restore Deleted File brings it back.",
                    rel_path.rsplit('/').next().unwrap_or(rel_path)
                ));
            }
            Err(e) => self.error(format!("Could not delete {rel_path}:\n{e}")),
        }
    }

    pub(crate) fn restore_deleted(&mut self) {
        let Some((folder, rel_path)) = self.trashed.pop() else {
            return;
        };
        let Some(pack) = &self.pack else { return };
        match pack.restore_trashed(&folder) {
            Ok(_) => {
                self.rescan();
                self.open_asset(&rel_path);
                self.success(format!("Restored {rel_path}"));
            }
            Err(e) => {
                self.trashed.push((folder, rel_path.clone()));
                self.error(format!("Could not restore {rel_path}:\n{e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_split_off_the_file_name_only() {
        assert_eq!(split_extension("block/stone.png"), ("block/stone", ".png"));
        assert_eq!(split_extension("lang/en_us.json"), ("lang/en_us", ".json"));
        assert_eq!(
            split_extension("folder.v2/readme"),
            ("folder.v2/readme", "")
        );
        assert_eq!(split_extension(".hidden"), (".hidden", ""));
    }

    #[test]
    fn copies_get_the_first_free_name() {
        let taken = ["a/stone_copy.png", "a/stone_copy2.png"];
        assert_eq!(
            copy_path("a/stone.png", |p| taken.contains(&p)),
            "a/stone_copy3.png"
        );
        assert_eq!(copy_path("a/stone.png", |_| false), "a/stone_copy.png");
    }
}

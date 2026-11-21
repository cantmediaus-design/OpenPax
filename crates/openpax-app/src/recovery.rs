//! Crash recovery: unsaved edits are copied into the pack's `.openpax/recovery` folder every
//! few seconds, and offered back the next time the pack is opened if OpenPax did not close
//! normally. The real files are never touched until the user saves.

use std::collections::HashSet;
use std::path::Path;

use eframe::egui::{self, Key};

use crate::app::App;
use crate::dialogs;
use crate::theme::{SPACE_L, SPACE_S};
use crate::{icons, widgets};

pub const RECOVERY_DIR: &str = ".openpax/recovery";
/// Seconds between recovery snapshots while there are unsaved changes.
pub const INTERVAL: f64 = 20.0;
/// Suffixes of the extra files saved next to a recovered asset.
const SUFFIXES: [&str; 3] = [".layers.json", ".layers.png", ".mcmeta"];

/// Snapshot bookkeeping for the open pack.
#[derive(Default)]
pub struct Recovery {
    /// `egui` time of the last snapshot.
    last: Option<f64>,
    /// Assets that have files in the recovery folder.
    written: HashSet<String>,
    /// Recovered work the user has not decided about yet (it chose Not Now); kept on exit.
    pending: bool,
}

fn recovery_path(rel_path: &str, suffix: &str) -> String {
    format!("{RECOVERY_DIR}/{rel_path}{suffix}")
}

/// Assets with recovered files under `root` (the recovery folder), as pack-relative paths.
pub fn recovered_assets(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect(root, "", &mut files);
    let mut assets: Vec<String> = files
        .into_iter()
        .filter(|f| !SUFFIXES.iter().any(|s| f.ends_with(s)))
        .collect();
    assets.sort();
    assets
}

fn collect(dir: &Path, rel: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if entry.path().is_dir() {
            collect(&entry.path(), &child, out);
        } else {
            out.push(child);
        }
    }
}

impl App {
    /// Writes snapshots of unsaved editors and removes those of editors that are now saved or
    /// closed. Runs every [`INTERVAL`] seconds; call every frame.
    pub(crate) fn autosave_tick(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let last = *self.recovery.last.get_or_insert(now);
        let dirty = !self.dirty_ids().is_empty();
        if now - last < INTERVAL {
            if dirty || !self.recovery.written.is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                    INTERVAL - (now - last),
                ));
            }
            return;
        }
        self.recovery.last = Some(now);
        self.write_recovery();
    }

    fn write_recovery(&mut self) {
        let Some(pack) = &self.pack else { return };
        let mut written = HashSet::new();
        for id in self.dirty_ids() {
            let Some(editor) = self.editors.get(&id) else {
                continue;
            };
            let rel_path = editor.rel_path().to_owned();
            let Ok(files) = editor.snapshot() else {
                continue;
            };
            // Old extra files (say, layers since merged away) must not survive the new snapshot.
            for suffix in SUFFIXES {
                let _ = pack.remove(&recovery_path(&rel_path, suffix));
            }
            let ok = files.iter().all(|(suffix, bytes)| {
                pack.write(&recovery_path(&rel_path, suffix), bytes).is_ok()
            });
            if ok {
                written.insert(rel_path);
            } else {
                log::warn!("could not write recovery files for {rel_path}");
            }
        }
        for stale in self.recovery.written.difference(&written) {
            remove_recovery(pack, stale);
        }
        self.recovery.written = written;
    }

    /// Removes the recovery files after a normal exit or pack switch, unless they hold
    /// recovered work the user chose to decide about later.
    pub(crate) fn finish_recovery(&mut self) {
        if !self.recovery.pending {
            self.clear_recovery();
        } else if let Some(pack) = &self.pack {
            // Keep the old recovered work, but not snapshots of this session's edits.
            for rel_path in self.recovery.written.drain() {
                remove_recovery(pack, &rel_path);
            }
        }
    }

    /// Removes every recovery file of the open pack.
    pub(crate) fn clear_recovery(&mut self) {
        if let Some(pack) = &self.pack {
            let _ = std::fs::remove_dir_all(pack.abs(RECOVERY_DIR));
        }
        self.recovery = Recovery::default();
    }

    /// Asks about recovered work if the pack has any. Call right after opening a pack.
    pub(crate) fn check_recovery(&mut self) {
        let Some(pack) = &self.pack else { return };
        let assets: Vec<String> = recovered_assets(&pack.abs(RECOVERY_DIR))
            .into_iter()
            .filter(|a| pack.exists(a))
            .collect();
        self.recovery = Recovery::default();
        if assets.is_empty() {
            let _ = std::fs::remove_dir_all(pack.abs(RECOVERY_DIR));
        } else {
            self.recovery.pending = true;
            self.open_dialog(dialogs::Dialog::Recover { assets });
        }
    }

    pub(crate) fn recover_dialog(&mut self, ui: &mut egui::Ui, assets: &[String]) -> bool {
        dialogs::header(ui, icons::WARNING, "Recover Unsaved Work");
        let files = if assets.len() == 1 {
            "1 file".to_owned()
        } else {
            format!("{} files", assets.len())
        };
        ui.label(format!(
            "OpenPax did not close normally last time. It kept unsaved changes to {files}:"
        ));
        ui.add_space(SPACE_S);
        egui::ScrollArea::vertical()
            .max_height(160.0)
            .show(ui, |ui| {
                for asset in assets {
                    ui.monospace(asset);
                }
            });
        ui.add_space(SPACE_S);
        ui.weak("Recovered files open with their changes unsaved, so you can check them first.");

        ui.add_space(SPACE_L);
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let (mut recover, mut discard, mut later) = (false, false, false);
        egui::Sides::new().show(
            ui,
            |ui| {
                discard = widgets::secondary_button(ui, "Discard Them")
                    .on_hover_text("Delete the recovered changes for good")
                    .clicked();
            },
            |ui| {
                recover = widgets::primary_button(ui, true, "Recover").clicked();
                later = widgets::secondary_button(ui, "Not Now")
                    .on_hover_text("Ask again the next time this pack is opened")
                    .clicked();
            },
        );
        if recover || enter {
            self.recovery.pending = false;
            self.recover(assets);
            return true;
        }
        if discard {
            self.clear_recovery();
            self.info("Discarded the recovered changes");
            return true;
        }
        later
    }

    fn recover(&mut self, assets: &[String]) {
        let mut restored = 0;
        for asset in assets {
            self.open_asset(asset);
            let Some(pack) = &self.pack else { return };
            let read = |suffix: &str| pack.read(&recovery_path(asset, suffix)).ok();
            let Some(editor) = self
                .editors
                .values_mut()
                .find(|e| e.rel_path() == asset.as_str())
            else {
                continue;
            };
            match editor.restore_snapshot(read) {
                Ok(()) => {
                    restored += 1;
                    self.recovery.written.insert(asset.clone());
                }
                Err(e) => self.error(format!("Could not recover {asset}:\n{e}")),
            }
        }
        if restored > 0 {
            self.success(format!(
                "Recovered {restored} file{}. Save to keep the changes.",
                if restored == 1 { "" } else { "s" }
            ));
        }
    }
}

fn remove_recovery(pack: &openpax_core::Pack, rel_path: &str) {
    let _ = pack.remove(&recovery_path(rel_path, ""));
    for suffix in SUFFIXES {
        let _ = pack.remove(&recovery_path(rel_path, suffix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_files_are_not_listed_as_assets() {
        let dir = tempfile::tempdir().unwrap();
        let write = |rel: &str| {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        };
        write("assets/minecraft/textures/block/stone.png");
        write("assets/minecraft/textures/block/stone.png.layers.json");
        write("assets/minecraft/textures/block/stone.png.layers.png");
        write("assets/minecraft/textures/block/stone.png.mcmeta");
        write("assets/minecraft/models/block/stone.json");
        write("pack.png");
        assert_eq!(
            recovered_assets(dir.path()),
            [
                "assets/minecraft/models/block/stone.json",
                "assets/minecraft/textures/block/stone.png",
                "pack.png",
            ]
        );
        assert!(recovered_assets(&dir.path().join("missing")).is_empty());
    }
}

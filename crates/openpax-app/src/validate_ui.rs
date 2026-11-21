//! "Check Pack for Problems": the pack is validated in the background, then a dialog lists
//! what was found.

use eframe::egui::{self, RichText, Sense};
use openpax_core::validate::{self, Issue, Report, Severity};
use openpax_core::vanilla;

use crate::app::App;
use crate::dialogs::{footer_close, header, problem};
use crate::remix_ui::JobResult;
use crate::theme::{SPACE_M, SPACE_S};
use crate::toasts::Level;
use crate::{icons, widgets};

/// The asset to open for an issue: the file itself, or the texture a `.mcmeta` belongs to.
/// `pack.mcmeta` has no editor tab (it is edited in the inspector's Pack section).
fn asset_for(issue: &Issue) -> Option<&str> {
    let path = issue.path.as_str();
    (path != "pack.mcmeta").then(|| path.strip_suffix(".mcmeta").unwrap_or(path))
}

impl App {
    pub(crate) fn start_validate(&mut self, ctx: &egui::Context) {
        let Some(pack) = self.pack.clone() else {
            return;
        };
        self.start_job(ctx, "Checking the pack", move |progress| {
            // Default resources downloaded for the same pack format let references to
            // Minecraft's own files be checked too.
            let vanilla = pack
                .meta
                .format()
                .and_then(|format| vanilla::cached_for_format(&vanilla::cache_root(), format));
            let options = validate::Options { vanilla };
            let report = validate::validate(&pack, &options, progress)?;
            Ok(JobResult::Validation(Box::new(report)))
        });
    }

    pub(crate) fn validation_dialog(&mut self, ui: &mut egui::Ui, report: &Report) -> bool {
        header(ui, icons::BUG, "Check Pack for Problems");
        let mut open = None;
        if report.is_clean() {
            ui.label(format!(
                "Checked {} files. No problems found.",
                report.files_checked
            ));
        } else {
            ui.label(format!(
                "Checked {} files: {} error(s) and {} warning(s). Click a file to open it.",
                report.files_checked,
                report.errors(),
                report.warnings()
            ));
            ui.add_space(SPACE_M);
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for issue in &report.issues {
                        if issue_row(ui, issue) {
                            open = asset_for(issue);
                        }
                        ui.add_space(SPACE_S);
                    }
                });
        }
        if !report.vanilla_checked {
            ui.add_space(SPACE_S);
            ui.weak(
                "References to Minecraft's own files were not checked. They are once the \
                 default pack for this pack's game version has been downloaded (File → Load \
                 Default Pack).",
            );
        }
        if !self.dirty_ids().is_empty() || self.pack_dirty() {
            problem(
                ui,
                "The saved files were checked; unsaved edits are not included.",
            );
        }
        if let Some(rel_path) = open {
            self.open_asset(rel_path);
            return true;
        }
        footer_close(ui)
    }
}

/// One problem: its file (clickable) above its description. Returns true when clicked.
fn issue_row(ui: &mut egui::Ui, issue: &Issue) -> bool {
    let level = match issue.severity {
        Severity::Error => Level::Error,
        Severity::Warning => Level::Warning,
    };
    let mut clicked = false;
    ui.horizontal_top(|ui| {
        ui.label(widgets::icon(level.icon()).color(level.color(ui)))
            .on_hover_text(issue.severity.name());
        ui.vertical(|ui| {
            let path = RichText::new(widgets::shorten_path(&issue.path, 76)).monospace();
            if asset_for(issue).is_some() {
                let label = ui
                    .add(egui::Label::new(path).sense(Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!("Open {}", issue.path));
                clicked = label.clicked();
            } else {
                ui.label(path);
            }
            ui.add(egui::Label::new(RichText::new(&issue.message).weak()).wrap());
        });
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issues_open_the_file_they_are_about() {
        let issue = |path: &str| Issue {
            severity: Severity::Error,
            code: "invalid-json",
            path: path.to_owned(),
            message: String::new(),
        };
        assert_eq!(asset_for(&issue("pack.mcmeta")), None);
        assert_eq!(
            asset_for(&issue("assets/a/textures/lava.png.mcmeta")),
            Some("assets/a/textures/lava.png")
        );
        assert_eq!(
            asset_for(&issue("assets/a/models/x.json")),
            Some("assets/a/models/x.json")
        );
    }
}

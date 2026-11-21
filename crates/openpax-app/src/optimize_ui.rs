//! Dialogs for "Optimize Model" (the open model, with a live preview) and "Optimize All
//! Models" (analysed in the background, then confirmed from a summary).

use std::collections::HashSet;

use eframe::egui::{self, RichText};
use openpax_core::Progress;
use openpax_model::ModelReport;

use crate::app::App;
use crate::dialogs::{Dialog, footer, header, problem};
use crate::editor::{Editor, TextureEditor};
use crate::optimize::{self, OptimizePlan, OptimizeSettings, PackGraph};
use crate::remix_ui::JobResult;
use crate::theme::{SPACE_L, SPACE_M, SPACE_S};
use crate::{icons, widgets};

/// The "Optimize Model" dialog for one open model.
pub struct OptimizeModelState {
    editor: u64,
    rel_path: String,
    graph: PackGraph,
    /// The plan for `planned_with`; recomputed when the options change.
    preview: Option<Result<OptimizePlan, String>>,
    planned_with: Option<OptimizeSettings>,
}

impl App {
    /// Paths of open files with unsaved edits; optimizing must not overwrite them.
    fn unsaved_paths(&self) -> HashSet<String> {
        self.editors
            .values()
            .filter(|e| e.dirty())
            .map(|e| e.rel_path().to_owned())
            .collect()
    }

    pub(crate) fn start_optimize_model(&mut self) {
        let (Some(pack), Some(id)) = (&self.pack, self.active) else {
            return;
        };
        let Some(Editor::Model(editor)) = self.editors.get(&id) else {
            return;
        };
        let mut graph = match PackGraph::scan(pack, &mut Progress::none()) {
            Ok(graph) => graph,
            Err(e) => {
                self.error(format!("Could not read the pack's models:\n{e}"));
                return;
            }
        };
        // Optimize what is on screen, including unsaved edits.
        graph.set_model(&editor.rel_path, editor.doc.model().clone());
        let state = OptimizeModelState {
            editor: id,
            rel_path: editor.rel_path.clone(),
            graph,
            preview: None,
            planned_with: None,
        };
        self.open_dialog(Dialog::OptimizeModel(Box::new(state)));
    }

    pub(crate) fn optimize_model_dialog(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut OptimizeModelState,
    ) -> bool {
        header(ui, icons::MAGIC_WAND, "Optimize Model");
        ui.label(RichText::new(optimize::short_name(&state.rel_path)).monospace());
        ui.add_space(SPACE_M);
        options_ui(ui, &mut self.settings.optimize);

        if state.planned_with != Some(self.settings.optimize) {
            state.planned_with = Some(self.settings.optimize);
            state.preview = self.pack.as_ref().map(|pack| {
                let mut locked = self.unsaved_paths();
                locked.remove(&state.rel_path);
                optimize::plan(
                    pack,
                    &state.graph,
                    std::slice::from_ref(&state.rel_path),
                    &self.settings.optimize,
                    &locked,
                    &mut Progress::none(),
                )
                .map_err(|e| e.to_string())
            });
        }

        ui.add_space(SPACE_L);
        let plan = match &state.preview {
            Some(Ok(plan)) => plan,
            Some(Err(e)) => {
                problem(ui, e);
                return footer(ui, "Apply", false).1;
            }
            None => return footer(ui, "Apply", false).1,
        };
        summary_grid(ui, &plan.totals, plan);
        notes_ui(ui, plan);
        if plan.is_empty() {
            ui.add_space(SPACE_S);
            ui.weak("Nothing to optimize with these options.");
        } else if !plan.textures.is_empty() {
            ui.add_space(SPACE_S);
            problem(
                ui,
                "Texture changes are written to disk and save this model. The originals are \
                 backed up in .openpax/backups, but Undo cannot restore them.",
            );
        }

        let (confirm, cancel) = footer(ui, "Apply", !plan.is_empty());
        if confirm {
            let plan = plan.clone();
            self.apply_model_plan(state.editor, &state.rel_path, &plan);
            return true;
        }
        cancel
    }

    fn apply_model_plan(&mut self, id: u64, rel_path: &str, plan: &OptimizePlan) {
        let Some(Editor::Model(editor)) = self.editors.get_mut(&id) else {
            return;
        };
        if let Some(change) = plan.models.iter().find(|m| m.rel_path == rel_path) {
            let model = change.model.clone();
            editor.doc.modify(move |m| *m = model);
        }
        let report = optimize::describe(&plan.totals);
        if plan.textures.is_empty() {
            self.success(format!("Optimized: {report}. Save to keep the changes."));
            return;
        }
        let Some(pack) = &self.pack else { return };
        match plan.apply(pack) {
            Ok(backup) => {
                self.after_files_changed(plan);
                // The plan already wrote the model; this marks the tab saved and rescans.
                self.save_editor(id);
                self.success(format!(
                    "Optimized: {report}; {}. Originals backed up in {backup}",
                    optimize::describe_textures(plan)
                ));
            }
            Err(e) => self.error(format!("Could not write the optimized files:\n{e}")),
        }
    }

    pub(crate) fn start_optimize_pack(&mut self) {
        self.open_dialog(Dialog::OptimizePack);
    }

    pub(crate) fn optimize_pack_dialog(&mut self, ui: &mut egui::Ui) -> bool {
        header(ui, icons::SPARKLE, "Optimize All Models");
        ui.label(
            "Every model in the pack is checked. You will see what would change before \
             anything is written.",
        );
        ui.add_space(SPACE_M);
        options_ui(ui, &mut self.settings.optimize);
        let unsaved = self.unsaved_paths();
        if !unsaved.is_empty() {
            problem(
                ui,
                &format!(
                    "{} open file(s) have unsaved changes and will be left alone.",
                    unsaved.len()
                ),
            );
        }
        let (confirm, cancel) = footer(ui, "Analyze", true);
        if confirm {
            let Some(pack) = self.pack.clone() else {
                return true;
            };
            let settings = self.settings.optimize;
            let ctx = ui.ctx().clone();
            self.start_job(&ctx, "Optimizing models", move |progress| {
                let graph = PackGraph::scan(&pack, progress)?;
                let targets: Vec<String> = graph
                    .models
                    .keys()
                    .filter(|path| !unsaved.contains(*path))
                    .cloned()
                    .collect();
                let mut plan =
                    optimize::plan(&pack, &graph, &targets, &settings, &unsaved, progress)?;
                if !unsaved.is_empty() {
                    plan.notes.push(format!(
                        "{} file(s) with unsaved changes were skipped.",
                        unsaved.len()
                    ));
                }
                Ok(JobResult::OptimizePlan(Box::new(plan)))
            });
            return true;
        }
        cancel
    }

    pub(crate) fn optimize_summary_dialog(
        &mut self,
        ui: &mut egui::Ui,
        plan: &OptimizePlan,
    ) -> bool {
        header(ui, icons::SPARKLE, "Optimize All Models");
        if plan.is_empty() {
            ui.label(format!(
                "Checked {} models. Nothing to optimize with these options.",
                plan.models_scanned
            ));
            notes_ui(ui, plan);
            return crate::dialogs::footer_close(ui);
        }
        ui.label(format!(
            "Checked {} models. {} model(s) and {} texture(s) would change:",
            plan.models_scanned,
            plan.models.len(),
            plan.textures.len()
        ));
        ui.add_space(SPACE_M);
        summary_grid(ui, &plan.totals, plan);
        ui.add_space(SPACE_M);
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for change in &plan.models {
                    let r = &change.report;
                    let mut text = format!(
                        "{} → {} cubes, {} → {} faces",
                        r.elements_before, r.elements_after, r.faces_before, r.faces_after
                    );
                    if change.uvs_remapped {
                        text.push_str(", UVs moved");
                    }
                    file_row(ui, icons::CUBE, &change.rel_path, &text);
                }
                for change in &plan.textures {
                    let text = format!(
                        "{}×{} → {}×{}, {}",
                        change.before.0,
                        change.before.1,
                        change.after.0,
                        change.after.1,
                        change.how
                    );
                    file_row(ui, icons::IMAGE, &change.rel_path, &text);
                }
            });
        notes_ui(ui, plan);
        ui.add_space(SPACE_S);
        ui.weak("The originals are copied to .openpax/backups first.");
        let (confirm, cancel) = footer(ui, "Apply", true);
        if confirm {
            let Some(pack) = &self.pack else { return true };
            match plan.apply(pack) {
                Ok(backup) => {
                    self.after_files_changed(plan);
                    self.rescan();
                    self.success(format!(
                        "Optimized {} model(s) and {} texture(s): {}. Originals backed up in \
                         {backup}",
                        plan.models.len(),
                        plan.textures.len(),
                        optimize::describe(&plan.totals)
                    ));
                }
                Err(e) => self.error(format!("Could not write the optimized files:\n{e}")),
            }
            return true;
        }
        cancel
    }

    /// Refreshes thumbnails, viewports and clean editor tabs after a plan was written.
    fn after_files_changed(&mut self, plan: &OptimizePlan) {
        let changed: HashSet<&str> = plan.paths().collect();
        for path in &changed {
            self.thumbs.invalidate(path);
        }
        self.bump_assets();
        let Some(pack) = &self.pack else { return };
        for editor in self.editors.values_mut() {
            if editor.dirty() || !changed.contains(editor.rel_path()) {
                continue;
            }
            let rel_path = editor.rel_path().to_owned();
            match editor {
                Editor::Texture(texture) => {
                    if let Ok(fresh) = TextureEditor::open(pack, &rel_path) {
                        **texture = fresh;
                    }
                }
                Editor::Model(model) => {
                    if let Some(change) = plan.models.iter().find(|m| m.rel_path == rel_path) {
                        let new = change.model.clone();
                        model.doc.modify(move |m| *m = new);
                        model.mark_saved();
                    }
                }
                Editor::Text(_) => {}
            }
        }
    }
}

fn options_ui(ui: &mut egui::Ui, settings: &mut OptimizeSettings) {
    let m = &mut settings.model;
    group(ui, "Fewer parts");
    ui.checkbox(&mut m.merge_elements, "Merge touching cubes that line up")
        .on_hover_text(
            "Two cubes become one when they share a side, have the same rotation and their \
             textures continue seamlessly.",
        );
    ui.checkbox(
        &mut m.remove_empty_elements,
        "Remove cubes with no faces left",
    );

    group(ui, "Hide unnecessary sides");
    ui.checkbox(
        &mut m.cull_hidden_faces,
        "Remove sides buried inside solid cubes",
    )
    .on_hover_text(
        "Only sides fully covered by unrotated cubes whose textures are completely opaque \
             are removed, so nothing that can be seen disappears.",
    );
    ui.checkbox(
        &mut m.cull_transparent_faces,
        "Remove fully transparent sides",
    );
    ui.checkbox(
        &mut m.add_block_cullfaces,
        "Let the game hide outer sides next to full blocks",
    )
    .on_hover_text("Adds \"cullface\" to sides on the edge of the block, like vanilla models.");

    group(ui, "Smaller texture maps");
    ui.checkbox(
        &mut settings.downscale_textures,
        "Store blown-up pixel art at its real size",
    )
    .on_hover_text(
        "A 64×64 texture drawn in 4×4 blocks is saved as 16×16. It looks exactly the same; \
             textures are never made smaller than 16×16.",
    );
    ui.checkbox(
        &mut settings.repack_textures,
        "Pack the used areas of the model's own textures",
    )
    .on_hover_text(
        "Unused space is cut away and the UVs are moved to match. Only textures that no \
             other model uses, outside the minecraft namespace and not animated.",
    );
    ui.checkbox(
        &mut settings.model.prune_texture_vars,
        "Remove unused texture variables",
    );
}

fn group(ui: &mut egui::Ui, title: &str) {
    ui.add_space(SPACE_S);
    widgets::caption(ui, title);
}

fn summary_grid(ui: &mut egui::Ui, totals: &ModelReport, plan: &OptimizePlan) {
    let row = |ui: &mut egui::Ui, label: &str, before: u64, after: u64| {
        ui.label(label);
        ui.label(before.to_string());
        ui.label("→");
        let text = RichText::new(after.to_string()).strong();
        ui.label(if after < before {
            text.color(crate::theme::of(ui).accent)
        } else {
            text
        });
        ui.end_row();
    };
    egui::Grid::new("optimize_summary")
        .num_columns(4)
        .spacing([SPACE_L, SPACE_S])
        .show(ui, |ui| {
            row(
                ui,
                "Cubes",
                totals.elements_before as u64,
                totals.elements_after as u64,
            );
            row(
                ui,
                "Faces",
                totals.faces_before as u64,
                totals.faces_after as u64,
            );
            if !plan.textures.is_empty() {
                let (before, after) = plan.texels();
                row(ui, "Texture pixels", before, after);
            }
        });
    if totals.cullfaces_added > 0 {
        ui.weak(format!(
            "{} outer side(s) can be hidden next to full blocks.",
            totals.cullfaces_added
        ));
    }
}

fn notes_ui(ui: &mut egui::Ui, plan: &OptimizePlan) {
    if plan.notes.is_empty() {
        return;
    }
    ui.add_space(SPACE_S);
    egui::CollapsingHeader::new(format!("Left alone ({})", plan.notes.len()))
        .id_salt("optimize_notes")
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(120.0)
                .show(ui, |ui| {
                    for note in &plan.notes {
                        ui.weak(note);
                    }
                });
        });
}

fn file_row(ui: &mut egui::Ui, glyph: &str, rel_path: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(widgets::icon(glyph));
        ui.monospace(widgets::shorten_path(rel_path, 48))
            .on_hover_text(rel_path);
        ui.weak(detail);
    });
}

//! The "Load Default Pack" and "Remix Packs" dialogs and the background work behind them.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, RichText, TextureHandle, TextureOptions, pos2, vec2};
use openpax_core::remix::{self, RemixOptions, RemixPlan, Source, SourceFiles};
use openpax_core::vanilla::{self, VersionEntry, VersionKind};
use openpax_core::{Error, PackMeta, Progress, RESOURCE_FORMATS};

use crate::app::App;
use crate::dialogs::{Dialog, footer, form, format_picker, header, problem, text_row};
use crate::jobs::{Job, Outcome};
use crate::theme::{SPACE_M, SPACE_S};
use crate::{icons, widgets};

const THUMB: f32 = 22.0;
const THUMBS_PER_FRAME: usize = 6;
const MAX_THUMB_PIXELS: u32 = 1024 * 1024;

/// The list of game versions offered for download.
pub enum Versions {
    Idle,
    Loading(Job<Vec<VersionEntry>>),
    Ready(Vec<VersionEntry>),
    /// The manifest could not be loaded; only already-downloaded versions are offered.
    Failed(String, Vec<VersionEntry>),
}

/// What a background job hands back to the UI.
pub enum JobResult {
    /// A pack scanned for the remix dialog, with its declared format.
    SourceAdded(Box<SourceFiles>, Option<u32>),
    /// A pack folder written to disk, to be opened.
    PackCreated(PathBuf),
    /// What optimizing every model would change.
    OptimizePlan(Box<crate::optimize::OptimizePlan>),
}

fn has_content(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some())
}

fn invalid_folder_name(name: &str) -> bool {
    name.trim().is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
}

/// Entries for versions that are cached but not listed (when offline).
fn cached_entries(known: &[VersionEntry]) -> Vec<VersionEntry> {
    vanilla::cached_versions(&vanilla::cache_root())
        .into_iter()
        .filter(|id| !known.iter().any(|v| &v.id == id))
        .map(|id| VersionEntry {
            id,
            kind: VersionKind::Release,
            url: String::new(),
            sha1: String::new(),
            release_time: String::new(),
        })
        .collect()
}

fn source_format(src: &SourceFiles) -> Option<u32> {
    let bytes = src.read("pack.mcmeta").ok()?;
    serde_json::from_slice::<PackMeta>(&bytes).ok()?.format()
}

// ---- Load Default Pack ----

#[derive(Default)]
pub struct LoadDefaultState {
    show_snapshots: bool,
    version: Option<String>,
    textures_only: bool,
    name: String,
    /// The name last filled in automatically; once the user types their own it is kept.
    auto_name: String,
}

impl LoadDefaultState {
    fn refresh_name(&mut self) {
        let Some(id) = &self.version else { return };
        let auto = if self.textures_only {
            format!("Vanilla {id} textures")
        } else {
            format!("Vanilla {id}")
        };
        if self.name == self.auto_name {
            self.name.clone_from(&auto);
        }
        self.auto_name = auto;
    }
}

// ---- Remix ----

pub struct RemixState {
    plan: RemixPlan,
    name: String,
    description: String,
    auto_description: String,
    format: u32,
    textures_only: bool,
    filter: String,
    vanilla_version: Option<String>,
    show_snapshots: bool,
    thumbs: HashMap<(usize, String), Option<TextureHandle>>,
}

impl RemixState {
    pub fn new() -> Self {
        Self {
            plan: remix::plan(Vec::new()),
            name: "remix".to_owned(),
            description: String::new(),
            auto_description: String::new(),
            format: RESOURCE_FORMATS[0].1,
            textures_only: false,
            filter: String::new(),
            vanilla_version: None,
            show_snapshots: false,
            thumbs: HashMap::new(),
        }
    }

    /// Rebuilds the plan with the sources in `order` (old indices), keeping per-file choices.
    fn reorder(&mut self, order: &[usize]) {
        let old = std::mem::replace(&mut self.plan, remix::plan(Vec::new()));
        let mut sources: Vec<Option<SourceFiles>> = old.sources.into_iter().map(Some).collect();
        let reordered = order.iter().filter_map(|&i| sources[i].take()).collect();
        self.plan = remix::plan(reordered);
        for (path, old_index) in old.overrides {
            if let Some(new_index) = order.iter().position(|&i| i == old_index) {
                self.plan.overrides.insert(path, new_index);
            }
        }
        self.thumbs.clear();
        self.refresh_description();
    }

    fn add(&mut self, source: SourceFiles, format: Option<u32>) {
        if self.plan.sources.is_empty()
            && let Some(format) = format
        {
            self.format = format;
        }
        let mut sources = std::mem::take(&mut self.plan.sources);
        sources.push(source);
        let overrides = std::mem::take(&mut self.plan.overrides);
        self.plan = remix::plan(sources);
        self.plan.overrides = overrides;
        self.refresh_description();
    }

    fn refresh_description(&mut self) {
        let labels: Vec<&str> = self.plan.sources.iter().map(|s| s.label.as_str()).collect();
        let auto = format!("Remix of {}", labels.join(" + "));
        if self.description == self.auto_description {
            self.description.clone_from(&auto);
        }
        self.auto_description = auto;
    }

    fn thumbnail(
        &mut self,
        ctx: &egui::Context,
        budget: &mut usize,
        source: usize,
        rel: &str,
    ) -> Option<&TextureHandle> {
        let key = (source, rel.to_owned());
        if !self.thumbs.contains_key(&key) {
            if *budget == 0 {
                ctx.request_repaint();
                return None;
            }
            *budget -= 1;
            let texture = self.plan.sources[source]
                .read(rel)
                .ok()
                .and_then(|bytes| openpax_paint::decode_png(&bytes).ok())
                .filter(|b| b.width() > 0 && b.width() * b.height() <= MAX_THUMB_PIXELS)
                .map(|b| {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [b.width() as usize, b.height() as usize],
                        b.data(),
                    );
                    ctx.load_texture(
                        format!("remix:{source}:{rel}"),
                        image,
                        TextureOptions::NEAREST,
                    )
                });
            self.thumbs.insert(key.clone(), texture);
        }
        self.thumbs.get(&key).and_then(Option::as_ref)
    }
}

/// Draws a square thumbnail (the first frame of an animation strip).
fn thumb_image(ui: &mut egui::Ui, texture: Option<&TextureHandle>) {
    match texture {
        Some(texture) => {
            let [w, h] = texture.size();
            let side = w.min(h) as f32;
            let uv =
                egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(side / w as f32, side / h as f32));
            ui.add(
                egui::Image::from_texture((texture.id(), vec2(THUMB, THUMB)))
                    .uv(uv)
                    .texture_options(TextureOptions::NEAREST),
            );
        }
        None => {
            ui.add_sized(
                [THUMB, THUMB],
                egui::Label::new(widgets::icon(icons::IMAGE).weak()),
            );
        }
    }
}

fn source_icon(source: &SourceFiles) -> &'static str {
    match &source.source {
        Source::Zip(_) => icons::FILE_ZIP,
        Source::Folder(path) if path.starts_with(vanilla::cache_root()) => icons::PACKAGE,
        Source::Folder(_) => icons::FOLDER,
    }
}

impl App {
    // ---- jobs ----

    /// Starts loading the version list unless it is loaded or loading.
    pub(crate) fn request_versions(&mut self, ctx: &egui::Context) {
        if matches!(self.versions, Versions::Idle | Versions::Failed(..)) {
            self.versions = Versions::Loading(Job::spawn(ctx, "Loading versions", |_| {
                let mut versions = vanilla::fetch_manifest(&vanilla::cache_root())?;
                versions.extend(cached_entries(&versions));
                Ok(versions)
            }));
        }
        self.vanilla_cached = vanilla::cached_versions(&vanilla::cache_root())
            .into_iter()
            .collect();
    }

    /// Collects finished background work. Called once per frame.
    pub(crate) fn poll_jobs(&mut self) {
        if let Versions::Loading(job) = &mut self.versions
            && let Some(outcome) = job.poll()
        {
            self.versions = match outcome {
                Outcome::Done(versions) => Versions::Ready(versions),
                Outcome::Cancelled => Versions::Idle,
                Outcome::Failed(e) => Versions::Failed(e, cached_entries(&[])),
            };
        }
        let Some(job) = &mut self.job else { return };
        let Some(outcome) = job.poll() else { return };
        self.job = None;
        self.vanilla_cached = vanilla::cached_versions(&vanilla::cache_root())
            .into_iter()
            .collect();
        match outcome {
            Outcome::Done(JobResult::SourceAdded(source, format)) => {
                if let Some(Dialog::Remix(state)) = self
                    .dialogs
                    .iter_mut()
                    .rev()
                    .find(|d| matches!(d, Dialog::Remix(_)))
                {
                    state.add(*source, format);
                }
            }
            Outcome::Done(JobResult::PackCreated(root)) => {
                self.dialogs
                    .retain(|d| !matches!(d, Dialog::LoadDefault(_) | Dialog::Remix(_)));
                self.open_new_pack(root);
            }
            Outcome::Done(JobResult::OptimizePlan(plan)) => {
                self.open_dialog(Dialog::OptimizeSummary(plan));
            }
            Outcome::Cancelled => self.info("Cancelled"),
            Outcome::Failed(e) => self.error(e),
        }
    }

    pub(crate) fn start_job(
        &mut self,
        ctx: &egui::Context,
        title: impl Into<String>,
        work: impl FnOnce(&mut Progress) -> Result<JobResult, Error> + Send + 'static,
    ) {
        if self.job.is_some() {
            self.warn("Wait for the current task to finish first.");
            return;
        }
        self.job = Some(Job::spawn(ctx, title, work));
    }

    /// The progress dialog shown while a job runs.
    pub(crate) fn show_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        egui::Modal::new(egui::Id::unique("openpax_job")).show(ctx, |ui| {
            ui.set_width(crate::theme::DIALOG_WIDTH);
            header(ui, icons::RECENT, &job.title);
            let step = if job.step.is_empty() {
                "Starting…"
            } else {
                &job.step
            };
            let detail = if job.step.starts_with("Downloading") && job.total > 0 {
                let mb = |b: u64| b as f64 / 1_048_576.0;
                format!("{step}  ({:.1} / {:.1} MB)", mb(job.done), mb(job.total))
            } else if job.total > 0 {
                format!("{step}  ({} / {})", job.done, job.total)
            } else {
                step.to_owned()
            };
            ui.label(RichText::new(detail).weak());
            ui.add_space(SPACE_M);
            let bar = match job.fraction() {
                Some(f) => egui::ProgressBar::new(f).show_percentage(),
                None => egui::ProgressBar::new(0.0).animate(true),
            };
            ui.add(bar);
            ui.add_space(SPACE_M);
            egui::Sides::new().show(
                ui,
                |_| {},
                |ui| {
                    let cancelling = job.cancelling();
                    let label = if cancelling {
                        "Cancelling…"
                    } else {
                        "Cancel"
                    };
                    if ui
                        .add_enabled(
                            !cancelling,
                            egui::Button::new(label).min_size(vec2(72.0, 28.0)),
                        )
                        .clicked()
                    {
                        job.cancel();
                    }
                },
            );
        });
    }

    /// The version list for a picker; `None` while it loads.
    fn version_choices(&self) -> Option<&[VersionEntry]> {
        match &self.versions {
            Versions::Ready(v) | Versions::Failed(_, v) => Some(v),
            Versions::Idle | Versions::Loading(_) => None,
        }
    }

    /// A version combo box with a snapshot toggle. Returns whether the selection changed.
    fn version_picker(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        show_snapshots: &mut bool,
        selected: &mut Option<String>,
    ) -> bool {
        let mut changed = false;
        let mut retry = false;
        match &self.versions {
            Versions::Idle | Versions::Loading(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Loading versions…");
                });
                return false;
            }
            Versions::Failed(e, cached) => {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(ui.visuals().warn_fg_color, widgets::icon(icons::WARNING))
                            .on_hover_text(e);
                        ui.weak(if cached.is_empty() {
                            "Could not reach Mojang."
                        } else {
                            "Offline: showing downloaded versions."
                        });
                        retry = ui.small_button("Retry").clicked();
                    });
                });
            }
            Versions::Ready(_) => {}
        }
        if retry {
            let ctx = ui.ctx().clone();
            self.request_versions(&ctx);
            return false;
        }
        let Some(versions) = self.version_choices() else {
            return false;
        };
        if versions.is_empty() {
            return false;
        }
        let show = *show_snapshots;
        let visible = move |v: &&VersionEntry| show || v.kind == VersionKind::Release;
        if selected
            .as_ref()
            .is_none_or(|s| !versions.iter().any(|v| &v.id == s))
        {
            *selected = versions.iter().find(visible).map(|v| v.id.clone());
            changed = true;
        }
        let cached = &self.vanilla_cached;
        let label = |v: &VersionEntry| {
            let mut text = v.id.clone();
            if v.kind == VersionKind::Snapshot {
                text.push_str("  (snapshot)");
            }
            if cached.contains(&v.id) {
                text.push_str("  ");
                text.push_str(icons::CHECK);
                text.push_str(" downloaded");
            }
            text
        };
        ui.horizontal(|ui| {
            let current = selected
                .as_ref()
                .and_then(|s| versions.iter().find(|v| &v.id == s))
                .map_or_else(|| "Choose…".to_owned(), label);
            egui::ComboBox::from_id_salt(id)
                .selected_text(current)
                .width(200.0)
                .height(320.0)
                .show_ui(ui, |ui| {
                    for v in versions.iter().filter(visible) {
                        let is = selected.as_deref() == Some(v.id.as_str());
                        if ui.selectable_label(is, label(v)).clicked() && !is {
                            *selected = Some(v.id.clone());
                            changed = true;
                        }
                    }
                });
            if ui.checkbox(show_snapshots, "Snapshots").changed() && !*show_snapshots {
                // Fall back to the newest release if a snapshot was picked.
                if let Some(s) = selected.as_ref()
                    && versions
                        .iter()
                        .any(|v| &v.id == s && v.kind == VersionKind::Snapshot)
                {
                    *selected = versions
                        .iter()
                        .find(|v| v.kind == VersionKind::Release)
                        .map(|v| v.id.clone());
                    changed = true;
                }
            }
        });
        changed
    }

    fn version_entry(&self, id: &str) -> Option<VersionEntry> {
        self.version_choices()?.iter().find(|v| v.id == id).cloned()
    }

    // ---- Load Default Pack ----

    pub(crate) fn load_default_dialog(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut LoadDefaultState,
    ) -> bool {
        header(ui, icons::PACKAGE, "Load Default Pack");
        ui.label(
            RichText::new(
                "Creates an editable pack from the vanilla resources of a Minecraft version. \
                 Each version is downloaded from Mojang once and cached.",
            )
            .weak(),
        );
        ui.add_space(SPACE_M);
        let mut picked = false;
        form(ui, "load_default", |ui| {
            ui.label("Version");
            picked = self.version_picker(
                ui,
                "default_version",
                &mut state.show_snapshots,
                &mut state.version,
            );
            ui.end_row();
            ui.label("Contents");
            let toggled = ui
                .checkbox(&mut state.textures_only, "Textures only")
                .changed();
            ui.end_row();
            if picked || toggled || state.name.is_empty() {
                state.refresh_name();
            }
            text_row(ui, "Folder name", &mut state.name, false);
        });
        let name_ok = !invalid_folder_name(&state.name);
        let entry = state
            .version
            .as_deref()
            .and_then(|id| self.version_entry(id));
        if !name_ok {
            problem(ui, "Enter a valid folder name.");
        } else if entry.is_none() {
            problem(ui, "Choose a version.");
        }
        let (confirm, cancel) = footer(ui, "Choose Location…", name_ok && entry.is_some());
        if confirm
            && let Some(entry) = entry
            && let Some(parent) = rfd::FileDialog::new()
                .set_title("Create the pack folder inside…")
                .pick_folder()
        {
            let dest = parent.join(state.name.trim());
            if has_content(&dest) {
                self.error(format!(
                    "{} already exists and is not empty.",
                    dest.display()
                ));
                return false;
            }
            let textures_only = state.textures_only;
            let ctx = ui.ctx().clone();
            self.start_job(
                &ctx,
                format!("Loading Minecraft {}", entry.id),
                move |progress| {
                    let dir = vanilla::ensure_version(&vanilla::cache_root(), &entry, progress)?;
                    let source = remix::scan_source(&Source::Folder(dir))?;
                    let format = source_format(&source).unwrap_or(RESOURCE_FORMATS[0].1);
                    let plan = remix::plan(vec![source]);
                    let options = RemixOptions {
                        textures_only,
                        description: format!("Minecraft {} default resources", entry.id),
                        format,
                    };
                    remix::execute(&plan, &options, &dest, progress)?;
                    Ok(JobResult::PackCreated(dest))
                },
            );
        }
        cancel
    }

    // ---- Remix ----

    fn add_remix_source(&mut self, ctx: &egui::Context, source: Source, label: Option<String>) {
        self.start_job(ctx, "Reading pack", move |_| {
            let mut files = remix::scan_source(&source)?;
            if let Some(label) = label {
                files.label = label;
            }
            let format = source_format(&files);
            Ok(JobResult::SourceAdded(Box::new(files), format))
        });
    }

    fn add_vanilla_source(&mut self, ctx: &egui::Context, entry: VersionEntry) {
        self.start_job(
            ctx,
            format!("Loading Minecraft {}", entry.id),
            move |progress| {
                let dir = vanilla::ensure_version(&vanilla::cache_root(), &entry, progress)?;
                let mut files = remix::scan_source(&Source::Folder(dir))?;
                files.label = format!("Minecraft {}", entry.id);
                let format = source_format(&files);
                Ok(JobResult::SourceAdded(Box::new(files), format))
            },
        );
    }

    pub(crate) fn remix_dialog(&mut self, ui: &mut egui::Ui, state: &mut RemixState) -> bool {
        let ctx = ui.ctx().clone();
        header(ui, icons::MERGE_DOWN, "Remix Packs");
        ui.label(
            RichText::new(
                "Combine packs into a new one. When several contain the same file, the one \
                 higher in the list wins, unless you pick another below. Language, sound, \
                 atlas and font files are merged.",
            )
            .weak(),
        );
        ui.add_space(SPACE_M);

        self.remix_sources(ui, state);
        ui.add_space(SPACE_S);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(widgets::icon_label(icons::FOLDER_OPEN, "Add Folder…"))
                .clicked()
                && let Some(dir) = rfd::FileDialog::new()
                    .set_title("Add a resource pack folder")
                    .pick_folder()
            {
                self.add_remix_source(&ctx, Source::Folder(dir), None);
            }
            if ui
                .button(widgets::icon_label(icons::FILE_ZIP, "Add Zip…"))
                .clicked()
                && let Some(zip) = rfd::FileDialog::new()
                    .set_title("Add a resource pack zip")
                    .add_filter("Resource pack", &["zip", "jar"])
                    .pick_file()
            {
                self.add_remix_source(&ctx, Source::Zip(zip), None);
            }
            if let Some(pack) = &self.pack {
                let root = pack.root().to_path_buf();
                let label = pack.name();
                if ui
                    .button(widgets::icon_label(icons::PACKAGE, "Add Open Pack"))
                    .on_hover_text("Adds the pack open in OpenPax as it is saved on disk")
                    .clicked()
                {
                    self.add_remix_source(&ctx, Source::Folder(root), Some(label));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Vanilla");
            self.version_picker(
                ui,
                "remix_version",
                &mut state.show_snapshots,
                &mut state.vanilla_version,
            );
            let entry = state
                .vanilla_version
                .as_deref()
                .and_then(|id| self.version_entry(id));
            if ui
                .add_enabled(
                    entry.is_some(),
                    egui::Button::new(widgets::icon_label(icons::PLUS, "Add")),
                )
                .on_hover_text("Adds the default resources of this version (downloaded once)")
                .clicked()
                && let Some(entry) = entry
            {
                self.add_vanilla_source(&ctx, entry);
            }
        });

        ui.separator();
        self.remix_conflicts(ui, state);
        ui.separator();

        form(ui, "remix_options", |ui| {
            text_row(ui, "Folder name", &mut state.name, false);
            text_row(ui, "Description", &mut state.description, false);
            ui.label("Game version");
            ui.horizontal(|ui| format_picker(ui, &mut state.format));
            ui.end_row();
            ui.label("Contents");
            ui.checkbox(&mut state.textures_only, "Textures only");
            ui.end_row();
        });

        let enough = state.plan.sources.len() >= 2;
        let name_ok = !invalid_folder_name(&state.name);
        if !enough {
            problem(ui, "Add at least two packs.");
        } else if !name_ok {
            problem(ui, "Enter a valid folder name.");
        }
        let (confirm, cancel) = footer(ui, "Create Remix…", enough && name_ok);
        if confirm
            && let Some(parent) = rfd::FileDialog::new()
                .set_title("Create the remixed pack inside…")
                .pick_folder()
        {
            let dest = parent.join(state.name.trim());
            if has_content(&dest) {
                self.error(format!(
                    "{} already exists and is not empty.",
                    dest.display()
                ));
                return false;
            }
            let plan = state.plan.clone();
            let options = RemixOptions {
                textures_only: state.textures_only,
                description: state.description.clone(),
                format: state.format,
            };
            self.start_job(&ctx, "Creating remix", move |progress| {
                remix::execute(&plan, &options, &dest, progress)?;
                Ok(JobResult::PackCreated(dest))
            });
        }
        cancel
    }

    fn remix_sources(&mut self, ui: &mut egui::Ui, state: &mut RemixState) {
        widgets::caption(ui, "PACKS  (TOP WINS)");
        ui.add_space(SPACE_S);
        if state.plan.sources.is_empty() {
            ui.weak("No packs yet. Add folders, zips or a vanilla version below.");
            return;
        }
        let count = state.plan.sources.len();
        let mut order: Option<Vec<usize>> = None;
        egui::ScrollArea::vertical()
            .id_salt("remix_sources")
            .max_height(150.0)
            .show(ui, |ui| {
                for (i, source) in state.plan.sources.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{}.", i + 1));
                        ui.label(widgets::icon(source_icon(source)));
                        let path = match &source.source {
                            Source::Folder(p) | Source::Zip(p) => p.display().to_string(),
                        };
                        ui.add(egui::Label::new(RichText::new(&source.label).strong()).truncate())
                            .on_hover_text(path);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut reorder =
                                |ui: &mut egui::Ui, enabled, glyph, tip, swap: Option<usize>| {
                                    if widgets::icon_button_enabled(ui, enabled, glyph, tip)
                                        .clicked()
                                    {
                                        let mut o: Vec<usize> = (0..count).collect();
                                        match swap {
                                            Some(j) => o.swap(i, j),
                                            None => {
                                                o.remove(i);
                                            }
                                        }
                                        order = Some(o);
                                    }
                                };
                            reorder(ui, true, icons::CLOSE, "Remove", None);
                            reorder(
                                ui,
                                i + 1 < count,
                                icons::ARROW_DOWN,
                                "Lower priority",
                                Some(i + 1),
                            );
                            reorder(
                                ui,
                                i > 0,
                                icons::ARROW_UP,
                                "Higher priority",
                                Some(i.saturating_sub(1)),
                            );
                            ui.weak(format!("{} files", source.files.len()));
                        });
                    });
                }
            });
        if let Some(order) = order {
            state.reorder(&order);
        }
    }

    fn remix_conflicts(&mut self, ui: &mut egui::Ui, state: &mut RemixState) {
        let total = state.plan.conflicts.len();
        ui.horizontal(|ui| {
            widgets::caption(ui, &format!("SHARED FILES  ({total})"));
            if !state.plan.overrides.is_empty() {
                ui.weak(format!("{} picked by hand", state.plan.overrides.len()));
                if ui.small_button("Reset").clicked() {
                    state.plan.overrides.clear();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.filter)
                        .hint_text(widgets::icon_label(icons::SEARCH, "Filter…"))
                        .desired_width(200.0),
                );
            });
        });
        ui.add_space(SPACE_S);
        if total == 0 {
            ui.weak(if state.plan.sources.len() < 2 {
                "Files that more than one pack contains will be listed here."
            } else {
                "These packs share no files, so nothing needs deciding."
            });
            return;
        }

        let filter = state.filter.to_lowercase();
        let rows: Vec<usize> = state
            .plan
            .conflicts
            .iter()
            .enumerate()
            .filter(|(_, c)| filter.is_empty() || c.rel_path.to_lowercase().contains(&filter))
            .map(|(i, _)| i)
            .collect();
        let ctx = ui.ctx().clone();
        let mut budget = THUMBS_PER_FRAME;
        let row_height = THUMB + 6.0;
        egui::ScrollArea::vertical()
            .id_salt("remix_conflicts")
            .max_height(240.0)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, rows.len(), |ui, range| {
                for &index in &rows[range] {
                    let conflict = state.plan.conflicts[index].clone();
                    let rel = conflict.rel_path.as_str();
                    let is_png = rel.ends_with(".png");
                    let merging = state.plan.merges(rel);
                    let winner = state.plan.winner(rel);
                    ui.horizontal(|ui| {
                        ui.set_min_height(row_height);
                        let short = rel.strip_prefix("assets/").unwrap_or(rel);
                        ui.add_sized(
                            [ui.available_width() * 0.45, row_height],
                            egui::Label::new(RichText::new(short).monospace()).truncate(),
                        )
                        .on_hover_text(rel);
                        let mut choice: Option<Option<usize>> = None;
                        if conflict.merge.is_some()
                            && ui
                                .selectable_label(merging, "Merge")
                                .on_hover_text("Combine every pack's entries")
                                .clicked()
                        {
                            choice = Some(None);
                        }
                        for &source in &conflict.candidates {
                            let selected = !merging && winner == Some(source);
                            let label = state.plan.sources[source].label.clone();
                            let response = if is_png {
                                let texture =
                                    state.thumbnail(&ctx, &mut budget, source, rel).cloned();
                                let frame = egui::Frame::new()
                                    .inner_margin(2.0)
                                    .corner_radius(crate::theme::RADIUS)
                                    .stroke(if selected {
                                        egui::Stroke::new(2.0, crate::theme::of(ui).accent)
                                    } else {
                                        egui::Stroke::NONE
                                    });
                                frame
                                    .show(ui, |ui| thumb_image(ui, texture.as_ref()))
                                    .response
                                    .interact(egui::Sense::click())
                            } else {
                                ui.selectable_label(selected, format!("{}", source + 1))
                            };
                            if response
                                .on_hover_text(format!("Use the file from {label}"))
                                .clicked()
                            {
                                choice = Some(Some(source));
                            }
                        }
                        match choice {
                            Some(None) => {
                                state.plan.overrides.remove(rel);
                            }
                            Some(Some(source)) => {
                                // The top pack wins anyway unless the file would be merged.
                                if conflict.merge.is_none()
                                    && conflict.candidates.first() == Some(&source)
                                {
                                    state.plan.overrides.remove(rel);
                                } else {
                                    state.plan.overrides.insert(rel.to_owned(), source);
                                }
                            }
                            None => {}
                        }
                    });
                }
            });
    }
}

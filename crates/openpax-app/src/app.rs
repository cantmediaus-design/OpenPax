//! Application state, top-level layout, menus and pack/file actions.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Layout, RichText, TextureHandle, TextureOptions, vec2};
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, TabViewer};
use openpax_core::{AssetKind, Pack, PackMeta};
use openpax_model::Element;

use crate::command_palette::PaletteState;
use crate::commands::{self, Active, Command};
use crate::dialogs::{Dialog, Then};
use crate::editor::{Editor, TextEditor, TextureEditor, ToolState};
use crate::explorer::{self, Action, Explorer, Thumbnails, Tree};
use crate::jobs::Job;
use crate::model_editor::ModelEditor;
use crate::remix_ui::{JobResult, RemixState, Versions};
use crate::settings::{PackSession, SESSIONS_KEY, SETTINGS_KEY, Settings, remember_session};
use crate::theme::{self, SPACE_M, SPACE_S, SPACE_XL, SPACE_XXL, ThemeChoice};
use crate::toasts::{Level, Toasts};
use crate::{clipboard, icons, widgets};

const RECENT_KEY: &str = "recent_packs";
const MAX_RECENT: usize = 8;
/// The pack's icon in Minecraft's pack list, next to `pack.mcmeta`.
pub(crate) const PACK_ICON: &str = "pack.png";
/// Seconds before the status bar message dims.
const STATUS_FRESH: f64 = 6.0;

/// The latest message in the status bar.
pub struct Status {
    text: String,
    level: Level,
    /// `egui` time when the message was first drawn.
    shown_at: Option<f64>,
}

pub struct App {
    pub(crate) pack: Option<Pack>,
    /// `pack.mcmeta` as it is on disk, to tell whether the Pack section has unsaved edits.
    saved_meta: Option<PackMeta>,
    tree: Tree,
    pub(crate) thumbs: Thumbnails,
    search: String,
    focus_search: bool,
    explorer_filter: explorer::Filter,
    dock: DockState<u64>,
    pub(crate) editors: HashMap<u64, Editor>,
    next_id: u64,
    /// The editor tab that menus, shortcuts and the side panels act on.
    pub(crate) active: Option<u64>,
    pub(crate) tools: ToolState,
    pub(crate) checker: TextureHandle,
    recent: Vec<String>,
    pub(crate) settings: Settings,
    /// Open modal dialogs; the last one is on top.
    pub(crate) dialogs: Vec<Dialog>,
    /// True until the top dialog has been drawn once (to focus its first field).
    pub(crate) dialog_fresh: bool,
    /// The command palette, while it is open.
    pub(crate) palette: Option<PaletteState>,
    /// Open tabs of recently used packs, restored when a pack is opened again.
    sessions: Vec<PackSession>,
    /// Assets deleted this session, newest last: `(trash folder, original path)`.
    pub(crate) trashed: Vec<(String, String)>,
    /// Crash-recovery snapshots of unsaved edits.
    pub(crate) recovery: crate::recovery::Recovery,
    /// The step of the guided tour being shown.
    pub(crate) tour: Option<usize>,
    /// Where the panels were drawn this frame, for the tour.
    pub(crate) layout: crate::tour::Layout,
    status: Status,
    toasts: Toasts,
    confetti: crate::confetti::Confetti,
    pub(crate) allow_quit: bool,
    title_stale: bool,
    /// GPU handles for the 3D viewport; `None` if eframe is not running on wgpu.
    render_state: Option<eframe::egui_wgpu::RenderState>,
    /// Bumped on every save so viewports reload textures edited in other tabs.
    asset_generation: u64,
    /// The background task in progress (download, remix), shown in a progress dialog.
    pub(crate) job: Option<Job<JobResult>>,
    /// Game versions offered by the default-pack and remix dialogs.
    pub(crate) versions: Versions,
    /// Versions whose default resources are already downloaded.
    pub(crate) vanilla_cached: HashSet<String>,
}

/// The texture editor in the active tab, if that tab is a texture.
pub(crate) fn texture_mut(
    editors: &mut HashMap<u64, Editor>,
    active: Option<u64>,
) -> Option<&mut TextureEditor> {
    match editors.get_mut(&active?)? {
        Editor::Texture(editor) => Some(editor),
        _ => None,
    }
}

/// The model editor in the active tab, if that tab is a model.
pub(crate) fn model_mut(
    editors: &mut HashMap<u64, Editor>,
    active: Option<u64>,
) -> Option<&mut ModelEditor> {
    match editors.get_mut(&active?)? {
        Editor::Model(editor) => Some(editor),
        _ => None,
    }
}

fn is_pack_folder(path: &Path) -> bool {
    path.join("pack.mcmeta").is_file()
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial_pack: Option<PathBuf>,
        initial_assets: Vec<String>,
    ) -> Self {
        // White and light grey; tinted per theme where it is drawn.
        let checker = cc.egui_ctx.load_texture(
            "checker",
            egui::ColorImage::from_rgba_unmultiplied(
                [2, 2],
                &[
                    200, 200, 200, 255, 255, 255, 255, 255, 255, 255, 255, 255, 200, 200, 200, 255,
                ],
            ),
            TextureOptions::NEAREST_REPEAT,
        );
        let mut recent: Vec<String> = cc
            .storage
            .and_then(|s| eframe::get_value(s, RECENT_KEY))
            .unwrap_or_default();
        // Packs that were moved or deleted since the last session drop off the list.
        recent.retain(|path| is_pack_folder(Path::new(path)));
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, SETTINGS_KEY))
            .unwrap_or_default();
        theme::install(&cc.egui_ctx, settings.theme);
        commands::set_overrides(&settings.shortcuts);
        let sessions: Vec<PackSession> = cc
            .storage
            .and_then(|s| eframe::get_value(s, SESSIONS_KEY))
            .unwrap_or_default();

        let mut app = Self {
            pack: None,
            saved_meta: None,
            tree: Tree::default(),
            thumbs: Thumbnails::default(),
            search: String::new(),
            focus_search: false,
            explorer_filter: explorer::Filter::default(),
            dock: DockState::new(Vec::new()),
            editors: HashMap::new(),
            next_id: 1,
            active: None,
            tools: settings.tools(),
            checker,
            recent,
            settings,
            dialogs: Vec::new(),
            dialog_fresh: false,
            palette: None,
            sessions,
            trashed: Vec::new(),
            recovery: crate::recovery::Recovery::default(),
            tour: None,
            layout: crate::tour::Layout::default(),
            status: Status {
                text: "Ready".to_owned(),
                level: Level::Info,
                shown_at: None,
            },
            toasts: Toasts::default(),
            confetti: crate::confetti::Confetti::default(),
            allow_quit: false,
            title_stale: true,
            render_state: cc.wgpu_render_state.clone(),
            asset_generation: 0,
            job: None,
            versions: Versions::Idle,
            vanilla_cached: HashSet::new(),
        };
        // Open the pack given on the command line, else the last one if it is still there.
        if let Some(root) = initial_pack {
            app.open_pack(root);
            for rel_path in initial_assets {
                app.open_asset(&rel_path.replace('\\', "/"));
            }
        } else if let Some(last) = app.recent.first().cloned() {
            app.open_pack(PathBuf::from(last));
        }
        app
    }

    // ---- feedback ----

    /// Shows a message in the status bar; warnings and errors also pop up as a toast.
    pub(crate) fn notify(&mut self, level: Level, text: impl Into<String>) {
        let text = text.into();
        if matches!(level, Level::Warning | Level::Error) {
            self.toasts.push(level, text.clone());
        }
        self.set_status(level, &text);
    }

    /// Shows a message in the status bar only.
    fn set_status(&mut self, level: Level, text: &str) {
        // The status bar has a single line.
        let one_line = text.lines().collect::<Vec<_>>().join(" ");
        self.status = Status {
            text: one_line,
            level,
            shown_at: None,
        };
    }

    pub(crate) fn info(&mut self, text: impl Into<String>) {
        self.notify(Level::Info, text);
    }

    pub(crate) fn success(&mut self, text: impl Into<String>) {
        self.notify(Level::Success, text);
    }

    /// Fires a burst of confetti, unless celebrations are turned off in the View menu.
    pub(crate) fn celebrate(&mut self) {
        if self.settings.celebrations {
            self.confetti.burst();
        }
    }

    pub(crate) fn warn(&mut self, text: impl Into<String>) {
        self.notify(Level::Warning, text);
    }

    pub(crate) fn error(&mut self, text: impl Into<String>) {
        self.notify(Level::Error, text);
    }

    pub(crate) fn open_dialog(&mut self, dialog: Dialog) {
        self.dialogs.push(dialog);
        self.dialog_fresh = true;
    }

    pub(crate) fn dirty_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .editors
            .iter()
            .filter(|(_, e)| e.dirty())
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Whether the Pack section has edits that are not in `pack.mcmeta` yet.
    pub(crate) fn pack_dirty(&self) -> bool {
        match (&self.pack, &self.saved_meta) {
            (Some(pack), Some(saved)) => pack.meta != *saved,
            _ => false,
        }
    }

    pub(crate) fn save_pack_meta(&mut self) -> bool {
        let Some(pack) = &self.pack else { return false };
        match pack.save_meta() {
            Ok(()) => {
                self.saved_meta = Some(pack.meta.clone());
                self.success("Saved pack.mcmeta");
                true
            }
            Err(e) => {
                self.error(format!("Could not save pack.mcmeta:\n{e}"));
                false
            }
        }
    }

    pub(crate) fn revert_pack_meta(&mut self) {
        if let (Some(pack), Some(saved)) = (&mut self.pack, &self.saved_meta) {
            pack.meta = saved.clone();
        }
    }

    // ---- packs ----

    pub(crate) fn open_pack(&mut self, root: PathBuf) {
        let ids = self.dirty_ids();
        if !ids.is_empty() || self.pack_dirty() {
            // Ask to save first; the prompt opens the pack once the edits are dealt with.
            self.open_dialog(Dialog::ConfirmClose {
                ids,
                then: Then::OpenPack(root),
            });
            return;
        }
        let path = root.to_string_lossy().into_owned();
        match Pack::open(&root) {
            Ok(pack) => {
                self.remember_session();
                // Nothing is unsaved here, so the old pack's recovery files are not needed.
                self.finish_recovery();
                self.tree = Tree::build(&pack.index);
                self.success(format!(
                    "Opened {} ({} assets)",
                    pack.name(),
                    pack.index.entries.len()
                ));
                self.saved_meta = Some(pack.meta.clone());
                self.pack = Some(pack);
                self.thumbs.clear();
                self.editors.clear();
                self.dock = DockState::new(Vec::new());
                self.active = None;
                self.search.clear();
                self.explorer_filter.namespace = None;
                self.trashed.clear();
                self.title_stale = true;

                self.recent.retain(|p| p != &path);
                self.recent.insert(0, path.clone());
                self.recent.truncate(MAX_RECENT);
                self.restore_session(&path);
                self.check_recovery();
                if !self.settings.tour_done && self.tour.is_none() {
                    self.start_tour();
                }
            }
            Err(e) => {
                self.recent.retain(|p| p != &path);
                self.error(format!("Could not open pack:\n{e}"));
            }
        }
    }

    /// Opens a pack that was just made, with a celebration once it is actually open
    /// (opening waits if the current pack has unsaved edits to deal with first).
    pub(crate) fn open_new_pack(&mut self, root: PathBuf) {
        self.open_pack(root.clone());
        if self.pack.as_ref().is_some_and(|p| p.root() == root) {
            self.celebrate();
        }
    }

    /// The open pack and its tabs, in tab order.
    fn current_session(&self) -> Option<PackSession> {
        let pack = self.pack.as_ref()?;
        let tabs = self
            .dock
            .iter_all_tabs()
            .filter_map(|(_, id)| self.editors.get(id))
            .map(|e| e.rel_path().to_owned())
            .collect();
        let active = self
            .active
            .and_then(|id| self.editors.get(&id))
            .map(|e| e.rel_path().to_owned());
        Some(PackSession {
            pack: pack.root().to_string_lossy().into_owned(),
            tabs,
            active,
        })
    }

    /// Notes which tabs the open pack has, so they come back when it is opened again.
    fn remember_session(&mut self) {
        if let Some(session) = self.current_session() {
            remember_session(&mut self.sessions, session);
        }
    }

    /// Reopens the tabs that were open the last time this pack was used.
    fn restore_session(&mut self, pack_path: &str) {
        let Some(session) = self.sessions.iter().find(|s| s.pack == pack_path).cloned() else {
            return;
        };
        let Some(pack) = &self.pack else { return };
        let tabs: Vec<String> = session
            .tabs
            .into_iter()
            .filter(|t| {
                pack.index.entries.iter().any(|e| &e.rel_path == t)
                    || (t == PACK_ICON && pack.exists(PACK_ICON))
            })
            .collect();
        for tab in &tabs {
            self.open_asset(tab);
        }
        if let Some(active) = session.active.filter(|a| tabs.contains(a)) {
            self.open_asset(&active); // focuses the tab that is already open
        }
    }

    fn pick_and_open_pack(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title("Open resource pack folder")
            .pick_folder();
        if let Some(dir) = picked {
            self.open_pack(dir);
        }
    }

    /// Extracts a zip (asking for one if `zip` is `None`) and opens the pack inside it.
    fn import_zip(&mut self, zip: Option<PathBuf>) {
        let zip = zip.or_else(|| {
            rfd::FileDialog::new()
                .set_title("Import resource pack zip")
                .add_filter("Resource pack", &["zip"])
                .pick_file()
        });
        let Some(zip) = zip else { return };
        let Some(parent) = rfd::FileDialog::new()
            .set_title("Choose a folder to extract the pack into")
            .pick_folder()
        else {
            return;
        };
        let stem = zip
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dest = parent.join(stem);
        if let Err(e) = openpax_core::import_zip(&zip, &dest) {
            self.error(format!("Could not import zip:\n{e}"));
            return;
        }
        // Some zips wrap the pack in a single top-level folder.
        let root = if is_pack_folder(&dest) {
            dest
        } else {
            std::fs::read_dir(&dest)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .find(|p| is_pack_folder(p))
                .unwrap_or(dest)
        };
        self.open_pack(root);
    }

    pub(crate) fn export_zip(&mut self) {
        let Some(pack) = &self.pack else { return };
        let Some(dest) = rfd::FileDialog::new()
            .set_title("Export resource pack")
            .set_file_name(format!("{}.zip", pack.name()))
            .add_filter("Resource pack", &["zip"])
            .save_file()
        else {
            return;
        };
        match openpax_core::export_zip(pack.root(), &dest) {
            Ok(count) => {
                let message = format!("Exported {count} files to {}", dest.display());
                if self.dirty_ids().is_empty() && !self.pack_dirty() {
                    self.success(message);
                    self.celebrate();
                } else {
                    self.warn(format!("{message}\nUnsaved edits were not included."));
                }
            }
            Err(e) => self.error(format!("Export failed:\n{e}")),
        }
    }

    /// Makes viewports reload textures, after files changed on disk.
    pub(crate) fn bump_assets(&mut self) {
        self.asset_generation += 1;
    }

    pub(crate) fn rescan(&mut self) {
        let Some(pack) = &mut self.pack else { return };
        match pack.rescan() {
            Ok(()) => self.tree.rebuild(&pack.index),
            Err(e) => self.error(format!("Could not rescan pack:\n{e}")),
        }
    }

    // ---- editors ----

    pub(crate) fn open_asset(&mut self, rel_path: &str) {
        self.open_asset_as(rel_path, false);
    }

    /// Opens an asset in its dedicated editor, or in the text editor when `as_text` is set.
    pub(crate) fn open_asset_as(&mut self, rel_path: &str, as_text: bool) {
        if let Some((&id, _)) = self.editors.iter().find(|(_, e)| e.rel_path() == rel_path) {
            if let Some(path) = self.dock.find_tab(&id) {
                let _ = self.dock.set_active_tab(path);
                self.dock.set_focused_node_and_surface(path.node_path());
            }
            self.active = Some(id);
            return;
        }
        let Some(pack) = &self.pack else { return };
        let kind = pack
            .index
            .entries
            .iter()
            .find(|e| e.rel_path == rel_path)
            .map(|e| e.kind)
            // The pack icon lives outside assets/, so it is not in the index.
            .or_else(|| {
                (rel_path == PACK_ICON && pack.exists(PACK_ICON)).then_some(AssetKind::Texture)
            });
        let mut fallback = None;
        let editor = match kind {
            Some(AssetKind::Texture) => {
                TextureEditor::open(pack, rel_path).map(|e| Editor::Texture(Box::new(e)))
            }
            Some(AssetKind::Model) if !as_text => match ModelEditor::open(pack, rel_path) {
                Ok(editor) => Ok(Editor::Model(Box::new(editor))),
                Err(e) => {
                    fallback = Some(format!(
                        "Opened as text: this is not a model OpenPax can read.\n{e}"
                    ));
                    TextEditor::open(pack, rel_path).map(Editor::Text)
                }
            },
            Some(kind) if kind.is_text() => TextEditor::open(pack, rel_path).map(Editor::Text),
            _ => Err("OpenPax has no editor for this file type yet.".to_owned()),
        };
        match editor {
            Ok(editor) => {
                let id = self.next_id;
                self.next_id += 1;
                self.editors.insert(id, editor);
                self.dock.push_to_focused_leaf(id);
                self.active = Some(id);
                if let Some(message) = fallback {
                    self.warn(message);
                }
            }
            Err(e) => self.error(format!("Could not open {rel_path}:\n{e}")),
        }
    }

    /// Saves one editor; reports failure and returns `false`.
    pub(crate) fn save_editor(&mut self, id: u64) -> bool {
        let (Some(pack), Some(editor)) = (&self.pack, self.editors.get_mut(&id)) else {
            return false;
        };
        let rel_path = editor.rel_path().to_owned();
        match editor.save(pack) {
            Ok(()) => {
                self.thumbs.invalidate(&rel_path);
                self.asset_generation += 1;
                self.success(format!("Saved {rel_path}"));
                self.rescan();
                true
            }
            Err(e) => {
                self.error(format!("Could not save {rel_path}:\n{e}"));
                false
            }
        }
    }

    fn save_all(&mut self) {
        let ids = self.dirty_ids();
        let count = ids.len();
        for id in ids {
            if !self.save_editor(id) {
                return;
            }
        }
        if self.pack_dirty() && !self.save_pack_meta() {
            return;
        }
        if count > 1 {
            self.success(format!("Saved {count} files"));
        }
    }

    /// Closes a tab without asking.
    pub(crate) fn close_editor(&mut self, id: u64) {
        if let Some(path) = self.dock.find_tab(&id) {
            self.dock.remove_tab(path);
        }
        self.editors.remove(&id);
    }

    // ---- commands ----

    pub(crate) fn active_kind(&self) -> Active {
        self.active
            .and_then(|id| self.editors.get(&id))
            .map_or(Active::None, Editor::kind)
    }

    /// Whether a command can run right now (decides greyed-out menu items and buttons).
    pub(crate) fn enabled(&self, command: Command) -> bool {
        if !command.needs().met(self.pack.is_some(), self.active_kind()) {
            return false;
        }
        let editor = self.active.and_then(|id| self.editors.get(&id));
        match (command, editor) {
            (Command::Undo, Some(editor)) => editor.can_undo_redo().0,
            (Command::Redo, Some(editor)) => editor.can_undo_redo().1,
            (Command::Save, Some(editor)) => editor.dirty(),
            (Command::SaveAll, _) => !self.dirty_ids().is_empty() || self.pack_dirty(),
            (Command::RestoreDeleted, _) => !self.trashed.is_empty(),
            (Command::Deselect, Some(Editor::Texture(editor))) => {
                editor.doc.selection().is_some() || editor.doc.stroke_active()
            }
            (Command::DuplicateElement | Command::DeleteElement, Some(Editor::Model(editor))) => {
                editor.doc.selected().is_some()
            }
            _ => true,
        }
    }

    pub(crate) fn run(&mut self, command: Command, ctx: &egui::Context) {
        if !self.enabled(command) {
            return;
        }
        match command {
            Command::NewPack => self.open_dialog(Dialog::new_pack()),
            Command::OpenPack => self.pick_and_open_pack(),
            Command::ImportZip => self.import_zip(None),
            Command::LoadDefaultPack => {
                self.request_versions(ctx);
                self.open_dialog(Dialog::LoadDefault(Box::default()));
            }
            Command::Remix => {
                self.request_versions(ctx);
                self.open_dialog(Dialog::Remix(Box::new(RemixState::new())));
            }
            Command::NewTexture => self.open_dialog(Dialog::new_texture()),
            Command::NewModel => self.open_dialog(Dialog::new_model()),
            Command::Save => {
                if let Some(id) = self.active {
                    self.save_editor(id);
                }
            }
            Command::SaveAll => self.save_all(),
            Command::CloseTab => {
                if let Some(id) = self.active {
                    if self.editors.get(&id).is_some_and(Editor::dirty) {
                        self.open_dialog(Dialog::ConfirmClose {
                            ids: vec![id],
                            then: Then::CloseTabs,
                        });
                    } else {
                        self.close_editor(id);
                    }
                }
            }
            Command::ExportZip => {
                let ids = self.dirty_ids();
                if ids.is_empty() && !self.pack_dirty() {
                    self.export_zip();
                } else {
                    self.open_dialog(Dialog::ConfirmClose {
                        ids,
                        then: Then::Export,
                    });
                }
            }
            Command::Rescan => {
                self.rescan();
                self.thumbs.clear();
                self.info("Rescanned the pack folder");
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Undo => {
                if let Some(editor) = self.active.and_then(|id| self.editors.get_mut(&id)) {
                    editor.undo();
                }
            }
            Command::Redo => {
                if let Some(editor) = self.active.and_then(|id| self.editors.get_mut(&id)) {
                    editor.redo();
                }
            }
            Command::SwapColors => self.tools.swap_colors(),
            Command::BrushSmaller => self.tools.resize_brush(-1),
            Command::BrushLarger => self.tools.resize_brush(1),
            Command::Tool(tool) => self.tools.tool = tool,
            Command::ToggleExplorer => self.settings.show_explorer ^= true,
            Command::ToggleInspector => self.settings.show_inspector ^= true,
            Command::FocusSearch => {
                self.settings.show_explorer = true;
                self.focus_search = true;
            }
            Command::OpenPalette => self.palette = Some(PaletteState::new()),
            Command::ShowShortcuts => self.open_dialog(Dialog::Shortcuts(Default::default())),
            Command::About => self.open_dialog(Dialog::About),
            Command::StartTour => self.start_tour(),
            Command::Cut | Command::Copy | Command::Paste => self.clipboard_command(command),
            Command::RenameAsset | Command::DuplicateAsset | Command::DeleteAsset => {
                let Some(rel_path) = self.active_asset() else {
                    return;
                };
                match command {
                    Command::RenameAsset => self.start_rename(&rel_path),
                    Command::DuplicateAsset => self.duplicate_asset(&rel_path),
                    _ => self.start_delete(&rel_path),
                }
            }
            Command::RestoreDeleted => self.restore_deleted(),
            Command::ToggleGrid => {
                if let Some(editor) = texture_mut(&mut self.editors, self.active) {
                    editor.show_grid ^= true;
                } else if let Some(editor) = model_mut(&mut self.editors, self.active) {
                    editor.show_grid ^= true;
                }
            }
            Command::SelectAll
            | Command::Deselect
            | Command::ClearSelection
            | Command::FlipHorizontal
            | Command::FlipVertical
            | Command::ResizeCanvas
            | Command::ZoomIn
            | Command::ZoomOut
            | Command::ZoomFit
            | Command::ToggleTiling => self.run_texture(command),
            Command::AddCube
            | Command::DuplicateElement
            | Command::DeleteElement
            | Command::ResetView => self.run_model(command),
            Command::OptimizeModel => self.start_optimize_model(),
            Command::OptimizePack => self.start_optimize_pack(),
        }
    }

    fn run_texture(&mut self, command: Command) {
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        let doc = &mut editor.doc;
        match command {
            Command::SelectAll => doc.select_all(),
            // Escape first abandons a stroke in progress, then clears the selection.
            Command::Deselect if doc.stroke_active() => doc.stroke_cancel(),
            Command::Deselect => doc.deselect(),
            Command::ClearSelection => doc.clear_selected(),
            Command::FlipHorizontal => doc.flip_horizontal(),
            Command::FlipVertical => doc.flip_vertical(),
            Command::ZoomIn => editor.zoom_step(1),
            Command::ZoomOut => editor.zoom_step(-1),
            Command::ZoomFit => editor.fit_pending = true,
            Command::ToggleTiling => editor.tile_preview ^= true,
            Command::ResizeCanvas => {
                let dialog = Dialog::Resize {
                    width: doc.width(),
                    height: doc.height(),
                };
                self.open_dialog(dialog);
            }
            _ => {}
        }
    }

    /// Copy, cut and paste of pixels in the active texture, through the system clipboard.
    fn clipboard_command(&mut self, command: Command) {
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        let doc = &mut editor.doc;
        doc.stroke_end();
        match command {
            Command::Copy | Command::Cut => {
                let image = doc.copy_selected();
                let size = format!("{} × {}", image.width(), image.height());
                if let Err(e) = clipboard::copy_image(&image) {
                    self.error(format!("Could not copy to the clipboard:\n{e}"));
                    return;
                }
                if command == Command::Cut {
                    doc.clear_selected();
                    self.info(format!("Cut {size} pixels"));
                } else {
                    self.info(format!("Copied {size} pixels"));
                }
            }
            Command::Paste => {
                let image = match clipboard::paste_image() {
                    Ok(image) => image,
                    Err(e) => {
                        self.warn(e);
                        return;
                    }
                };
                // Paste where the selection is, so copy + paste in place is a no-op move.
                let at = doc.selection_bounds().map_or((0, 0), |r| (r.x, r.y));
                doc.paste(&image, at);
                let (width, height) = (doc.width(), doc.height());
                let cropped =
                    at.0 as u32 + image.width() > width || at.1 as u32 + image.height() > height;
                self.tools.tool = openpax_paint::ToolKind::Move;
                if cropped {
                    self.warn(format!(
                        "Pasted a {} × {} image onto a {width} × {height} canvas, so part of it \
                         was cut off. Resize the canvas first to keep all of it.",
                        image.width(),
                        image.height(),
                    ));
                } else {
                    self.info("Pasted as a new layer. Drag it into place with the Move tool.");
                }
            }
            _ => {}
        }
    }

    fn run_model(&mut self, command: Command) {
        let Some(editor) = model_mut(&mut self.editors, self.active) else {
            return;
        };
        match command {
            Command::AddCube => {
                let doc = &mut editor.doc;
                let texture = doc
                    .model()
                    .textures
                    .keys()
                    .find(|k| k.as_str() != "particle")
                    .map_or("#0".to_owned(), |k| format!("#{k}"));
                doc.add_element(Element::cube([4.0, 0.0, 4.0], [12.0, 8.0, 12.0], &texture));
            }
            Command::DuplicateElement => editor.doc.duplicate_selected(),
            Command::DeleteElement => editor.doc.remove_selected(),
            Command::ResetView => editor.reset_view(),
            _ => {}
        }
    }

    // ---- input ----

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if !self.dialogs.is_empty()
            || self.job.is_some()
            || self.palette.is_some()
            || self.tour.is_some()
        {
            return;
        }
        let typing = ctx.text_edit_focused();
        // Leave Escape and friends to an open menu or popup.
        let popup_open = egui::Popup::is_any_open(ctx);
        let has_pack = self.pack.is_some();
        let active = self.active_kind();

        let mut fired = Vec::new();
        ctx.input_mut(|input| {
            for (command, shortcut) in commands::bindings() {
                let bare = !shortcut.modifiers.command;
                // A bare key (rebound, say) never fires while typing, or it could not be typed.
                if (typing && (!command.works_while_typing() || bare))
                    || (popup_open && bare)
                    || !command.needs().met(has_pack, active)
                {
                    continue;
                }
                if input.consume_shortcut(&shortcut) {
                    fired.push(command);
                }
            }
        });
        // egui delivers Ctrl+C / X / V as clipboard events rather than key presses. Text fields
        // get them while they have focus; otherwise they act on the canvas.
        if !typing && !popup_open && active == Active::Texture {
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    let command = match event {
                        egui::Event::Copy => Command::Copy,
                        egui::Event::Cut => Command::Cut,
                        egui::Event::Paste(_) | egui::Event::PasteImage(_) => Command::Paste,
                        _ => return true,
                    };
                    if !fired.contains(&command) {
                        fired.push(command);
                    }
                    false
                });
            });
        }
        for command in fired {
            self.run(command, ctx);
        }
    }

    fn close_requested(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_quit {
            return;
        }
        let ids = self.dirty_ids();
        if !ids.is_empty() || self.pack_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            let already_asking = matches!(
                self.dialogs.last(),
                Some(Dialog::ConfirmClose {
                    then: Then::Quit,
                    ..
                })
            );
            if !already_asking {
                self.open_dialog(Dialog::ConfirmClose {
                    ids,
                    then: Then::Quit,
                });
            }
        }
    }

    /// Opens packs, zips and images dropped onto the window.
    fn dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        for path in dropped {
            let extension = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase());
            if path.is_dir() {
                if is_pack_folder(&path) {
                    self.open_pack(path);
                } else {
                    self.warn(format!(
                        "{} is not a resource pack: it has no pack.mcmeta.",
                        path.display()
                    ));
                }
            } else if extension.as_deref() == Some("zip") {
                self.import_zip(Some(path));
            } else if extension.as_deref() == Some("png") {
                if self.pack.is_some() {
                    self.open_dialog(Dialog::import_texture(path));
                } else {
                    self.warn("Open a pack first, then drop the image to add it as a texture.");
                }
            } else {
                self.warn(format!(
                    "Cannot open {}. Drop a pack folder, a .zip or a .png.",
                    path.display()
                ));
            }
        }
    }

    fn drop_overlay(ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("drop_overlay"),
        ));
        let tokens = theme::tokens(ctx.theme() == egui::Theme::Dark);
        painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(160));
        painter.rect_stroke(
            rect.shrink(SPACE_XL),
            theme::RADIUS_L,
            egui::Stroke::new(2.0, tokens.accent),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop a pack folder, a pack .zip, or a .png texture",
            egui::FontId::proportional(20.0),
            egui::Color32::WHITE,
        );
    }

    // ---- layout ----

    /// A menu entry for a command, with its icon and shortcut.
    fn menu_item(&mut self, ui: &mut egui::Ui, command: Command) {
        let mut button = egui::Button::new(widgets::icon_label(command.icon(), command.label()));
        if let Some(shortcut) = command.shortcut() {
            button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
        }
        if ui.add_enabled(self.enabled(command), button).clicked() {
            let ctx = ui.ctx().clone();
            self.run(command, &ctx);
        }
    }

    /// A checkable menu entry for a command that toggles something.
    fn menu_toggle(&mut self, ui: &mut egui::Ui, command: Command, checked: bool) {
        let glyph = if checked { icons::CHECK } else { " " };
        let mut button = egui::Button::new(widgets::icon_label(glyph, command.label()));
        if let Some(shortcut) = command.shortcut() {
            button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
        }
        if ui.add_enabled(self.enabled(command), button).clicked() {
            let ctx = ui.ctx().clone();
            self.run(command, &ctx);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                self.menu_item(ui, Command::NewPack);
                self.menu_item(ui, Command::OpenPack);
                self.menu_item(ui, Command::ImportZip);
                self.menu_item(ui, Command::LoadDefaultPack);
                self.menu_item(ui, Command::Remix);
                ui.menu_button(widgets::icon_label(icons::RECENT, "Open Recent"), |ui| {
                    if self.recent.is_empty() {
                        ui.weak("No recent packs");
                    }
                    for path in self.recent.clone() {
                        let name = Path::new(&path)
                            .file_name()
                            .map_or(path.clone(), |n| n.to_string_lossy().into_owned());
                        if ui
                            .button(widgets::icon_label(icons::PACKAGE, &name))
                            .on_hover_text(&path)
                            .clicked()
                        {
                            self.open_pack(PathBuf::from(path));
                        }
                    }
                });
                ui.separator();
                self.menu_item(ui, Command::NewTexture);
                self.menu_item(ui, Command::NewModel);
                ui.separator();
                self.menu_item(ui, Command::Save);
                self.menu_item(ui, Command::SaveAll);
                self.menu_item(ui, Command::CloseTab);
                ui.separator();
                self.menu_item(ui, Command::RenameAsset);
                self.menu_item(ui, Command::DuplicateAsset);
                self.menu_item(ui, Command::DeleteAsset);
                self.menu_item(ui, Command::RestoreDeleted);
                ui.separator();
                self.menu_item(ui, Command::ExportZip);
                self.menu_item(ui, Command::Rescan);
                self.menu_item(ui, Command::OptimizePack);
                ui.separator();
                self.menu_item(ui, Command::Quit);
            });

            ui.menu_button("Edit", |ui| {
                self.menu_item(ui, Command::Undo);
                self.menu_item(ui, Command::Redo);
                ui.separator();
                if self.active_kind() != Active::Model {
                    self.menu_item(ui, Command::Cut);
                    self.menu_item(ui, Command::Copy);
                    self.menu_item(ui, Command::Paste);
                    ui.separator();
                }
                if self.active_kind() == Active::Model {
                    self.menu_item(ui, Command::AddCube);
                    self.menu_item(ui, Command::DuplicateElement);
                    self.menu_item(ui, Command::DeleteElement);
                    ui.separator();
                    self.menu_item(ui, Command::OptimizeModel);
                } else {
                    self.menu_item(ui, Command::SelectAll);
                    self.menu_item(ui, Command::Deselect);
                    self.menu_item(ui, Command::ClearSelection);
                    ui.separator();
                    self.menu_item(ui, Command::FlipHorizontal);
                    self.menu_item(ui, Command::FlipVertical);
                    self.menu_item(ui, Command::ResizeCanvas);
                    ui.separator();
                    self.menu_item(ui, Command::SwapColors);
                    self.menu_item(ui, Command::BrushSmaller);
                    self.menu_item(ui, Command::BrushLarger);
                }
            });

            ui.menu_button("View", |ui| {
                let (grid, tiling) = match self.active.and_then(|id| self.editors.get(&id)) {
                    Some(Editor::Texture(editor)) => (editor.show_grid, editor.tile_preview),
                    Some(Editor::Model(editor)) => (editor.show_grid, false),
                    _ => (false, false),
                };
                if self.active_kind() == Active::Model {
                    self.menu_toggle(ui, Command::ToggleGrid, grid);
                    self.menu_item(ui, Command::ResetView);
                } else {
                    self.menu_item(ui, Command::ZoomIn);
                    self.menu_item(ui, Command::ZoomOut);
                    self.menu_item(ui, Command::ZoomFit);
                    ui.separator();
                    self.menu_toggle(ui, Command::ToggleGrid, grid);
                    self.menu_toggle(ui, Command::ToggleTiling, tiling);
                }
                ui.separator();
                self.menu_toggle(ui, Command::ToggleExplorer, self.settings.show_explorer);
                self.menu_toggle(ui, Command::ToggleInspector, self.settings.show_inspector);
                self.menu_item(ui, Command::FocusSearch);
                self.menu_item(ui, Command::OpenPalette);
                ui.separator();
                ui.menu_button(widgets::icon_label(icons::MOON, "Theme"), |ui| {
                    for choice in ThemeChoice::ALL {
                        if ui
                            .radio_value(&mut self.settings.theme, choice, choice.name())
                            .changed()
                        {
                            theme::set_choice(ui.ctx(), choice);
                        }
                    }
                });
                ui.checkbox(&mut self.settings.celebrations, "Celebrations")
                    .on_hover_text("Confetti when you create or export a pack");
            });

            ui.menu_button("Tools", |ui| {
                for tool in openpax_paint::ToolKind::ALL {
                    self.menu_toggle(ui, Command::Tool(tool), self.tools.tool == tool);
                }
            });

            ui.menu_button("Help", |ui| {
                self.menu_item(ui, Command::OpenPalette);
                self.menu_item(ui, Command::ShowShortcuts);
                self.menu_item(ui, Command::StartTour);
                ui.separator();
                ui.hyperlink_to(
                    widgets::icon_label(icons::BOOK, "Documentation"),
                    env!("CARGO_PKG_REPOSITORY"),
                );
                self.menu_item(ui, Command::About);
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let now = ui.ctx().input(|i| i.time);
        let age = now - *self.status.shown_at.get_or_insert(now);
        ui.horizontal(|ui| {
            let fresh = age < STATUS_FRESH;
            let mut icon = widgets::icon(self.status.level.icon()).size(14.0);
            let mut text = RichText::new(&self.status.text);
            if fresh {
                icon = icon.color(self.status.level.color(ui));
                // Dim once the message is old news.
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(STATUS_FRESH - age));
            } else {
                icon = icon.weak();
                text = text.weak();
            }
            ui.label(icon);
            ui.add(egui::Label::new(text).truncate());

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.status_readout(ui);
            });
        });
    }

    /// The right-hand side of the status bar: facts about the active editor.
    fn status_readout(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        match self.active_kind() {
            Active::Texture => {
                let mut run = None;
                ui.spacing_mut().item_spacing.x = SPACE_S;
                for command in [Command::ZoomFit, Command::ZoomIn] {
                    if small_icon_button(ui, command.icon(), &command.tooltip(&ctx)) {
                        run = Some(command);
                    }
                }
                let Some(editor) = texture_mut(&mut self.editors, self.active) else {
                    return;
                };
                let zoom = format!("{:.0}%", editor.zoom * 100.0);
                ui.add_sized(
                    [48.0, ui.spacing().interact_size.y],
                    egui::Label::new(RichText::new(zoom).monospace()),
                );
                let command = Command::ZoomOut;
                if small_icon_button(ui, command.icon(), &command.tooltip(&ctx)) {
                    run = Some(command);
                }
                ui.separator();
                let (width, height) = (editor.doc.width(), editor.doc.height());
                ui.monospace(format!("{width} × {height}"));
                if let Some((x, y)) = editor.hover {
                    ui.separator();
                    ui.monospace(format!("{x}, {y}"));
                }
                if let Some(command) = run {
                    self.run(command, &ctx);
                }
            }
            Active::Model => {
                if let Some(editor) = model_mut(&mut self.editors, self.active) {
                    let model = editor.doc.model();
                    ui.monospace(format!(
                        "{} elements   {} textures",
                        model.elements().len(),
                        model.textures.len()
                    ));
                }
            }
            Active::Text | Active::None => {
                if let Some(pack) = &self.pack {
                    ui.weak(format!("{} assets", pack.index.entries.len()));
                }
            }
        }
    }

    fn explorer_panel(&mut self, ui: &mut egui::Ui) {
        let Some(pack) = &self.pack else { return };
        let ctx = ui.ctx().clone();
        let mut run = None;

        ui.add_space(SPACE_S);
        ui.horizontal(|ui| {
            ui.label(widgets::icon(icons::PACKAGE).color(theme::of(ui).accent));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let grid = &mut self.settings.explorer_grid;
                let (glyph, tip) = if *grid {
                    (icons::LIST, "Show as a list")
                } else {
                    (icons::GRID, "Show thumbnails in a grid")
                };
                if widgets::icon_button(ui, glyph, tip).clicked() {
                    *grid = !*grid;
                }
                for command in [Command::Rescan, Command::NewModel, Command::NewTexture] {
                    if widgets::icon_button(ui, command.icon(), &command.tooltip(&ctx)).clicked() {
                        run = Some(command);
                    }
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(pack.name()).strong()).truncate());
                });
            });
        });

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_S;
            let clear_width = if self.search.is_empty() {
                0.0
            } else {
                theme::ICON_BUTTON + SPACE_S
            };
            let hint = widgets::icon_label(icons::SEARCH, "Search assets…");
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text(hint)
                    .desired_width(ui.available_width() - clear_width),
            );
            if std::mem::take(&mut self.focus_search) {
                field.request_focus();
            }
            if !self.search.is_empty()
                && widgets::icon_button(ui, icons::CLOSE, "Clear the search").clicked()
            {
                self.search.clear();
            }
        });
        ui.add_space(SPACE_S);
        self.explorer_filter_row(ui);
        ui.add_space(SPACE_S);

        let Some(pack) = &self.pack else { return };
        let active = self
            .active
            .and_then(|id| self.editors.get(&id))
            .map(|e| e.rel_path());
        let mut explorer = Explorer {
            pack,
            thumbs: &mut self.thumbs,
            active,
            actions: Vec::new(),
        };
        let view = explorer::View {
            search: &self.search,
            filter: &self.explorer_filter,
            grid: self.settings.explorer_grid,
        };
        explorer.show(ui, &mut self.tree, &view);
        let actions = explorer.actions;

        for action in actions {
            match action {
                Action::Open(rel_path) => self.open_asset(&rel_path),
                Action::OpenAsText(rel_path) => self.open_asset_as(&rel_path, true),
                Action::Copy(text) => {
                    self.info(format!("Copied {text}"));
                    ctx.copy_text(text);
                }
                Action::Reveal(rel_path) => {
                    let Some(pack) = &self.pack else { continue };
                    if let Err(e) = explorer::reveal(&pack.abs(&rel_path)) {
                        self.error(format!("Could not open the file manager:\n{e}"));
                    }
                }
                Action::NewTexture(namespace, folder) => {
                    self.open_dialog(Dialog::new_texture_in(&namespace, &folder));
                }
                Action::NewModel(namespace, folder) => {
                    self.open_dialog(Dialog::new_model_in(&namespace, &folder));
                }
                Action::Rename(rel_path) => self.start_rename(&rel_path),
                Action::Duplicate(rel_path) => self.duplicate_asset(&rel_path),
                Action::Delete(rel_path) => self.start_delete(&rel_path),
            }
        }
        if let Some(command) = run {
            self.run(command, &ctx);
        }
    }

    /// Kind and namespace pickers, and the list / grid switch, under the explorer's search.
    fn explorer_filter_row(&mut self, ui: &mut egui::Ui) {
        let Some(pack) = &self.pack else { return };
        let filter = &mut self.explorer_filter;
        let namespaces = pack.index.namespaces();
        let show_namespaces = namespaces.len() > 1 || filter.namespace.is_some();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_S;
            // Each picker takes an equal share of the row.
            let pickers = if show_namespaces { 2.0 } else { 1.0 };
            let share = (ui.available_width() - SPACE_S * (pickers - 1.0)) / pickers;
            let kind = filter.kind;
            ui.scope(|ui| {
                ui.spacing_mut().combo_width = share;
                egui::ComboBox::from_id_salt("explorer_kind")
                    .selected_text(widgets::icon_label(kind.icon(), kind.name()))
                    .width(share)
                    .truncate()
                    .show_ui(ui, |ui| {
                        for choice in explorer::KindFilter::ALL {
                            let label = widgets::icon_label(choice.icon(), choice.name());
                            ui.selectable_value(&mut filter.kind, choice, label);
                        }
                    })
                    .response
                    .on_hover_text("Show only one kind of file");
            });
            if show_namespaces {
                let shown = filter.namespace.as_deref().unwrap_or("All namespaces");
                egui::ComboBox::from_id_salt("explorer_namespace")
                    .selected_text(shown)
                    .width(ui.available_width())
                    .truncate()
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut filter.namespace, None, "All namespaces");
                        for namespace in namespaces {
                            ui.selectable_value(
                                &mut filter.namespace,
                                Some(namespace.to_owned()),
                                namespace,
                            );
                        }
                    })
                    .response
                    .on_hover_text("Show only one namespace");
            }
        });
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let accent = theme::of(ui).accent;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.18).max(SPACE_XL));
            ui.label(RichText::new(icons::PACKAGE).size(56.0).color(accent));
            ui.add_space(SPACE_S);
            ui.label(RichText::new("OpenPax").size(32.0).strong());
            ui.label(RichText::new("Open-source Minecraft resource pack studio").weak());
            ui.add_space(SPACE_XXL);

            // Centre the row of buttons by giving it a fixed width.
            let button = vec2(150.0, 36.0);
            let row_width = button.x * 3.0 + SPACE_M * 2.0;
            ui.allocate_ui_with_layout(
                vec2(row_width, button.y),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_M;
                    for (index, command) in
                        [Command::OpenPack, Command::NewPack, Command::ImportZip]
                            .into_iter()
                            .enumerate()
                    {
                        let label = command.label().trim_end_matches('…');
                        let tokens = theme::of(ui);
                        let mut text = RichText::new(widgets::icon_label(command.icon(), label));
                        if index == 0 {
                            text = text.color(tokens.on_accent).strong();
                        }
                        let mut widget = egui::Button::new(text).min_size(button);
                        if index == 0 {
                            widget = widget.fill(tokens.accent);
                        }
                        if ui
                            .add(widget)
                            .on_hover_text(command.tooltip(&ctx))
                            .clicked()
                        {
                            self.run(command, &ctx);
                        }
                    }
                },
            );
            ui.add_space(SPACE_M);
            let row2_width = button.x * 2.0 + SPACE_M;
            ui.allocate_ui_with_layout(
                vec2(row2_width, button.y),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_M;
                    for command in [Command::LoadDefaultPack, Command::Remix] {
                        let label = command.label().trim_end_matches('…');
                        let widget = egui::Button::new(widgets::icon_label(command.icon(), label))
                            .min_size(button);
                        if ui
                            .add(widget)
                            .on_hover_text(command.tooltip(&ctx))
                            .clicked()
                        {
                            self.run(command, &ctx);
                        }
                    }
                },
            );
            ui.add_space(SPACE_S);
            ui.label(RichText::new("or drop a pack folder or zip onto this window").weak());
            if self.recent.is_empty() {
                ui.add_space(SPACE_M);
                ui.label(
                    RichText::new(widgets::icon_label(
                        icons::LIGHTBULB,
                        "New to resource packs? Load Default Pack gives you Minecraft's own \
                         textures and models to change.",
                    ))
                    .color(accent),
                );
            }
            if let Some(shortcut) = Command::OpenPalette.shortcut() {
                ui.add_space(SPACE_S);
                ui.label(
                    RichText::new(format!(
                        "Press {} to find any command",
                        ctx.format_shortcut(&shortcut)
                    ))
                    .weak(),
                );
            }

            if !self.recent.is_empty() {
                ui.add_space(SPACE_XXL);
                widgets::caption(ui, "RECENT PACKS");
                ui.add_space(SPACE_S);
                for path in self.recent.clone() {
                    let name = Path::new(&path)
                        .file_name()
                        .map_or(path.clone(), |n| n.to_string_lossy().into_owned());
                    let entry = egui::Button::new(widgets::icon_label(icons::PACKAGE, &name))
                        .right_text(RichText::new(widgets::shorten_path(&path, 44)).weak())
                        .frame_when_inactive(false)
                        .min_size(vec2(row_width, 32.0));
                    if ui.add(entry).on_hover_text(&path).clicked() {
                        self.open_pack(PathBuf::from(path));
                    }
                }
            }
        });
    }

    fn central(&mut self, ui: &mut egui::Ui) {
        if self.pack.is_none() {
            self.welcome(ui);
            return;
        }
        if self.editors.is_empty() {
            widgets::empty_state(
                ui,
                icons::IMAGE,
                "Nothing open",
                "Pick an asset in the explorer, or find it by name in the Command Palette.",
            );
            self.active = None;
            return;
        }

        let Some(pack) = &self.pack else { return };
        let mut close_requests = Vec::new();
        let style = theme::dock_style(ui);
        let mut viewer = Viewer {
            pack,
            render_state: self.render_state.as_ref(),
            asset_generation: self.asset_generation,
            editors: &mut self.editors,
            tools: &mut self.tools,
            checker: &self.checker,
            show_hint: &mut self.settings.show_viewport_hint,
            close_requests: &mut close_requests,
        };
        DockArea::new(&mut self.dock)
            .style(style)
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .show_inside(ui, &mut viewer);

        // Forget editors whose tabs were closed, then work out which tab is active.
        let open: HashSet<u64> = self.dock.iter_all_tabs().map(|(_, id)| *id).collect();
        self.editors.retain(|id, _| open.contains(id));
        if let Some((_, id)) = self.dock.find_active_focused() {
            self.active = Some(*id);
        }
        if self.active.is_none_or(|id| !self.editors.contains_key(&id)) {
            self.active = self.editors.keys().min().copied();
        }
        if !close_requests.is_empty() && self.dialogs.is_empty() {
            self.open_dialog(Dialog::ConfirmClose {
                ids: close_requests,
                then: Then::CloseTabs,
            });
        }

        // Note it in the status bar when a model refers to textures this pack does not have
        // (the viewport itself keeps a notice up for as long as that is the case).
        let notices: Vec<String> = self
            .editors
            .values_mut()
            .filter_map(|editor| match editor {
                Editor::Model(model) => model.take_missing_notice(),
                _ => None,
            })
            .collect();
        for notice in notices {
            self.set_status(Level::Warning, &notice);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        if !std::mem::take(&mut self.title_stale) {
            return;
        }
        let title = match &self.pack {
            Some(pack) => format!("{} - OpenPax", pack.name()),
            None => "OpenPax".to_owned(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }
}

/// A compact icon button for the status bar. Returns true when clicked.
fn small_icon_button(ui: &mut egui::Ui, glyph: &str, tooltip: &str) -> bool {
    ui.add(
        egui::Button::new(RichText::new(glyph).size(14.0))
            .frame_when_inactive(false)
            .min_size(vec2(22.0, 20.0)),
    )
    .on_hover_text(tooltip)
    .clicked()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.thumbs.begin_frame();
        self.poll_jobs();
        self.autosave_tick(&ctx);
        self.update_title(&ctx);
        self.close_requested(&ctx);
        self.dropped_files(&ctx);
        self.shortcuts(&ctx);

        egui::Panel::top("menu_bar").show(ui, |ui| self.menu_bar(ui));
        let status = egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui));
        self.layout = crate::tour::Layout::default();
        if self.pack.is_some() {
            if self.settings.show_explorer {
                let panel = egui::Panel::left("explorer")
                    .resizable(true)
                    .default_size(260.0)
                    .size_range(180.0..=520.0)
                    .show(ui, |ui| self.explorer_panel(ui));
                self.layout.explorer = Some(panel.response.rect);
            }
            if self.settings.show_inspector {
                let panel = egui::Panel::right("inspector")
                    .resizable(true)
                    .default_size(300.0)
                    .size_range(240.0..=520.0)
                    .show(ui, |ui| self.inspector_panel(ui));
                self.layout.inspector = Some(panel.response.rect);
            }
        }
        let frame = egui::Frame::central_panel(ui.style()).inner_margin(0);
        let central = egui::CentralPanel::default()
            .frame(frame)
            .show(ui, |ui| self.central(ui));
        self.layout.central = Some(central.response.rect);

        self.show_dialog(&ctx);
        self.show_command_palette(&ctx);
        self.show_tour(&ctx);
        self.show_job(&ctx);
        self.toasts.show(&ctx, status.response.rect.height());
        self.confetti.show(&ctx);
        Self::drop_overlay(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.store_tools(&self.tools);
        eframe::set_value(storage, RECENT_KEY, &self.recent);
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
        self.remember_session();
        eframe::set_value(storage, SESSIONS_KEY, &self.sessions);
    }

    fn on_exit(&mut self) {
        // A normal exit has dealt with every unsaved change (saved or discarded), so the
        // recovery files are only needed after a crash.
        if self.dirty_ids().is_empty() {
            self.finish_recovery();
        }
    }
}

struct Viewer<'a> {
    pack: &'a Pack,
    render_state: Option<&'a eframe::egui_wgpu::RenderState>,
    asset_generation: u64,
    editors: &'a mut HashMap<u64, Editor>,
    tools: &'a mut ToolState,
    checker: &'a TextureHandle,
    show_hint: &'a mut bool,
    close_requests: &'a mut Vec<u64>,
}

impl TabViewer for Viewer<'_> {
    type Tab = u64;

    fn id(&mut self, tab: &mut u64) -> egui::Id {
        egui::Id::unique(("editor_tab", *tab))
    }

    fn title(&mut self, tab: &mut u64) -> egui::WidgetText {
        self.editors
            .get(tab)
            .map(Editor::title)
            .unwrap_or_default()
            .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut u64) {
        match self.editors.get_mut(tab) {
            Some(Editor::Texture(editor)) => editor.ui(ui, self.tools, self.checker),
            Some(Editor::Model(editor)) => {
                editor.ui(
                    ui,
                    self.pack,
                    self.render_state,
                    self.asset_generation,
                    self.show_hint,
                );
            }
            Some(Editor::Text(editor)) => editor.ui(ui),
            None => {}
        }
    }

    fn on_close(&mut self, tab: &mut u64) -> OnCloseResponse {
        if self.editors.get(tab).is_some_and(Editor::dirty) {
            self.close_requests.push(*tab);
            OnCloseResponse::Ignore
        } else {
            OnCloseResponse::Close
        }
    }

    fn allowed_in_windows(&self, _tab: &mut u64) -> bool {
        false
    }

    fn scroll_bars(&self, _tab: &u64) -> [bool; 2] {
        [false, false]
    }
}

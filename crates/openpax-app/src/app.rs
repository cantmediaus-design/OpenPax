//! Application state, top-level layout, menus and pack/file actions.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, TextureHandle, TextureOptions};
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, TabViewer};
use openpax_core::{AssetKind, Pack};
use openpax_paint::ToolKind;

use crate::dialogs::Dialog;
use crate::editor::{Editor, TextEditor, TextureEditor, ToolState};
use crate::explorer::{Explorer, Thumbnails, Tree};
use crate::model_editor::ModelEditor;

const RECENT_KEY: &str = "recent_packs";
const MAX_RECENT: usize = 8;

pub const TOOL_KEYS: [(ToolKind, Key, &str); 9] = [
    (ToolKind::Pencil, Key::B, "B"),
    (ToolKind::Eraser, Key::E, "E"),
    (ToolKind::Fill, Key::G, "G"),
    (ToolKind::Picker, Key::I, "I"),
    (ToolKind::Line, Key::L, "L"),
    (ToolKind::Rectangle, Key::R, "R"),
    (ToolKind::Ellipse, Key::O, "O"),
    (ToolKind::Select, Key::M, "M"),
    (ToolKind::Move, Key::V, "V"),
];

pub struct App {
    pub(crate) pack: Option<Pack>,
    tree: Tree,
    pub(crate) thumbs: Thumbnails,
    search: String,
    dock: DockState<u64>,
    pub(crate) editors: HashMap<u64, Editor>,
    next_id: u64,
    /// The editor tab that menus, shortcuts and the side panels act on.
    pub(crate) active: Option<u64>,
    pub(crate) tools: ToolState,
    checker: TextureHandle,
    recent: Vec<String>,
    pub(crate) dialog: Dialog,
    pub(crate) status: String,
    pub(crate) allow_quit: bool,
    /// GPU handles for the 3D viewport; `None` if eframe is not running on wgpu.
    render_state: Option<eframe::egui_wgpu::RenderState>,
    /// Bumped on every save so viewports reload textures edited in other tabs.
    asset_generation: u64,
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

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial_pack: Option<PathBuf>,
        initial_assets: Vec<String>,
    ) -> Self {
        let checker = cc.egui_ctx.load_texture(
            "checker",
            egui::ColorImage::from_rgba_unmultiplied(
                [2, 2],
                &[
                    58, 58, 58, 255, 74, 74, 74, 255, 74, 74, 74, 255, 58, 58, 58, 255,
                ],
            ),
            TextureOptions::NEAREST_REPEAT,
        );
        let recent: Vec<String> = cc
            .storage
            .and_then(|s| eframe::get_value(s, RECENT_KEY))
            .unwrap_or_default();

        let mut app = Self {
            pack: None,
            tree: Tree::default(),
            thumbs: Thumbnails::default(),
            search: String::new(),
            dock: DockState::new(Vec::new()),
            editors: HashMap::new(),
            next_id: 1,
            active: None,
            tools: ToolState::default(),
            checker,
            recent,
            dialog: Dialog::None,
            status: "Ready".to_owned(),
            allow_quit: false,
            render_state: cc.wgpu_render_state.clone(),
            asset_generation: 0,
        };
        // Open the pack given on the command line, else the last one if it is still there.
        if let Some(root) = initial_pack {
            app.open_pack(root);
            for rel_path in initial_assets {
                app.open_asset(&rel_path.replace('\\', "/"));
            }
        } else if let Some(last) = app.recent.first().cloned()
            && Path::new(&last).join("pack.mcmeta").is_file()
        {
            app.open_pack(PathBuf::from(last));
        }
        app
    }

    pub(crate) fn message(&mut self, text: impl Into<String>) {
        self.dialog = Dialog::Message(text.into());
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

    // ---- packs ----

    pub(crate) fn open_pack(&mut self, root: PathBuf) {
        if !self.dirty_ids().is_empty() {
            self.message("Save or close the modified files before switching packs.");
            return;
        }
        match Pack::open(&root) {
            Ok(pack) => {
                self.tree = Tree::build(&pack.index);
                self.status = format!(
                    "Opened {} ({} assets)",
                    pack.name(),
                    pack.index.entries.len()
                );
                self.pack = Some(pack);
                self.thumbs.clear();
                self.editors.clear();
                self.dock = DockState::new(Vec::new());
                self.active = None;

                let path = root.to_string_lossy().into_owned();
                self.recent.retain(|p| p != &path);
                self.recent.insert(0, path);
                self.recent.truncate(MAX_RECENT);
            }
            Err(e) => self.message(format!("Could not open pack:\n{e}")),
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

    fn import_zip(&mut self) {
        let Some(zip) = rfd::FileDialog::new()
            .set_title("Import resource pack zip")
            .add_filter("Resource pack", &["zip"])
            .pick_file()
        else {
            return;
        };
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
            self.message(format!("Could not import zip:\n{e}"));
            return;
        }
        // Some zips wrap the pack in a single top-level folder.
        let root = if dest.join("pack.mcmeta").is_file() {
            dest
        } else {
            std::fs::read_dir(&dest)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .find(|p| p.join("pack.mcmeta").is_file())
                .unwrap_or(dest)
        };
        self.open_pack(root);
    }

    fn export_zip(&mut self) {
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
                let unsaved = if self.dirty_ids().is_empty() {
                    ""
                } else {
                    " (unsaved edits not included)"
                };
                self.status = format!("Exported {count} files to {}{unsaved}", dest.display());
            }
            Err(e) => self.message(format!("Export failed:\n{e}")),
        }
    }

    pub(crate) fn rescan(&mut self) {
        let Some(pack) = &mut self.pack else { return };
        match pack.rescan() {
            Ok(()) => self.tree = Tree::build(&pack.index),
            Err(e) => self.message(format!("Could not rescan pack:\n{e}")),
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
            .map(|e| e.kind);
        let editor = match kind {
            Some(AssetKind::Texture) => {
                TextureEditor::open(pack, rel_path).map(|e| Editor::Texture(Box::new(e)))
            }
            Some(AssetKind::Model) if !as_text => match ModelEditor::open(pack, rel_path) {
                Ok(editor) => Ok(Editor::Model(Box::new(editor))),
                Err(e) => {
                    self.status = format!("Opened as text; not a model OpenPax can read: {e}");
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
            }
            Err(e) => self.message(format!("Could not open {rel_path}:\n{e}")),
        }
    }

    /// Saves one editor; reports failure in a dialog and returns `false`.
    pub(crate) fn save_editor(&mut self, id: u64) -> bool {
        let (Some(pack), Some(editor)) = (&self.pack, self.editors.get_mut(&id)) else {
            return false;
        };
        let rel_path = editor.rel_path().to_owned();
        match editor.save(pack) {
            Ok(()) => {
                self.thumbs.invalidate(&rel_path);
                self.asset_generation += 1;
                self.status = format!("Saved {rel_path}");
                self.rescan();
                true
            }
            Err(e) => {
                self.message(format!("Could not save {rel_path}:\n{e}"));
                false
            }
        }
    }

    fn save_all(&mut self) {
        for id in self.dirty_ids() {
            if !self.save_editor(id) {
                break;
            }
        }
    }

    /// Closes a tab without asking.
    pub(crate) fn close_editor(&mut self, id: u64) {
        if let Some(path) = self.dock.find_tab(&id) {
            self.dock.remove_tab(path);
        }
        self.editors.remove(&id);
    }

    // ---- input ----

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let cmd_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);

        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::S)))
            && let Some(id) = self.active
        {
            self.save_editor(id);
        }
        if ctx.text_edit_focused() || !matches!(self.dialog, Dialog::None) {
            return;
        }
        if let Some(editor) = model_mut(&mut self.editors, self.active) {
            ctx.input_mut(|i| {
                if i.consume_shortcut(&cmd_shift(Key::Z)) || i.consume_shortcut(&cmd(Key::Y)) {
                    editor.doc.redo();
                } else if i.consume_shortcut(&cmd(Key::Z)) {
                    editor.doc.undo();
                }
                if i.consume_shortcut(&cmd(Key::D)) {
                    editor.doc.duplicate_selected();
                }
                if i.key_pressed(Key::Delete) {
                    editor.doc.remove_selected();
                }
            });
            return;
        }
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };

        ctx.input_mut(|i| {
            if i.consume_shortcut(&cmd_shift(Key::Z)) || i.consume_shortcut(&cmd(Key::Y)) {
                editor.doc.redo();
            } else if i.consume_shortcut(&cmd(Key::Z)) {
                editor.doc.undo();
            }
            if i.consume_shortcut(&cmd(Key::A)) {
                editor.doc.select_all();
            }
            if i.consume_shortcut(&cmd(Key::D)) {
                editor.doc.deselect();
            }
            if i.key_pressed(Key::Delete) {
                editor.doc.clear_selected();
            }
            if i.modifiers.is_none() {
                for (tool, key, _) in TOOL_KEYS {
                    if i.key_pressed(key) {
                        self.tools.tool = tool;
                    }
                }
            }
        });
    }

    fn close_requested(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_quit {
            return;
        }
        let ids = self.dirty_ids();
        if !ids.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Dialog::ConfirmClose { ids, quit: true };
        }
    }

    // ---- layout ----

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let has_pack = self.pack.is_some();
        let has_editor = self.active.is_some();
        let has_texture = texture_mut(&mut self.editors, self.active).is_some();
        let item = |ui: &mut egui::Ui, enabled: bool, text: &str, shortcut: &str| {
            ui.add_enabled(enabled, egui::Button::new(text).shortcut_text(shortcut))
                .clicked()
        };

        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Pack…").clicked() {
                    self.dialog = Dialog::new_pack();
                }
                if ui.button("Open Pack Folder…").clicked() {
                    self.pick_and_open_pack();
                }
                if ui.button("Import Pack Zip…").clicked() {
                    self.import_zip();
                }
                ui.menu_button("Open Recent", |ui| {
                    if self.recent.is_empty() {
                        ui.weak("No recent packs");
                    }
                    for path in self.recent.clone() {
                        if ui.button(&path).clicked() {
                            self.open_pack(PathBuf::from(path));
                        }
                    }
                });
                ui.separator();
                if item(ui, has_pack, "New Texture…", "") {
                    self.dialog = Dialog::new_texture();
                }
                if item(ui, has_pack, "New Model…", "") {
                    self.dialog = Dialog::new_model();
                }
                if item(ui, has_editor, "Save", "Ctrl+S")
                    && let Some(id) = self.active
                {
                    self.save_editor(id);
                }
                if item(ui, has_editor, "Save All", "") {
                    self.save_all();
                }
                ui.separator();
                if item(ui, has_pack, "Export Zip…", "") {
                    self.export_zip();
                }
                if item(ui, has_pack, "Rescan Pack Folder", "") {
                    self.rescan();
                    self.thumbs.clear();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            ui.menu_button("Edit", |ui| {
                let active = self.active.and_then(|id| self.editors.get_mut(&id));
                let (can_undo, can_redo) = active
                    .as_ref()
                    .map_or((false, false), |e| e.can_undo_redo());
                let undo = item(ui, can_undo, "Undo", "Ctrl+Z");
                let redo = item(ui, can_redo, "Redo", "Ctrl+Y");
                if let Some(active) = active {
                    if undo {
                        active.undo();
                    }
                    if redo {
                        active.redo();
                    }
                }
                let editor = texture_mut(&mut self.editors, self.active);
                ui.separator();
                let select_all = item(ui, has_texture, "Select All", "Ctrl+A");
                let deselect = item(ui, has_texture, "Deselect", "Ctrl+D");
                let clear = item(ui, has_texture, "Clear Selection", "Del");
                ui.separator();
                let flip_h = item(ui, has_texture, "Flip Horizontally", "");
                let flip_v = item(ui, has_texture, "Flip Vertically", "");
                let resize = item(ui, has_texture, "Resize Canvas…", "");
                if let Some(editor) = editor {
                    let doc = &mut editor.doc;
                    if select_all {
                        doc.select_all();
                    }
                    if deselect {
                        doc.deselect();
                    }
                    if clear {
                        doc.clear_selected();
                    }
                    if flip_h {
                        doc.flip_horizontal();
                    }
                    if flip_v {
                        doc.flip_vertical();
                    }
                    if resize {
                        self.dialog = Dialog::Resize {
                            width: doc.width(),
                            height: doc.height(),
                        };
                    }
                }
            });

            ui.menu_button("View", |ui| {
                match texture_mut(&mut self.editors, self.active) {
                    Some(editor) => {
                        ui.checkbox(&mut editor.show_grid, "Pixel Grid");
                        ui.checkbox(&mut editor.tile_preview, "Tiling Preview");
                        if ui.button("Zoom to Fit").clicked() {
                            editor.fit_pending = true;
                        }
                    }
                    None => match model_mut(&mut self.editors, self.active) {
                        Some(editor) => {
                            ui.checkbox(&mut editor.show_grid, "Grid and Block Outline");
                            if ui.button("Reset View").clicked() {
                                editor.reset_view();
                            }
                        }
                        None => {
                            ui.weak("Open a texture or model to see view options");
                        }
                    },
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(editor) = texture_mut(&mut self.editors, self.active) {
                    let hover = editor
                        .hover
                        .map(|(x, y)| format!("({x}, {y})   "))
                        .unwrap_or_default();
                    ui.monospace(format!(
                        "{hover}{}×{}   {:.0}%",
                        editor.doc.width(),
                        editor.doc.height(),
                        editor.zoom * 100.0
                    ));
                } else if let Some(editor) = model_mut(&mut self.editors, self.active) {
                    let model = editor.doc.model();
                    ui.monospace(format!(
                        "{} elements   {} textures",
                        model.elements().len(),
                        model.textures.len()
                    ));
                }
            });
        });
    }

    fn explorer_panel(&mut self, ui: &mut egui::Ui) {
        let Some(pack) = &self.pack else { return };
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.strong(pack.name());
            ui.weak(format!("{} assets", pack.index.entries.len()));
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search assets…")
                .desired_width(f32::INFINITY),
        );
        ui.separator();

        let active = self
            .active
            .and_then(|id| self.editors.get(&id))
            .map(|e| e.rel_path());
        let mut explorer = Explorer {
            pack,
            thumbs: &mut self.thumbs,
            active,
            open: None,
            open_text: None,
        };
        egui::ScrollArea::both()
            .auto_shrink(false)
            .show(ui, |ui| explorer.show(ui, &self.tree, &self.search));
        let (open, open_text) = (explorer.open, explorer.open_text);
        if let Some(rel_path) = open {
            self.open_asset(&rel_path);
        }
        if let Some(rel_path) = open_text {
            self.open_asset_as(&rel_path, true);
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.25);
            ui.heading("OpenPax");
            ui.label("Open-source Minecraft resource pack studio");
            ui.add_space(16.0);
            if ui.button("New Pack…").clicked() {
                self.dialog = Dialog::new_pack();
            }
            if ui.button("Open Pack Folder…").clicked() {
                self.pick_and_open_pack();
            }
            if ui.button("Import Pack Zip…").clicked() {
                self.import_zip();
            }
            if !self.recent.is_empty() {
                ui.add_space(16.0);
                ui.weak("Recent");
                for path in self.recent.clone() {
                    if ui.link(&path).clicked() {
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
            ui.centered_and_justified(|ui| {
                ui.weak("Select an asset in the explorer, or use File → New Texture…");
            });
            self.active = None;
            return;
        }

        let Some(pack) = &self.pack else { return };
        let mut close_requests = Vec::new();
        let mut viewer = Viewer {
            pack,
            render_state: self.render_state.as_ref(),
            asset_generation: self.asset_generation,
            editors: &mut self.editors,
            tools: &mut self.tools,
            checker: &self.checker,
            close_requests: &mut close_requests,
        };
        DockArea::new(&mut self.dock)
            .style(egui_dock::Style::from_egui(ui.style().as_ref()))
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
        if !close_requests.is_empty() && matches!(self.dialog, Dialog::None) {
            self.dialog = Dialog::ConfirmClose {
                ids: close_requests,
                quit: false,
            };
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.thumbs.begin_frame();
        self.close_requested(&ctx);
        self.shortcuts(&ctx);

        egui::Panel::top("menu_bar").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui));
        if self.pack.is_some() {
            egui::Panel::left("explorer")
                .resizable(true)
                .default_size(260.0)
                .show(ui, |ui| self.explorer_panel(ui));
            egui::Panel::right("inspector")
                .resizable(true)
                .default_size(280.0)
                .show(ui, |ui| self.inspector_panel(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| self.central(ui));

        self.show_dialog(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, RECENT_KEY, &self.recent);
    }
}

struct Viewer<'a> {
    pack: &'a Pack,
    render_state: Option<&'a eframe::egui_wgpu::RenderState>,
    asset_generation: u64,
    editors: &'a mut HashMap<u64, Editor>,
    tools: &'a mut ToolState,
    checker: &'a TextureHandle,
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
                editor.ui(ui, self.pack, self.render_state, self.asset_generation);
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

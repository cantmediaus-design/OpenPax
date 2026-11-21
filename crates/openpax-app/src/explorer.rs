//! The pack explorer: a virtualised folder tree of assets with texture thumbnails.

use std::collections::{BTreeMap, HashMap, HashSet};

use eframe::egui::{self, Rect, RichText, Sense, TextureHandle, TextureOptions, Vec2, pos2, vec2};
use openpax_core::{AssetEntry, AssetIndex, AssetKind, Pack};
use serde::{Deserialize, Serialize};

use crate::theme::{self, ROW_HEIGHT, SPACE_S};
use crate::{icons, widgets};

pub const THUMB_SIZE: f32 = 16.0;
/// Thumbnails decoded per frame, so opening a big pack does not stall the UI.
const THUMBS_PER_FRAME: usize = 6;
const MAX_THUMB_PIXELS: u32 = 1024 * 1024;
/// Folders above this depth start out expanded (namespace and category).
const DEFAULT_OPEN_DEPTH: usize = 2;
const INDENT: f32 = 14.0;
/// Size of a thumbnail in the grid view, and of a whole grid cell.
const GRID_THUMB: f32 = 44.0;
const GRID_CELL: Vec2 = vec2(64.0, 72.0);

/// Which kinds of asset the explorer lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KindFilter {
    #[default]
    All,
    Textures,
    Models,
    States,
    Language,
    Sounds,
    Other,
}

impl KindFilter {
    pub const ALL: [Self; 7] = [
        Self::All,
        Self::Textures,
        Self::Models,
        Self::States,
        Self::Language,
        Self::Sounds,
        Self::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::All => "All files",
            Self::Textures => "Textures",
            Self::Models => "Models",
            Self::States => "Blockstates & items",
            Self::Language => "Language",
            Self::Sounds => "Sounds",
            Self::Other => "Other",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::All => icons::FUNNEL,
            Self::Textures => icons::IMAGE,
            Self::Models => icons::CUBE,
            Self::States => icons::FILE_CODE,
            Self::Language => icons::LANGUAGE,
            Self::Sounds => icons::SOUND,
            Self::Other => icons::FILE,
        }
    }

    pub fn accepts(self, entry: &AssetEntry) -> bool {
        let sounds = entry.kind == AssetKind::Sound
            || (entry.category.is_empty() && entry.path == "sounds.json");
        match self {
            Self::All => true,
            Self::Textures => entry.kind == AssetKind::Texture,
            Self::Models => entry.kind == AssetKind::Model,
            Self::States => matches!(
                entry.kind,
                AssetKind::Blockstate | AssetKind::ItemDefinition
            ),
            Self::Language => entry.kind == AssetKind::Lang,
            Self::Sounds => sounds,
            Self::Other => {
                !sounds
                    && !matches!(
                        entry.kind,
                        AssetKind::Texture
                            | AssetKind::Model
                            | AssetKind::Blockstate
                            | AssetKind::ItemDefinition
                            | AssetKind::Lang
                    )
            }
        }
    }
}

/// What the explorer lists besides the search text: a kind of asset and a namespace.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Filter {
    pub kind: KindFilter,
    pub namespace: Option<String>,
}

impl Filter {
    pub fn is_active(&self) -> bool {
        self.kind != KindFilter::All || self.namespace.is_some()
    }

    pub fn accepts(&self, entry: &AssetEntry) -> bool {
        self.kind.accepts(entry)
            && self
                .namespace
                .as_ref()
                .is_none_or(|n| *n == entry.namespace)
    }
}

#[derive(Default)]
struct Node {
    folders: BTreeMap<String, Node>,
    /// `(file name, index into AssetIndex::entries)`
    files: Vec<(String, usize)>,
    /// Files in this folder and all folders below it.
    total: usize,
}

/// One visible line of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Folder {
        /// `/`-separated path from the tree root, starting with a slash.
        path: String,
        name: String,
        depth: usize,
        open: bool,
        files: usize,
    },
    File {
        /// Index into `AssetIndex::entries`.
        index: usize,
        depth: usize,
    },
}

/// Assets arranged as namespace / category / folders, plus which folders are expanded.
#[derive(Default)]
pub struct Tree {
    root: Node,
    /// Folders whose expanded state differs from the default for their depth.
    toggled: HashSet<String>,
    rows: Option<Vec<Row>>,
    /// Lower-cased search text, the filter, and the entries they match.
    search: Option<(String, Filter, Vec<usize>)>,
}

impl Tree {
    pub fn build(index: &AssetIndex) -> Self {
        let mut tree = Self::default();
        tree.rebuild(index);
        tree
    }

    /// Rebuilds the tree for a changed index, keeping folders expanded as they were.
    pub fn rebuild(&mut self, index: &AssetIndex) {
        let mut root = Node::default();
        for (i, entry) in index.entries.iter().enumerate() {
            root.total += 1;
            let mut node = root.folders.entry(entry.namespace.clone()).or_default();
            node.total += 1;
            if !entry.category.is_empty() {
                node = node.folders.entry(entry.category.clone()).or_default();
                node.total += 1;
            }
            let mut parts: Vec<&str> = entry.path.split('/').collect();
            let file = parts.pop().unwrap_or_default();
            for part in parts {
                node = node.folders.entry(part.to_owned()).or_default();
                node.total += 1;
            }
            node.files.push((file.to_owned(), i));
        }
        self.root = root;
        self.rows = None;
        self.search = None;
    }

    pub fn toggle(&mut self, path: &str) {
        if !self.toggled.remove(path) {
            self.toggled.insert(path.to_owned());
        }
        self.rows = None;
    }

    /// The visible rows, top to bottom.
    pub fn rows(&mut self) -> &[Row] {
        if self.rows.is_none() {
            let mut rows = Vec::new();
            flatten(&self.root, "", 0, &self.toggled, &mut rows);
            self.rows = Some(rows);
        }
        self.rows.as_deref().unwrap_or_default()
    }

    /// Entries whose path contains `needle` (case-insensitive) and that pass `filter`.
    pub fn matches(&mut self, index: &AssetIndex, needle: &str, filter: &Filter) -> &[usize] {
        let needle = needle.trim().to_lowercase();
        if self
            .search
            .as_ref()
            .is_none_or(|(n, f, _)| *n != needle || f != filter)
        {
            let found = index
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| filter.accepts(e) && e.rel_path.to_lowercase().contains(&needle))
                .map(|(i, _)| i)
                .collect();
            self.search = Some((needle, filter.clone(), found));
        }
        self.search.as_ref().map_or(&[], |(_, _, found)| found)
    }
}

fn flatten(node: &Node, path: &str, depth: usize, toggled: &HashSet<String>, out: &mut Vec<Row>) {
    for (name, child) in &node.folders {
        let child_path = format!("{path}/{name}");
        let open = (depth < DEFAULT_OPEN_DEPTH) != toggled.contains(&child_path);
        out.push(Row::Folder {
            path: child_path.clone(),
            name: name.clone(),
            depth,
            open,
            files: child.total,
        });
        if open {
            flatten(child, &child_path, depth + 1, toggled, out);
        }
    }
    for (_, index) in &node.files {
        out.push(Row::File {
            index: *index,
            depth,
        });
    }
}

/// Lazily loaded texture thumbnails, keyed by pack-relative path.
#[derive(Default)]
pub struct Thumbnails {
    cache: HashMap<String, Option<TextureHandle>>,
    loaded_this_frame: usize,
}

impl Thumbnails {
    pub fn begin_frame(&mut self) {
        self.loaded_this_frame = 0;
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn invalidate(&mut self, rel_path: &str) {
        self.cache.remove(rel_path);
    }

    pub fn get(
        &mut self,
        ctx: &egui::Context,
        pack: &Pack,
        rel_path: &str,
    ) -> Option<&TextureHandle> {
        if !self.cache.contains_key(rel_path) {
            if self.loaded_this_frame >= THUMBS_PER_FRAME {
                ctx.request_repaint();
                return None;
            }
            self.loaded_this_frame += 1;
            let texture = pack
                .read(rel_path)
                .ok()
                .and_then(|bytes| openpax_paint::decode_png(&bytes).ok())
                .filter(|b| b.width() * b.height() <= MAX_THUMB_PIXELS && b.width() > 0)
                .map(|bitmap| {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [bitmap.width() as usize, bitmap.height() as usize],
                        bitmap.data(),
                    );
                    ctx.load_texture(format!("thumb:{rel_path}"), image, TextureOptions::NEAREST)
                });
            self.cache.insert(rel_path.to_owned(), texture);
        }
        self.cache.get(rel_path).and_then(Option::as_ref)
    }

    /// Draws a square thumbnail of a texture, or `fallback` while it is loading or missing.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        pack: &Pack,
        rel_path: &str,
        size: f32,
        fallback: &str,
    ) {
        match self.get(ui.ctx(), pack, rel_path) {
            Some(texture) => {
                // Show the first square of animation strips rather than squashing them.
                let [w, h] = texture.size();
                let side = w.min(h) as f32;
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(side / w as f32, side / h as f32));
                ui.add(
                    egui::Image::from_texture((texture.id(), vec2(size, size)))
                        .uv(uv)
                        .texture_options(TextureOptions::NEAREST),
                );
            }
            None => {
                ui.add_sized(
                    [size, size],
                    egui::Label::new(RichText::new(fallback).size(size).weak()).selectable(false),
                );
            }
        }
    }
}

/// `namespace:path` without the file extension, as models and blockstates refer to assets.
pub fn resource_location(entry: &AssetEntry) -> String {
    let path = entry
        .path
        .rsplit_once('.')
        .map_or(entry.path.as_str(), |(stem, _)| stem);
    format!("{}:{path}", entry.namespace)
}

/// Splits a tree folder path into its namespace and the path below `category`, if the folder
/// is inside that category.
fn folder_location(path: &str, category: &str) -> (String, String) {
    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let namespace = parts.next().unwrap_or("minecraft").to_owned();
    let below = if parts.next() == Some(category) {
        parts.collect::<Vec<_>>().join("/")
    } else {
        String::new()
    };
    (namespace, below)
}

/// What the user asked for in the explorer this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Open(String),
    OpenAsText(String),
    Copy(String),
    /// Show a pack-relative path in the system file manager.
    Reveal(String),
    /// Start the New Texture dialog with this namespace and folder.
    NewTexture(String, String),
    NewModel(String, String),
    Rename(String),
    Duplicate(String),
    Delete(String),
}

/// How the explorer lists assets.
pub struct View<'a> {
    pub search: &'a str,
    pub filter: &'a Filter,
    /// Large thumbnails in a grid instead of the folder tree.
    pub grid: bool,
}

pub struct Explorer<'a> {
    pub pack: &'a Pack,
    pub thumbs: &'a mut Thumbnails,
    /// Path of the asset shown in the focused editor tab.
    pub active: Option<&'a str>,
    pub actions: Vec<Action>,
}

impl Explorer<'_> {
    pub fn show(&mut self, ui: &mut egui::Ui, tree: &mut Tree, view: &View<'_>) {
        ui.spacing_mut().item_spacing.y = 0.0;
        if view.search.trim().is_empty() && !view.filter.is_active() && !view.grid {
            self.tree(ui, tree);
        } else {
            self.results(ui, tree, view);
        }
    }

    fn tree(&mut self, ui: &mut egui::Ui, tree: &mut Tree) {
        let mut toggle = None;
        let rows = tree.rows();
        if rows.is_empty() {
            widgets::empty_state(
                ui,
                icons::FOLDER_OPEN,
                "This pack has no assets yet",
                "Use File → New Texture… to add one.",
            );
            return;
        }
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            ROW_HEIGHT,
            rows.len(),
            |ui, range| {
                for row in &rows[range] {
                    match row {
                        Row::Folder {
                            path,
                            name,
                            depth,
                            open,
                            files,
                        } => {
                            if self.folder(ui, path, name, *depth, *open, *files) {
                                toggle = Some(path.clone());
                            }
                        }
                        Row::File { index, depth } => {
                            let entry = &self.pack.index.entries[*index];
                            self.file(ui, entry, entry.file_name(), *depth);
                        }
                    }
                }
            },
        );
        if let Some(path) = toggle {
            tree.toggle(&path);
        }
    }

    fn results(&mut self, ui: &mut egui::Ui, tree: &mut Tree, view: &View<'_>) {
        let found = tree.matches(&self.pack.index, view.search, view.filter);
        ui.horizontal(|ui| {
            ui.add_space(SPACE_S);
            let noun = if found.len() == 1 { "file" } else { "files" };
            widgets::caption(ui, &format!("{} {noun}", found.len()));
        });
        ui.add_space(SPACE_S);
        if found.is_empty() {
            let hint = if view.filter.is_active() {
                "Try a shorter search, or show all files."
            } else {
                "Try a shorter search."
            };
            widgets::empty_state(ui, icons::SEARCH, "No matching assets", hint);
            return;
        }
        if view.grid {
            self.grid(ui, found);
            return;
        }
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            ROW_HEIGHT,
            found.len(),
            |ui, range| {
                for index in &found[range] {
                    let entry = &self.pack.index.entries[*index];
                    let label = if entry.category.is_empty() {
                        format!("{}:{}", entry.namespace, entry.path)
                    } else {
                        format!("{}:{}/{}", entry.namespace, entry.category, entry.path)
                    };
                    self.file(ui, entry, &label, 0);
                }
            },
        );
    }

    /// Draws a folder row; returns true when it was clicked.
    fn folder(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        name: &str,
        depth: usize,
        open: bool,
        files: usize,
    ) -> bool {
        let (rect, response) = widgets::list_row(ui, false);
        let mut row = widgets::row_ui(ui, rect, depth as f32 * INDENT);
        let caret = if open {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        };
        row.label(RichText::new(caret).size(10.0).weak());
        let folder = if open {
            icons::FOLDER_OPEN
        } else {
            icons::FOLDER
        };
        row.label(
            RichText::new(folder)
                .size(THUMB_SIZE)
                .color(theme::of(ui).accent),
        );
        row.add(egui::Label::new(name).selectable(false).truncate());
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(SPACE_S);
            ui.label(RichText::new(files.to_string()).small().weak());
        });

        response.context_menu(|ui| {
            if ui
                .button(widgets::icon_label(icons::IMAGE, "New Texture Here…"))
                .clicked()
            {
                let (namespace, folder) = folder_location(path, "textures");
                self.actions.push(Action::NewTexture(namespace, folder));
            }
            if ui
                .button(widgets::icon_label(icons::CUBE, "New Model Here…"))
                .clicked()
            {
                let (namespace, folder) = folder_location(path, "models");
                self.actions.push(Action::NewModel(namespace, folder));
            }
            ui.separator();
            if ui
                .button(widgets::icon_label(icons::REVEAL, "Reveal in File Manager"))
                .clicked()
            {
                self.actions.push(Action::Reveal(format!("assets{path}")));
            }
        });
        response.clicked()
    }

    /// Assets as large thumbnails, several to a row.
    fn grid(&mut self, ui: &mut egui::Ui, found: &[usize]) {
        let columns = ((ui.available_width() / GRID_CELL.x).floor() as usize).max(1);
        let rows = found.len().div_ceil(columns);
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            GRID_CELL.y,
            rows,
            |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let start = row * columns;
                        for index in &found[start..(start + columns).min(found.len())] {
                            let entry = &self.pack.index.entries[*index];
                            self.tile(ui, entry);
                        }
                    });
                }
            },
        );
    }

    fn tile(&mut self, ui: &mut egui::Ui, entry: &AssetEntry) {
        let selected = self.active == Some(entry.rel_path.as_str());
        let (rect, response) = ui.allocate_exact_size(GRID_CELL, Sense::click());
        let fill = if selected {
            Some(ui.visuals().selection.bg_fill)
        } else if response.hovered() {
            Some(ui.visuals().widgets.hovered.weak_bg_fill)
        } else {
            None
        };
        if let Some(fill) = fill {
            ui.painter()
                .rect_filled(rect.shrink(2.0), theme::RADIUS, fill);
        }
        let thumb = Rect::from_center_size(
            pos2(rect.center().x, rect.top() + 6.0 + GRID_THUMB / 2.0),
            vec2(GRID_THUMB, GRID_THUMB),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(thumb));
        let glyph = icons::asset(entry.kind);
        if entry.kind == AssetKind::Texture {
            self.thumbs
                .show(&mut child, self.pack, &entry.rel_path, GRID_THUMB, glyph);
        } else {
            child.add_sized(
                [GRID_THUMB, GRID_THUMB],
                egui::Label::new(RichText::new(glyph).size(GRID_THUMB * 0.7).weak())
                    .selectable(false),
            );
        }
        let label_rect = Rect::from_min_max(
            pos2(rect.left() + 3.0, thumb.bottom() + 2.0),
            pos2(rect.right() - 3.0, rect.bottom() - 2.0),
        );
        let mut label_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(label_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        let name = entry.file_name();
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        label_ui.add(
            egui::Label::new(
                RichText::new(stem)
                    .small()
                    .color(widgets::row_text_color(ui, selected)),
            )
            .selectable(false)
            .truncate(),
        );
        if response.clicked() {
            self.actions.push(Action::Open(entry.rel_path.clone()));
        }
        response.context_menu(|ui| self.file_menu(ui, entry));
        response.on_hover_text(&entry.rel_path);
    }

    fn file(&mut self, ui: &mut egui::Ui, entry: &AssetEntry, label: &str, depth: usize) {
        let selected = self.active == Some(entry.rel_path.as_str());
        let (rect, response) = widgets::list_row(ui, selected);
        // Files line up with the names of sibling folders (past the caret).
        let mut row = widgets::row_ui(ui, rect, depth as f32 * INDENT + 16.0);
        let glyph = icons::asset(entry.kind);
        if entry.kind == AssetKind::Texture {
            self.thumbs
                .show(&mut row, self.pack, &entry.rel_path, THUMB_SIZE, glyph);
        } else {
            row.add_sized(
                [THUMB_SIZE, THUMB_SIZE],
                egui::Label::new(RichText::new(glyph).size(THUMB_SIZE).weak()).selectable(false),
            );
        }
        let color = widgets::row_text_color(ui, selected);
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if entry.has_mcmeta {
                ui.add_space(SPACE_S);
                ui.label(RichText::new(icons::ANIMATION).size(12.0).weak())
                    .on_hover_text("Has a .mcmeta file (animation or other metadata)");
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(RichText::new(label).color(color))
                        .selectable(false)
                        .truncate(),
                );
            });
        });

        if response.clicked() {
            self.actions.push(Action::Open(entry.rel_path.clone()));
        }
        response.context_menu(|ui| self.file_menu(ui, entry));
        response.on_hover_text(&entry.rel_path);
    }

    /// The right-click menu of a file.
    fn file_menu(&mut self, ui: &mut egui::Ui, entry: &AssetEntry) {
        {
            if ui
                .button(widgets::icon_label(icons::FILE, "Open"))
                .clicked()
            {
                self.actions.push(Action::Open(entry.rel_path.clone()));
            }
            if entry.kind.is_text()
                && ui
                    .button(widgets::icon_label(icons::FILE_CODE, "Open as Text"))
                    .clicked()
            {
                self.actions
                    .push(Action::OpenAsText(entry.rel_path.clone()));
            }
            ui.separator();
            if ui
                .button(widgets::icon_label(
                    icons::CLIPBOARD,
                    "Copy Resource Location",
                ))
                .clicked()
            {
                self.actions.push(Action::Copy(resource_location(entry)));
            }
            if ui
                .button(widgets::icon_label(icons::CLIPBOARD, "Copy Path"))
                .clicked()
            {
                self.actions.push(Action::Copy(entry.rel_path.clone()));
            }
            if ui
                .button(widgets::icon_label(icons::REVEAL, "Reveal in File Manager"))
                .clicked()
            {
                self.actions.push(Action::Reveal(entry.rel_path.clone()));
            }
            ui.separator();
            if ui
                .button(widgets::icon_label(icons::RENAME, "Rename or Move…"))
                .clicked()
            {
                self.actions.push(Action::Rename(entry.rel_path.clone()));
            }
            if ui
                .button(widgets::icon_label(icons::COPY, "Duplicate"))
                .clicked()
            {
                self.actions.push(Action::Duplicate(entry.rel_path.clone()));
            }
            if ui
                .button(widgets::icon_label(icons::TRASH, "Delete…"))
                .clicked()
            {
                self.actions.push(Action::Delete(entry.rel_path.clone()));
            }
        }
    }
}

/// Opens the system file manager with `path` selected (or its folder, where selecting a
/// file is not supported).
pub fn reveal(path: &std::path::Path) -> std::io::Result<()> {
    use std::process::Command;
    if cfg!(target_os = "windows") {
        // Explorer wants backslashes, and reports failure even when it worked.
        let native = path.to_string_lossy().replace('/', "\\");
        let argument = if path.is_dir() {
            native
        } else {
            format!("/select,{native}")
        };
        Command::new("explorer").arg(argument).spawn().map(drop)
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg("-R").arg(path).spawn().map(drop)
    } else {
        let folder = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        Command::new("xdg-open").arg(folder).spawn().map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> (tempfile::TempDir, Pack) {
        let dir = tempfile::tempdir().unwrap();
        let mut pack = Pack::create(dir.path().join("pack"), "test", 46).unwrap();
        for rel in [
            "assets/minecraft/textures/block/stone.png",
            "assets/minecraft/textures/block/deep/slate.png",
            "assets/minecraft/textures/item/stick.png",
            "assets/minecraft/models/block/stone.json",
            "assets/other/sounds.json",
        ] {
            pack.write(rel, b"{}").unwrap();
        }
        pack.rescan().unwrap();
        (dir, pack)
    }

    fn folders(tree: &mut Tree) -> Vec<(String, bool, usize)> {
        tree.rows()
            .iter()
            .filter_map(|row| match row {
                Row::Folder {
                    path, open, files, ..
                } => Some((path.clone(), *open, *files)),
                Row::File { .. } => None,
            })
            .collect()
    }

    #[test]
    fn rows_follow_expanded_folders() {
        let (_dir, pack) = pack();
        let mut tree = Tree::build(&pack.index);
        // Namespaces and categories start open; deeper folders start closed.
        assert_eq!(
            folders(&mut tree),
            [
                ("/minecraft".to_owned(), true, 4),
                ("/minecraft/models".to_owned(), true, 1),
                ("/minecraft/models/block".to_owned(), false, 1),
                ("/minecraft/textures".to_owned(), true, 3),
                ("/minecraft/textures/block".to_owned(), false, 2),
                ("/minecraft/textures/item".to_owned(), false, 1),
                ("/other".to_owned(), true, 1),
            ]
        );
        let files = |tree: &mut Tree| {
            tree.rows()
                .iter()
                .filter(|r| matches!(r, Row::File { .. }))
                .count()
        };
        assert_eq!(files(&mut tree), 1, "only the loose sounds.json is visible");

        tree.toggle("/minecraft/textures/block");
        assert_eq!(files(&mut tree), 2);
        assert!(
            folders(&mut tree)
                .iter()
                .any(|(path, open, _)| path == "/minecraft/textures/block/deep" && !open)
        );
        tree.toggle("/minecraft");
        assert_eq!(folders(&mut tree).len(), 2);

        // A rescan keeps the user's expanded folders.
        tree.rebuild(&pack.index);
        assert_eq!(folders(&mut tree).len(), 2);
        tree.toggle("/minecraft");
        assert_eq!(files(&mut tree), 2);
    }

    #[test]
    fn search_matches_any_part_of_the_path() {
        let (_dir, pack) = pack();
        let mut tree = Tree::build(&pack.index);
        let all = Filter::default();
        assert_eq!(tree.matches(&pack.index, " STONE ", &all).len(), 2);
        assert_eq!(tree.matches(&pack.index, "deep/", &all).len(), 1);
        assert!(tree.matches(&pack.index, "nothing", &all).is_empty());
    }

    #[test]
    fn filters_narrow_by_kind_and_namespace() {
        let (_dir, pack) = pack();
        let mut tree = Tree::build(&pack.index);
        let kind = |kind| Filter {
            kind,
            namespace: None,
        };
        let count = |tree: &mut Tree, search: &str, filter: &Filter| {
            tree.matches(&pack.index, search, filter).len()
        };
        assert_eq!(count(&mut tree, "", &kind(KindFilter::Textures)), 3);
        assert_eq!(count(&mut tree, "stone", &kind(KindFilter::Models)), 1);
        assert_eq!(count(&mut tree, "", &kind(KindFilter::Sounds)), 1);
        assert_eq!(count(&mut tree, "", &kind(KindFilter::Other)), 0);
        let other = Filter {
            kind: KindFilter::All,
            namespace: Some("other".to_owned()),
        };
        assert!(other.is_active());
        assert_eq!(count(&mut tree, "", &other), 1);
        // Every asset is in exactly one of the specific kinds.
        let total: usize = KindFilter::ALL[1..]
            .iter()
            .map(|k| count(&mut tree, "", &kind(*k)))
            .sum();
        assert_eq!(total, pack.index.entries.len());
    }

    #[test]
    fn locations_come_from_entries_and_folders() {
        let (_dir, pack) = pack();
        let slate = pack
            .index
            .entries
            .iter()
            .find(|e| e.rel_path.ends_with("slate.png"))
            .unwrap();
        assert_eq!(resource_location(slate), "minecraft:block/deep/slate");

        assert_eq!(
            folder_location("/minecraft/textures/block/deep", "textures"),
            ("minecraft".to_owned(), "block/deep".to_owned())
        );
        assert_eq!(
            folder_location("/minecraft/textures/block", "models"),
            ("minecraft".to_owned(), String::new())
        );
        assert_eq!(
            folder_location("/other", "textures"),
            ("other".to_owned(), String::new())
        );
    }
}

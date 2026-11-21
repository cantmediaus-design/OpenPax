//! The pack explorer: a folder tree of assets with texture thumbnails.

use std::collections::{BTreeMap, HashMap};

use eframe::egui::{self, Rect, TextureHandle, TextureOptions, pos2, vec2};
use openpax_core::{AssetEntry, AssetIndex, AssetKind, Pack};

const THUMB_SIZE: f32 = 16.0;
/// Thumbnails decoded per frame, so opening a big pack does not stall the UI.
const THUMBS_PER_FRAME: usize = 6;
const MAX_THUMB_PIXELS: u32 = 1024 * 1024;

#[derive(Default)]
struct Node {
    folders: BTreeMap<String, Node>,
    /// `(file name, index into AssetIndex::entries)`
    files: Vec<(String, usize)>,
}

/// Assets arranged as namespace / category / folders.
#[derive(Default)]
pub struct Tree {
    root: Node,
}

impl Tree {
    pub fn build(index: &AssetIndex) -> Self {
        let mut root = Node::default();
        for (i, entry) in index.entries.iter().enumerate() {
            let mut node = root.folders.entry(entry.namespace.clone()).or_default();
            if !entry.category.is_empty() {
                node = node.folders.entry(entry.category.clone()).or_default();
            }
            let mut parts: Vec<&str> = entry.path.split('/').collect();
            let file = parts.pop().unwrap_or_default();
            for part in parts {
                node = node.folders.entry(part.to_owned()).or_default();
            }
            node.files.push((file.to_owned(), i));
        }
        Self { root }
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

    fn get(&mut self, ctx: &egui::Context, pack: &Pack, rel_path: &str) -> Option<&TextureHandle> {
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
}

/// A short plain-text tag for assets without a thumbnail (emoji coverage in the UI font is spotty).
fn icon(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Texture => "img",
        AssetKind::Sound => "♪",
        AssetKind::Lang | AssetKind::Text => "T",
        AssetKind::Font => "F",
        AssetKind::Other => "?",
        _ => "{ }",
    }
}

pub struct Explorer<'a> {
    pub pack: &'a Pack,
    pub thumbs: &'a mut Thumbnails,
    /// Path of the asset shown in the focused editor tab.
    pub active: Option<&'a str>,
    /// Set to the asset the user clicked.
    pub open: Option<String>,
}

impl Explorer<'_> {
    pub fn show(&mut self, ui: &mut egui::Ui, tree: &Tree, search: &str) {
        let needle = search.trim().to_lowercase();
        if needle.is_empty() {
            self.node(ui, &tree.root, "", 0);
        } else {
            let entries = &self.pack.index.entries;
            let mut any = false;
            for entry in entries
                .iter()
                .filter(|e| e.rel_path.to_lowercase().contains(&needle))
            {
                any = true;
                let label = format!("{}:{}", entry.namespace, entry.path);
                self.file(ui, entry, &label);
            }
            if !any {
                ui.weak("No matching assets");
            }
        }
    }

    fn node(&mut self, ui: &mut egui::Ui, node: &Node, path: &str, depth: usize) {
        for (name, child) in &node.folders {
            let child_path = format!("{path}/{name}");
            egui::CollapsingHeader::new(name)
                .id_salt(&child_path)
                .default_open(depth < 3)
                .show(ui, |ui| self.node(ui, child, &child_path, depth + 1));
        }
        for (name, index) in &node.files {
            let entry = &self.pack.index.entries[*index];
            self.file(ui, entry, name);
        }
    }

    fn file(&mut self, ui: &mut egui::Ui, entry: &AssetEntry, label: &str) {
        ui.horizontal(|ui| {
            let thumb = (entry.kind == AssetKind::Texture)
                .then(|| self.thumbs.get(ui.ctx(), self.pack, &entry.rel_path))
                .flatten();
            match thumb {
                Some(texture) => {
                    // Show the first square of animation strips rather than squashing them.
                    let [w, h] = texture.size();
                    let side = w.min(h) as f32;
                    let uv =
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(side / w as f32, side / h as f32));
                    ui.add(
                        egui::Image::from_texture((texture.id(), vec2(THUMB_SIZE, THUMB_SIZE)))
                            .uv(uv)
                            .texture_options(TextureOptions::NEAREST),
                    );
                }
                None => {
                    ui.add_sized(
                        [THUMB_SIZE, THUMB_SIZE],
                        egui::Label::new(egui::RichText::new(icon(entry.kind)).weak()),
                    );
                }
            }
            let selected = self.active == Some(entry.rel_path.as_str());
            let suffix = if entry.has_mcmeta { "  •" } else { "" };
            let response = ui
                .selectable_label(selected, format!("{label}{suffix}"))
                .on_hover_text(&entry.rel_path);
            if response.clicked() {
                self.open = Some(entry.rel_path.clone());
            }
        });
    }
}

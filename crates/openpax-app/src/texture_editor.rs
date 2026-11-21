//! The pixel editor tab: document, view, canvas drawing and pointer input.

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, PointerButton, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, TextureHandle, TextureId, TextureOptions, Vec2, pos2, vec2,
};
use openpax_core::{Animation, Pack, TextureMeta};
use openpax_paint::analysis::{CheckOptions, Issue};
use openpax_paint::color::VisionSim;
use openpax_paint::tiles::{GridKind, TileRef};
use openpax_paint::{
    Bitmap, Document, Guide, LayerKind, Palette, PlayDirection, RenderOptions, Rgba, SelectMode,
    ToolKind, ToolSettings,
};
use serde::{Deserialize, Serialize};

use crate::theme::{self, RAIL_WIDTH, TOOLBAR_HEIGHT};

pub const ZOOM_LEVELS: [f32; 16] = [
    0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0, 96.0, 128.0,
];
pub const MIN_ZOOM: f32 = ZOOM_LEVELS[0];
pub const MAX_ZOOM: f32 = ZOOM_LEVELS[ZOOM_LEVELS.len() - 1];
pub const MAX_BRUSH: u32 = 64;
pub const MAX_RECENT_COLORS: usize = 12;
/// Layer thumbnails of documents up to this many pixels refresh while a stroke is in progress.
const LIVE_THUMB_PIXELS: u32 = 256 * 256;
pub const LAYERS_JSON: &str = ".layers.json";
pub const LAYERS_PNG: &str = ".layers.png";
/// Milliseconds per Minecraft game tick.
pub const TICK_MS: u32 = 50;
const RULER: f32 = 18.0;
const TIMELINE_HEIGHT: f32 = 132.0;

/// Which copies the tiling preview draws around the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tiling {
    #[default]
    Off,
    Both,
    Horizontal,
    Vertical,
}

impl Tiling {
    pub const ALL: [Tiling; 4] = [Self::Off, Self::Both, Self::Horizontal, Self::Vertical];

    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Both => "Both directions",
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Both,
            Self::Both => Self::Horizontal,
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Off,
        }
    }
}

/// What the secondary (right) mouse button does on the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RightButton {
    #[default]
    SecondaryColor,
    Eraser,
    Picker,
}

/// What the middle mouse button does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MiddleButton {
    #[default]
    Pan,
    Picker,
}

/// Canvas background behind transparent pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Background {
    Checker,
    Solid(Rgba),
}

/// View and input preferences shared by every texture tab.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewPrefs {
    pub grid_color: Rgba,
    /// The pixel grid shows from this zoom up.
    pub grid_min_zoom: f32,
    pub tile_grid: bool,
    pub tile_grid_size: (u32, u32),
    pub tile_grid_offset: (i32, i32),
    pub tile_grid_color: Rgba,
    pub tile_grid_kind: GridKind,
    pub background: Background,
    /// Pixels per checkerboard square.
    pub checker_size: u32,
    pub checker_a: Rgba,
    pub checker_b: Rgba,
    /// The wheel zooms in whole steps only (no blurry in-between sizes).
    pub integer_zoom: bool,
    pub rulers: bool,
    pub snap_to_guides: bool,
    pub tiling: Tiling,
    pub onion_before: u32,
    pub onion_after: u32,
    pub onion_opacity: f32,
    /// Tint previous frames red and next frames blue.
    pub onion_tint: bool,
    pub play_direction: PlayDirection,
    pub play_loop: bool,
    pub vision: VisionSim,
    /// Pen pressure response: < 1 softer, > 1 harder.
    pub pressure_curve: f32,
    pub right_button: RightButton,
    pub middle_button: MiddleButton,
    /// Tools on the extra mouse buttons (back / forward).
    pub extra1: Option<ToolKind>,
    pub extra2: Option<ToolKind>,
    pub show_timeline: bool,
    pub show_issues: bool,
    pub checks: CheckOptions,
}

impl Default for ViewPrefs {
    fn default() -> Self {
        Self {
            grid_color: [128, 128, 128, 90],
            grid_min_zoom: 6.0,
            tile_grid: false,
            tile_grid_size: (16, 16),
            tile_grid_offset: (0, 0),
            tile_grid_color: [0, 160, 255, 160],
            tile_grid_kind: GridKind::Rectangular,
            background: Background::Checker,
            checker_size: 1,
            checker_a: [200, 200, 200, 255],
            checker_b: [255, 255, 255, 255],
            integer_zoom: true,
            rulers: false,
            snap_to_guides: true,
            tiling: Tiling::Off,
            onion_before: 1,
            onion_after: 0,
            onion_opacity: 0.35,
            onion_tint: true,
            play_direction: PlayDirection::Forward,
            play_loop: true,
            vision: VisionSim::Normal,
            pressure_curve: 1.0,
            right_button: RightButton::SecondaryColor,
            middle_button: MiddleButton::Pan,
            extra1: None,
            extra2: None,
            show_timeline: true,
            show_issues: false,
            checks: CheckOptions {
                strays: false,
                seams: true,
                ..Default::default()
            },
        }
    }
}

/// A saved brush shape.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomBrush {
    pub name: String,
    pub image: Arc<Bitmap>,
}

/// What the tile stamp places.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileBrush {
    pub tile: TileRef,
    /// Paint a terrain with autotiling instead of a fixed tile.
    pub terrain: Option<usize>,
    /// Pick randomly (by weight) among these tiles.
    pub random: Vec<usize>,
}

/// Tool selection shared by all texture editors.
pub struct ToolState {
    pub tool: ToolKind,
    /// Brush settings; `settings.color` is the primary colour.
    pub settings: ToolSettings,
    /// The colour the secondary mouse button paints with.
    pub secondary: Rgba,
    pub palette: Palette,
    /// Colours painted with lately, most recent first.
    pub recent: Vec<Rgba>,
    pub view: ViewPrefs,
    pub brushes: Vec<CustomBrush>,
    pub tile: TileBrush,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            tool: ToolKind::Pencil,
            settings: ToolSettings::default(),
            secondary: [255, 255, 255, 255],
            palette: Palette::default(),
            recent: Vec::new(),
            view: ViewPrefs::default(),
            brushes: Vec::new(),
            tile: TileBrush::default(),
        }
    }
}

impl ToolState {
    pub fn swap_colors(&mut self) {
        std::mem::swap(&mut self.settings.color, &mut self.secondary);
    }

    /// Remembers a colour that was just painted with.
    pub fn note_color(&mut self, color: Rgba) {
        self.recent.retain(|c| *c != color);
        self.recent.insert(0, color);
        self.recent.truncate(MAX_RECENT_COLORS);
    }

    pub fn resize_brush(&mut self, delta: i32) {
        let size = self.settings.size as i32 + delta;
        self.settings.size = size.clamp(1, MAX_BRUSH as i32) as u32;
    }

    /// Loads the user's saved brushes (PNG files in the brushes folder).
    pub fn load_brushes(&mut self) {
        let dir = crate::user_files::dir("brushes");
        let mut brushes = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for path in entries.flatten().map(|e| e.path()) {
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("png"))
                    && let Ok(bytes) = std::fs::read(&path)
                    && let Ok(image) = openpax_paint::decode_png(&bytes)
                {
                    let name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    brushes.push(CustomBrush {
                        name,
                        image: Arc::new(image),
                    });
                }
            }
        }
        brushes.sort_by(|a, b| a.name.cmp(&b.name));
        self.brushes = brushes;
    }

    /// Saves a brush shape to the brushes folder and adds it to the list.
    pub fn save_brush(&mut self, name: &str, image: Bitmap) -> Result<(), String> {
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == ' ' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = crate::user_files::dir("brushes").join(format!("{safe}.png"));
        let png = openpax_paint::encode_png(&image).map_err(|e| e.to_string())?;
        std::fs::write(&path, png).map_err(|e| e.to_string())?;
        self.brushes.retain(|b| b.name != safe);
        self.brushes.push(CustomBrush {
            name: safe,
            image: Arc::new(image),
        });
        Ok(())
    }

    pub fn delete_brush(&mut self, name: &str) {
        let path = crate::user_files::dir("brushes").join(format!("{name}.png"));
        let _ = std::fs::remove_file(path);
        self.brushes.retain(|b| b.name != name);
    }
}

/// Splits a Minecraft animation strip into frames (or keeps a still image as one frame).
pub fn doc_from_image(bitmap: Bitmap, meta: &TextureMeta) -> Document {
    let (w, h) = (bitmap.width(), bitmap.height());
    if let Some(anim) = &meta.animation {
        let (fw, fh) = anim.frame_size(w, h);
        let count = anim.frame_count(w, h);
        if count > 1 && w % fw == 0 && h % fh == 0 {
            let columns = (w / fw).max(1);
            let frames: Vec<Bitmap> = (0..count)
                .map(|i| {
                    bitmap.crop(openpax_paint::Rect::new(
                        ((i % columns) * fw) as i32,
                        ((i / columns) * fh) as i32,
                        fw as i32,
                        fh as i32,
                    ))
                })
                .collect();
            let doc = Document::from_frames(fw, fh, frames);
            let mut parts = doc.parts();
            let ticks = anim.frametime.unwrap_or(1).max(1);
            for f in &mut parts.frames {
                f.duration_ms = ticks * TICK_MS;
            }
            return Document::from_parts(parts);
        }
    }
    Document::from_bitmap(bitmap)
}

/// The image saved to the pack: the flattened frame, or all frames as a vertical strip.
pub fn export_image(doc: &Document) -> Bitmap {
    let count = doc.frame_count();
    if count <= 1 {
        return doc.flatten();
    }
    let (w, h) = (doc.width(), doc.height());
    let mut strip = Bitmap::new(w, h * count as u32);
    for f in 0..count {
        strip.blit(&doc.flatten_frame(f), 0, (f as u32 * h) as i32);
    }
    strip
}

/// Brings the `.mcmeta` animation in line with the document's frames and durations.
pub fn sync_meta(doc: &Document, meta: &mut TextureMeta) {
    let count = doc.frame_count();
    if count <= 1 {
        return;
    }
    let (w, h) = (doc.width(), doc.height() * count as u32);
    let anim = meta.animation.get_or_insert_with(Animation::default);
    // Non-square frames need their size spelled out.
    if doc.width() == doc.height() {
        anim.width = None;
        anim.height = None;
    } else {
        anim.width = Some(doc.width());
        anim.height = Some(doc.height());
    }
    let ticks: Vec<u32> = doc
        .frames()
        .iter()
        .map(|f| (f.duration_ms as f32 / TICK_MS as f32).round().max(1.0) as u32)
        .collect();
    let uniform = ticks.iter().all(|t| *t == ticks[0]);
    let custom_valid = anim.frames.as_ref().is_some_and(|fr| {
        anim.sequence(w, h)
            .iter()
            .all(|(i, _)| (*i as usize) < count)
            && !fr.is_empty()
    });
    if uniform {
        anim.frametime = (ticks[0] != 1 || anim.frametime.is_some()).then_some(ticks[0]);
        if !custom_valid {
            anim.frames = None;
        }
    } else {
        // The most common duration becomes the default; the rest are listed per frame.
        let mut counts = std::collections::BTreeMap::new();
        for t in &ticks {
            *counts.entry(*t).or_insert(0) += 1;
        }
        let common = counts
            .iter()
            .max_by_key(|(_, n)| **n)
            .map_or(1, |(t, _)| *t);
        anim.frametime = Some(common);
        let sequence: Vec<(u32, u32)> = ticks
            .iter()
            .enumerate()
            .map(|(i, t)| (i as u32, *t))
            .collect();
        anim.set_sequence(&sequence, w, h);
    }
}

/// Whether the sidecar is needed to keep everything the PNG cannot hold.
fn needs_sidecar(doc: &Document) -> bool {
    doc.layers().len() > 1
        || !doc.tags().is_empty()
        || doc.extras != openpax_paint::Extras::default()
        || doc.layers().iter().any(|l| {
            l.kind != LayerKind::Pixel
                || !l.effects.is_empty()
                || l.blend != openpax_paint::BlendMode::Normal
        })
        || (0..doc.frame_count()).any(|f| doc.is_linked(0, f))
}

/// Files of a texture as (suffix after the asset path, bytes); empty bytes mean remove.
pub type TextureFiles = Vec<(&'static str, Vec<u8>)>;

pub struct TextureEditor {
    pub rel_path: String,
    pub doc: Document,
    saved_revision: u64,
    pub meta: TextureMeta,
    saved_meta: TextureMeta,
    texture: Option<TextureHandle>,
    texture_key: (u64, usize, VisionSim),
    /// Flattened frames for the timeline, preview and onion skin: `(revision, texture)`.
    frame_textures: Vec<Option<(u64, TextureHandle)>>,
    layer_thumbs: Vec<TextureHandle>,
    layer_thumbs_key: (u64, usize),
    pub zoom: f32,
    pan: Vec2,
    pub fit_pending: bool,
    pub show_grid: bool,
    pub onion_skin: bool,
    /// Pixel under the cursor, for the status bar.
    pub hover: Option<(i32, i32)>,
    /// The mouse button and tool of the stroke in progress.
    stroke_button: Option<PointerButton>,
    /// Layer being renamed in the layers panel: `(index, text so far, focus requested)`.
    pub renaming: Option<(usize, String, bool)>,
    /// Points clicked so far with the polyline or polygon tool.
    pub poly_points: Vec<(i32, i32)>,
    pub playing: bool,
    play_order: Vec<usize>,
    play_pos: usize,
    play_next_at: f64,
    /// Problems found by the checks, for the overlay.
    pub issues: Vec<Issue>,
    issues_revision: u64,
    /// A guide being dragged: `(vertical, existing index or None for a new one, position)`.
    guide_drag: Option<(bool, Option<usize>, i32)>,
    /// The last tile cell painted in this drag.
    last_tile_cell: Option<(i32, i32)>,
    /// A frame being dragged in the timeline.
    pub frame_drag: Option<usize>,
    /// Pen pressure of the latest touch event.
    pressure: Option<f32>,
    /// A message for the status bar (why a stroke did not start).
    pub notice: Option<&'static str>,
    /// Wheel movement not yet turned into a zoom step.
    scroll_acc: f32,
}

impl TextureEditor {
    pub fn open(pack: &Pack, rel_path: &str) -> Result<Self, String> {
        let bytes = pack.read(rel_path).map_err(|e| e.to_string())?;
        let bitmap = openpax_paint::decode_png(&bytes).map_err(|e| e.to_string())?;
        let meta = pack
            .texture_meta(rel_path)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();

        // Layers live in a sidecar; use them only if they still match the PNG on disk.
        let layered = pack
            .read(&Pack::sidecar_path(rel_path, LAYERS_JSON))
            .ok()
            .zip(pack.read(&Pack::sidecar_path(rel_path, LAYERS_PNG)).ok())
            .and_then(|(json, png)| {
                openpax_paint::decode_layered(&String::from_utf8_lossy(&json), &png).ok()
            })
            .filter(|doc| export_image(doc) == bitmap);
        let doc = layered.unwrap_or_else(|| doc_from_image(bitmap, &meta));
        Ok(Self::with_doc(rel_path, doc, meta))
    }

    pub fn with_doc(rel_path: &str, doc: Document, meta: TextureMeta) -> Self {
        Self {
            rel_path: rel_path.to_owned(),
            saved_revision: doc.revision(),
            doc,
            saved_meta: meta.clone(),
            meta,
            texture: None,
            texture_key: (u64::MAX, 0, VisionSim::Normal),
            frame_textures: Vec::new(),
            layer_thumbs: Vec::new(),
            layer_thumbs_key: (u64::MAX, 0),
            zoom: 8.0,
            pan: Vec2::ZERO,
            fit_pending: true,
            show_grid: true,
            onion_skin: false,
            hover: None,
            stroke_button: None,
            renaming: None,
            poly_points: Vec::new(),
            playing: false,
            play_order: Vec::new(),
            play_pos: 0,
            play_next_at: 0.0,
            issues: Vec::new(),
            issues_revision: u64::MAX,
            guide_drag: None,
            last_tile_cell: None,
            frame_drag: None,
            pressure: None,
            notice: None,
            scroll_acc: 0.0,
        }
    }

    pub fn dirty(&self) -> bool {
        self.doc.revision() != self.saved_revision || self.meta != self.saved_meta
    }

    /// The files [`Self::save`] writes, as `(suffix after the asset path, bytes)`; an empty
    /// byte list means "remove this file".
    /// Also returns the `.mcmeta` contents to write.
    pub fn files(&self) -> Result<(TextureFiles, TextureMeta), String> {
        let mut meta = self.meta.clone();
        sync_meta(&self.doc, &mut meta);
        let png = openpax_paint::encode_png(&export_image(&self.doc)).map_err(|e| e.to_string())?;
        let mut files = vec![("", png)];
        if needs_sidecar(&self.doc) {
            let (json, strip) =
                openpax_paint::encode_layered(&self.doc).map_err(|e| e.to_string())?;
            files.push((LAYERS_JSON, json.into_bytes()));
            files.push((LAYERS_PNG, strip));
        } else {
            files.push((LAYERS_JSON, Vec::new()));
            files.push((LAYERS_PNG, Vec::new()));
        }
        Ok((files, meta))
    }

    pub fn save(&mut self, pack: &Pack) -> Result<(), String> {
        self.doc.stroke_end();
        self.doc.commit_floating();
        let (files, meta) = self.files()?;
        self.meta = meta;
        for (suffix, bytes) in files {
            match suffix {
                "" => pack
                    .write(&self.rel_path, &bytes)
                    .map_err(|e| e.to_string())?,
                suffix if bytes.is_empty() => pack
                    .remove(&Pack::sidecar_path(&self.rel_path, suffix))
                    .map_err(|e| e.to_string())?,
                suffix => pack
                    .write(&Pack::sidecar_path(&self.rel_path, suffix), &bytes)
                    .map_err(|e| e.to_string())?,
            }
        }
        if self.meta != self.saved_meta {
            pack.write_texture_meta(&self.rel_path, &self.meta)
                .map_err(|e| e.to_string())?;
            self.saved_meta = self.meta.clone();
        }
        self.saved_revision = self.doc.revision();
        Ok(())
    }

    /// Replaces the image and metadata with recovered, unsaved ones.
    pub fn restore(&mut self, doc: Document, meta: TextureMeta) {
        self.doc = doc;
        self.meta = meta;
        // Neither revision can match the new document's, so it counts as unsaved and redraws.
        self.saved_revision = u64::MAX;
        self.texture_key.0 = u64::MAX;
        self.layer_thumbs_key.0 = u64::MAX;
        self.frame_textures.clear();
        self.fit_pending = true;
    }

    /// The canvas image as a GPU texture (current frame, reference layers included).
    pub fn texture(&mut self, ctx: &egui::Context, vision: VisionSim) -> TextureId {
        let key = (self.doc.revision(), self.doc.current_frame(), vision);
        if self.texture.is_none() || self.texture_key != key {
            let mut flat = self.doc.render(
                self.doc.current_frame(),
                RenderOptions {
                    reference: true,
                    solo: None,
                },
            );
            if vision != VisionSim::Normal {
                flat.map_pixels(|_, _, c| vision.apply(c));
            }
            let image = color_image(&flat);
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => {
                    let name = format!("doc:{}", self.rel_path);
                    self.texture = Some(ctx.load_texture(name, image, TextureOptions::NEAREST));
                }
            }
            self.texture_key = key;
        }
        self.texture
            .as_ref()
            .expect("texture was just created")
            .id()
    }

    /// One flattened frame as a texture (cached until the document changes).
    pub fn frame_texture(&mut self, ctx: &egui::Context, frame: usize) -> TextureId {
        let revision = self.doc.revision();
        self.frame_textures
            .resize_with(self.doc.frame_count(), || None);
        let stale = self.frame_textures[frame]
            .as_ref()
            .is_none_or(|(r, _)| *r != revision);
        // Big documents wait for the stroke to finish.
        let live = self.doc.width() * self.doc.height() <= LIVE_THUMB_PIXELS;
        if stale && (live || !self.doc.stroke_active() || self.frame_textures[frame].is_none()) {
            let image = color_image(&self.doc.flatten_frame(frame));
            let name = format!("frame{frame}:{}", self.rel_path);
            match &mut self.frame_textures[frame] {
                Some((r, texture)) => {
                    texture.set(image, TextureOptions::NEAREST);
                    *r = revision;
                }
                slot @ None => {
                    *slot = Some((
                        revision,
                        ctx.load_texture(name, image, TextureOptions::NEAREST),
                    ))
                }
            }
        }
        self.frame_textures[frame]
            .as_ref()
            .expect("filled above")
            .1
            .id()
    }

    /// One texture per layer (bottom first) for the layers panel, at the current frame.
    pub fn layer_textures(&mut self, ctx: &egui::Context) -> &[TextureHandle] {
        let key = (self.doc.revision(), self.doc.current_frame());
        let stale =
            self.layer_thumbs_key != key || self.layer_thumbs.len() != self.doc.layers().len();
        let live = self.doc.width() * self.doc.height() <= LIVE_THUMB_PIXELS;
        if stale && (live || !self.doc.stroke_active() || self.layer_thumbs.is_empty()) {
            let frame = self.doc.current_frame();
            self.layer_thumbs = (0..self.doc.layers().len())
                .map(|index| {
                    let pixels = match self.doc.layers()[index].kind {
                        LayerKind::Group => self.doc.render(
                            frame,
                            RenderOptions {
                                reference: true,
                                solo: None,
                            },
                        ),
                        _ => self.doc.layer_image(index, frame),
                    };
                    let name = format!("layer{index}:{}", self.rel_path);
                    ctx.load_texture(name, color_image(&pixels), TextureOptions::NEAREST)
                })
                .collect();
            self.layer_thumbs_key = key;
        }
        &self.layer_thumbs
    }

    /// Steps to the next zoom level above (`direction > 0`) or below the current one,
    /// keeping the centre of the view in place.
    pub fn zoom_step(&mut self, direction: i32) {
        let current = self.zoom;
        let next = if direction > 0 {
            ZOOM_LEVELS.iter().copied().find(|z| *z > current * 1.01)
        } else {
            ZOOM_LEVELS
                .iter()
                .rev()
                .copied()
                .find(|z| *z < current * 0.99)
        };
        if let Some(next) = next {
            self.pan *= next / current;
            self.zoom = next;
        }
    }

    fn size(&self) -> Vec2 {
        vec2(self.doc.width() as f32, self.doc.height() as f32)
    }

    /// Screen position of the image's top-left corner.
    fn origin(&self, view: Rect) -> Pos2 {
        view.center() + self.pan - self.size() * self.zoom / 2.0
    }

    /// A line for the status bar: selection size, the stroke being measured.
    pub fn status_text(&self) -> String {
        let mut parts = Vec::new();
        if let Some((x, y)) = self.hover {
            parts.push(format!("{x}, {y}"));
        }
        if let Some(((x0, y0), (x1, y1))) = self.doc.stroke_span()
            && self.doc.stroke_active()
        {
            let (dx, dy) = (x1 - x0, y1 - y0);
            let len = f64::from(dx * dx + dy * dy).sqrt();
            let angle = f64::from(-dy).atan2(f64::from(dx)).to_degrees();
            parts.push(format!(
                "Δ {dx}, {dy}  ({}×{} px, {len:.1} px, {angle:.0}°)",
                dx.abs() + 1,
                dy.abs() + 1
            ));
        } else if let Some(b) = self.doc.selection_bounds() {
            parts.push(format!("Selection {}×{} at {}, {}", b.w, b.h, b.x, b.y));
        }
        if self.doc.frame_count() > 1 {
            parts.push(format!(
                "Frame {}/{}",
                self.doc.current_frame() + 1,
                self.doc.frame_count()
            ));
        }
        parts.join("   ")
    }

    // ---- playback ----

    /// Starts or stops playing the animation (in the active tag, if the current frame is in one).
    pub fn toggle_play(&mut self, ctx: &egui::Context, prefs: &ViewPrefs) {
        if self.playing {
            self.playing = false;
            return;
        }
        let current = self.doc.current_frame();
        let tag = self
            .doc
            .tags()
            .iter()
            .find(|t| (t.from..=t.to).contains(&current))
            .cloned();
        self.play_order = match tag {
            Some(t) => t.direction.order(t.from, t.to),
            None => prefs.play_direction.order(0, self.doc.frame_count() - 1),
        };
        if self.play_order.len() < 2 {
            return;
        }
        self.play_pos = self
            .play_order
            .iter()
            .position(|f| *f == current)
            .unwrap_or(0);
        self.playing = true;
        self.play_next_at = ctx.input(|i| i.time) + self.frame_seconds(current);
    }

    fn frame_seconds(&self, frame: usize) -> f64 {
        f64::from(self.doc.frames().get(frame).map_or(100, |f| f.duration_ms)) / 1000.0
    }

    /// Advances playback to the frame due now.
    pub fn tick_playback(&mut self, ctx: &egui::Context, prefs: &ViewPrefs) {
        if !self.playing || self.play_order.is_empty() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let mut guard = 0;
        while now >= self.play_next_at && guard < 1000 {
            guard += 1;
            self.play_pos += 1;
            if self.play_pos >= self.play_order.len() {
                if !prefs.play_loop {
                    self.playing = false;
                    return;
                }
                self.play_pos = 0;
            }
            let frame = self.play_order[self.play_pos].min(self.doc.frame_count() - 1);
            self.play_next_at += self.frame_seconds(frame).max(0.01);
            if now - self.play_next_at > 2.0 {
                self.play_next_at = now; // fell far behind (window was hidden)
            }
        }
        let frame = self.play_order[self.play_pos].min(self.doc.frame_count() - 1);
        self.doc.set_current_frame(frame);
        let wait = (self.play_next_at - now).max(0.0);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
    }

    /// Re-runs the checks when the image changed.
    pub fn refresh_issues(&mut self, prefs: &ViewPrefs) {
        let key = self.doc.revision() ^ (self.doc.current_frame() as u64) << 48;
        if self.issues_revision != key {
            self.issues = openpax_paint::analysis::check(&self.doc.flatten(), &prefs.checks);
            self.issues_revision = key;
        }
    }

    /// Finishes a polyline or polygon from the clicked points.
    pub fn finish_polygon(&mut self, tools: &ToolState) {
        let points = std::mem::take(&mut self.poly_points);
        if points.is_empty() {
            return;
        }
        let closed = tools.tool == ToolKind::Polygon;
        self.doc.draw_polygon(&tools.settings, &points, closed);
    }

    // ---- drawing ----

    pub fn ui(&mut self, ui: &mut egui::Ui, tools: &mut ToolState, checker: &TextureHandle) {
        let full = ui.available_rect_before_wrap();
        let (bar, rest) = full.split_top_bottom_at_y(full.top() + TOOLBAR_HEIGHT);
        let (rail, mut work) = rest.split_left_right_at_x(rest.left() + RAIL_WIDTH);
        let ctx = ui.ctx().clone();
        self.tick_playback(&ctx, &tools.view);
        if tools.view.show_timeline {
            let height = TIMELINE_HEIGHT.min(work.height() * 0.45);
            let (top, timeline) = work.split_top_bottom_at_y(work.bottom() - height);
            work = top;
            crate::timeline::show(ui, timeline, self, tools);
        }
        if tools.view.show_issues {
            self.refresh_issues(&tools.view);
        }
        self.canvas(ui, work, tools, checker);
        crate::tool_options::options_bar(ui, bar, self, tools);
        crate::tool_options::tool_rail(ui, rail, tools, checker);
    }

    fn canvas(
        &mut self,
        ui: &mut egui::Ui,
        area: Rect,
        tools: &mut ToolState,
        checker: &TextureHandle,
    ) {
        let tokens = theme::of(ui);
        let prefs = tools.view.clone();
        let view = if prefs.rulers {
            Rect::from_min_max(area.min + vec2(RULER, RULER), area.max)
        } else {
            area
        };
        let response = ui.allocate_rect(area, Sense::click_and_drag());
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, ui.visuals().extreme_bg_color);

        let size = self.size();
        if self.fit_pending {
            let fit = ((view.width() - 48.0) / size.x).min((view.height() - 48.0) / size.y);
            self.zoom = if fit >= 1.0 { fit.floor() } else { fit }.clamp(MIN_ZOOM, MAX_ZOOM);
            self.pan = Vec2::ZERO;
            self.fit_pending = false;
        }

        let input = ui.ctx().input(|i| CanvasInput {
            scroll: i.smooth_scroll_delta.y,
            pinch: i.zoom_delta(),
            pointer: i.pointer.latest_pos(),
            space: i.key_down(egui::Key::Space),
            alt: i.modifiers.alt,
            shift: i.modifiers.shift,
            ctrl: i.modifiers.command,
            pressed: [
                PointerButton::Primary,
                PointerButton::Secondary,
                PointerButton::Middle,
                PointerButton::Extra1,
                PointerButton::Extra2,
            ]
            .into_iter()
            .find(|b| i.pointer.button_pressed(*b)),
            double_clicked: i.pointer.button_double_clicked(PointerButton::Primary),
            primary_down: i.pointer.primary_down(),
            secondary_down: i.pointer.secondary_down(),
            middle_down: i.pointer.middle_down(),
            extra1_down: i.pointer.button_down(PointerButton::Extra1),
            extra2_down: i.pointer.button_down(PointerButton::Extra2),
            any_released: i.pointer.any_released(),
            time: i.time,
            touch_force: i.events.iter().rev().find_map(|e| match e {
                egui::Event::Touch { force, .. } => *force,
                _ => None,
            }),
        });
        if let Some(force) = input.touch_force {
            self.pressure = Some(force.clamp(0.0, 1.0));
        }
        let pointer = input.pointer;
        let over = response.contains_pointer() && pointer.is_some_and(|p| view.contains(p));

        // Zoom towards the cursor.
        let factor = if prefs.integer_zoom {
            1.0
        } else {
            (input.scroll * 0.01).exp() * input.pinch
        };
        if over && let Some(pos) = pointer {
            let pixel = (pos - self.origin(view)) / self.zoom;
            let before = self.zoom;
            if prefs.integer_zoom {
                // Whole steps only: collect wheel movement until it adds up to a notch.
                self.scroll_acc += input.scroll + (input.pinch - 1.0) * 200.0;
                if self.scroll_acc >= 40.0 {
                    self.zoom_step(1);
                    self.scroll_acc = 0.0;
                } else if self.scroll_acc <= -40.0 {
                    self.zoom_step(-1);
                    self.scroll_acc = 0.0;
                }
                // zoom_step keeps the centre; re-anchor on the cursor instead.
            } else if factor != 1.0 {
                self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
            }
            if self.zoom != before {
                let centered = view.center() - size * self.zoom / 2.0;
                self.pan = pos - pixel * self.zoom - centered;
            }
        }

        // Pan with the middle button (unless it picks) or space + drag. The canvas can be
        // moved anywhere; nothing clamps it to the image.
        let middle_pans = prefs.middle_button == MiddleButton::Pan;
        let panning = (input.middle_down && middle_pans) || input.space;
        if (middle_pans && response.dragged_by(PointerButton::Middle))
            || (input.space && response.dragged_by(PointerButton::Primary))
        {
            self.pan += response.drag_delta();
        }

        let origin = self.origin(view);
        let zoom = self.zoom;
        let image_rect = Rect::from_min_size(origin, size * zoom);
        let to_screen = |x: f32, y: f32| origin + vec2(x, y) * zoom;
        let painter = painter.with_clip_rect(view);

        // Background: checkerboard or a solid colour.
        match prefs.background {
            Background::Checker => {
                let cell = prefs.checker_size.max(1) as f32;
                let uv = Rect::from_min_max(
                    pos2(0.0, 0.0),
                    pos2(size.x / 2.0 / cell, size.y / 2.0 / cell),
                );
                // The checker texture is grey and white; tint it towards the chosen colours.
                let tint = mix_tint(prefs.checker_a, prefs.checker_b, tokens.checker);
                painter.image(checker.id(), image_rect, uv, tint);
            }
            Background::Solid(c) => {
                painter.rect_filled(
                    image_rect,
                    0.0,
                    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                );
            }
        }
        let full_uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let texture_id = self.texture(ui.ctx(), prefs.vision);
        let offsets: Vec<(i32, i32)> = match prefs.tiling {
            Tiling::Off => Vec::new(),
            Tiling::Both => (-1..=1)
                .flat_map(|y| (-1..=1).map(move |x| (x, y)))
                .filter(|p| *p != (0, 0))
                .collect(),
            Tiling::Horizontal => vec![(-1, 0), (1, 0)],
            Tiling::Vertical => vec![(0, -1), (0, 1)],
        };
        for (dx, dy) in &offsets {
            let offset = vec2(*dx as f32 * size.x, *dy as f32 * size.y) * zoom;
            painter.image(
                texture_id,
                image_rect.translate(offset),
                full_uv,
                Color32::from_white_alpha(150),
            );
        }
        // Onion skin: neighbouring frames faintly behind the current one.
        if self.onion_skin && self.doc.frame_count() > 1 {
            let current = self.doc.current_frame() as i64;
            let count = self.doc.frame_count() as i64;
            let ctx = ui.ctx().clone();
            let mut layers = Vec::new();
            for d in 1..=i64::from(prefs.onion_before) {
                layers.push((current - d, d, true));
            }
            for d in 1..=i64::from(prefs.onion_after) {
                layers.push((current + d, d, false));
            }
            for (frame, distance, before) in layers {
                let frame = if prefs.play_loop {
                    frame.rem_euclid(count)
                } else {
                    frame
                };
                if frame < 0 || frame >= count || frame == current {
                    continue;
                }
                let alpha = (prefs.onion_opacity / distance as f32 * 255.0) as u8;
                let tint = if prefs.onion_tint {
                    if before {
                        Color32::from_rgba_unmultiplied(255, 90, 90, alpha)
                    } else {
                        Color32::from_rgba_unmultiplied(90, 140, 255, alpha)
                    }
                } else {
                    Color32::from_white_alpha(alpha)
                };
                let id = self.frame_texture(&ctx, frame as usize);
                painter.image(id, image_rect, full_uv, tint);
            }
        }
        painter.image(texture_id, image_rect, full_uv, Color32::WHITE);

        // Pixel grid.
        if self.show_grid && zoom >= prefs.grid_min_zoom && size.x + size.y <= 2048.0 {
            let grid_color = if prefs.grid_color == ViewPrefs::default().grid_color {
                tokens.pixel_grid
            } else {
                rgba32(prefs.grid_color)
            };
            let stroke = Stroke::new(1.0, grid_color);
            for x in 1..self.doc.width() {
                painter.vline(to_screen(x as f32, 0.0).x, image_rect.y_range(), stroke);
            }
            for y in 1..self.doc.height() {
                painter.hline(image_rect.x_range(), to_screen(0.0, y as f32).y, stroke);
            }
        }
        // Tile grid (or the active tilemap's grid).
        self.draw_tile_grid(&painter, &prefs, image_rect, &to_screen);

        // Guides.
        let guide_color = tokens.frame_line;
        for g in &self.doc.extras.guides {
            if g.vertical {
                painter.vline(
                    to_screen(g.position as f32, 0.0).x,
                    view.y_range(),
                    Stroke::new(1.0, guide_color),
                );
            } else {
                painter.hline(
                    view.x_range(),
                    to_screen(0.0, g.position as f32).y,
                    Stroke::new(1.0, guide_color),
                );
            }
        }
        // Pivot and anchors.
        let cross = |p: Pos2, color: Color32| {
            painter.line_segment(
                [p - vec2(6.0, 0.0), p + vec2(6.0, 0.0)],
                Stroke::new(1.5, color),
            );
            painter.line_segment(
                [p - vec2(0.0, 6.0), p + vec2(0.0, 6.0)],
                Stroke::new(1.5, color),
            );
            painter.circle_stroke(p, 3.0, Stroke::new(1.0, color));
        };
        if let Some((px, py)) = self.doc.extras.pivot {
            cross(
                to_screen(px as f32 + 0.5, py as f32 + 0.5),
                Color32::from_rgb(80, 120, 255),
            );
        }
        for a in &self.doc.extras.anchors {
            let p = to_screen(a.x as f32 + 0.5, a.y as f32 + 0.5);
            cross(p, Color32::from_rgb(255, 80, 80));
            painter.text(
                p + vec2(6.0, -6.0),
                Align2::LEFT_BOTTOM,
                &a.name,
                FontId::proportional(11.0),
                Color32::from_rgb(255, 80, 80),
            );
        }

        painter.rect_stroke(
            image_rect,
            0.0,
            Stroke::new(1.0, tokens.canvas_border),
            StrokeKind::Outside,
        );

        // Problems found by the checks.
        if prefs.show_issues {
            for issue in &self.issues {
                let r = Rect::from_min_size(
                    to_screen(issue.x as f32, issue.y as f32),
                    Vec2::splat(zoom),
                );
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(2.0, Color32::from_rgb(255, 60, 60)),
                    StrokeKind::Inside,
                );
            }
        }

        // Selection: marching ants.
        if let Some(sel) = self.doc.selection() {
            let phase = (input.time * 8.0) as f32 % 8.0;
            let mut shapes = Vec::new();
            for ((x0, y0), (x1, y1)) in sel.edges() {
                let a = to_screen(x0 as f32, y0 as f32);
                let b = to_screen(x1 as f32, y1 as f32);
                shapes.push(Shape::line_segment(
                    [a, b],
                    Stroke::new(1.0, Color32::WHITE),
                ));
                let offset = phase + (x0 + y0) as f32 * zoom;
                shapes.extend(Shape::dashed_line_with_offset(
                    &[a, b],
                    Stroke::new(1.0, Color32::BLACK),
                    &[4.0],
                    &[4.0],
                    offset,
                ));
            }
            painter.extend(shapes);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(120));
        }
        // Lasso path in progress.
        if let Some(path) = self.doc.stroke_path() {
            let points: Vec<Pos2> = path
                .iter()
                .map(|(x, y)| to_screen(*x as f32 + 0.5, *y as f32 + 0.5))
                .collect();
            painter.add(Shape::line(points, Stroke::new(1.0, tokens.accent)));
        }
        // Polygon points so far, with a rubber band to the cursor.
        if !self.poly_points.is_empty() {
            let mut points: Vec<Pos2> = self
                .poly_points
                .iter()
                .map(|(x, y)| to_screen(*x as f32 + 0.5, *y as f32 + 0.5))
                .collect();
            if let Some(p) = pointer {
                points.push(p);
            }
            painter.add(Shape::line(points.clone(), Stroke::new(1.0, tokens.accent)));
            for p in &points[..self.poly_points.len()] {
                painter.circle_filled(*p, 2.5, tokens.accent);
            }
        }

        // Rulers.
        if prefs.rulers {
            self.rulers(ui, area, view, &input, origin, zoom);
        }

        // Tools.
        let pixel = pointer.map(|pos| {
            let p = (pos - origin) / zoom;
            (p.x.floor() as i32, p.y.floor() as i32)
        });
        self.hover = pixel.filter(|_| over);

        if over && panning {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        let selection_tool = tools.tool.is_selection();
        if over && !panning && self.guide_drag.is_none() {
            // Holding Alt turns painting tools into the colour picker.
            let tool = if input.alt && !selection_tool && tools.tool != ToolKind::Move {
                ToolKind::Picker
            } else {
                tools.tool
            };
            ui.ctx().set_cursor_icon(match tool {
                ToolKind::Move => CursorIcon::Move,
                _ => CursorIcon::Crosshair,
            });
            if let Some((x, y)) = pixel {
                self.draw_cursor(&painter, tool, tools, (x, y), &to_screen);
                if let Some(button) = input.pressed {
                    self.press(tool, button, (x, y), &input, tools);
                }
                if input.double_clicked && tools.tool.is_multi_click() {
                    self.finish_polygon(tools);
                }
            }
        }
        // Tile stamping while dragging.
        if tools.tool == ToolKind::TileStamp
            && (input.primary_down || input.secondary_down)
            && over
            && let Some(p) = pixel
        {
            self.stamp_tile(p, input.secondary_down, tools);
        }
        if input.any_released {
            self.last_tile_cell = None;
        }
        if self.doc.stroke_active() {
            let down = match self.stroke_button {
                Some(PointerButton::Secondary) => input.secondary_down,
                Some(PointerButton::Middle) => input.middle_down,
                Some(PointerButton::Extra1) => input.extra1_down,
                Some(PointerButton::Extra2) => input.extra2_down,
                _ => input.primary_down,
            };
            match pixel {
                Some(p) if down => {
                    let p = self.snap(p, &prefs);
                    self.doc.stroke_move(p);
                }
                _ if !down => {
                    self.doc.stroke_end();
                    self.stroke_button = None;
                }
                _ => {}
            }
            ui.ctx().request_repaint();
        }

        // Measurement next to the cursor while dragging shapes and selections.
        if let (Some(((x0, y0), (x1, y1))), Some(pos)) = (self.doc.stroke_span(), pointer)
            && matches!(
                self.doc.stroke_tool(),
                Some(
                    ToolKind::Line
                        | ToolKind::Rectangle
                        | ToolKind::Ellipse
                        | ToolKind::Select
                        | ToolKind::Move
                )
            )
        {
            let text = format!("{}×{}", (x1 - x0).abs() + 1, (y1 - y0).abs() + 1);
            let r = painter.text(
                pos + vec2(14.0, 14.0),
                Align2::LEFT_TOP,
                &text,
                FontId::monospace(11.0),
                Color32::WHITE,
            );
            painter.rect_filled(r.expand(2.0), 2.0, Color32::from_black_alpha(160));
            painter.text(
                pos + vec2(14.0, 14.0),
                Align2::LEFT_TOP,
                text,
                FontId::monospace(11.0),
                Color32::WHITE,
            );
        }
    }

    fn draw_tile_grid(
        &self,
        painter: &egui::Painter,
        prefs: &ViewPrefs,
        image_rect: Rect,
        to_screen: &dyn Fn(f32, f32) -> Pos2,
    ) {
        // A tilemap layer shows its own grid; otherwise the tile grid from the view settings.
        let layer = self.doc.active_layer();
        if let (Some(map), Some(set)) = (&layer.tilemap, &self.doc.extras.tileset) {
            let stroke = Stroke::new(1.0, rgba32(prefs.tile_grid_color));
            for row in 0..map.rows as i32 {
                for col in 0..map.cols as i32 {
                    let (x, y) = map.cell_origin(set, col, row);
                    let (w, h) = (set.tile_w as f32, set.tile_h as f32);
                    let (x, y) = (x as f32, y as f32);
                    let shape = match map.grid {
                        GridKind::Rectangular => {
                            vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
                        }
                        GridKind::Isometric => vec![
                            (x + w / 2.0, y),
                            (x + w, y + h / 2.0),
                            (x + w / 2.0, y + h),
                            (x, y + h / 2.0),
                        ],
                        GridKind::HexPointy => vec![
                            (x + w / 2.0, y),
                            (x + w, y + h / 4.0),
                            (x + w, y + h * 3.0 / 4.0),
                            (x + w / 2.0, y + h),
                            (x, y + h * 3.0 / 4.0),
                            (x, y + h / 4.0),
                        ],
                        GridKind::HexFlat => vec![
                            (x + w / 4.0, y),
                            (x + w * 3.0 / 4.0, y),
                            (x + w, y + h / 2.0),
                            (x + w * 3.0 / 4.0, y + h),
                            (x + w / 4.0, y + h),
                            (x, y + h / 2.0),
                        ],
                    };
                    let points: Vec<Pos2> = shape.iter().map(|(a, b)| to_screen(*a, *b)).collect();
                    painter.add(Shape::closed_line(points, stroke));
                }
            }
            return;
        }
        if !prefs.tile_grid {
            return;
        }
        let (tw, th) = (
            prefs.tile_grid_size.0.max(1) as i32,
            prefs.tile_grid_size.1.max(1) as i32,
        );
        let (ox, oy) = prefs.tile_grid_offset;
        let stroke = Stroke::new(1.5, rgba32(prefs.tile_grid_color));
        let (w, h) = (self.doc.width() as i32, self.doc.height() as i32);
        match prefs.tile_grid_kind {
            GridKind::Rectangular | GridKind::HexPointy | GridKind::HexFlat => {
                let mut x = ox.rem_euclid(tw);
                while x <= w {
                    painter.vline(to_screen(x as f32, 0.0).x, image_rect.y_range(), stroke);
                    x += tw;
                }
                let mut y = oy.rem_euclid(th);
                while y <= h {
                    painter.hline(image_rect.x_range(), to_screen(0.0, y as f32).y, stroke);
                    y += th;
                }
            }
            GridKind::Isometric => {
                // Diagonal lines of slope ±th/tw.
                let slope = th as f32 / tw as f32;
                let span = (w + h) / tw.min(th).max(1) + 2;
                for k in -span..=span {
                    let c = (k * tw) as f32 + ox as f32;
                    let a = to_screen(c, oy as f32);
                    let b = to_screen(c + h as f32 / slope, h as f32 + oy as f32);
                    painter.line_segment([a, b], stroke);
                    let b2 = to_screen(c - h as f32 / slope, h as f32 + oy as f32);
                    painter.line_segment([a, b2], stroke);
                }
            }
        }
    }

    fn draw_cursor(
        &self,
        painter: &egui::Painter,
        tool: ToolKind,
        tools: &ToolState,
        (x, y): (i32, i32),
        to_screen: &dyn Fn(f32, f32) -> Pos2,
    ) {
        let offsets = if tool.uses_brush() {
            tools.settings.brush_offsets()
        } else {
            vec![(0, 0, None)]
        };
        if offsets.len() <= 1 || offsets.len() > 4096 {
            let r = Rect::from_min_max(
                to_screen(x as f32, y as f32),
                to_screen(x as f32 + 1.0, y as f32 + 1.0),
            );
            outline(painter, r, Color32::BLACK, Color32::WHITE);
            return;
        }
        // Outline of the brush shape.
        let set: std::collections::HashSet<(i32, i32)> =
            offsets.iter().map(|(dx, dy, _)| (*dx, *dy)).collect();
        for (dx, dy) in &set {
            let (px, py) = ((x + dx) as f32, (y + dy) as f32);
            for (nx, ny, a, b) in [
                (0, -1, (0.0, 0.0), (1.0, 0.0)),
                (0, 1, (0.0, 1.0), (1.0, 1.0)),
                (-1, 0, (0.0, 0.0), (0.0, 1.0)),
                (1, 0, (1.0, 0.0), (1.0, 1.0)),
            ] {
                if !set.contains(&(dx + nx, dy + ny)) {
                    let p0 = to_screen(px + a.0, py + a.1);
                    let p1 = to_screen(px + b.0, py + b.1);
                    painter.line_segment([p0, p1], Stroke::new(2.0, Color32::BLACK));
                    painter.line_segment([p0, p1], Stroke::new(1.0, Color32::WHITE));
                }
            }
        }
    }

    fn rulers(
        &mut self,
        ui: &mut egui::Ui,
        area: Rect,
        view: Rect,
        input: &CanvasInput,
        origin: Pos2,
        zoom: f32,
    ) {
        let tokens = theme::of(ui);
        let painter = ui.painter_at(area);
        let top = Rect::from_min_max(
            pos2(view.left(), area.top()),
            pos2(area.right(), view.top()),
        );
        let left = Rect::from_min_max(
            pos2(area.left(), view.top()),
            pos2(view.left(), area.bottom()),
        );
        painter.rect_filled(top, 0.0, tokens.toolbar);
        painter.rect_filled(left, 0.0, tokens.toolbar);
        painter.rect_filled(Rect::from_min_max(area.min, view.min), 0.0, tokens.toolbar);
        let text = ui.visuals().weak_text_color();
        // Label every N pixels so labels stay ~50 points apart.
        let step = [
            1, 2, 4, 5, 8, 10, 16, 20, 32, 50, 64, 100, 128, 256, 512, 1024,
        ]
        .into_iter()
        .find(|s| *s as f32 * zoom >= 48.0)
        .unwrap_or(2048);
        let first_x = ((view.left() - origin.x) / zoom / step as f32).floor() as i32 * step;
        let mut x = first_x;
        while (origin.x + x as f32 * zoom) < view.right() {
            let sx = origin.x + x as f32 * zoom;
            painter.vline(
                sx,
                top.bottom() - 6.0..=top.bottom(),
                Stroke::new(1.0, text),
            );
            painter.text(
                pos2(sx + 2.0, top.top() + 1.0),
                Align2::LEFT_TOP,
                x.to_string(),
                FontId::monospace(9.0),
                text,
            );
            x += step;
        }
        let first_y = ((view.top() - origin.y) / zoom / step as f32).floor() as i32 * step;
        let mut y = first_y;
        while (origin.y + y as f32 * zoom) < view.bottom() {
            let sy = origin.y + y as f32 * zoom;
            painter.hline(
                left.right() - 6.0..=left.right(),
                sy,
                Stroke::new(1.0, text),
            );
            painter.text(
                pos2(left.left() + 1.0, sy + 1.0),
                Align2::LEFT_TOP,
                y.to_string(),
                FontId::monospace(9.0),
                text,
            );
            y += step;
        }
        // Guide handles; drag from a ruler to make a guide, drag one back to remove it.
        let to_pixel = |p: Pos2| {
            (
                ((p.x - origin.x) / zoom).round() as i32,
                ((p.y - origin.y) / zoom).round() as i32,
            )
        };
        let guide_color = Color32::from_rgb(0, 200, 255);
        for g in &self.doc.extras.guides {
            if g.vertical {
                let sx = origin.x + g.position as f32 * zoom;
                painter.vline(sx, top.y_range(), Stroke::new(2.0, guide_color));
            } else {
                let sy = origin.y + g.position as f32 * zoom;
                painter.hline(left.x_range(), sy, Stroke::new(2.0, guide_color));
            }
        }
        let Some(pos) = input.pointer else { return };
        if let Some(button) = input.pressed
            && button == PointerButton::Primary
            && self.guide_drag.is_none()
        {
            let (px, py) = to_pixel(pos);
            if top.contains(pos) {
                let existing = self.doc.extras.guides.iter().position(|g| {
                    g.vertical && (g.position as f32 * zoom + origin.x - pos.x).abs() < 5.0
                });
                self.guide_drag = Some((true, existing, px));
            } else if left.contains(pos) {
                let existing = self.doc.extras.guides.iter().position(|g| {
                    !g.vertical && (g.position as f32 * zoom + origin.y - pos.y).abs() < 5.0
                });
                self.guide_drag = Some((false, existing, py));
            }
        }
        if let Some((vertical, existing, _)) = self.guide_drag {
            let (px, py) = to_pixel(pos);
            let position = if vertical { px } else { py };
            self.guide_drag = Some((vertical, existing, position));
            if vertical {
                painter.vline(pos.x, area.y_range(), Stroke::new(1.0, guide_color));
            } else {
                painter.hline(area.x_range(), pos.y, Stroke::new(1.0, guide_color));
            }
            ui.ctx().set_cursor_icon(if vertical {
                CursorIcon::ResizeHorizontal
            } else {
                CursorIcon::ResizeVertical
            });
            if !input.primary_down {
                let remove = !view.contains(pos);
                self.doc
                    .edit_extras(if remove { "Remove guide" } else { "Move guide" }, |e| {
                        if let Some(i) = existing {
                            if remove {
                                e.guides.remove(i);
                            } else {
                                e.guides[i].position = position;
                            }
                        } else if !remove {
                            e.guides.push(Guide { vertical, position });
                        }
                    });
                self.guide_drag = None;
            }
        }
    }

    /// Snaps a pixel to nearby guides (for selections and shapes).
    fn snap(&self, (x, y): (i32, i32), prefs: &ViewPrefs) -> (i32, i32) {
        if !prefs.snap_to_guides || self.doc.extras.guides.is_empty() {
            return (x, y);
        }
        let snaps = matches!(
            self.doc.stroke_tool(),
            Some(
                ToolKind::Select
                    | ToolKind::Rectangle
                    | ToolKind::Ellipse
                    | ToolKind::Line
                    | ToolKind::Move
            )
        );
        if !snaps {
            return (x, y);
        }
        let mut out = (x, y);
        for g in &self.doc.extras.guides {
            // A guide sits on a pixel edge; snap to the pixel on either side of it.
            let (v, slot) = if g.vertical {
                (x, &mut out.0)
            } else {
                (y, &mut out.1)
            };
            if v == g.position - 1 || v == g.position {
                *slot = v;
            } else if (v - g.position).abs() <= 1 {
                *slot = if v < g.position {
                    g.position - 1
                } else {
                    g.position
                };
            }
        }
        out
    }

    /// Starts a tool interaction for a mouse button.
    fn press(
        &mut self,
        tool: ToolKind,
        button: PointerButton,
        at: (i32, i32),
        input: &CanvasInput,
        tools: &mut ToolState,
    ) {
        let prefs = &tools.view;
        // Map the button to a tool and colour.
        let (tool, secondary) = match button {
            PointerButton::Primary => (tool, false),
            PointerButton::Secondary => match prefs.right_button {
                RightButton::SecondaryColor => (tool, true),
                RightButton::Eraser => (ToolKind::Eraser, false),
                RightButton::Picker => (ToolKind::Picker, true),
            },
            PointerButton::Middle => match prefs.middle_button {
                MiddleButton::Pan => return,
                MiddleButton::Picker => (ToolKind::Picker, false),
            },
            PointerButton::Extra1 => match prefs.extra1 {
                Some(t) => (t, false),
                None => return,
            },
            PointerButton::Extra2 => match prefs.extra2 {
                Some(t) => (t, false),
                None => return,
            },
        };
        if tool == ToolKind::TileStamp {
            return; // handled while dragging
        }
        if tool.is_multi_click() {
            if secondary {
                self.finish_polygon(tools);
            } else {
                self.poly_points.push(at);
            }
            return;
        }
        if let Some(reason) = self.doc.edit_blocker()
            && !tool.is_selection()
            && tool != ToolKind::Picker
        {
            self.notice = Some(reason);
            return;
        }
        let mut settings = tools.settings.clone();
        settings.secondary_button = secondary;
        if secondary && tool != ToolKind::Shade {
            settings.color = tools.secondary;
        }
        if tool == ToolKind::Dither && secondary {
            settings.color = tools.secondary;
            settings.dither_color = tools.settings.color;
        }
        settings.constrain = input.shift && !tool.is_selection();
        settings.from_center = input.ctrl;
        if tool.is_selection() {
            settings.select_mode = match (input.shift, input.alt) {
                (true, true) => SelectMode::Intersect,
                (true, false) => SelectMode::Add,
                (false, true) => SelectMode::Subtract,
                _ => settings.select_mode,
            };
        }
        if tool == ToolKind::Shade && settings.ramp.is_empty() {
            settings.ramp = tools.palette.colors.clone();
        }
        if let Some(force) = self.pressure {
            settings.pressure = force.powf(prefs.pressure_curve.max(0.1));
        }
        let at = self.snap(at, prefs);
        match self.doc.stroke_begin(tool, &settings, at) {
            // The picker returns a colour instead of starting a stroke.
            Some(color) if color[3] != 0 => {
                if secondary {
                    tools.secondary = color;
                } else {
                    tools.settings.color = color;
                }
            }
            Some(_) => {}
            None => {
                if self.doc.stroke_active() {
                    self.stroke_button = Some(button);
                }
                if tool.paints() {
                    tools.note_color(settings.color);
                }
            }
        }
    }

    /// Places (or with `erase`, removes) a tile under the pointer on the active tilemap layer.
    fn stamp_tile(&mut self, at: (i32, i32), erase: bool, tools: &ToolState) {
        let index = self.doc.active();
        let layer = &self.doc.layers()[index];
        let (Some(map), Some(set)) = (&layer.tilemap, &self.doc.extras.tileset) else {
            return;
        };
        let Some(cell) = map.cell_at(set, at.0, at.1) else {
            return;
        };
        if self.last_tile_cell == Some(cell) {
            return;
        }
        self.last_tile_cell = Some(cell);
        let set = set.clone();
        let brush = tools.tile.clone();
        let label = if erase { "Erase tile" } else { "Place tile" };
        self.doc.edit_tilemap(index, label, |m| {
            if erase {
                m.set(cell.0, cell.1, None);
            } else if let Some(terrain) = brush.terrain {
                m.paint_terrain(&set, terrain, cell.0, cell.1);
            } else if !brush.random.is_empty() {
                let seed = (cell.0 as u64) << 32 ^ cell.1 as u64 ^ 0x9e37;
                if let Some(id) = set.pick_weighted(&brush.random, seed) {
                    m.set(cell.0, cell.1, Some(TileRef { id, ..brush.tile }));
                }
            } else if brush.tile.id < set.tiles.len() {
                m.set(cell.0, cell.1, Some(brush.tile));
            }
        });
    }
}

pub struct CanvasInput {
    scroll: f32,
    pinch: f32,
    pointer: Option<Pos2>,
    space: bool,
    alt: bool,
    shift: bool,
    ctrl: bool,
    pressed: Option<PointerButton>,
    double_clicked: bool,
    primary_down: bool,
    secondary_down: bool,
    middle_down: bool,
    extra1_down: bool,
    extra2_down: bool,
    any_released: bool,
    time: f64,
    touch_force: Option<f32>,
}

/// A two-tone outline that stays visible on any background.
pub fn outline(painter: &egui::Painter, rect: Rect, inner: Color32, outer: Color32) {
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, inner), StrokeKind::Inside);
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, outer), StrokeKind::Outside);
}

pub fn rgba32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

pub fn color_image(b: &Bitmap) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([b.width() as usize, b.height() as usize], b.data())
}

/// The checker texture is light grey and white; scale it so its squares come out as the two
/// chosen colours (approximately: a tint can only darken).
fn mix_tint(a: Rgba, b: Rgba, theme_tint: Color32) -> Color32 {
    if a == ViewPrefs::default().checker_a && b == ViewPrefs::default().checker_b {
        return theme_tint;
    }
    let avg = |i: usize| ((u16::from(a[i]) + u16::from(b[i])) / 2) as u8;
    Color32::from_rgb(avg(0), avg(1), avg(2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpax_core::Frame as McFrame;

    fn strip() -> Bitmap {
        let mut b = Bitmap::new(2, 6);
        b.set(0, 0, [255, 0, 0, 255]);
        b.set(1, 3, [0, 255, 0, 255]);
        b.set(0, 5, [0, 0, 255, 255]);
        b
    }

    #[test]
    fn animation_strips_become_frames_and_back() {
        let meta = TextureMeta {
            animation: Some(Animation {
                frametime: Some(3),
                ..Default::default()
            }),
            ..Default::default()
        };
        let doc = doc_from_image(strip(), &meta);
        assert_eq!((doc.width(), doc.height(), doc.frame_count()), (2, 2, 3));
        assert_eq!(doc.frames()[1].duration_ms, 150);
        assert_eq!(doc.flatten_frame(1).get(1, 1), Some([0, 255, 0, 255]));
        assert_eq!(export_image(&doc), strip());

        let mut meta = meta;
        sync_meta(&doc, &mut meta);
        let anim = meta.animation.as_ref().unwrap();
        assert_eq!(anim.frametime, Some(3));
        assert_eq!(anim.frames, None);
    }

    #[test]
    fn uneven_durations_are_written_per_frame() {
        let mut doc = doc_from_image(
            strip(),
            &TextureMeta {
                animation: Some(Animation::default()),
                ..Default::default()
            },
        );
        doc.set_frame_duration(2, 250);
        let mut meta = TextureMeta::default();
        sync_meta(&doc, &mut meta);
        let anim = meta.animation.unwrap();
        assert_eq!(anim.frametime, Some(1));
        assert_eq!(
            anim.frames,
            Some(vec![
                McFrame::Index(0),
                McFrame::Index(1),
                McFrame::Timed {
                    index: 2,
                    time: Some(5)
                }
            ])
        );
    }

    #[test]
    fn frames_and_layers_survive_save_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let pack = Pack::create(dir.path().join("pack"), "test", 46).unwrap();
        let rel = "assets/minecraft/textures/block/anim.png";
        pack.write(rel, &openpax_paint::encode_png(&Bitmap::new(4, 4)).unwrap())
            .unwrap();
        let mut editor = TextureEditor::open(&pack, rel).unwrap();
        editor.doc.add_layer();
        editor.doc.fill_selected([9, 9, 9, 255]);
        editor.doc.add_frame(false);
        editor.save(&pack).unwrap();
        assert!(!editor.dirty());

        let on_disk = openpax_paint::decode_png(&pack.read(rel).unwrap()).unwrap();
        assert_eq!(
            (on_disk.width(), on_disk.height()),
            (4, 8),
            "frames are stacked into a strip"
        );
        let meta = pack.texture_meta(rel).unwrap().unwrap();
        assert!(
            meta.animation.is_some(),
            "an .mcmeta is written for the frames"
        );

        let reopened = TextureEditor::open(&pack, rel).unwrap();
        assert_eq!(reopened.doc.frame_count(), 2);
        assert_eq!(reopened.doc.layers().len(), 2);
        assert_eq!(
            reopened.doc.flatten_frame(0).get(0, 0),
            Some([9, 9, 9, 255])
        );
    }
}

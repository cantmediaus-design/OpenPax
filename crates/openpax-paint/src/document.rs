//! The layered, animated document: layers × frames of cels, selection, floating pixels and
//! undo history.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::blend::BlendMode;
use crate::effects::{self, LayerEffect};
use crate::mask::{Mask, SelectMode};
use crate::raster::{constrain_line, constrain_square, line_points, pixel_perfect};
use crate::resample::{self, ScaleMethod};
use crate::tiles::{Tilemap, Tileset};
use crate::tools::{PaintMode, Painter, region_mask};
use crate::{Bitmap, Rect, Rgba, TRANSPARENT, ToolKind, ToolSettings};

pub type CelId = u32;
pub type LayerId = u32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayerKind {
    #[default]
    Pixel,
    /// A folder: its children are composited together, then onto the layers below.
    Group,
    /// A tracing image: shown in the editor, never exported.
    Reference,
    /// Tiles from the document's tileset placed on a grid.
    Tilemap,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    /// Refuses edits.
    pub locked: bool,
    /// Painting keeps each pixel's existing transparency.
    pub alpha_lock: bool,
    /// Only shows where the nearest unclipped layer below is opaque.
    pub clip: bool,
    /// A colour label for organising the layer list.
    pub color_tag: Option<Rgba>,
    pub kind: LayerKind,
    /// The group this layer is in.
    pub parent: Option<LayerId>,
    /// Groups: children hidden in the layer list.
    pub collapsed: bool,
    pub effects: Vec<LayerEffect>,
    /// One cel per frame. Frames that share a cel id are linked.
    pub cels: Vec<CelId>,
    /// Tilemap layers: the placed tiles.
    pub tilemap: Option<Tilemap>,
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            locked: false,
            alpha_lock: false,
            clip: false,
            color_tag: None,
            kind: LayerKind::Pixel,
            parent: None,
            collapsed: false,
            effects: Vec::new(),
            cels: Vec::new(),
            tilemap: None,
        }
    }
}

/// One step of the animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub duration_ms: u32,
}

impl Default for Frame {
    fn default() -> Self {
        Self { duration_ms: 100 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayDirection {
    #[default]
    Forward,
    Reverse,
    PingPong,
}

impl PlayDirection {
    pub const ALL: [PlayDirection; 3] = [Self::Forward, Self::Reverse, Self::PingPong];

    pub fn name(self) -> &'static str {
        match self {
            Self::Forward => "Forward",
            Self::Reverse => "Reverse",
            Self::PingPong => "Ping-pong",
        }
    }

    /// The order of frame indices one pass of `from..=to` plays in.
    pub fn order(self, from: usize, to: usize) -> Vec<usize> {
        let forward: Vec<usize> = (from..=to).collect();
        match self {
            Self::Forward => forward,
            Self::Reverse => forward.into_iter().rev().collect(),
            Self::PingPong => {
                let mut v = forward.clone();
                if to > from + 1 {
                    v.extend(forward[1..forward.len() - 1].iter().rev());
                } else if to == from + 1 {
                    // two frames ping-pong is just alternating
                }
                v
            }
        }
    }
}

/// A named range of frames, such as "idle" or "walk".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub from: usize,
    pub to: usize,
    pub direction: PlayDirection,
    pub color: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guide {
    pub vertical: bool,
    /// Pixel-edge coordinate.
    pub position: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub name: String,
    pub x: i32,
    pub y: i32,
}

/// A selection saved under a name, stored as run lengths alternating unselected/selected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedSelection {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub runs: Vec<u32>,
}

impl SavedSelection {
    pub fn new(name: impl Into<String>, mask: &Mask) -> Self {
        let mut runs = Vec::new();
        let mut current = false;
        let mut count = 0u32;
        for &bit in mask.bits() {
            if bit == current {
                count += 1;
            } else {
                runs.push(count);
                current = bit;
                count = 1;
            }
        }
        runs.push(count);
        Self {
            name: name.into(),
            width: mask.width(),
            height: mask.height(),
            runs,
        }
    }

    pub fn mask(&self) -> Option<Mask> {
        let mut bits = Vec::with_capacity((self.width * self.height) as usize);
        let mut on = false;
        for run in &self.runs {
            bits.extend(std::iter::repeat_n(on, *run as usize));
            on = !on;
        }
        Mask::from_bits(self.width, self.height, bits)
    }
}

/// Document settings that are saved with it but are not pixels.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Extras {
    pub guides: Vec<Guide>,
    /// The sprite's origin for game engines.
    pub pivot: Option<(i32, i32)>,
    pub anchors: Vec<Anchor>,
    pub saved_selections: Vec<SavedSelection>,
    /// The document's own palette (indexed colour mode works against it).
    pub palette: Vec<Rgba>,
    /// Painting snaps to the document palette.
    pub indexed: bool,
    pub tileset: Option<Tileset>,
}

/// Which cels a filter or colour operation touches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    /// The active layer in the current frame (or the selected frame range).
    #[default]
    Cel,
    /// The active layer in every frame.
    Layer,
    /// Every layer in the current frame.
    Frame,
    /// Every layer in every frame.
    All,
}

impl Scope {
    pub const ALL: [Scope; 4] = [Self::Cel, Self::Layer, Self::Frame, Self::All];

    pub fn name(self) -> &'static str {
        match self {
            Self::Cel => "Current layer and frame",
            Self::Layer => "Current layer, all frames",
            Self::Frame => "All layers, current frame",
            Self::All => "Everything",
        }
    }
}

/// A geometric change to the canvas, a layer or the selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transform {
    FlipHorizontal,
    FlipVertical,
    Rotate90 { clockwise: bool },
    Rotate180,
    /// Degrees clockwise; `smooth` uses RotSprite.
    Rotate { degrees: f32, smooth: bool },
    Scale {
        width: u32,
        height: u32,
        method: ScaleMethod,
    },
}

impl Transform {
    fn apply(self, image: &Bitmap) -> Bitmap {
        match self {
            Self::FlipHorizontal => image.flipped_horizontal(),
            Self::FlipVertical => image.flipped_vertical(),
            Self::Rotate90 { clockwise } => image.rotated_90(clockwise),
            Self::Rotate180 => image.rotated_180(),
            Self::Rotate { degrees, smooth } => {
                // Rotate on a canvas big enough for any angle, then trim.
                let side = ((image.width().pow(2) + image.height().pow(2)) as f32).sqrt().ceil() as u32 + 2;
                let dx = (side - image.width()) as i32 / 2;
                let dy = (side - image.height()) as i32 / 2;
                let padded = image.resized_offset(side, side, dx, dy);
                let rotated = resample::rotate(&padded, degrees, smooth);
                match rotated.opaque_bounds() {
                    Some(b) => rotated.crop(b),
                    None => Bitmap::new(1, 1),
                }
            }
            Self::Scale {
                width,
                height,
                method,
            } => resample::scale(image, width.max(1), height.max(1), method),
        }
    }

    fn apply_mask(self, mask: &Mask) -> Mask {
        let as_bitmap = Bitmap::from_rgba(
            mask.width(),
            mask.height(),
            mask.bits()
                .iter()
                .flat_map(|b| if *b { [255u8; 4] } else { [0u8; 4] })
                .collect(),
        )
        .expect("size matches");
        let out = self.apply(&as_bitmap);
        Mask::from_alpha(&out)
    }
}

/// Selected pixels lifted off their layer so they can be moved and transformed before
/// being put down again.
#[derive(Clone, Debug, PartialEq)]
pub struct Floating {
    pub image: Bitmap,
    /// Which pixels of `image` are part of the selection (same size as `image`).
    pub shape: Mask,
    /// Canvas position of the image's top-left corner.
    pub pos: (i32, i32),
    cel: CelId,
    /// The cel before the pixels were lifted, for undo.
    base: Bitmap,
    selection_before: Option<Mask>,
    /// Whether committing writes over the pixels underneath (paste) or composites.
    opaque: bool,
}

/// Everything a structural edit can change.
#[derive(Clone, Debug, PartialEq)]
struct State {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    frames: Vec<Frame>,
    current: usize,
    tags: Vec<Tag>,
    cels: BTreeMap<CelId, Bitmap>,
    selection: Option<Mask>,
    extras: Extras,
}

/// Undo steps kept per document; older steps are forgotten.
pub const MAX_UNDO_STEPS: usize = 200;
/// Rough memory budget for one document's undo history, in bytes of pixel data.
const MAX_UNDO_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug)]
struct Patch {
    cel: CelId,
    rect: Rect,
    before: Vec<u8>,
    after: Vec<u8>,
}

#[derive(Debug)]
enum EditKind {
    Pixels {
        patches: Vec<Patch>,
        selection_before: Option<Mask>,
        selection_after: Option<Mask>,
        /// Layer and frame to show again on undo / redo.
        place: (usize, usize),
    },
    Structure {
        before: Box<State>,
        after: Box<State>,
    },
    /// A change to one layer's properties (not its pixels).
    Layer {
        index: usize,
        before: Box<Layer>,
        after: Box<Layer>,
    },
    Selection {
        before: Option<Mask>,
        after: Option<Mask>,
    },
}

#[derive(Debug)]
struct Edit {
    label: String,
    kind: EditKind,
    /// A key under which immediately following edits of the same kind merge (slider drags).
    merge: Option<String>,
}

impl Edit {
    /// Approximate memory held by this step.
    fn bytes(&self) -> usize {
        let state = |s: &State| s.cels.values().map(|c| c.data().len()).sum::<usize>();
        let mask = |m: &Option<Mask>| m.as_ref().map_or(0, |m| m.bits().len());
        match &self.kind {
            EditKind::Pixels {
                patches,
                selection_before,
                selection_after,
                ..
            } => {
                patches
                    .iter()
                    .map(|p| p.before.len() + p.after.len())
                    .sum::<usize>()
                    + mask(selection_before)
                    + mask(selection_after)
            }
            EditKind::Structure { before, after } => state(before) + state(after),
            EditKind::Layer { .. } => 256,
            EditKind::Selection { before, after } => mask(before) + mask(after),
        }
    }
}

#[derive(Debug)]
struct Stroke {
    tool: ToolKind,
    settings: ToolSettings,
    start: (i32, i32),
    last: (i32, i32),
    /// Every point visited (freehand and lasso tools).
    path: Vec<(i32, i32)>,
    /// The cels being painted and how they were when the stroke began.
    targets: Vec<(CelId, Bitmap)>,
    /// What the fill tool looks at.
    sample: Option<Bitmap>,
    selection_before: Option<Mask>,
    alpha_lock: bool,
    moved: bool,
    /// Move tool: the floating position when the drag began.
    float_start: Option<(i32, i32)>,
}

/// Options for compositing a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderOptions {
    /// Include reference layers (the editor shows them, exports never do).
    pub reference: bool,
    /// Draw only this layer (and leave everything else out).
    pub solo: Option<usize>,
}

/// A layered, animated image with selection and undo history. Layer 0 is the bottom layer.
#[derive(Debug)]
pub struct Document {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    frames: Vec<Frame>,
    current: usize,
    tags: Vec<Tag>,
    cels: BTreeMap<CelId, Bitmap>,
    next_id: u32,
    selection: Option<Mask>,
    floating: Option<Floating>,
    /// Frames that painting and transforms apply to together (inclusive range).
    frame_range: Option<(usize, usize)>,
    pub extras: Extras,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    revision: u64,
    stroke: Option<Stroke>,
    /// Whether the next mergeable edit may merge into the previous one.
    coalescing: bool,
}

/// What a document is built from (used by the file formats).
pub struct Parts {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    pub active: usize,
    pub frames: Vec<Frame>,
    pub current: usize,
    pub tags: Vec<Tag>,
    pub cels: BTreeMap<CelId, Bitmap>,
    pub extras: Extras,
}

impl Document {
    pub fn new(width: u32, height: u32) -> Self {
        Self::from_bitmap(Bitmap::new(width, height))
    }

    pub fn from_bitmap(bitmap: Bitmap) -> Self {
        let (w, h) = (bitmap.width(), bitmap.height());
        Self::from_frames(w, h, vec![bitmap])
    }

    /// One layer with one frame per bitmap (all must have the document size).
    pub fn from_frames(width: u32, height: u32, frames: Vec<Bitmap>) -> Self {
        assert!(!frames.is_empty(), "a document needs at least one frame");
        let mut cels = BTreeMap::new();
        let mut ids = Vec::new();
        for (i, bitmap) in frames.into_iter().enumerate() {
            assert_eq!((bitmap.width(), bitmap.height()), (width, height));
            cels.insert(i as CelId + 1, bitmap);
            ids.push(i as CelId + 1);
        }
        let layer = Layer {
            id: 1,
            name: "Layer 1".into(),
            cels: ids.clone(),
            ..Default::default()
        };
        Self::from_parts(Parts {
            width,
            height,
            layers: vec![layer],
            active: 0,
            frames: vec![Frame::default(); ids.len()],
            current: 0,
            tags: Vec::new(),
            cels,
            extras: Extras::default(),
        })
    }

    /// Assembles a document, repairing anything inconsistent (missing cels, wrong sizes).
    pub fn from_parts(parts: Parts) -> Self {
        let Parts {
            width,
            height,
            mut layers,
            active,
            mut frames,
            current,
            mut tags,
            mut cels,
            extras,
        } = parts;
        if frames.is_empty() {
            frames.push(Frame::default());
        }
        if layers.is_empty() {
            layers.push(Layer {
                id: 1,
                name: "Layer 1".into(),
                ..Default::default()
            });
        }
        let mut next_id = cels.keys().max().copied().unwrap_or(0).max(
            layers.iter().map(|l| l.id).max().unwrap_or(0),
        ) + 1;
        // Unique layer ids.
        let mut seen = std::collections::HashSet::new();
        for layer in &mut layers {
            if !seen.insert(layer.id) || layer.id == 0 {
                layer.id = next_id;
                seen.insert(next_id);
                next_id += 1;
            }
        }
        for layer in &mut layers {
            layer.cels.resize(frames.len(), 0);
            for cel in &mut layer.cels {
                let ok = cels
                    .get(cel)
                    .is_some_and(|b| (b.width(), b.height()) == (width, height));
                if !ok {
                    *cel = next_id;
                    cels.insert(next_id, Bitmap::new(width, height));
                    next_id += 1;
                }
            }
        }
        let ids: std::collections::HashSet<LayerId> = layers.iter().map(|l| l.id).collect();
        for layer in &mut layers {
            if layer.parent.is_some_and(|p| !ids.contains(&p) || p == layer.id) {
                layer.parent = None;
            }
        }
        tags.retain(|t| t.from <= t.to && t.to < frames.len());
        let mut doc = Self {
            width,
            height,
            active: active.min(layers.len() - 1),
            layers,
            current: current.min(frames.len() - 1),
            frames,
            tags,
            cels,
            next_id,
            selection: None,
            floating: None,
            frame_range: None,
            extras,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            stroke: None,
            coalescing: false,
        };
        doc.collect_garbage();
        doc
    }

    /// The parts of the document, for saving.
    pub fn parts(&self) -> Parts {
        Parts {
            width: self.width,
            height: self.height,
            layers: self.layers.clone(),
            active: self.active,
            frames: self.frames.clone(),
            current: self.current,
            tags: self.tags.clone(),
            cels: self.cels.clone(),
            extras: self.extras.clone(),
        }
    }

    // ---- basic accessors ----

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width as i32, self.height as i32)
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn active_layer(&self) -> &Layer {
        &self.layers[self.active]
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.layers.len() && index != self.active {
            self.commit_floating();
            self.active = index;
            self.revision += 1;
        }
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn current_frame(&self) -> usize {
        self.current
    }

    pub fn set_current_frame(&mut self, frame: usize) {
        if frame < self.frames.len() && frame != self.current {
            self.commit_floating();
            self.stroke_end();
            self.current = frame;
            self.revision += 1;
        }
    }

    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    pub fn cel_id(&self, layer: usize, frame: usize) -> CelId {
        self.layers[layer].cels[frame]
    }

    /// The pixels of one layer in one frame.
    pub fn cel(&self, layer: usize, frame: usize) -> &Bitmap {
        &self.cels[&self.cel_id(layer, frame)]
    }

    /// The pixels of a layer in the current frame.
    pub fn layer_pixels(&self, layer: usize) -> &Bitmap {
        self.cel(layer, self.current)
    }

    /// Whether this cel is shared with another frame (a linked cel).
    pub fn is_linked(&self, layer: usize, frame: usize) -> bool {
        let id = self.cel_id(layer, frame);
        self.layers[layer].cels.iter().filter(|c| **c == id).count() > 1
    }

    pub fn selection(&self) -> Option<&Mask> {
        self.selection.as_ref()
    }

    pub fn selection_bounds(&self) -> Option<Rect> {
        self.selection.as_ref().and_then(Mask::bounds)
    }

    pub fn floating(&self) -> Option<&Floating> {
        self.floating.as_ref()
    }

    pub fn frame_range(&self) -> Option<(usize, usize)> {
        self.frame_range
    }

    /// Selects several frames so painting and transforms apply to all of them.
    pub fn set_frame_range(&mut self, range: Option<(usize, usize)>) {
        self.frame_range = range
            .map(|(a, b)| (a.min(b), a.max(b).min(self.frames.len() - 1)))
            .filter(|(a, b)| a < b);
        self.revision += 1;
    }

    /// The frames an edit applies to: the selected range if it contains the current frame.
    pub fn edit_frames(&self) -> Vec<usize> {
        match self.frame_range {
            Some((a, b)) if (a..=b).contains(&self.current) => (a..=b).collect(),
            _ => vec![self.current],
        }
    }

    /// Changes whenever the visible result may have changed; use it to invalidate caches.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.floating.is_some()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Names of the undoable steps (oldest first) and redoable steps (next first).
    pub fn history(&self) -> (Vec<&str>, Vec<&str>) {
        (
            self.undo.iter().map(|e| e.label.as_str()).collect(),
            self.redo.iter().rev().map(|e| e.label.as_str()).collect(),
        )
    }

    /// Why the active layer cannot be painted on right now, if it cannot.
    pub fn edit_blocker(&self) -> Option<&'static str> {
        let layer = &self.layers[self.active];
        if layer.locked {
            Some("The layer is locked")
        } else if layer.kind == LayerKind::Group {
            Some("Groups have no pixels of their own; pick a layer inside")
        } else if layer.kind == LayerKind::Tilemap {
            Some("Tilemap layers are painted with the Tile Stamp tool")
        } else if !self.layer_shown(self.active) {
            Some("The layer is hidden")
        } else {
            None
        }
    }

    /// Whether a layer and all the groups it is in are visible.
    pub fn layer_shown(&self, index: usize) -> bool {
        let mut layer = &self.layers[index];
        loop {
            if !layer.visible {
                return false;
            }
            match layer.parent.and_then(|p| self.layer_index(p)) {
                Some(i) => layer = &self.layers[i],
                None => return true,
            }
        }
    }

    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    /// Nesting depth of a layer (0 at the top level).
    pub fn layer_depth(&self, index: usize) -> usize {
        let mut depth = 0;
        let mut parent = self.layers[index].parent;
        while let Some(p) = parent.and_then(|p| self.layer_index(p)) {
            depth += 1;
            parent = self.layers[p].parent;
            if depth > self.layers.len() {
                break;
            }
        }
        depth
    }

    /// Layer indices in display order (top first), children right under their group,
    /// skipping children of collapsed groups when `hide_collapsed`.
    pub fn layer_tree(&self, hide_collapsed: bool) -> Vec<usize> {
        let mut out = Vec::new();
        self.tree_into(None, hide_collapsed, &mut out, 0);
        out
    }

    fn tree_into(&self, parent: Option<LayerId>, hide: bool, out: &mut Vec<usize>, depth: usize) {
        if depth > self.layers.len() {
            return;
        }
        for i in (0..self.layers.len()).rev() {
            let layer = &self.layers[i];
            if layer.parent != parent {
                continue;
            }
            out.push(i);
            if layer.kind == LayerKind::Group && !(hide && layer.collapsed) {
                self.tree_into(Some(layer.id), hide, out, depth + 1);
            }
        }
    }

    fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id - 1
    }

    fn new_cel(&mut self, bitmap: Bitmap) -> CelId {
        let id = self.new_id();
        self.cels.insert(id, bitmap);
        id
    }

    /// Drops cels no layer refers to any more.
    fn collect_garbage(&mut self) {
        let used: std::collections::HashSet<CelId> =
            self.layers.iter().flat_map(|l| l.cels.iter().copied()).collect();
        self.cels.retain(|id, _| used.contains(id));
    }

    // ---- rendering ----

    /// The visible image of the current frame as exported (no reference layers).
    pub fn flatten(&self) -> Bitmap {
        self.flatten_frame(self.current)
    }

    /// The exported image of one frame.
    pub fn flatten_frame(&self, frame: usize) -> Bitmap {
        self.render(frame, RenderOptions::default())
    }

    /// Composites a frame.
    pub fn render(&self, frame: usize, options: RenderOptions) -> Bitmap {
        if let Some(solo) = options.solo {
            let mut out = Bitmap::new(self.width, self.height);
            if solo < self.layers.len() {
                out.composite(&self.layer_image(solo, frame), 1.0);
            }
            return out;
        }
        self.render_children(None, frame, options, 0)
    }

    fn render_children(
        &self,
        parent: Option<LayerId>,
        frame: usize,
        options: RenderOptions,
        depth: usize,
    ) -> Bitmap {
        let mut out = Bitmap::new(self.width, self.height);
        if depth > self.layers.len() {
            return out;
        }
        let mut clip_base: Option<Bitmap> = None;
        for (index, layer) in self.layers.iter().enumerate() {
            if layer.parent != parent {
                continue;
            }
            let hidden = !layer.visible
                || (layer.kind == LayerKind::Reference && !options.reference);
            if hidden {
                if !layer.clip {
                    clip_base = Some(Bitmap::new(self.width, self.height));
                }
                continue;
            }
            let image = match layer.kind {
                LayerKind::Group => {
                    let inner = self.render_children(Some(layer.id), frame, options, depth + 1);
                    effects::apply_all(&inner, &layer.effects)
                }
                _ => self.layer_image(index, frame),
            };
            if layer.clip && clip_base.is_some() {
                out.composite_blend(&image, layer.blend, layer.opacity, clip_base.as_ref());
            } else {
                out.composite_blend(&image, layer.blend, layer.opacity, None);
                clip_base = Some(image);
            }
        }
        out
    }

    /// A layer's own pixels in a frame, with floating pixels, tiles and effects applied.
    pub fn layer_image(&self, index: usize, frame: usize) -> Bitmap {
        let layer = &self.layers[index];
        let mut image = match (&layer.tilemap, &self.extras.tileset) {
            (Some(map), Some(set)) if layer.kind == LayerKind::Tilemap => {
                map.render(set, self.width, self.height)
            }
            _ => self.cel(index, frame).clone(),
        };
        if let Some(f) = &self.floating
            && index == self.active
            && frame == self.current
            && layer.cels[frame] == f.cel
        {
            draw_floating(&mut image, f);
        }
        if layer.effects.iter().any(|e| e.enabled) {
            image = effects::apply_all(&image, &layer.effects);
        }
        image
    }

    // ---- recording ----

    fn push_undo(&mut self, edit: Edit) {
        self.redo.clear();
        self.revision += 1;
        if self.coalescing
            && let Some(key) = &edit.merge
            && let Some(last) = self.undo.last_mut()
            && last.merge.as_ref() == Some(key)
        {
            match (&mut last.kind, edit.kind) {
                (EditKind::Layer { after, .. }, EditKind::Layer { after: new, .. }) => *after = new,
                (EditKind::Structure { after, .. }, EditKind::Structure { after: new, .. }) => {
                    *after = new;
                }
                (_, kind) => {
                    self.undo.push(Edit {
                        label: edit.label,
                        kind,
                        merge: edit.merge,
                    });
                }
            }
            return;
        }
        self.coalescing = edit.merge.is_some();
        self.undo.push(edit);
        let mut bytes: usize = self.undo.iter().map(Edit::bytes).sum();
        let mut drop = 0;
        while self.undo.len() - drop > 1
            && (self.undo.len() - drop > MAX_UNDO_STEPS || bytes > MAX_UNDO_BYTES)
        {
            bytes -= self.undo[drop].bytes();
            drop += 1;
        }
        self.undo.drain(..drop);
    }

    /// Ends a run of merged changes (one slider drag); the next one starts a new undo step.
    pub fn end_coalescing(&mut self) {
        self.coalescing = false;
    }

    /// Records the difference between each base and its cel as one undo step.
    fn record_cels(&mut self, label: &str, bases: &[(CelId, Bitmap)], selection_before: Option<Mask>) {
        let mut patches = Vec::new();
        for (id, base) in bases {
            let Some(current) = self.cels.get(id) else {
                continue;
            };
            if let Some(rect) = current.diff_bounds(base) {
                patches.push(Patch {
                    cel: *id,
                    rect,
                    before: base.patch(rect),
                    after: current.patch(rect),
                });
            }
        }
        if patches.is_empty() && selection_before == self.selection {
            return;
        }
        self.push_undo(Edit {
            label: label.to_owned(),
            kind: EditKind::Pixels {
                patches,
                selection_before,
                selection_after: self.selection.clone(),
                place: (self.active, self.current),
            },
            merge: None,
        });
    }

    fn state(&self) -> State {
        State {
            width: self.width,
            height: self.height,
            layers: self.layers.clone(),
            active: self.active,
            frames: self.frames.clone(),
            current: self.current,
            tags: self.tags.clone(),
            cels: self.cels.clone(),
            selection: self.selection.clone(),
            extras: self.extras.clone(),
        }
    }

    fn restore(&mut self, state: &State) {
        self.width = state.width;
        self.height = state.height;
        self.layers.clone_from(&state.layers);
        self.active = state.active;
        self.frames.clone_from(&state.frames);
        self.current = state.current;
        self.tags.clone_from(&state.tags);
        self.cels.clone_from(&state.cels);
        self.selection.clone_from(&state.selection);
        self.extras.clone_from(&state.extras);
        self.frame_range = None;
    }

    fn structural_merge(&mut self, label: &str, merge: Option<&str>, edit: impl FnOnce(&mut Self)) {
        self.stroke_end();
        self.commit_floating();
        let before = self.state();
        edit(self);
        self.collect_garbage();
        self.active = self.active.min(self.layers.len() - 1);
        self.current = self.current.min(self.frames.len() - 1);
        let after = self.state();
        if before != after {
            self.push_undo(Edit {
                label: label.to_owned(),
                kind: EditKind::Structure {
                    before: Box::new(before),
                    after: Box::new(after),
                },
                merge: merge.map(str::to_owned),
            });
        }
    }

    fn structural(&mut self, label: &str, edit: impl FnOnce(&mut Self)) {
        self.structural_merge(label, None, edit);
    }

    /// Changes the document's extras (guides, pivot, palette...) as one undo step.
    pub fn edit_extras(&mut self, label: &str, edit: impl FnOnce(&mut Extras)) {
        self.structural(label, |doc| edit(&mut doc.extras));
    }

    // ---- layer properties ----

    /// Changes one layer's properties as one undo step. Calls with the same `merge` key in a
    /// row (one slider drag) merge until [`Self::end_coalescing`].
    pub fn edit_layer(
        &mut self,
        index: usize,
        label: &str,
        merge: Option<&str>,
        edit: impl FnOnce(&mut Layer),
    ) {
        let Some(layer) = self.layers.get_mut(index) else {
            return;
        };
        let before = layer.clone();
        edit(layer);
        // Pixels and identity are not properties.
        layer.cels.clone_from(&before.cels);
        layer.id = before.id;
        layer.opacity = layer.opacity.clamp(0.0, 1.0);
        let after = layer.clone();
        if before == after {
            return;
        }
        self.push_undo(Edit {
            label: label.to_owned(),
            kind: EditKind::Layer {
                index,
                before: Box::new(before),
                after: Box::new(after),
            },
            merge: merge.map(|m| format!("{m}:{index}")),
        });
    }

    pub fn set_layer_visible(&mut self, index: usize, visible: bool) {
        let label = if visible { "Show layer" } else { "Hide layer" };
        self.edit_layer(index, label, None, |l| l.visible = visible);
    }

    pub fn set_layer_opacity(&mut self, index: usize, opacity: f32) {
        self.edit_layer(index, "Layer opacity", Some("opacity"), |l| l.opacity = opacity);
    }

    pub fn set_layer_name(&mut self, index: usize, name: String) {
        self.edit_layer(index, "Rename layer", None, |l| l.name = name);
    }

    pub fn set_layer_blend(&mut self, index: usize, blend: BlendMode) {
        self.edit_layer(index, "Blend mode", None, |l| l.blend = blend);
    }

    // ---- selection ----

    fn set_selection_recorded(&mut self, label: &str, selection: Option<Mask>) {
        self.stroke_end();
        self.commit_floating();
        let selection = selection.filter(|m| !m.is_empty());
        if selection == self.selection {
            return;
        }
        let before = std::mem::replace(&mut self.selection, selection);
        let after = self.selection.clone();
        self.push_undo(Edit {
            label: label.to_owned(),
            kind: EditKind::Selection { before, after },
            merge: None,
        });
    }

    pub fn select_all(&mut self) {
        let all = Mask::full(self.width, self.height);
        self.set_selection_recorded("Select all", Some(all));
    }

    pub fn deselect(&mut self) {
        self.set_selection_recorded("Deselect", None);
    }

    pub fn invert_selection(&mut self) {
        let mut mask = self
            .selection
            .clone()
            .unwrap_or_else(|| Mask::new(self.width, self.height));
        mask.invert();
        self.set_selection_recorded("Invert selection", Some(mask));
    }

    /// Replaces or combines the selection with a mask.
    pub fn select_mask(&mut self, mask: Mask, mode: SelectMode) {
        let combined = Mask::combine(self.selection.as_ref(), mask, mode);
        self.set_selection_recorded("Select", combined);
    }

    /// Selects the opaque pixels of a layer.
    pub fn select_layer_alpha(&mut self, index: usize, mode: SelectMode) {
        if index < self.layers.len() {
            let mask = Mask::from_alpha(&self.layer_image(index, self.current));
            self.select_mask(mask, mode);
        }
    }

    pub fn save_selection(&mut self, name: &str) {
        if let Some(mask) = self.selection.clone() {
            let saved = SavedSelection::new(name, &mask);
            self.edit_extras("Save selection", |e| {
                e.saved_selections.retain(|s| s.name != saved.name);
                e.saved_selections.push(saved);
            });
        }
    }

    pub fn load_selection(&mut self, name: &str, mode: SelectMode) {
        let mask = self
            .extras
            .saved_selections
            .iter()
            .find(|s| s.name == name)
            .and_then(SavedSelection::mask)
            .map(|m| m.resized(self.width, self.height, 0, 0));
        if let Some(mask) = mask {
            self.select_mask(mask, mode);
        }
    }

    // ---- strokes ----

    /// Starts a tool interaction at a pixel. For [`ToolKind::Picker`] returns the picked colour
    /// and starts nothing.
    pub fn stroke_begin(&mut self, tool: ToolKind, settings: &ToolSettings, at: (i32, i32)) -> Option<Rgba> {
        self.stroke_end();
        if tool == ToolKind::Picker {
            let image = if settings.pick_active_layer {
                self.layer_image(self.active, self.current)
            } else {
                self.render(self.current, RenderOptions { reference: true, solo: None })
            };
            return image.get(at.0, at.1);
        }
        if tool != ToolKind::Move {
            self.commit_floating();
        }
        if tool == ToolKind::TileStamp || tool.is_multi_click() {
            return None;
        }
        let paints = !tool.is_selection() && tool != ToolKind::Move;
        if paints && self.edit_blocker().is_some() {
            return None;
        }
        if tool == ToolKind::Move {
            if self.floating.is_none() {
                if self.edit_blocker().is_some() {
                    return None;
                }
                self.lift(false);
            }
        }
        if tool == ToolKind::MagicWand {
            let sample = self.sample_for(settings);
            if let Some(mask) = region_mask(&sample, at.0, at.1, settings) {
                let combined = Mask::combine(self.selection.as_ref(), mask, settings.select_mode);
                self.set_selection_recorded("Magic wand", combined);
            }
            return None;
        }
        let frames = if paints { self.edit_frames() } else { vec![self.current] };
        let mut targets: Vec<(CelId, Bitmap)> = Vec::new();
        for f in frames {
            let id = self.layers[self.active].cels[f];
            if !targets.iter().any(|(t, _)| *t == id) {
                targets.push((id, self.cels[&id].clone()));
            }
        }
        let sample = (tool == ToolKind::Fill).then(|| self.sample_for(settings));
        let float_start = self.floating.as_ref().map(|f| f.pos);
        self.stroke = Some(Stroke {
            tool,
            settings: settings.clone(),
            start: at,
            last: at,
            path: vec![at],
            targets,
            sample,
            selection_before: self.selection.clone(),
            alpha_lock: self.layers[self.active].alpha_lock,
            moved: false,
            float_start,
        });
        self.stroke_apply(at, true);
        None
    }

    /// What fill and the magic wand look at.
    fn sample_for(&self, settings: &ToolSettings) -> Bitmap {
        if settings.sample_merged {
            self.flatten()
        } else {
            self.layer_image(self.active, self.current)
        }
    }

    /// Continues the current stroke to a new pixel.
    pub fn stroke_move(&mut self, to: (i32, i32)) {
        if self.stroke.as_ref().is_some_and(|s| s.last != to) {
            self.stroke_apply(to, false);
        }
    }

    pub fn stroke_active(&self) -> bool {
        self.stroke.is_some()
    }

    /// The tool of the stroke in progress.
    pub fn stroke_tool(&self) -> Option<ToolKind> {
        self.stroke.as_ref().map(|s| s.tool)
    }

    /// The path of a lasso stroke in progress, for drawing it.
    pub fn stroke_path(&self) -> Option<&[(i32, i32)]> {
        self.stroke
            .as_ref()
            .filter(|s| s.tool == ToolKind::Lasso)
            .map(|s| s.path.as_slice())
    }

    /// The start and current point of the stroke in progress (for measurement readouts).
    pub fn stroke_span(&self) -> Option<((i32, i32), (i32, i32))> {
        self.stroke.as_ref().map(|s| (s.start, s.last))
    }

    fn stroke_apply(&mut self, to: (i32, i32), first: bool) {
        let Some(mut stroke) = self.stroke.take() else {
            return;
        };
        stroke.moved |= !first;
        if stroke.last != to || first {
            for p in line_points(stroke.last, to) {
                if stroke.path.last() != Some(&p) {
                    stroke.path.push(p);
                }
            }
        }
        let bounds = self.bounds();
        let s = &stroke.settings;
        // Shapes may be constrained and drawn from their centre.
        let mut end = to;
        let mut start = stroke.start;
        if s.constrain {
            end = match stroke.tool {
                ToolKind::Line => constrain_line(start, to),
                ToolKind::Rectangle | ToolKind::Ellipse | ToolKind::Select => {
                    constrain_square(start, to)
                }
                _ => to,
            };
        }
        if s.from_center
            && matches!(
                stroke.tool,
                ToolKind::Rectangle | ToolKind::Ellipse | ToolKind::Select
            )
        {
            start = (2 * stroke.start.0 - end.0, 2 * stroke.start.1 - end.1);
        }

        match stroke.tool {
            ToolKind::Select => {
                let rect = Rect::from_points(start, end).intersect(&bounds);
                let new = rect.map(|r| Mask::from_rect(self.width, self.height, r));
                self.selection = match new {
                    Some(m) => Mask::combine(stroke.selection_before.as_ref(), m, s.select_mode),
                    None => stroke.selection_before.clone(),
                };
            }
            ToolKind::Lasso => {
                let mask = Mask::from_polygon(self.width, self.height, &stroke.path);
                self.selection = Mask::combine(stroke.selection_before.as_ref(), mask, s.select_mode);
            }
            ToolKind::Move => {
                if let (Some(f), Some(origin)) = (&mut self.floating, stroke.float_start) {
                    let (dx, dy) = (to.0 - stroke.start.0, to.1 - stroke.start.1);
                    f.pos = (origin.0 + dx, origin.1 + dy);
                    self.selection = Some(floating_selection(f, self.width, self.height));
                }
            }
            tool => {
                let clip = stroke.selection_before.as_ref();
                let erase = tool == ToolKind::Eraser;
                let color = if erase { TRANSPARENT } else { s.color };
                let mode = match tool {
                    ToolKind::Shade => PaintMode::Shade {
                        lighter: !s.secondary_button,
                    },
                    ToolKind::Dither => PaintMode::Dither,
                    ToolKind::Eraser => PaintMode::Erase,
                    _ => PaintMode::Replace,
                };
                let indexed = self.extras.indexed && !self.extras.palette.is_empty();
                let color = if indexed && color[3] != 0 {
                    crate::color::nearest_index(&self.extras.palette, color)
                        .map_or(color, |i| self.extras.palette[i])
                } else {
                    color
                };
                let redraw = matches!(
                    tool,
                    ToolKind::Line | ToolKind::Rectangle | ToolKind::Ellipse
                ) || (s.pixel_perfect
                    && s.effective_size() == 1
                    && matches!(tool, ToolKind::Pencil | ToolKind::Eraser | ToolKind::Shade | ToolKind::Dither));
                for (id, base) in &stroke.targets {
                    let Some(target) = self.cels.get_mut(id) else {
                        continue;
                    };
                    if redraw {
                        target.clone_from(base);
                    }
                    let mut painter = Painter {
                        target,
                        base,
                        clip,
                        alpha_lock: stroke.alpha_lock,
                        settings: s,
                        color,
                        mode,
                    };
                    match tool {
                        ToolKind::Pencil | ToolKind::Eraser | ToolKind::Shade | ToolKind::Dither => {
                            if redraw {
                                painter.path(&pixel_perfect(&stroke.path));
                            } else {
                                painter.line(stroke.last, to);
                            }
                        }
                        ToolKind::Fill if first => {
                            if let Some(sample) = &stroke.sample {
                                painter.fill(to.0, to.1, sample);
                            }
                        }
                        ToolKind::Line => painter.line(start, end),
                        ToolKind::Rectangle => painter.rectangle(start, end),
                        ToolKind::Ellipse => painter.ellipse(start, end),
                        _ => {}
                    }
                }
            }
        }

        stroke.last = to;
        self.stroke = Some(stroke);
        self.revision += 1;
    }

    /// Abandons the current stroke, putting the layer and selection back as they were.
    pub fn stroke_cancel(&mut self) {
        if let Some(stroke) = self.stroke.take() {
            for (id, base) in stroke.targets {
                if let Some(cel) = self.cels.get_mut(&id) {
                    *cel = base;
                }
            }
            if let (Some(f), Some(origin)) = (&mut self.floating, stroke.float_start) {
                f.pos = origin;
            }
            self.selection = stroke.selection_before;
            self.revision += 1;
        }
    }

    /// Finishes the current stroke and records it in the undo history.
    pub fn stroke_end(&mut self) {
        let Some(stroke) = self.stroke.take() else {
            return;
        };
        match stroke.tool {
            ToolKind::Select | ToolKind::Lasso => {
                let plain_click = !stroke.moved && stroke.settings.select_mode == SelectMode::Replace;
                let after = if plain_click {
                    None // a plain click deselects
                } else {
                    self.selection.take()
                };
                self.selection = stroke.selection_before;
                let label = if stroke.tool == ToolKind::Lasso { "Lasso" } else { "Select" };
                self.set_selection_recorded(label, after);
            }
            ToolKind::Move => {
                // Moving a whole layer (nothing was selected) puts it down straight away.
                if stroke.selection_before.is_none()
                    || self.floating.as_ref().is_some_and(|f| f.selection_before.is_none())
                {
                    self.commit_floating();
                }
                self.revision += 1;
            }
            tool => {
                let label = tool.name();
                self.record_cels(label, &stroke.targets, stroke.selection_before);
            }
        }
    }

    /// Draws a polyline or polygon from clicked points as one undo step.
    pub fn draw_polygon(&mut self, settings: &ToolSettings, points: &[(i32, i32)], closed: bool) {
        if points.is_empty() || self.edit_blocker().is_some() {
            return;
        }
        let color = settings.color;
        let alpha_lock = self.layers[self.active].alpha_lock;
        self.paint_cels("Polygon", Scope::Cel, |cel, base, clip| {
            let mut painter = Painter {
                target: cel,
                base,
                clip,
                alpha_lock,
                settings,
                color,
                mode: PaintMode::Replace,
            };
            painter.polygon(points, closed);
        });
    }

    /// Runs a pixel operation over the cels of a scope as one undo step. Pixels outside the
    /// selection are left alone.
    fn paint_cels(
        &mut self,
        label: &str,
        scope: Scope,
        mut op: impl FnMut(&mut Bitmap, &Bitmap, Option<&Mask>),
    ) {
        self.stroke_end();
        self.commit_floating();
        let mut ids: Vec<CelId> = Vec::new();
        let layers: Vec<usize> = match scope {
            Scope::Cel | Scope::Layer => vec![self.active],
            Scope::Frame | Scope::All => (0..self.layers.len()).collect(),
        };
        let frames: Vec<usize> = match scope {
            Scope::Cel | Scope::Frame => self.edit_frames(),
            Scope::Layer | Scope::All => (0..self.frames.len()).collect(),
        };
        for &l in &layers {
            let layer = &self.layers[l];
            if layer.locked || !matches!(layer.kind, LayerKind::Pixel | LayerKind::Reference) {
                continue;
            }
            for &f in &frames {
                let id = layer.cels[f];
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        let selection = self.selection.clone();
        let mut bases = Vec::new();
        for id in ids {
            let Some(cel) = self.cels.get_mut(&id) else {
                continue;
            };
            let base = cel.clone();
            op(cel, &base, selection.as_ref());
            if let Some(mask) = &selection {
                // Keep everything outside the selection.
                let edited = cel.clone();
                cel.map_pixels(|x, y, _| {
                    if mask.get(x, y) {
                        edited.get(x, y).unwrap_or(TRANSPARENT)
                    } else {
                        base.get(x, y).unwrap_or(TRANSPARENT)
                    }
                });
            }
            bases.push((id, base));
        }
        self.record_cels(label, &bases, self.selection.clone());
    }

    /// Applies an image operation (a filter) to the cels of a scope as one undo step,
    /// limited to the selection.
    pub fn apply_filter(&mut self, label: &str, scope: Scope, op: impl Fn(&Bitmap) -> Bitmap) {
        self.paint_cels(label, scope, |cel, base, _| {
            let out = op(base);
            if (out.width(), out.height()) == (cel.width(), cel.height()) {
                *cel = out;
            }
        });
    }

    /// Erases the selection (or the whole active layer when nothing is selected).
    pub fn clear_selected(&mut self) {
        if self.floating.is_some() {
            // Deleting floating pixels drops them where they are.
            if let Some(f) = self.floating.take() {
                let base = f.base.clone();
                self.selection = None;
                let bases = vec![(f.cel, base)];
                self.record_cels("Delete", &bases, f.selection_before);
            }
            return;
        }
        if self.edit_blocker().is_some() {
            return;
        }
        self.paint_cels("Delete", Scope::Cel, |cel, _, _| {
            cel.map_pixels(|_, _, _| TRANSPARENT);
        });
    }

    /// Fills the selection (or the layer) with a colour.
    pub fn fill_selected(&mut self, color: Rgba) {
        if self.edit_blocker().is_some() {
            return;
        }
        self.paint_cels("Fill selection", Scope::Cel, |cel, _, _| {
            cel.map_pixels(|_, _, _| color);
        });
    }

    // ---- floating pixels ----

    /// Lifts the selected pixels (or the whole layer) off the active cel.
    fn lift(&mut self, keep_original: bool) {
        if self.floating.is_some() {
            return;
        }
        let id = self.layers[self.active].cels[self.current];
        let base = self.cels[&id].clone();
        let whole = Mask::full(self.width, self.height);
        let mask = self.selection.clone().unwrap_or(whole);
        let Some(bounds) = mask.bounds() else { return };
        let mut image = base.crop(bounds);
        let mut shape = Mask::new(bounds.w as u32, bounds.h as u32);
        for y in 0..bounds.h {
            for x in 0..bounds.w {
                if mask.get(bounds.x + x, bounds.y + y) {
                    shape.set(x, y, true);
                } else {
                    image.set(x, y, TRANSPARENT);
                }
            }
        }
        if !keep_original {
            let cel = self.cels.get_mut(&id).expect("active cel exists");
            cel.map_pixels(|x, y, c| if mask.get(x, y) { TRANSPARENT } else { c });
        }
        self.floating = Some(Floating {
            image,
            shape,
            pos: (bounds.x, bounds.y),
            cel: id,
            base,
            selection_before: self.selection.clone(),
            opaque: false,
        });
        self.revision += 1;
    }

    /// Puts floating pixels down and records the whole move / paste as one undo step.
    pub fn commit_floating(&mut self) {
        let Some(f) = self.floating.take() else {
            return;
        };
        if let Some(cel) = self.cels.get_mut(&f.cel) {
            if f.opaque {
                for y in 0..f.image.height() as i32 {
                    for x in 0..f.image.width() as i32 {
                        if f.shape.get(x, y) {
                            cel.set(f.pos.0 + x, f.pos.1 + y, f.image.get(x, y).unwrap_or(TRANSPARENT));
                        }
                    }
                }
            } else {
                draw_floating(cel, &f);
            }
        }
        let selection = floating_selection(&f, self.width, self.height);
        // A whole layer that was moved leaves nothing selected.
        let keep = f.opaque || f.selection_before.is_some();
        self.selection = (keep && !selection.is_empty()).then_some(selection);
        let label = if f.opaque { "Paste" } else { "Move" };
        let bases = vec![(f.cel, f.base)];
        self.record_cels(label, &bases, f.selection_before);
    }

    /// Moves the selected pixels by whole pixels (arrow-key nudges).
    pub fn nudge(&mut self, dx: i32, dy: i32) {
        self.stroke_end();
        if self.floating.is_none() {
            if self.edit_blocker().is_some() || self.selection.is_none() {
                return;
            }
            self.lift(false);
        }
        if let Some(f) = &mut self.floating {
            f.pos.0 += dx;
            f.pos.1 += dy;
            self.selection = Some(floating_selection(f, self.width, self.height));
            self.revision += 1;
        }
    }

    /// Pastes an image as floating pixels on the active layer at `at`.
    pub fn paste_floating(&mut self, image: &Bitmap, at: (i32, i32)) {
        if image.width() == 0 || image.height() == 0 || self.edit_blocker().is_some() {
            return;
        }
        self.stroke_end();
        self.commit_floating();
        let id = self.layers[self.active].cels[self.current];
        let mut shape = Mask::new(image.width(), image.height());
        for y in 0..image.height() as i32 {
            for x in 0..image.width() as i32 {
                shape.set(x, y, image.get(x, y).is_some_and(|c| c[3] > 0));
            }
        }
        if shape.is_empty() {
            shape = Mask::full(image.width(), image.height());
        }
        let f = Floating {
            image: image.clone(),
            shape,
            pos: at,
            cel: id,
            base: self.cels[&id].clone(),
            selection_before: self.selection.clone(),
            opaque: true,
        };
        self.selection = Some(floating_selection(&f, self.width, self.height));
        self.floating = Some(f);
        self.revision += 1;
    }

    // ---- clipboard ----

    /// The active layer's selected pixels (or the whole layer), cropped to the selection.
    pub fn copy_selected(&self) -> Bitmap {
        if let Some(f) = &self.floating {
            return f.image.clone();
        }
        let pixels = self.layer_image(self.active, self.current);
        self.masked_crop(&pixels)
    }

    /// The flattened image's selected pixels.
    pub fn copy_merged(&self) -> Bitmap {
        self.masked_crop(&self.flatten())
    }

    fn masked_crop(&self, pixels: &Bitmap) -> Bitmap {
        match &self.selection {
            Some(mask) => {
                let Some(bounds) = mask.bounds() else {
                    return Bitmap::new(0, 0);
                };
                let mut out = pixels.crop(bounds);
                out.map_pixels(|x, y, c| {
                    if mask.get(bounds.x + x, bounds.y + y) {
                        c
                    } else {
                        TRANSPARENT
                    }
                });
                out
            }
            None => pixels.clone(),
        }
    }

    /// Pastes an image onto a new layer above the active one with its top-left corner at `at`,
    /// and selects it. Parts outside the canvas are cut off. One undo step.
    pub fn paste(&mut self, image: &Bitmap, at: (i32, i32)) {
        if image.width() == 0 || image.height() == 0 {
            return;
        }
        self.structural("Paste as new layer", |doc| {
            let mut pixels = Bitmap::new(doc.width, doc.height);
            pixels.blit(image, at.0, at.1);
            let rect = Rect::new(at.0, at.1, image.width() as i32, image.height() as i32);
            doc.insert_layer_above("Pasted", LayerKind::Pixel, Some(pixels));
            doc.selection = rect
                .intersect(&doc.bounds())
                .map(|r| Mask::from_rect(doc.width, doc.height, r));
        });
    }

    // ---- layers ----

    /// Inserts a layer above the active one (in the same group) and makes it active. The
    /// given pixels go into every frame (linked); `None` gives empty cels.
    fn insert_layer_above(&mut self, name: &str, kind: LayerKind, pixels: Option<Bitmap>) {
        let id = self.new_id();
        let cels: Vec<CelId> = match pixels {
            Some(p) => {
                let cel = self.new_cel(p);
                vec![cel; self.frames.len()]
            }
            None => (0..self.frames.len())
                .map(|_| self.new_cel(Bitmap::new(self.width, self.height)))
                .collect(),
        };
        let parent = self.layers.get(self.active).and_then(|l| {
            if l.kind == LayerKind::Group && !l.collapsed {
                Some(l.id)
            } else {
                l.parent
            }
        });
        let layer = Layer {
            id,
            name: name.to_owned(),
            kind,
            parent,
            cels,
            ..Default::default()
        };
        let at = (self.active + 1).min(self.layers.len());
        self.layers.insert(at, layer);
        self.active = at;
    }

    fn next_layer_name(&self, prefix: &str) -> String {
        let mut n = self.layers.len() + 1;
        loop {
            let name = format!("{prefix} {n}");
            if !self.layers.iter().any(|l| l.name == name) {
                return name;
            }
            n += 1;
        }
    }

    /// Adds an empty layer above the active one and makes it active.
    pub fn add_layer(&mut self) {
        self.structural("Add layer", |doc| {
            let name = doc.next_layer_name("Layer");
            doc.insert_layer_above(&name, LayerKind::Pixel, None);
        });
    }

    pub fn add_group(&mut self) {
        self.structural("Add group", |doc| {
            let name = doc.next_layer_name("Group");
            doc.insert_layer_above(&name, LayerKind::Group, None);
        });
    }

    /// Adds a reference image layer (shown while editing, never exported).
    pub fn add_reference_layer(&mut self, name: &str, image: &Bitmap) {
        self.structural("Add reference layer", |doc| {
            let mut pixels = Bitmap::new(doc.width, doc.height);
            // Fit the reference into the canvas, keeping its proportions.
            let scale = (doc.width as f32 / image.width().max(1) as f32)
                .min(doc.height as f32 / image.height().max(1) as f32);
            let (w, h) = (
                ((image.width() as f32 * scale).round() as u32).max(1),
                ((image.height() as f32 * scale).round() as u32).max(1),
            );
            let fitted = resample::nearest(image, w, h);
            pixels.blit(&fitted, (doc.width - w) as i32 / 2, (doc.height - h) as i32 / 2);
            doc.insert_layer_above(name, LayerKind::Reference, Some(pixels));
            doc.layers[doc.active].opacity = 0.5;
            doc.layers[doc.active].locked = true;
        });
    }

    /// Adds a tilemap layer that places tiles from the document's tileset.
    pub fn add_tilemap_layer(&mut self, map: Tilemap) {
        self.structural("Add tilemap layer", |doc| {
            let name = doc.next_layer_name("Tilemap");
            doc.insert_layer_above(&name, LayerKind::Tilemap, None);
            doc.layers[doc.active].tilemap = Some(map);
        });
    }

    /// Changes the tilemap of a tilemap layer as one undo step.
    pub fn edit_tilemap(&mut self, index: usize, label: &str, edit: impl FnOnce(&mut Tilemap)) {
        self.structural(label, |doc| {
            if let Some(map) = doc.layers.get_mut(index).and_then(|l| l.tilemap.as_mut()) {
                edit(map);
            }
        });
    }

    /// Indices of a layer and everything inside it (when it is a group).
    fn subtree(&self, index: usize) -> Vec<usize> {
        let mut out = vec![index];
        let mut i = 0;
        while i < out.len() {
            let id = self.layers[out[i]].id;
            if self.layers[out[i]].kind == LayerKind::Group {
                for (j, l) in self.layers.iter().enumerate() {
                    if l.parent == Some(id) && !out.contains(&j) {
                        out.push(j);
                    }
                }
            }
            i += 1;
        }
        out
    }

    pub fn duplicate_layer(&mut self) {
        self.structural("Duplicate layer", |doc| {
            let mut copy = doc.layers[doc.active].clone();
            copy.name.push_str(" copy");
            copy.id = doc.new_id();
            // Copies get their own pixels (linked frames stay linked to each other).
            let mut map: BTreeMap<CelId, CelId> = BTreeMap::new();
            for cel in &mut copy.cels {
                let new = match map.get(cel) {
                    Some(n) => *n,
                    None => {
                        let bitmap = doc.cels[cel].clone();
                        let n = doc.new_cel(bitmap);
                        map.insert(*cel, n);
                        n
                    }
                };
                *cel = new;
            }
            doc.layers.insert(doc.active + 1, copy);
            doc.active += 1;
        });
    }

    /// Removes the active layer (and, for a group, everything in it). The last pixel
    /// layer cannot be removed.
    pub fn remove_layer(&mut self) {
        self.structural("Delete layer", |doc| {
            let mut doomed = doc.subtree(doc.active);
            let remaining_pixel = doc
                .layers
                .iter()
                .enumerate()
                .filter(|(i, l)| !doomed.contains(i) && l.kind != LayerKind::Group)
                .count();
            if remaining_pixel == 0 {
                return;
            }
            doomed.sort_unstable_by(|a, b| b.cmp(a));
            for i in doomed {
                doc.layers.remove(i);
                if doc.active >= i && doc.active > 0 {
                    doc.active -= 1;
                }
            }
            doc.active = doc.active.min(doc.layers.len() - 1);
        });
    }

    /// Moves the active layer past its next sibling above (`up`) or below.
    pub fn move_layer(&mut self, up: bool) {
        self.structural("Move layer", |doc| {
            let parent = doc.layers[doc.active].parent;
            let sibling = if up {
                (doc.active + 1..doc.layers.len()).find(|i| doc.layers[*i].parent == parent)
            } else {
                (0..doc.active).rev().find(|i| doc.layers[*i].parent == parent)
            };
            if let Some(target) = sibling {
                doc.layers.swap(doc.active, target);
                doc.active = target;
            }
        });
    }

    /// Moves the active layer to `index` (0 is the bottom) as a single undo step. It joins
    /// the group of the layer it lands next to.
    pub fn move_layer_to(&mut self, index: usize) {
        self.structural("Move layer", |doc| {
            let target = index.min(doc.layers.len() - 1);
            let parent = doc.layers[target].parent;
            let layer = doc.layers.remove(doc.active);
            doc.layers.insert(target, layer);
            doc.active = target;
            if !doc.would_cycle(target, parent) {
                doc.layers[target].parent = parent;
            }
        });
    }

    fn would_cycle(&self, index: usize, parent: Option<LayerId>) -> bool {
        let Some(mut p) = parent else { return false };
        let id = self.layers[index].id;
        for _ in 0..=self.layers.len() {
            if p == id {
                return true;
            }
            match self.layer_index(p).and_then(|i| self.layers[i].parent) {
                Some(next) => p = next,
                None => return false,
            }
        }
        true
    }

    /// Puts a layer into a group (or at the top level with `None`).
    pub fn set_layer_parent(&mut self, index: usize, parent: Option<LayerId>) {
        if index >= self.layers.len() || self.would_cycle(index, parent) {
            return;
        }
        if let Some(p) = parent
            && self
                .layer_index(p)
                .is_none_or(|i| self.layers[i].kind != LayerKind::Group)
        {
            return;
        }
        self.structural("Move into group", |doc| {
            doc.layers[index].parent = parent;
            // Keep the layer above its new group's other children for a tidy list.
            if let Some(p) = parent.and_then(|p| doc.layer_index(p)) {
                let layer = doc.layers.remove(index);
                let at = if index < p { p - 1 } else { p };
                doc.layers.insert(at, layer);
                doc.active = at;
            }
        });
    }

    /// Wraps the active layer in a new group.
    pub fn group_active(&mut self) {
        self.structural("Group layer", |doc| {
            let id = doc.new_id();
            let index = doc.active;
            let parent = doc.layers[index].parent;
            let cels = (0..doc.frames.len())
                .map(|_| doc.new_cel(Bitmap::new(doc.width, doc.height)))
                .collect();
            let name = doc.next_layer_name("Group");
            doc.layers.insert(
                index + 1,
                Layer {
                    id,
                    name,
                    kind: LayerKind::Group,
                    parent,
                    cels,
                    ..Default::default()
                },
            );
            doc.layers[index].parent = Some(id);
            doc.active = index + 1;
        });
    }

    /// Dissolves the active group, keeping its children.
    pub fn ungroup(&mut self) {
        if self.layers[self.active].kind != LayerKind::Group {
            return;
        }
        self.structural("Ungroup", |doc| {
            let group = doc.layers.remove(doc.active);
            for l in &mut doc.layers {
                if l.parent == Some(group.id) {
                    l.parent = group.parent;
                }
            }
            doc.active = doc.active.saturating_sub(1).min(doc.layers.len() - 1);
        });
    }

    /// Composites the active layer onto the next layer below it in the same group.
    pub fn merge_down(&mut self) {
        self.structural("Merge down", |doc| {
            let parent = doc.layers[doc.active].parent;
            let Some(below) = (0..doc.active).rev().find(|i| doc.layers[*i].parent == parent) else {
                return;
            };
            let upper_kind = doc.layers[doc.active].kind;
            let lower = &doc.layers[below];
            if lower.kind != LayerKind::Pixel || upper_kind == LayerKind::Group {
                return;
            }
            let upper = doc.active;
            let mut merged = Vec::new();
            for f in 0..doc.frames.len() {
                let mut base = doc.cel(below, f).clone();
                let top = doc.layer_image(upper, f);
                let u = &doc.layers[upper];
                if u.visible {
                    base.composite_blend(&top, u.blend, u.opacity, None);
                }
                merged.push(base);
            }
            let cels = doc.share_identical(merged);
            doc.layers[below].cels = cels;
            doc.layers.remove(upper);
            doc.active = below;
        });
    }

    /// Stores per-frame bitmaps as cels, linking frames whose pixels are identical.
    fn share_identical(&mut self, bitmaps: Vec<Bitmap>) -> Vec<CelId> {
        let mut out: Vec<CelId> = Vec::new();
        for (i, b) in bitmaps.iter().enumerate() {
            let same = (0..i).find(|j| bitmaps[*j] == *b);
            let id = match same {
                Some(j) => out[j],
                None => self.new_cel(b.clone()),
            };
            out.push(id);
        }
        out
    }

    /// Replaces all visible layers with one layer holding their composite.
    pub fn merge_visible(&mut self) {
        self.structural("Merge visible", |doc| {
            let frames: Vec<Bitmap> = (0..doc.frames.len()).map(|f| doc.flatten_frame(f)).collect();
            let cels = doc.share_identical(frames);
            let id = doc.new_id();
            let keep: Vec<Layer> = doc
                .layers
                .iter()
                .filter(|l| !doc.layer_shown_by_id(l.id) || l.kind == LayerKind::Reference)
                .cloned()
                .collect();
            let mut layers = keep;
            for l in &mut layers {
                l.parent = None;
            }
            layers.retain(|l| l.kind != LayerKind::Group);
            layers.insert(
                0,
                Layer {
                    id,
                    name: "Merged".into(),
                    cels,
                    ..Default::default()
                },
            );
            doc.layers = layers;
            doc.active = 0;
        });
    }

    fn layer_shown_by_id(&self, id: LayerId) -> bool {
        self.layer_index(id).is_some_and(|i| self.layer_shown(i))
    }

    /// Replaces every layer with a single one holding the composite.
    pub fn flatten_layers(&mut self) {
        self.structural("Flatten", |doc| {
            let frames: Vec<Bitmap> = (0..doc.frames.len()).map(|f| doc.flatten_frame(f)).collect();
            let cels = doc.share_identical(frames);
            let id = doc.new_id();
            doc.layers = vec![Layer {
                id,
                name: "Layer 1".into(),
                cels,
                ..Default::default()
            }];
            doc.active = 0;
        });
    }

    /// Copies (or, with `cut`, moves) the selected pixels to a new layer.
    pub fn layer_from_selection(&mut self, cut: bool) {
        if self.selection.is_none() || self.edit_blocker().is_some() {
            return;
        }
        let label = if cut { "Layer via cut" } else { "Layer via copy" };
        self.structural(label, |doc| {
            let Some(mask) = doc.selection.clone() else { return };
            let source = doc.active;
            let mut new_cels = Vec::new();
            for f in 0..doc.frames.len() {
                let mut part = doc.cel(source, f).clone();
                part.map_pixels(|x, y, c| if mask.get(x, y) { c } else { TRANSPARENT });
                new_cels.push(part);
                if cut {
                    let id = doc.layers[source].cels[f];
                    if let Some(cel) = doc.cels.get_mut(&id) {
                        cel.map_pixels(|x, y, c| if mask.get(x, y) { TRANSPARENT } else { c });
                    }
                }
            }
            let cels = doc.share_identical(new_cels);
            let id = doc.new_id();
            let parent = doc.layers[source].parent;
            doc.layers.insert(
                source + 1,
                Layer {
                    id,
                    name: doc.next_layer_name("Layer"),
                    parent,
                    cels,
                    ..Default::default()
                },
            );
            doc.active = source + 1;
        });
    }

    // ---- frames ----

    /// Inserts a frame after the current one. `copy` duplicates the current frame's pixels;
    /// otherwise the new frame is empty. Makes it current.
    pub fn add_frame(&mut self, copy: bool) {
        let label = if copy { "Duplicate frame" } else { "New frame" };
        self.structural(label, |doc| {
            let at = doc.current + 1;
            let duration = doc.frames[doc.current];
            doc.frames.insert(at, duration);
            for l in 0..doc.layers.len() {
                let bitmap = if copy {
                    doc.cel(l, doc.current).clone()
                } else {
                    Bitmap::new(doc.width, doc.height)
                };
                let id = doc.new_cel(bitmap);
                doc.layers[l].cels.insert(at, id);
            }
            doc.shift_tags(at, 1);
            doc.current = at;
        });
    }

    /// Inserts a frame after the current one whose cels are linked to the current frame's.
    pub fn add_linked_frame(&mut self) {
        self.structural("New linked frame", |doc| {
            let at = doc.current + 1;
            let duration = doc.frames[doc.current];
            doc.frames.insert(at, duration);
            for layer in &mut doc.layers {
                let id = layer.cels[doc.current];
                layer.cels.insert(at, id);
            }
            doc.shift_tags(at, 1);
            doc.current = at;
        });
    }

    /// Adjusts tags after frames were inserted (`delta` > 0) or removed at `at`.
    fn shift_tags(&mut self, at: usize, delta: isize) {
        let count = self.frames.len();
        self.tags.retain_mut(|tag| {
            if delta > 0 {
                if tag.from >= at {
                    tag.from += 1;
                }
                if tag.to >= at {
                    tag.to += 1;
                }
            } else {
                // Frame `at` was removed.
                if tag.from == at && tag.to == at {
                    return false;
                }
                if tag.from > at {
                    tag.from -= 1;
                }
                if tag.to >= at && tag.to > 0 {
                    tag.to -= 1;
                }
            }
            tag.to = tag.to.min(count.saturating_sub(1));
            tag.from <= tag.to
        });
    }

    /// Removes the current frame (or the selected range). At least one frame stays.
    pub fn remove_frame(&mut self) {
        let frames = self.edit_frames();
        if frames.len() >= self.frames.len() {
            return;
        }
        self.structural("Delete frame", |doc| {
            for &f in frames.iter().rev() {
                doc.frames.remove(f);
                for layer in &mut doc.layers {
                    layer.cels.remove(f);
                }
                doc.shift_tags(f, -1);
            }
            doc.current = frames[0].min(doc.frames.len() - 1);
            doc.frame_range = None;
        });
    }

    /// Moves a frame to another position.
    pub fn move_frame(&mut self, from: usize, to: usize) {
        if from >= self.frames.len() || to >= self.frames.len() || from == to {
            return;
        }
        self.structural("Move frame", |doc| {
            let frame = doc.frames.remove(from);
            doc.frames.insert(to, frame);
            for layer in &mut doc.layers {
                let cel = layer.cels.remove(from);
                layer.cels.insert(to, cel);
            }
            doc.current = to;
        });
    }

    /// Sets how long a frame shows. Consecutive calls merge (one drag) until
    /// [`Self::end_coalescing`].
    pub fn set_frame_duration(&mut self, frame: usize, ms: u32) {
        if frame >= self.frames.len() || self.frames[frame].duration_ms == ms {
            return;
        }
        self.structural_merge("Frame duration", Some("duration"), |doc| {
            doc.frames[frame].duration_ms = ms.max(1);
        });
    }

    /// Sets every frame's duration.
    pub fn set_all_durations(&mut self, ms: u32) {
        self.structural("Frame durations", |doc| {
            for f in &mut doc.frames {
                f.duration_ms = ms.max(1);
            }
        });
    }

    /// Links the cels of a layer across an inclusive frame range to the first one's pixels.
    pub fn link_cels(&mut self, layer: usize, from: usize, to: usize) {
        if layer >= self.layers.len() || from >= to || to >= self.frames.len() {
            return;
        }
        self.structural("Link cels", |doc| {
            let id = doc.layers[layer].cels[from];
            for f in from + 1..=to {
                doc.layers[layer].cels[f] = id;
            }
        });
    }

    /// Gives a linked cel its own copy of the pixels.
    pub fn unlink_cel(&mut self, layer: usize, frame: usize) {
        if layer >= self.layers.len() || frame >= self.frames.len() || !self.is_linked(layer, frame) {
            return;
        }
        self.structural("Unlink cel", |doc| {
            let bitmap = doc.cel(layer, frame).clone();
            let id = doc.new_cel(bitmap);
            doc.layers[layer].cels[frame] = id;
        });
    }

    /// Copies one frame's cels over another frame's (all layers).
    pub fn copy_frame_to(&mut self, from: usize, to: usize) {
        if from >= self.frames.len() || to >= self.frames.len() || from == to {
            return;
        }
        self.structural("Paste frame", |doc| {
            for l in 0..doc.layers.len() {
                let bitmap = doc.cel(l, from).clone();
                let id = doc.new_cel(bitmap);
                doc.layers[l].cels[to] = id;
            }
            doc.frames[to] = doc.frames[from];
        });
    }

    pub fn add_tag(&mut self, tag: Tag) {
        if tag.from > tag.to || tag.to >= self.frames.len() {
            return;
        }
        self.structural("Add tag", |doc| doc.tags.push(tag));
    }

    pub fn set_tag(&mut self, index: usize, tag: Tag) {
        if index >= self.tags.len() || tag.from > tag.to || tag.to >= self.frames.len() {
            return;
        }
        self.structural("Edit tag", |doc| doc.tags[index] = tag);
    }

    pub fn remove_tag(&mut self, index: usize) {
        if index < self.tags.len() {
            self.structural("Delete tag", |doc| {
                doc.tags.remove(index);
            });
        }
    }

    // ---- canvas ----

    /// Changes the canvas size, keeping the top-left corner in place.
    pub fn resize_canvas(&mut self, width: u32, height: u32) {
        self.resize_canvas_anchored(width, height, (0.0, 0.0));
    }

    /// Changes the canvas size. `anchor` says where the old image stays: (0, 0) top-left,
    /// (0.5, 0.5) centre, (1, 1) bottom-right.
    pub fn resize_canvas_anchored(&mut self, width: u32, height: u32, anchor: (f32, f32)) {
        if width == 0 || height == 0 {
            return;
        }
        let dx = ((width as f32 - self.width as f32) * anchor.0).round() as i32;
        let dy = ((height as f32 - self.height as f32) * anchor.1).round() as i32;
        self.reframe("Resize canvas", width, height, |b| b.resized_offset(width, height, dx, dy), dx, dy);
    }

    /// Applies a size-changing operation to every cel, moving the selection and guides along.
    fn reframe(
        &mut self,
        label: &str,
        width: u32,
        height: u32,
        op: impl Fn(&Bitmap) -> Bitmap,
        dx: i32,
        dy: i32,
    ) {
        self.structural(label, |doc| {
            for cel in doc.cels.values_mut() {
                *cel = op(cel);
            }
            doc.selection = doc
                .selection
                .as_ref()
                .map(|m| m.resized(width, height, dx, dy))
                .filter(|m| !m.is_empty());
            for g in &mut doc.extras.guides {
                g.position += if g.vertical { dx } else { dy };
            }
            if let Some(p) = &mut doc.extras.pivot {
                p.0 += dx;
                p.1 += dy;
            }
            for a in &mut doc.extras.anchors {
                a.x += dx;
                a.y += dy;
            }
            doc.width = width;
            doc.height = height;
        });
    }

    /// Resamples every layer to a new size.
    pub fn scale_image(&mut self, width: u32, height: u32, method: ScaleMethod) {
        if width == 0 || height == 0 {
            return;
        }
        let (ow, oh) = (self.width, self.height);
        self.structural("Scale image", |doc| {
            for cel in doc.cels.values_mut() {
                *cel = resample::scale(cel, width, height, method);
            }
            doc.selection = None;
            let (sx, sy) = (width as f32 / ow as f32, height as f32 / oh as f32);
            for g in &mut doc.extras.guides {
                g.position = (g.position as f32 * if g.vertical { sx } else { sy }).round() as i32;
            }
            if let Some(p) = &mut doc.extras.pivot {
                *p = ((p.0 as f32 * sx).round() as i32, (p.1 as f32 * sy).round() as i32);
            }
            doc.width = width;
            doc.height = height;
        });
    }

    /// Crops the canvas to the selection's bounds.
    pub fn crop_to_selection(&mut self) {
        if let Some(b) = self.selection_bounds() {
            self.crop(b, "Crop");
        }
    }

    /// Crops away fully transparent borders (across all frames and layers).
    pub fn trim(&mut self) {
        let mut bounds: Option<Rect> = None;
        for f in 0..self.frames.len() {
            if let Some(b) = self.flatten_frame(f).opaque_bounds() {
                bounds = Some(match bounds {
                    Some(a) => {
                        let x0 = a.x.min(b.x);
                        let y0 = a.y.min(b.y);
                        let x1 = (a.x + a.w).max(b.x + b.w);
                        let y1 = (a.y + a.h).max(b.y + b.h);
                        Rect::new(x0, y0, x1 - x0, y1 - y0)
                    }
                    None => b,
                });
            }
        }
        if let Some(b) = bounds
            && b != self.bounds()
        {
            self.crop(b, "Trim");
        }
    }

    fn crop(&mut self, rect: Rect, label: &str) {
        let (w, h) = (rect.w as u32, rect.h as u32);
        if w == 0 || h == 0 {
            return;
        }
        self.reframe(label, w, h, |b| b.crop(rect), -rect.x, -rect.y);
    }

    pub fn flip_horizontal(&mut self) {
        self.transform_canvas(Transform::FlipHorizontal);
    }

    pub fn flip_vertical(&mut self) {
        self.transform_canvas(Transform::FlipVertical);
    }

    /// Flips or rotates the whole canvas (every layer and frame).
    pub fn transform_canvas(&mut self, t: Transform) {
        let (w, h) = match t {
            Transform::Rotate90 { .. } => (self.height, self.width),
            Transform::Scale { width, height, method } => {
                self.scale_image(width, height, method);
                return;
            }
            _ => (self.width, self.height),
        };
        let label = match t {
            Transform::FlipHorizontal => "Flip canvas horizontally",
            Transform::FlipVertical => "Flip canvas vertically",
            _ => "Rotate canvas",
        };
        self.structural(label, |doc| {
            for cel in doc.cels.values_mut() {
                *cel = match t {
                    Transform::Rotate { degrees, smooth } => resample::rotate(cel, degrees, smooth),
                    _ => t.apply(cel),
                };
            }
            doc.selection = None;
            doc.width = w;
            doc.height = h;
        });
    }

    /// Flips or rotates the active layer in the frames being edited, around the canvas centre.
    pub fn transform_layer(&mut self, t: Transform) {
        if self.edit_blocker().is_some() {
            return;
        }
        let (w, h) = (self.width, self.height);
        self.apply_filter("Transform layer", Scope::Cel, |b| match t {
            Transform::Rotate { degrees, smooth } => resample::rotate(b, degrees, smooth),
            Transform::Rotate90 { clockwise } if w != h => {
                resample::rotate(b, if clockwise { 90.0 } else { -90.0 }, false)
            }
            Transform::Scale { width, height, method } => {
                let scaled = resample::scale(b, width, height, method);
                scaled.resized_offset(w, h, (w as i32 - width as i32) / 2, (h as i32 - height as i32) / 2)
            }
            _ => t.apply(b),
        });
    }

    /// Flips, rotates or scales the selected pixels (lifting them first) around their centre.
    /// Without a selection, transforms the layer.
    pub fn transform_selection(&mut self, t: Transform) {
        if self.selection.is_none() && self.floating.is_none() {
            self.transform_layer(t);
            return;
        }
        self.stroke_end();
        if self.floating.is_none() {
            if self.edit_blocker().is_some() {
                return;
            }
            self.lift(false);
        }
        if let Some(f) = &mut self.floating {
            let (cx, cy) = (
                f.pos.0 as f32 + f.image.width() as f32 / 2.0,
                f.pos.1 as f32 + f.image.height() as f32 / 2.0,
            );
            let image = t.apply(&f.image);
            let mut shape = t.apply_mask(&f.shape);
            if matches!(t, Transform::Scale { .. } | Transform::Rotate { .. }) {
                // The new shape is wherever the transformed pixels are.
                shape = Mask::from_alpha(&image);
                if shape.is_empty() {
                    shape = Mask::full(image.width(), image.height());
                }
            }
            f.pos = (
                (cx - image.width() as f32 / 2.0).round() as i32,
                (cy - image.height() as f32 / 2.0).round() as i32,
            );
            f.image = image;
            f.shape = shape;
            self.selection = Some(floating_selection(f, self.width, self.height));
            self.revision += 1;
        }
    }

    // ---- history ----

    pub fn undo(&mut self) -> bool {
        self.stroke_end();
        if self.floating.is_some() {
            self.commit_floating();
        }
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        self.apply_edit(&edit, true);
        self.coalescing = false;
        self.redo.push(edit);
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        self.stroke_end();
        self.commit_floating();
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.apply_edit(&edit, false);
        self.coalescing = false;
        self.undo.push(edit);
        self.revision += 1;
        true
    }

    fn apply_edit(&mut self, edit: &Edit, backwards: bool) {
        match &edit.kind {
            EditKind::Pixels {
                patches,
                selection_before,
                selection_after,
                place,
            } => {
                for p in patches {
                    if let Some(cel) = self.cels.get_mut(&p.cel) {
                        cel.apply_patch(p.rect, if backwards { &p.before } else { &p.after });
                    }
                }
                self.selection = if backwards {
                    selection_before.clone()
                } else {
                    selection_after.clone()
                };
                if place.0 < self.layers.len() {
                    self.active = place.0;
                }
                if place.1 < self.frames.len() {
                    self.current = place.1;
                }
            }
            EditKind::Structure { before, after } => {
                self.restore(if backwards { before } else { after });
            }
            EditKind::Layer {
                index,
                before,
                after,
            } => {
                if let Some(layer) = self.layers.get_mut(*index) {
                    let cels = layer.cels.clone();
                    *layer = if backwards { (**before).clone() } else { (**after).clone() };
                    layer.cels = cels;
                }
            }
            EditKind::Selection { before, after } => {
                self.selection = if backwards { before.clone() } else { after.clone() };
            }
        }
    }
}

/// Composites floating pixels onto a cel image (only where its shape is set).
fn draw_floating(image: &mut Bitmap, f: &Floating) {
    for y in 0..f.image.height() as i32 {
        for x in 0..f.image.width() as i32 {
            if !f.shape.get(x, y) {
                continue;
            }
            let (cx, cy) = (f.pos.0 + x, f.pos.1 + y);
            let (Some(src), Some(dst)) = (f.image.get(x, y), image.get(cx, cy)) else {
                continue;
            };
            let out = if f.opaque {
                src
            } else {
                crate::blend::blend_pixel(dst, src, BlendMode::Normal, 1.0)
            };
            image.set(cx, cy, out);
        }
    }
}

/// The floating shape placed on the canvas as a selection.
fn floating_selection(f: &Floating, width: u32, height: u32) -> Mask {
    let mut mask = Mask::new(width, height);
    for y in 0..f.shape.height() as i32 {
        for x in 0..f.shape.width() as i32 {
            if f.shape.get(x, y) {
                mask.set(f.pos.0 + x, f.pos.1 + y, true);
            }
        }
    }
    mask
}

#[cfg(test)]
mod tests;

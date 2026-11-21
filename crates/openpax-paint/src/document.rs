use crate::tools::{Painter, move_region};
use crate::{Bitmap, Rect, Rgba, TRANSPARENT, ToolKind, ToolSettings};

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub pixels: Bitmap,
}

impl Layer {
    pub fn new(name: impl Into<String>, pixels: Bitmap) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity: 1.0,
            pixels,
        }
    }
}

/// Everything a structural edit (layers, canvas size) can change.
#[derive(Clone, Debug, PartialEq)]
struct State {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    selection: Option<Rect>,
}

#[derive(Debug)]
enum Edit {
    Pixels {
        layer: usize,
        rect: Rect,
        before: Vec<u8>,
        after: Vec<u8>,
        selection_before: Option<Rect>,
        selection_after: Option<Rect>,
    },
    Structure {
        before: Box<State>,
        after: Box<State>,
    },
    /// A change to one layer's name, visibility or opacity.
    Layer {
        index: usize,
        before: LayerProps,
        after: LayerProps,
        /// Only the opacity changed, so a following opacity change may be merged into this.
        opacity_only: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct LayerProps {
    name: String,
    visible: bool,
    opacity: f32,
}

impl LayerProps {
    fn of(layer: &Layer) -> Self {
        Self {
            name: layer.name.clone(),
            visible: layer.visible,
            opacity: layer.opacity,
        }
    }

    fn apply(&self, layer: &mut Layer) {
        layer.name.clone_from(&self.name);
        layer.visible = self.visible;
        layer.opacity = self.opacity;
    }
}

#[derive(Debug)]
struct Stroke {
    tool: ToolKind,
    settings: ToolSettings,
    start: (i32, i32),
    last: (i32, i32),
    /// The active layer as it was when the stroke began.
    base: Bitmap,
    selection_before: Option<Rect>,
    moved: bool,
}

/// A layered image with selection and undo history. Layer 0 is the bottom layer.
#[derive(Debug)]
pub struct Document {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    selection: Option<Rect>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    revision: u64,
    stroke: Option<Stroke>,
    /// Whether the next opacity change may merge into the previous one (one slider drag).
    coalescing: bool,
}

impl Document {
    pub fn new(width: u32, height: u32) -> Self {
        Self::from_bitmap(Bitmap::new(width, height))
    }

    pub fn from_bitmap(bitmap: Bitmap) -> Self {
        let (width, height) = (bitmap.width(), bitmap.height());
        Self::from_layers(width, height, vec![Layer::new("Layer 1", bitmap)], 0)
    }

    /// Builds a document from existing layers. Panics if a layer's size differs or there are none.
    pub fn from_layers(width: u32, height: u32, layers: Vec<Layer>, active: usize) -> Self {
        assert!(!layers.is_empty(), "a document needs at least one layer");
        assert!(
            layers
                .iter()
                .all(|l| (l.pixels.width(), l.pixels.height()) == (width, height))
        );
        Self {
            width,
            height,
            active: active.min(layers.len() - 1),
            layers,
            selection: None,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            stroke: None,
            coalescing: false,
        }
    }

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

    pub fn set_active(&mut self, index: usize) {
        if index < self.layers.len() {
            self.active = index;
        }
    }

    pub fn selection(&self) -> Option<Rect> {
        self.selection
    }

    /// Changes whenever the visible result may have changed; use it to invalidate caches.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// All visible layers composited together.
    pub fn flatten(&self) -> Bitmap {
        let mut out = Bitmap::new(self.width, self.height);
        for layer in self.layers.iter().filter(|l| l.visible) {
            out.composite(&layer.pixels, layer.opacity);
        }
        out
    }

    // ---- layer properties ----

    /// Changes a layer's properties as one undo step. Consecutive opacity changes to the same
    /// layer merge into one step until [`Self::end_coalescing`] is called.
    fn layer_edit(&mut self, index: usize, opacity_only: bool, edit: impl FnOnce(&mut Layer)) {
        let Some(layer) = self.layers.get_mut(index) else {
            return;
        };
        let before = LayerProps::of(layer);
        edit(layer);
        let after = LayerProps::of(layer);
        if before == after {
            return;
        }
        self.revision += 1;
        self.redo.clear();
        if opacity_only
            && self.coalescing
            && let Some(Edit::Layer {
                index: last_index,
                after: last_after,
                opacity_only: true,
                ..
            }) = self.undo.last_mut()
            && *last_index == index
        {
            *last_after = after;
            return;
        }
        self.undo.push(Edit::Layer {
            index,
            before,
            after,
            opacity_only,
        });
        self.coalescing = opacity_only;
    }

    /// Ends a run of merged opacity changes; the next one starts a new undo step.
    pub fn end_coalescing(&mut self) {
        self.coalescing = false;
    }

    pub fn set_layer_visible(&mut self, index: usize, visible: bool) {
        self.layer_edit(index, false, |layer| layer.visible = visible);
    }

    pub fn set_layer_opacity(&mut self, index: usize, opacity: f32) {
        self.layer_edit(index, true, |layer| layer.opacity = opacity.clamp(0.0, 1.0));
    }

    pub fn set_layer_name(&mut self, index: usize, name: String) {
        self.layer_edit(index, false, |layer| layer.name = name);
    }

    // ---- selection ----

    pub fn select_all(&mut self) {
        self.selection = Some(self.bounds());
    }

    pub fn deselect(&mut self) {
        self.selection = None;
    }

    // ---- strokes ----

    /// Starts a tool interaction at a pixel. For [`ToolKind::Picker`] returns the picked colour
    /// (of the flattened image) and starts nothing.
    pub fn stroke_begin(
        &mut self,
        tool: ToolKind,
        settings: ToolSettings,
        at: (i32, i32),
    ) -> Option<Rgba> {
        self.stroke_end();
        if tool == ToolKind::Picker {
            return self.flatten().get(at.0, at.1);
        }
        self.stroke = Some(Stroke {
            tool,
            settings,
            start: at,
            last: at,
            base: self.layers[self.active].pixels.clone(),
            selection_before: self.selection,
            moved: false,
        });
        self.stroke_apply(at, true);
        None
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

    fn stroke_apply(&mut self, to: (i32, i32), first: bool) {
        let Some(mut stroke) = self.stroke.take() else {
            return;
        };
        stroke.moved |= !first;
        let bounds = self.bounds();
        let clip = stroke.selection_before;
        let color = if stroke.tool == ToolKind::Eraser {
            TRANSPARENT
        } else {
            stroke.settings.color
        };
        let layer = &mut self.layers[self.active].pixels;

        match stroke.tool {
            ToolKind::Select => {
                self.selection = Rect::from_points(stroke.start, to).intersect(&bounds);
            }
            ToolKind::Move => {
                let (dx, dy) = (to.0 - stroke.start.0, to.1 - stroke.start.1);
                let region = clip.unwrap_or(bounds);
                layer.clone_from(&stroke.base);
                move_region(layer, &stroke.base, region, dx, dy);
                if clip.is_some() {
                    self.selection = region.translated(dx, dy).intersect(&bounds);
                }
            }
            tool => {
                if matches!(
                    tool,
                    ToolKind::Line | ToolKind::Rectangle | ToolKind::Ellipse
                ) {
                    layer.clone_from(&stroke.base);
                }
                let mut painter = Painter {
                    target: layer,
                    clip,
                    settings: stroke.settings,
                    color,
                };
                match tool {
                    ToolKind::Pencil | ToolKind::Eraser => painter.line(stroke.last, to),
                    ToolKind::Fill if first => painter.fill(to.0, to.1),
                    ToolKind::Line => painter.line(stroke.start, to),
                    ToolKind::Rectangle => painter.rectangle(stroke.start, to),
                    ToolKind::Ellipse => painter.ellipse(stroke.start, to),
                    _ => {}
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
            self.layers[self.active].pixels = stroke.base;
            self.selection = stroke.selection_before;
            self.revision += 1;
        }
    }

    /// Finishes the current stroke and records it in the undo history.
    pub fn stroke_end(&mut self) {
        let Some(stroke) = self.stroke.take() else {
            return;
        };
        if stroke.tool == ToolKind::Select {
            if !stroke.moved {
                self.selection = None; // a plain click deselects
            }
            return;
        }
        self.record_pixels(&stroke.base, stroke.selection_before);
    }

    /// Records the difference between `base` and the active layer as one undo step.
    fn record_pixels(&mut self, base: &Bitmap, selection_before: Option<Rect>) {
        let current = &self.layers[self.active].pixels;
        let rect = current.diff_bounds(base);
        if rect.is_none() && selection_before == self.selection {
            return;
        }
        let rect = rect.unwrap_or(Rect::new(0, 0, 0, 0));
        self.undo.push(Edit::Pixels {
            layer: self.active,
            rect,
            before: base.patch(rect),
            after: current.patch(rect),
            selection_before,
            selection_after: self.selection,
        });
        self.redo.clear();
        self.revision += 1;
    }

    /// Runs a direct pixel edit on the active layer as one undo step.
    fn pixel_edit(&mut self, edit: impl FnOnce(&mut Bitmap, Option<Rect>)) {
        self.stroke_end();
        let base = self.layers[self.active].pixels.clone();
        edit(&mut self.layers[self.active].pixels, self.selection);
        self.record_pixels(&base, self.selection);
    }

    /// Erases the selection (or the whole active layer when nothing is selected).
    pub fn clear_selected(&mut self) {
        self.pixel_edit(|pixels, selection| {
            let region = selection.unwrap_or(pixels.bounds());
            for y in region.y..region.y + region.h {
                for x in region.x..region.x + region.w {
                    pixels.set(x, y, TRANSPARENT);
                }
            }
        });
    }

    // ---- structural edits ----

    fn state(&self) -> State {
        State {
            width: self.width,
            height: self.height,
            layers: self.layers.clone(),
            active: self.active,
            selection: self.selection,
        }
    }

    fn restore(&mut self, state: &State) {
        self.width = state.width;
        self.height = state.height;
        self.layers.clone_from(&state.layers);
        self.active = state.active;
        self.selection = state.selection;
    }

    fn structural(&mut self, edit: impl FnOnce(&mut Self)) {
        self.stroke_end();
        let before = self.state();
        edit(self);
        let after = self.state();
        if before != after {
            self.undo.push(Edit::Structure {
                before: Box::new(before),
                after: Box::new(after),
            });
            self.redo.clear();
            self.revision += 1;
        }
    }

    /// Adds an empty layer above the active one and makes it active.
    pub fn add_layer(&mut self) {
        self.structural(|doc| {
            let name = format!("Layer {}", doc.layers.len() + 1);
            let layer = Layer::new(name, Bitmap::new(doc.width, doc.height));
            doc.layers.insert(doc.active + 1, layer);
            doc.active += 1;
        });
    }

    pub fn duplicate_layer(&mut self) {
        self.structural(|doc| {
            let mut copy = doc.layers[doc.active].clone();
            copy.name.push_str(" copy");
            doc.layers.insert(doc.active + 1, copy);
            doc.active += 1;
        });
    }

    /// Removes the active layer; the last remaining layer cannot be removed.
    pub fn remove_layer(&mut self) {
        self.structural(|doc| {
            if doc.layers.len() > 1 {
                doc.layers.remove(doc.active);
                doc.active = doc.active.min(doc.layers.len() - 1);
            }
        });
    }

    /// Moves the active layer one step up (towards the top) or down.
    pub fn move_layer(&mut self, up: bool) {
        self.structural(|doc| {
            let target = if up {
                doc.active + 1
            } else {
                doc.active.wrapping_sub(1)
            };
            if target < doc.layers.len() {
                doc.layers.swap(doc.active, target);
                doc.active = target;
            }
        });
    }

    /// Moves the active layer to `index` (0 is the bottom) as a single undo step.
    pub fn move_layer_to(&mut self, index: usize) {
        self.structural(|doc| {
            let target = index.min(doc.layers.len() - 1);
            let layer = doc.layers.remove(doc.active);
            doc.layers.insert(target, layer);
            doc.active = target;
        });
    }

    /// Composites the active layer onto the one below it.
    pub fn merge_down(&mut self) {
        self.structural(|doc| {
            if doc.active == 0 {
                return;
            }
            let upper = doc.layers.remove(doc.active);
            doc.active -= 1;
            if upper.visible {
                doc.layers[doc.active]
                    .pixels
                    .composite(&upper.pixels, upper.opacity);
            }
        });
    }

    /// Changes the canvas size, keeping the top-left corner in place.
    pub fn resize_canvas(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.structural(|doc| {
            for layer in &mut doc.layers {
                layer.pixels = layer.pixels.resized(width, height);
            }
            doc.width = width;
            doc.height = height;
            doc.selection = doc.selection.and_then(|s| s.intersect(&doc.bounds()));
        });
    }

    pub fn flip_horizontal(&mut self) {
        self.structural(|doc| {
            for layer in &mut doc.layers {
                layer.pixels = layer.pixels.flipped_horizontal();
            }
            doc.selection = None;
        });
    }

    pub fn flip_vertical(&mut self) {
        self.structural(|doc| {
            for layer in &mut doc.layers {
                layer.pixels = layer.pixels.flipped_vertical();
            }
            doc.selection = None;
        });
    }

    // ---- history ----

    pub fn undo(&mut self) -> bool {
        self.stroke_end();
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        match &edit {
            Edit::Pixels {
                layer,
                rect,
                before,
                selection_before,
                ..
            } => {
                self.layers[*layer].pixels.apply_patch(*rect, before);
                self.selection = *selection_before;
                self.active = *layer;
            }
            Edit::Structure { before, .. } => self.restore(before),
            Edit::Layer { index, before, .. } => before.apply(&mut self.layers[*index]),
        }
        self.coalescing = false;
        self.redo.push(edit);
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        self.stroke_end();
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        match &edit {
            Edit::Pixels {
                layer,
                rect,
                after,
                selection_after,
                ..
            } => {
                self.layers[*layer].pixels.apply_patch(*rect, after);
                self.selection = *selection_after;
                self.active = *layer;
            }
            Edit::Structure { after, .. } => self.restore(after),
            Edit::Layer { index, after, .. } => after.apply(&mut self.layers[*index]),
        }
        self.coalescing = false;
        self.undo.push(edit);
        self.revision += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba = [255, 0, 0, 255];
    const BLUE: Rgba = [0, 0, 255, 255];

    fn settings(color: Rgba) -> ToolSettings {
        ToolSettings {
            color,
            ..Default::default()
        }
    }

    fn drag(doc: &mut Document, tool: ToolKind, s: ToolSettings, from: (i32, i32), to: (i32, i32)) {
        doc.stroke_begin(tool, s, from);
        doc.stroke_move(to);
        doc.stroke_end();
    }

    /// Renders the flattened document as rows of `#` (opaque) and `.` (transparent).
    fn ascii(doc: &Document) -> Vec<String> {
        let flat = doc.flatten();
        (0..flat.height() as i32)
            .map(|y| {
                (0..flat.width() as i32)
                    .map(|x| {
                        if flat.get(x, y).unwrap()[3] > 0 {
                            '#'
                        } else {
                            '.'
                        }
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn pencil_click_and_drag() {
        let mut doc = Document::new(4, 4);
        doc.stroke_begin(ToolKind::Pencil, settings(RED), (0, 0));
        doc.stroke_end();
        drag(&mut doc, ToolKind::Pencil, settings(RED), (0, 3), (3, 3));
        assert_eq!(ascii(&doc), ["#...", "....", "....", "####"]);
        assert_eq!(doc.flatten().get(0, 0), Some(RED));
    }

    #[test]
    fn pencil_survives_out_of_bounds_drag() {
        let mut doc = Document::new(3, 3);
        drag(&mut doc, ToolKind::Pencil, settings(RED), (-5, 1), (10, 1));
        assert_eq!(ascii(&doc), ["...", "###", "..."]);
    }

    #[test]
    fn brush_size_and_eraser() {
        let mut doc = Document::new(4, 4);
        let big = ToolSettings {
            size: 2,
            ..settings(RED)
        };
        doc.stroke_begin(ToolKind::Pencil, big, (2, 2));
        doc.stroke_end();
        assert_eq!(ascii(&doc), ["....", ".##.", ".##.", "...."]);
        doc.stroke_begin(ToolKind::Eraser, settings(RED), (1, 1));
        doc.stroke_end();
        assert_eq!(ascii(&doc), ["....", "..#.", ".##.", "...."]);
        assert_eq!(doc.flatten().get(1, 1), Some(TRANSPARENT));
    }

    #[test]
    fn diagonal_line_is_pixel_perfect() {
        let mut doc = Document::new(4, 4);
        drag(&mut doc, ToolKind::Line, settings(RED), (0, 0), (3, 3));
        assert_eq!(ascii(&doc), ["#...", ".#..", "..#.", "...#"]);
    }

    #[test]
    fn shape_preview_does_not_leave_trails() {
        let mut doc = Document::new(4, 4);
        doc.stroke_begin(ToolKind::Line, settings(RED), (0, 0));
        doc.stroke_move((3, 3));
        doc.stroke_move((3, 0));
        doc.stroke_end();
        assert_eq!(ascii(&doc), ["####", "....", "....", "...."]);
    }

    #[test]
    fn rectangle_outline_and_filled() {
        let mut doc = Document::new(4, 4);
        drag(&mut doc, ToolKind::Rectangle, settings(RED), (3, 3), (0, 0));
        assert_eq!(ascii(&doc), ["####", "#..#", "#..#", "####"]);
        let filled = ToolSettings {
            shape_filled: true,
            ..settings(RED)
        };
        drag(&mut doc, ToolKind::Rectangle, filled, (1, 1), (2, 2));
        assert_eq!(ascii(&doc), ["####", "####", "####", "####"]);
    }

    #[test]
    fn ellipse_outline() {
        let mut doc = Document::new(6, 6);
        drag(&mut doc, ToolKind::Ellipse, settings(RED), (0, 0), (5, 5));
        assert_eq!(
            ascii(&doc),
            [".####.", "#....#", "#....#", "#....#", "#....#", ".####."]
        );
    }

    #[test]
    fn fill_contiguous_versus_global() {
        let build = || {
            let mut doc = Document::new(5, 3);
            drag(&mut doc, ToolKind::Line, settings(RED), (2, 0), (2, 2));
            doc
        };
        let mut doc = build();
        doc.stroke_begin(ToolKind::Fill, settings(BLUE), (0, 0));
        doc.stroke_end();
        assert_eq!(doc.flatten().get(1, 2), Some(BLUE));
        assert_eq!(doc.flatten().get(2, 1), Some(RED));
        assert_eq!(
            doc.flatten().get(4, 0),
            Some(TRANSPARENT),
            "wall stops contiguous fill"
        );

        let mut doc = build();
        let global = ToolSettings {
            fill_contiguous: false,
            ..settings(BLUE)
        };
        doc.stroke_begin(ToolKind::Fill, global, (0, 0));
        doc.stroke_end();
        assert_eq!(doc.flatten().get(4, 0), Some(BLUE));
        assert_eq!(doc.flatten().get(2, 1), Some(RED));
    }

    #[test]
    fn mirror_draws_all_four_quadrants() {
        let mut doc = Document::new(4, 4);
        let mirrored = ToolSettings {
            mirror_x: true,
            mirror_y: true,
            ..settings(RED)
        };
        doc.stroke_begin(ToolKind::Pencil, mirrored, (0, 1));
        doc.stroke_end();
        assert_eq!(ascii(&doc), ["....", "#..#", "#..#", "...."]);
    }

    #[test]
    fn selection_clips_painting() {
        let mut doc = Document::new(4, 4);
        drag(&mut doc, ToolKind::Select, settings(RED), (1, 1), (2, 2));
        assert_eq!(doc.selection(), Some(Rect::new(1, 1, 2, 2)));
        let filled = ToolSettings {
            shape_filled: true,
            ..settings(RED)
        };
        drag(&mut doc, ToolKind::Rectangle, filled, (0, 0), (3, 3));
        assert_eq!(ascii(&doc), ["....", ".##.", ".##.", "...."]);

        doc.stroke_begin(ToolKind::Select, settings(RED), (0, 0));
        doc.stroke_end();
        assert_eq!(doc.selection(), None, "a click without dragging deselects");
    }

    #[test]
    fn move_selection_and_undo_restores_it() {
        let mut doc = Document::new(4, 4);
        doc.stroke_begin(ToolKind::Pencil, settings(RED), (0, 0));
        doc.stroke_end();
        drag(&mut doc, ToolKind::Select, settings(RED), (0, 0), (1, 1));
        drag(&mut doc, ToolKind::Move, settings(RED), (0, 0), (2, 1));
        assert_eq!(ascii(&doc), ["....", "..#.", "....", "...."]);
        assert_eq!(doc.selection(), Some(Rect::new(2, 1, 2, 2)));

        assert!(doc.undo());
        assert_eq!(ascii(&doc), ["#...", "....", "....", "...."]);
        assert_eq!(doc.selection(), Some(Rect::new(0, 0, 2, 2)));
    }

    #[test]
    fn picker_reads_flattened_colour_without_editing() {
        let mut doc = Document::new(2, 2);
        doc.stroke_begin(ToolKind::Pencil, settings(RED), (1, 1));
        doc.stroke_end();
        doc.add_layer();
        let revision = doc.revision();
        assert_eq!(
            doc.stroke_begin(ToolKind::Picker, settings(BLUE), (1, 1)),
            Some(RED)
        );
        assert!(!doc.stroke_active());
        assert_eq!(doc.revision(), revision);
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut doc = Document::new(4, 4);
        let empty = doc.flatten();
        drag(&mut doc, ToolKind::Pencil, settings(RED), (0, 0), (3, 0));
        let one = doc.flatten();
        drag(&mut doc, ToolKind::Pencil, settings(BLUE), (0, 0), (0, 3));
        let two = doc.flatten();

        assert!(doc.undo());
        assert_eq!(doc.flatten(), one);
        assert!(doc.undo());
        assert_eq!(doc.flatten(), empty);
        assert!(!doc.undo());
        assert!(doc.redo() && doc.redo());
        assert_eq!(doc.flatten(), two);
        assert!(!doc.redo());

        doc.undo();
        drag(&mut doc, ToolKind::Pencil, settings(RED), (3, 3), (3, 3));
        assert!(!doc.can_redo(), "a new edit discards the redo stack");
    }

    #[test]
    fn stroke_that_changes_nothing_is_not_recorded() {
        let mut doc = Document::new(2, 2);
        doc.stroke_begin(ToolKind::Eraser, settings(RED), (0, 0));
        doc.stroke_end();
        assert!(!doc.can_undo());
    }

    #[test]
    fn layers_composite_and_structural_undo() {
        let mut doc = Document::new(2, 1);
        doc.stroke_begin(ToolKind::Pencil, settings(RED), (0, 0));
        doc.stroke_end();
        doc.add_layer();
        assert_eq!((doc.layers().len(), doc.active()), (2, 1));
        drag(&mut doc, ToolKind::Pencil, settings(BLUE), (0, 0), (1, 0));
        assert_eq!(doc.flatten().get(0, 0), Some(BLUE));

        doc.set_layer_visible(1, false);
        assert_eq!(doc.flatten().get(0, 0), Some(RED));
        assert_eq!(doc.flatten().get(1, 0), Some(TRANSPARENT));
        doc.set_layer_visible(1, true);

        doc.move_layer(false);
        assert_eq!(doc.active(), 0);
        assert_eq!(
            doc.flatten().get(0, 0),
            Some(RED),
            "blue layer is now underneath"
        );
        doc.undo();
        assert_eq!(doc.flatten().get(0, 0), Some(BLUE));

        doc.merge_down();
        assert_eq!(doc.layers().len(), 1);
        assert_eq!(doc.flatten().get(1, 0), Some(BLUE));
        doc.undo();
        assert_eq!(doc.layers().len(), 2);

        doc.remove_layer();
        doc.remove_layer();
        assert_eq!(doc.layers().len(), 1, "the last layer cannot be removed");
    }

    #[test]
    fn resize_flip_and_clear() {
        let mut doc = Document::new(2, 2);
        doc.stroke_begin(ToolKind::Pencil, settings(RED), (0, 0));
        doc.stroke_end();

        doc.resize_canvas(3, 4);
        assert_eq!((doc.width(), doc.height()), (3, 4));
        assert_eq!(ascii(&doc), ["#..", "...", "...", "..."]);
        doc.flip_horizontal();
        assert_eq!(ascii(&doc)[0], "..#");
        doc.flip_vertical();
        assert_eq!(ascii(&doc)[3], "..#");

        doc.clear_selected();
        assert_eq!(doc.flatten(), Bitmap::new(3, 4));
        doc.undo();
        doc.undo();
        doc.undo();
        doc.undo();
        assert_eq!((doc.width(), doc.height()), (2, 2));
        assert_eq!(ascii(&doc), ["#.", ".."]);
    }

    #[test]
    fn layer_properties_are_undoable() {
        let mut doc = Document::new(2, 2);
        doc.set_layer_visible(0, false);
        doc.set_layer_name(0, "Base".to_owned());
        assert!(!doc.layers()[0].visible);

        assert!(doc.undo());
        assert_eq!(doc.layers()[0].name, "Layer 1");
        assert!(doc.undo());
        assert!(doc.layers()[0].visible);
        assert!(!doc.can_undo());
        assert!(doc.redo() && doc.redo());
        assert_eq!(doc.layers()[0].name, "Base");
        assert!(!doc.layers()[0].visible);

        // Setting a property to the value it already has records nothing.
        let revision = doc.revision();
        doc.set_layer_visible(0, false);
        assert_eq!(doc.revision(), revision);
    }

    #[test]
    fn one_opacity_drag_is_one_undo_step() {
        let mut doc = Document::new(2, 2);
        for step in 1..=5 {
            doc.set_layer_opacity(0, 1.0 - step as f32 * 0.1);
        }
        doc.end_coalescing();
        doc.set_layer_opacity(0, 0.2);

        assert!(doc.undo());
        assert!((doc.layers()[0].opacity - 0.5).abs() < 1e-6);
        assert!(doc.undo());
        assert_eq!(doc.layers()[0].opacity, 1.0);
        assert!(!doc.can_undo());
    }

    #[test]
    fn move_layer_to_reorders_in_one_step() {
        let mut doc = Document::new(1, 1);
        doc.add_layer();
        doc.add_layer();
        let names = |doc: &Document| -> Vec<String> {
            doc.layers().iter().map(|l| l.name.clone()).collect()
        };
        assert_eq!(names(&doc), ["Layer 1", "Layer 2", "Layer 3"]);

        doc.move_layer_to(0);
        assert_eq!(names(&doc), ["Layer 3", "Layer 1", "Layer 2"]);
        assert_eq!(doc.active(), 0);
        doc.move_layer_to(99);
        assert_eq!(names(&doc), ["Layer 1", "Layer 2", "Layer 3"]);
        assert_eq!(doc.active(), 2);

        assert!(doc.undo());
        assert_eq!(names(&doc), ["Layer 3", "Layer 1", "Layer 2"]);
    }

    #[test]
    fn cancelled_stroke_leaves_no_trace() {
        let mut doc = Document::new(4, 4);
        let settings = ToolSettings {
            color: [255, 0, 0, 255],
            ..Default::default()
        };
        doc.stroke_begin(ToolKind::Pencil, settings, (0, 0));
        doc.stroke_move((3, 3));
        assert_eq!(doc.flatten().get(2, 2), Some([255, 0, 0, 255]));
        doc.stroke_cancel();
        assert!(!doc.stroke_active());
        assert_eq!(doc.flatten(), Bitmap::new(4, 4));
        assert!(!doc.can_undo());
    }
}

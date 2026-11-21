use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::mask::{Mask, SelectMode};
use crate::raster::line_points;
use crate::{Bitmap, Rect, Rgba, TRANSPARENT};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolKind {
    Pencil,
    Eraser,
    Fill,
    Picker,
    Line,
    Polyline,
    Polygon,
    Rectangle,
    Ellipse,
    Shade,
    Dither,
    Select,
    Lasso,
    MagicWand,
    Move,
    TileStamp,
}

impl ToolKind {
    pub const ALL: [ToolKind; 16] = [
        Self::Pencil,
        Self::Eraser,
        Self::Fill,
        Self::Picker,
        Self::Line,
        Self::Polyline,
        Self::Polygon,
        Self::Rectangle,
        Self::Ellipse,
        Self::Shade,
        Self::Dither,
        Self::Select,
        Self::Lasso,
        Self::MagicWand,
        Self::Move,
        Self::TileStamp,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Pencil => "Pencil",
            Self::Eraser => "Eraser",
            Self::Fill => "Fill",
            Self::Picker => "Picker",
            Self::Line => "Line",
            Self::Polyline => "Polyline",
            Self::Polygon => "Polygon",
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Shade => "Shade",
            Self::Dither => "Dither Brush",
            Self::Select => "Select",
            Self::Lasso => "Lasso",
            Self::MagicWand => "Magic Wand",
            Self::Move => "Move",
            Self::TileStamp => "Tile Stamp",
        }
    }

    /// Tools that pick points with clicks and finish with Enter or a double-click.
    pub fn is_multi_click(self) -> bool {
        matches!(self, Self::Polyline | Self::Polygon)
    }

    /// Tools that change the selection rather than pixels.
    pub fn is_selection(self) -> bool {
        matches!(self, Self::Select | Self::Lasso | Self::MagicWand)
    }

    /// Whether the brush size and shape affect the tool.
    pub fn uses_brush(self) -> bool {
        matches!(
            self,
            Self::Pencil
                | Self::Eraser
                | Self::Line
                | Self::Polyline
                | Self::Polygon
                | Self::Rectangle
                | Self::Ellipse
                | Self::Shade
                | Self::Dither
        )
    }

    /// Whether the tool puts the current colour on the canvas.
    pub fn paints(self) -> bool {
        matches!(
            self,
            Self::Pencil
                | Self::Fill
                | Self::Line
                | Self::Polyline
                | Self::Polygon
                | Self::Rectangle
                | Self::Ellipse
                | Self::Dither
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BrushShape {
    #[default]
    Square,
    Circle,
    /// The custom brush image in [`ToolSettings::custom_brush`].
    Custom,
}

/// Mirroring and radial repetition of strokes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Symmetry {
    pub x: bool,
    pub y: bool,
    pub diagonal: bool,
    pub anti_diagonal: bool,
    /// Rotational copies around the centre (0 or 1 means off).
    pub radial: u32,
    /// The centre in pixel-edge coordinates; `None` is the middle of the canvas.
    pub center: Option<(f32, f32)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolSettings {
    pub color: Rgba,
    /// Brush diameter in pixels; also the outline width of shapes.
    pub size: u32,
    pub brush: BrushShape,
    /// Pixels of the custom brush; transparent pixels are not painted.
    #[serde(skip)]
    pub custom_brush: Option<Arc<Bitmap>>,
    /// Paint the custom brush with the current colour instead of its own colours.
    pub brush_tinted: bool,
    /// Remove doubled-up corner pixels from 1-pixel freehand strokes.
    pub pixel_perfect: bool,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub symmetry: Symmetry,
    /// Fill only the connected region (otherwise every pixel of the clicked colour).
    pub fill_contiguous: bool,
    /// Largest per-channel difference that still counts as the same colour (fill, wand).
    pub tolerance: u8,
    /// Fill and magic wand also spread across diagonal neighbours.
    pub diagonal: bool,
    /// Fill and magic wand look at all visible layers instead of the active one.
    pub sample_merged: bool,
    /// The picker reads the active layer instead of everything visible.
    pub pick_active_layer: bool,
    /// The stroke was started with the secondary button (shading darkens).
    #[serde(skip)]
    pub secondary_button: bool,
    /// Draw rectangles, ellipses and polygons filled instead of outlined.
    pub shape_filled: bool,
    /// Lines snap to clean angles and boxes to squares (usually Shift).
    pub constrain: bool,
    /// Draw shapes from their centre (usually Alt).
    pub from_center: bool,
    pub select_mode: SelectMode,
    /// Colours the shade tool steps through, darkest first.
    pub ramp: Vec<Rgba>,
    /// The second colour of the dither brush.
    pub dither_color: Rgba,
    /// Dither density, 0 (all primary) to 16 (all `dither_color`).
    pub dither_level: u8,
    /// Strokes wrap around the canvas edges (seamless tiles).
    pub wrap: bool,
    /// Pen pressure of the current stroke, 0..=1 (1 without a pen).
    #[serde(skip, default = "full_pressure")]
    pub pressure: f32,
    /// Pen pressure changes the brush size.
    pub pressure_size: bool,
    /// Pen pressure changes the opacity of painted pixels.
    pub pressure_opacity: bool,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 255],
            size: 1,
            brush: BrushShape::Square,
            custom_brush: None,
            brush_tinted: false,
            pixel_perfect: false,
            mirror_x: false,
            mirror_y: false,
            symmetry: Symmetry::default(),
            fill_contiguous: true,
            tolerance: 0,
            diagonal: false,
            sample_merged: false,
            pick_active_layer: false,
            secondary_button: false,
            shape_filled: false,
            constrain: false,
            from_center: false,
            select_mode: SelectMode::Replace,
            ramp: Vec::new(),
            dither_color: [255, 255, 255, 255],
            dither_level: 8,
            wrap: false,
            pressure: 1.0,
            pressure_size: false,
            pressure_opacity: false,
        }
    }
}

fn full_pressure() -> f32 {
    1.0
}

impl ToolSettings {
    /// The brush diameter after pen pressure.
    pub fn effective_size(&self) -> u32 {
        if self.pressure_size {
            ((self.size as f32 * self.pressure.clamp(0.0, 1.0)).round() as u32).max(1)
        } else {
            self.size.max(1)
        }
    }

    /// The pixel offsets (relative to the brush centre) the brush covers, with an optional
    /// own colour per pixel for custom brushes.
    pub fn brush_offsets(&self) -> Vec<(i32, i32, Option<Rgba>)> {
        let size = self.effective_size() as i32;
        match (self.brush, &self.custom_brush) {
            (BrushShape::Custom, Some(image)) => {
                let (w, h) = (image.width() as i32, image.height() as i32);
                let mut out = Vec::new();
                for y in 0..h {
                    for x in 0..w {
                        let c = image.get(x, y).unwrap_or(TRANSPARENT);
                        if c[3] > 0 {
                            let own = (!self.brush_tinted).then_some(c);
                            out.push((x - w / 2, y - h / 2, own));
                        }
                    }
                }
                out
            }
            (BrushShape::Circle, _) if size > 2 => {
                let r = f64::from(size) / 2.0;
                let mut out = Vec::new();
                let start = -(size / 2);
                for y in start..start + size {
                    for x in start..start + size {
                        let (fx, fy) = (
                            f64::from(x - start) + 0.5 - r,
                            f64::from(y - start) + 0.5 - r,
                        );
                        if fx * fx + fy * fy <= r * r + 0.25 {
                            out.push((x, y, None));
                        }
                    }
                }
                out
            }
            _ => {
                let start = -(size / 2);
                (start..start + size)
                    .flat_map(|y| (start..start + size).map(move |x| (x, y, None)))
                    .collect()
            }
        }
    }
}

/// What a painter writes for each pixel it touches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PaintMode {
    /// Write the colour (or the brush's own colours).
    Replace,
    /// Make pixels transparent.
    Erase,
    /// Step the existing colour along the ramp: lighter (towards the end) or darker.
    Shade { lighter: bool },
    /// Ordered dither between the colour and the settings' dither colour.
    Dither,
}

const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Writes pixels into a bitmap while honouring a selection clip, alpha lock, brush and
/// symmetry.
pub(crate) struct Painter<'a> {
    pub target: &'a mut Bitmap,
    /// The layer as it was before the stroke: shading reads it, alpha lock respects it.
    pub base: &'a Bitmap,
    pub clip: Option<&'a Mask>,
    pub alpha_lock: bool,
    pub settings: &'a ToolSettings,
    pub color: Rgba,
    pub mode: PaintMode,
}

impl Painter<'_> {
    fn put_color(&mut self, x: i32, y: i32, own: Option<Rgba>) {
        let (w, h) = (self.target.width() as i32, self.target.height() as i32);
        let (x, y) = if self.settings.wrap && w > 0 && h > 0 {
            (x.rem_euclid(w), y.rem_euclid(h))
        } else {
            (x, y)
        };
        if self.clip.is_some_and(|c| !c.get(x, y)) {
            return;
        }
        let Some(before) = self.base.get(x, y) else {
            return;
        };
        if self.alpha_lock && before[3] == 0 {
            return;
        }
        let mut color = match self.mode {
            PaintMode::Replace => own.unwrap_or(self.color),
            PaintMode::Erase => TRANSPARENT,
            PaintMode::Shade { lighter } => {
                let ramp = &self.settings.ramp;
                let Some(i) = ramp.iter().position(|c| *c == before) else {
                    return;
                };
                let j = if lighter {
                    (i + 1).min(ramp.len() - 1)
                } else {
                    i.saturating_sub(1)
                };
                ramp[j]
            }
            PaintMode::Dither => {
                let level = BAYER4[y.rem_euclid(4) as usize][x.rem_euclid(4) as usize];
                if level < self.settings.dither_level {
                    self.settings.dither_color
                } else {
                    self.color
                }
            }
        };
        if self.alpha_lock {
            if color[3] == 0 {
                return; // erasing would change the locked alpha
            }
            color[3] = before[3];
        }
        if self.settings.pressure_opacity && self.settings.pressure < 1.0 && color[3] != 0 {
            let src = [
                color[0],
                color[1],
                color[2],
                (f32::from(color[3]) * self.settings.pressure.max(0.0)) as u8,
            ];
            color = crate::blend::blend_pixel(before, src, crate::BlendMode::Normal, 1.0);
        }
        self.target.set(x, y, color);
    }

    /// Every symmetric image of a pixel (including the pixel itself), without duplicates.
    pub fn symmetric(&self, x: i32, y: i32) -> Vec<(i32, i32)> {
        let s = &self.settings.symmetry;
        let (w, h) = (self.target.width() as f32, self.target.height() as f32);
        let (cx, cy) = s.center.unwrap_or((w / 2.0, h / 2.0));
        let mirror_x = self.settings.mirror_x || s.x;
        let mirror_y = self.settings.mirror_y || s.y;
        let mut points = vec![(x, y)];
        let reflect_x = |p: (i32, i32)| ((2.0 * cx - p.0 as f32 - 1.0).round() as i32, p.1);
        let reflect_y = |p: (i32, i32)| (p.0, (2.0 * cy - p.1 as f32 - 1.0).round() as i32);
        // Pixel centre relative to the symmetry centre.
        let rel = |p: (i32, i32)| (p.0 as f32 + 0.5 - cx, p.1 as f32 + 0.5 - cy);
        let abs = |v: (f32, f32)| {
            (
                (cx + v.0 - 0.5).round() as i32,
                (cy + v.1 - 0.5).round() as i32,
            )
        };
        if mirror_x {
            let more: Vec<_> = points.iter().map(|p| reflect_x(*p)).collect();
            points.extend(more);
        }
        if mirror_y {
            let more: Vec<_> = points.iter().map(|p| reflect_y(*p)).collect();
            points.extend(more);
        }
        if s.diagonal {
            let more: Vec<_> = points
                .iter()
                .map(|p| {
                    let (dx, dy) = rel(*p);
                    abs((dy, dx))
                })
                .collect();
            points.extend(more);
        }
        if s.anti_diagonal {
            let more: Vec<_> = points
                .iter()
                .map(|p| {
                    let (dx, dy) = rel(*p);
                    abs((-dy, -dx))
                })
                .collect();
            points.extend(more);
        }
        if s.radial > 1 {
            let base = points.clone();
            for k in 1..s.radial {
                let a = std::f32::consts::TAU * k as f32 / s.radial as f32;
                let (sin, cos) = a.sin_cos();
                for p in &base {
                    let (dx, dy) = rel(*p);
                    points.push(abs((dx * cos - dy * sin, dx * sin + dy * cos)));
                }
            }
        }
        points.sort_unstable();
        points.dedup();
        points
    }

    /// One pixel plus its symmetric images.
    fn dot_color(&mut self, x: i32, y: i32, own: Option<Rgba>) {
        for (px, py) in self.symmetric(x, y) {
            self.put_color(px, py, own);
        }
    }

    pub fn dot(&mut self, x: i32, y: i32) {
        self.dot_color(x, y, None);
    }

    /// A brush stamp centred on the pixel.
    pub fn stamp(&mut self, x: i32, y: i32) {
        for (dx, dy, own) in self.settings.brush_offsets() {
            self.dot_color(x + dx, y + dy, own);
        }
    }

    /// A line of brush stamps, both ends included.
    pub fn line(&mut self, from: (i32, i32), to: (i32, i32)) {
        for (x, y) in line_points(from, to) {
            self.stamp(x, y);
        }
    }

    /// Stamps every point of a path (already rasterised).
    pub fn path(&mut self, points: &[(i32, i32)]) {
        for &(x, y) in points {
            self.stamp(x, y);
        }
    }

    pub fn rectangle(&mut self, a: (i32, i32), b: (i32, i32)) {
        let r = Rect::from_points(a, b);
        let (x1, y1) = (r.x + r.w - 1, r.y + r.h - 1);
        if self.settings.shape_filled {
            for y in r.y..=y1 {
                for x in r.x..=x1 {
                    self.dot(x, y);
                }
            }
        } else {
            self.line((r.x, r.y), (x1, r.y));
            self.line((r.x, y1), (x1, y1));
            self.line((r.x, r.y), (r.x, y1));
            self.line((x1, r.y), (x1, y1));
        }
    }

    /// Ellipse inscribed in the box spanned by the two corners.
    pub fn ellipse(&mut self, a: (i32, i32), b: (i32, i32)) {
        let r = Rect::from_points(a, b);
        let (rx, ry) = (f64::from(r.w) / 2.0, f64::from(r.h) / 2.0);
        let (cx, cy) = (f64::from(r.x) + rx, f64::from(r.y) + ry);
        let inside = |x: i32, y: i32| {
            let nx = (f64::from(x) + 0.5 - cx) / rx;
            let ny = (f64::from(y) + 0.5 - cy) / ry;
            r.contains(x, y) && nx * nx + ny * ny <= 1.0
        };
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                if !inside(x, y) {
                    continue;
                }
                let edge =
                    !(inside(x - 1, y) && inside(x + 1, y) && inside(x, y - 1) && inside(x, y + 1));
                if self.settings.shape_filled {
                    self.dot(x, y);
                } else if edge {
                    self.stamp(x, y);
                }
            }
        }
    }

    /// A chain of lines; `closed` joins the last point to the first. Filled polygons are
    /// filled with the even-odd rule.
    pub fn polygon(&mut self, points: &[(i32, i32)], closed: bool) {
        if points.is_empty() {
            return;
        }
        if closed && self.settings.shape_filled && points.len() >= 3 {
            let (w, h) = (self.target.width(), self.target.height());
            let mask = Mask::from_polygon(w, h, points);
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    if mask.get(x, y) {
                        self.dot(x, y);
                    }
                }
            }
            return;
        }
        for pair in points.windows(2) {
            self.line(pair[0], pair[1]);
        }
        if points.len() == 1 {
            self.stamp(points[0].0, points[0].1);
        }
        if closed && points.len() > 2 {
            self.line(points[points.len() - 1], points[0]);
        }
    }

    /// Flood fill from a pixel, or replace the colour everywhere when not contiguous.
    /// `sample` decides which pixels belong to the region (the layer itself or all layers).
    pub fn fill(&mut self, x: i32, y: i32, sample: &Bitmap) {
        let region = region_mask(sample, x, y, self.settings);
        let Some(region) = region else { return };
        for py in 0..sample.height() as i32 {
            for px in 0..sample.width() as i32 {
                if region.get(px, py) {
                    self.put_color(px, py, None);
                }
            }
        }
    }
}

/// Whether two colours are within `tolerance` on every channel. Fully transparent pixels
/// match each other whatever their colour channels say.
pub fn colors_match(a: Rgba, b: Rgba, tolerance: u8) -> bool {
    if a[3] == 0 && b[3] == 0 {
        return true;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| x.abs_diff(*y) <= tolerance)
}

/// The pixels a fill or magic-wand click at `(x, y)` covers.
pub fn region_mask(sample: &Bitmap, x: i32, y: i32, settings: &ToolSettings) -> Option<Mask> {
    let target = sample.get(x, y)?;
    let (w, h) = (sample.width(), sample.height());
    let tol = settings.tolerance;
    if !settings.fill_contiguous {
        return Some(Mask::from_fn(w, h, |px, py| {
            sample
                .get(px, py)
                .is_some_and(|c| colors_match(c, target, tol))
        }));
    }
    let mut mask = Mask::new(w, h);
    let mut stack = vec![(x, y)];
    while let Some((px, py)) = stack.pop() {
        if mask.get(px, py)
            || !sample
                .get(px, py)
                .is_some_and(|c| colors_match(c, target, tol))
        {
            continue;
        }
        mask.set(px, py, true);
        stack.extend([(px + 1, py), (px - 1, py), (px, py + 1), (px, py - 1)]);
        if settings.diagonal {
            stack.extend([
                (px + 1, py + 1),
                (px - 1, py - 1),
                (px + 1, py - 1),
                (px - 1, py + 1),
            ]);
        }
    }
    Some(mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_brush_is_round() {
        let s = ToolSettings {
            size: 5,
            brush: BrushShape::Circle,
            ..Default::default()
        };
        let offsets = s.brush_offsets();
        assert!(
            offsets.len() < 25 && offsets.len() >= 13,
            "{}",
            offsets.len()
        );
        assert!(!offsets.iter().any(|(x, y, _)| (*x, *y) == (-2, -2)));
    }

    #[test]
    fn tolerance_matching() {
        assert!(colors_match([10, 10, 10, 255], [12, 9, 10, 255], 2));
        assert!(!colors_match([10, 10, 10, 255], [13, 9, 10, 255], 2));
        assert!(colors_match([1, 2, 3, 0], [9, 9, 9, 0], 0));
    }

    #[test]
    fn radial_symmetry_makes_copies() {
        let mut bitmap = Bitmap::new(8, 8);
        let base = bitmap.clone();
        let settings = ToolSettings {
            symmetry: Symmetry {
                radial: 4,
                ..Default::default()
            },
            ..Default::default()
        };
        let p = Painter {
            target: &mut bitmap,
            base: &base,
            clip: None,
            alpha_lock: false,
            settings: &settings,
            color: [255; 4],
            mode: PaintMode::Replace,
        };
        assert_eq!(p.symmetric(0, 0).len(), 4);
        assert!(p.symmetric(0, 0).contains(&(7, 7)));
    }
}

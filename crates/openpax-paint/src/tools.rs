use crate::{Bitmap, Rect, Rgba, TRANSPARENT};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Pencil,
    Eraser,
    Fill,
    Picker,
    Line,
    Rectangle,
    Ellipse,
    Select,
    Move,
}

impl ToolKind {
    pub const ALL: [ToolKind; 9] = [
        Self::Pencil,
        Self::Eraser,
        Self::Fill,
        Self::Picker,
        Self::Line,
        Self::Rectangle,
        Self::Ellipse,
        Self::Select,
        Self::Move,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Pencil => "Pencil",
            Self::Eraser => "Eraser",
            Self::Fill => "Fill",
            Self::Picker => "Picker",
            Self::Line => "Line",
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Select => "Select",
            Self::Move => "Move",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolSettings {
    pub color: Rgba,
    /// Side of the square brush in pixels.
    pub size: u32,
    pub mirror_x: bool,
    pub mirror_y: bool,
    /// Fill only the connected region (otherwise every pixel of the clicked colour).
    pub fill_contiguous: bool,
    /// Draw rectangles and ellipses filled instead of outlined.
    pub shape_filled: bool,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 255],
            size: 1,
            mirror_x: false,
            mirror_y: false,
            fill_contiguous: true,
            shape_filled: false,
        }
    }
}

/// Writes pixels into a bitmap while honouring a selection clip, brush size and mirroring.
pub(crate) struct Painter<'a> {
    pub target: &'a mut Bitmap,
    pub clip: Option<Rect>,
    pub settings: ToolSettings,
    pub color: Rgba,
}

impl Painter<'_> {
    fn put(&mut self, x: i32, y: i32) {
        if self.clip.is_none_or(|c| c.contains(x, y)) {
            self.target.set(x, y, self.color);
        }
    }

    /// One pixel plus its mirror images.
    fn dot(&mut self, x: i32, y: i32) {
        let mx = self.target.width() as i32 - 1 - x;
        let my = self.target.height() as i32 - 1 - y;
        self.put(x, y);
        if self.settings.mirror_x {
            self.put(mx, y);
        }
        if self.settings.mirror_y {
            self.put(x, my);
        }
        if self.settings.mirror_x && self.settings.mirror_y {
            self.put(mx, my);
        }
    }

    /// A square brush stamp centred on the pixel.
    pub fn stamp(&mut self, x: i32, y: i32) {
        let size = self.settings.size.max(1) as i32;
        let (x0, y0) = (x - size / 2, y - size / 2);
        for by in y0..y0 + size {
            for bx in x0..x0 + size {
                self.dot(bx, by);
            }
        }
    }

    /// Bresenham line of brush stamps, both ends included.
    pub fn line(&mut self, from: (i32, i32), to: (i32, i32)) {
        let (mut x, mut y) = from;
        let dx = (to.0 - x).abs();
        let dy = -(to.1 - y).abs();
        let sx = if x < to.0 { 1 } else { -1 };
        let sy = if y < to.1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.stamp(x, y);
            if (x, y) == to {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
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

    /// Flood fill from a pixel, or replace the colour everywhere when not contiguous.
    pub fn fill(&mut self, x: i32, y: i32) {
        let Some(target_color) = self.target.get(x, y) else {
            return;
        };
        if target_color == self.color || !self.clip.is_none_or(|c| c.contains(x, y)) {
            return;
        }
        if self.settings.fill_contiguous {
            let mut stack = vec![(x, y)];
            while let Some((px, py)) = stack.pop() {
                if self.target.get(px, py) != Some(target_color)
                    || !self.clip.is_none_or(|c| c.contains(px, py))
                {
                    continue;
                }
                self.target.set(px, py, self.color);
                stack.extend([(px + 1, py), (px - 1, py), (px, py + 1), (px, py - 1)]);
            }
        } else {
            for py in 0..self.target.height() as i32 {
                for px in 0..self.target.width() as i32 {
                    if self.target.get(px, py) == Some(target_color) {
                        self.put(px, py);
                    }
                }
            }
        }
    }
}

/// Moves the pixels of `region` in `base` by `(dx, dy)`, writing the result into `target`.
///
/// `target` must start out equal to `base`. Transparent source pixels do not overwrite.
pub(crate) fn move_region(target: &mut Bitmap, base: &Bitmap, region: Rect, dx: i32, dy: i32) {
    for y in region.y..region.y + region.h {
        for x in region.x..region.x + region.w {
            target.set(x, y, TRANSPARENT);
        }
    }
    for y in region.y..region.y + region.h {
        for x in region.x..region.x + region.w {
            match base.get(x, y) {
                Some(color) if color[3] != 0 => target.set(x + dx, y + dy, color),
                _ => {}
            }
        }
    }
}

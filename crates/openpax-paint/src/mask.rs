//! Per-pixel selection masks.

use crate::{Bitmap, Rect};

/// A per-pixel selection the size of the document. `true` means selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    width: u32,
    height: u32,
    bits: Vec<bool>,
}

/// How a new selection combines with the existing one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SelectMode {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

impl Mask {
    /// An empty mask.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            bits: vec![false; width as usize * height as usize],
        }
    }

    pub fn full(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            bits: vec![true; width as usize * height as usize],
        }
    }

    pub fn from_rect(width: u32, height: u32, rect: Rect) -> Self {
        let mut mask = Self::new(width, height);
        if let Some(r) = rect.intersect(&mask.bounds_rect()) {
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    mask.set(x, y, true);
                }
            }
        }
        mask
    }

    /// Pixels whose centre lies inside the polygon (even-odd rule), plus the outline itself.
    pub fn from_polygon(width: u32, height: u32, points: &[(i32, i32)]) -> Self {
        let mut mask = Self::new(width, height);
        if points.is_empty() {
            return mask;
        }
        for y in 0..height as i32 {
            let cy = f64::from(y) + 0.5;
            let mut xs: Vec<f64> = Vec::new();
            for i in 0..points.len() {
                let (ax, ay) = points[i];
                let (bx, by) = points[(i + 1) % points.len()];
                let (ax, ay, bx, by) = (
                    f64::from(ax) + 0.5,
                    f64::from(ay) + 0.5,
                    f64::from(bx) + 0.5,
                    f64::from(by) + 0.5,
                );
                if (ay <= cy && by > cy) || (by <= cy && ay > cy) {
                    xs.push(ax + (cy - ay) / (by - ay) * (bx - ax));
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks(2) {
                if let [a, b] = pair {
                    let x0 = (a - 0.5).ceil() as i32;
                    let x1 = (b - 0.5).floor() as i32;
                    for x in x0..=x1 {
                        mask.set(x, y, true);
                    }
                }
            }
        }
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            for (x, y) in crate::raster::line_points(a, b) {
                mask.set(x, y, true);
            }
        }
        mask
    }

    /// Selects pixels of `image` for which `select` returns true.
    pub fn from_fn(width: u32, height: u32, mut select: impl FnMut(i32, i32) -> bool) -> Self {
        let mut mask = Self::new(width, height);
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                if select(x, y) {
                    mask.set(x, y, true);
                }
            }
        }
        mask
    }

    /// The opaque (alpha > 0) pixels of a bitmap.
    pub fn from_alpha(bitmap: &Bitmap) -> Self {
        Self::from_fn(bitmap.width(), bitmap.height(), |x, y| {
            bitmap.get(x, y).is_some_and(|c| c[3] > 0)
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    fn bounds_rect(&self) -> Rect {
        Rect::new(0, 0, self.width as i32, self.height as i32)
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height)
            .then(|| y as usize * self.width as usize + x as usize)
    }

    pub fn get(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_some_and(|i| self.bits[i])
    }

    pub fn set(&mut self, x: i32, y: i32, on: bool) {
        if let Some(i) = self.index(x, y) {
            self.bits[i] = on;
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.bits.iter().any(|b| *b)
    }

    pub fn count(&self) -> usize {
        self.bits.iter().filter(|b| **b).count()
    }

    /// The smallest rectangle containing every selected pixel.
    pub fn bounds(&self) -> Option<Rect> {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (i, on) in self.bits.iter().enumerate() {
            if *on {
                let x = (i % self.width as usize) as i32;
                let y = (i / self.width as usize) as i32;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        (x1 >= x0).then(|| Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1))
    }

    /// Whether the mask is exactly its bounding rectangle.
    pub fn is_rect(&self) -> bool {
        self.bounds()
            .is_some_and(|b| self.count() == (b.w * b.h) as usize)
    }

    pub fn union(&mut self, other: &Mask) {
        for (a, b) in self.bits.iter_mut().zip(&other.bits) {
            *a |= *b;
        }
    }

    pub fn subtract(&mut self, other: &Mask) {
        for (a, b) in self.bits.iter_mut().zip(&other.bits) {
            *a &= !*b;
        }
    }

    pub fn intersect(&mut self, other: &Mask) {
        for (a, b) in self.bits.iter_mut().zip(&other.bits) {
            *a &= *b;
        }
    }

    pub fn invert(&mut self) {
        for a in &mut self.bits {
            *a = !*a;
        }
    }

    /// Combines `new` into an optional existing selection. An empty result means no selection.
    pub fn combine(existing: Option<&Mask>, new: Mask, mode: SelectMode) -> Option<Mask> {
        let result = match (mode, existing) {
            (SelectMode::Subtract, None) => return None,
            (SelectMode::Replace, _) | (_, None) => new,
            (SelectMode::Add, Some(old)) => {
                let mut m = old.clone();
                m.union(&new);
                m
            }
            (SelectMode::Subtract, Some(old)) => {
                let mut m = old.clone();
                m.subtract(&new);
                m
            }
            (SelectMode::Intersect, Some(old)) => {
                let mut m = old.clone();
                m.intersect(&new);
                m
            }
        };
        (!result.is_empty()).then_some(result)
    }

    /// A copy shifted by `(dx, dy)`; pixels moved off the canvas are dropped.
    pub fn translated(&self, dx: i32, dy: i32) -> Mask {
        let mut out = Mask::new(self.width, self.height);
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                if self.get(x, y) {
                    out.set(x + dx, y + dy, true);
                }
            }
        }
        out
    }

    /// A copy with a new canvas size; `(dx, dy)` is where the old top-left corner lands.
    pub fn resized(&self, width: u32, height: u32, dx: i32, dy: i32) -> Mask {
        let mut out = Mask::new(width, height);
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                if self.get(x, y) {
                    out.set(x + dx, y + dy, true);
                }
            }
        }
        out
    }

    /// Edges between selected and unselected pixels, as unit segments in pixel-corner
    /// coordinates, for drawing marching ants.
    pub fn edges(&self) -> Vec<((i32, i32), (i32, i32))> {
        let mut out = Vec::new();
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                if !self.get(x, y) {
                    continue;
                }
                if !self.get(x, y - 1) {
                    out.push(((x, y), (x + 1, y)));
                }
                if !self.get(x, y + 1) {
                    out.push(((x, y + 1), (x + 1, y + 1)));
                }
                if !self.get(x - 1, y) {
                    out.push(((x, y), (x, y + 1)));
                }
                if !self.get(x + 1, y) {
                    out.push(((x + 1, y), (x + 1, y + 1)));
                }
            }
        }
        out
    }

    /// Raw bits row by row, for serialisation.
    pub fn bits(&self) -> &[bool] {
        &self.bits
    }

    pub fn from_bits(width: u32, height: u32, bits: Vec<bool>) -> Option<Self> {
        (bits.len() == width as usize * height as usize).then_some(Self {
            width,
            height,
            bits,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_operations() {
        let a = Mask::from_rect(4, 4, Rect::new(0, 0, 2, 2));
        let b = Mask::from_rect(4, 4, Rect::new(1, 1, 2, 2));
        let add = Mask::combine(Some(&a), b.clone(), SelectMode::Add).unwrap();
        assert_eq!(add.count(), 7);
        let sub = Mask::combine(Some(&a), b.clone(), SelectMode::Subtract).unwrap();
        assert_eq!(sub.count(), 3);
        assert!(!sub.get(1, 1));
        let int = Mask::combine(Some(&a), b.clone(), SelectMode::Intersect).unwrap();
        assert_eq!(int.bounds(), Some(Rect::new(1, 1, 1, 1)));
        assert_eq!(Mask::combine(None, b.clone(), SelectMode::Subtract), None);
        let mut inv = a.clone();
        inv.invert();
        assert_eq!(inv.count(), 12);
        assert!(a.is_rect() && !add.is_rect());
    }

    #[test]
    fn polygon_fills_a_triangle() {
        let m = Mask::from_polygon(5, 5, &[(0, 0), (4, 0), (0, 4)]);
        assert!(m.get(0, 0) && m.get(4, 0) && m.get(0, 4) && m.get(1, 1));
        assert!(!m.get(4, 4) && !m.get(3, 3));
    }

    #[test]
    fn edges_of_a_single_pixel() {
        let m = Mask::from_rect(3, 3, Rect::new(1, 1, 1, 1));
        assert_eq!(m.edges().len(), 4);
    }
}

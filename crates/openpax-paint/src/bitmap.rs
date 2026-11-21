use crate::blend::{BlendMode, blend_pixel};

/// Straight (non-premultiplied) RGBA.
pub type Rgba = [u8; 4];

pub const TRANSPARENT: Rgba = [0, 0, 0, 0];

/// An axis-aligned pixel rectangle. `w` and `h` are never negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            x,
            y,
            w: w.max(0),
            h: h.max(0),
        }
    }

    /// The smallest rectangle containing both pixels (inclusive).
    pub fn from_points(a: (i32, i32), b: (i32, i32)) -> Self {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        Self {
            x: x0,
            y: y0,
            w: x1 - x0 + 1,
            h: y1 - y0 + 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w).min(other.x + other.w);
        let y1 = (self.y + self.h).min(other.y + other.h);
        (x1 > x0 && y1 > y0).then(|| Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    }

    pub fn translated(&self, dx: i32, dy: i32) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            ..*self
        }
    }
}

/// A rectangular grid of RGBA pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Bitmap {
    /// A fully transparent bitmap.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; width as usize * height as usize * 4],
        }
    }

    pub fn filled(width: u32, height: u32, color: Rgba) -> Self {
        let mut bitmap = Self::new(width, height);
        for px in bitmap.data.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&color);
        }
        bitmap
    }

    /// Wraps raw RGBA bytes; `None` if the length does not match the size.
    pub fn from_rgba(width: u32, height: u32, data: Vec<u8>) -> Option<Self> {
        (data.len() == width as usize * height as usize * 4).then_some(Self {
            width,
            height,
            data,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bounds(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: self.width as i32,
            h: self.height as i32,
        }
    }

    /// Raw RGBA bytes, row by row.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height)
            .then(|| (y as usize * self.width as usize + x as usize) * 4)
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Rgba> {
        self.offset(x, y).map(|o| {
            [
                self.data[o],
                self.data[o + 1],
                self.data[o + 2],
                self.data[o + 3],
            ]
        })
    }

    /// Sets a pixel; out-of-bounds writes are ignored.
    pub fn set(&mut self, x: i32, y: i32, color: Rgba) {
        if let Some(o) = self.offset(x, y) {
            self.data[o..o + 4].copy_from_slice(&color);
        }
    }

    /// Copies out the pixels of `rect` (which must lie within the bitmap) as raw RGBA.
    pub fn patch(&self, rect: Rect) -> Vec<u8> {
        let mut out = Vec::with_capacity((rect.w * rect.h * 4) as usize);
        for y in rect.y..rect.y + rect.h {
            let start = self.offset(rect.x, y).expect("patch rect inside bitmap");
            out.extend_from_slice(&self.data[start..start + rect.w as usize * 4]);
        }
        out
    }

    /// Writes back pixels produced by [`Bitmap::patch`].
    pub fn apply_patch(&mut self, rect: Rect, patch: &[u8]) {
        let row = rect.w as usize * 4;
        for (i, y) in (rect.y..rect.y + rect.h).enumerate() {
            let start = self.offset(rect.x, y).expect("patch rect inside bitmap");
            self.data[start..start + row].copy_from_slice(&patch[i * row..(i + 1) * row]);
        }
    }

    /// The bounding box of pixels that differ from `other` (same size required).
    pub fn diff_bounds(&self, other: &Bitmap) -> Option<Rect> {
        assert_eq!((self.width, self.height), (other.width, other.height));
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (i, (a, b)) in self
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .zip(other.data.as_chunks::<4>().0)
            .enumerate()
        {
            if a != b {
                let x = (i % self.width as usize) as i32;
                let y = (i / self.width as usize) as i32;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        (x1 >= x0).then(|| Rect {
            x: x0,
            y: y0,
            w: x1 - x0 + 1,
            h: y1 - y0 + 1,
        })
    }

    /// A copy with a new size, keeping the top-left corner in place.
    pub fn resized(&self, width: u32, height: u32) -> Bitmap {
        let mut out = Bitmap::new(width, height);
        for y in 0..self.height.min(height) as i32 {
            for x in 0..self.width.min(width) as i32 {
                out.set(x, y, self.get(x, y).unwrap_or(TRANSPARENT));
            }
        }
        out
    }

    pub fn flipped_horizontal(&self) -> Bitmap {
        let mut out = Bitmap::new(self.width, self.height);
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                out.set(
                    self.width as i32 - 1 - x,
                    y,
                    self.get(x, y).unwrap_or(TRANSPARENT),
                );
            }
        }
        out
    }

    pub fn flipped_vertical(&self) -> Bitmap {
        let mut out = Bitmap::new(self.width, self.height);
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                out.set(
                    x,
                    self.height as i32 - 1 - y,
                    self.get(x, y).unwrap_or(TRANSPARENT),
                );
            }
        }
        out
    }

    /// Composites `src` over `self` (source-over, straight alpha) with an extra opacity factor.
    pub fn composite(&mut self, src: &Bitmap, opacity: f32) {
        self.composite_blend(src, BlendMode::Normal, opacity, None);
    }

    /// Composites `src` over `self` with a blend mode. When `clip` is given, the source's
    /// alpha is multiplied by the clip bitmap's alpha (clipping masks).
    pub fn composite_blend(
        &mut self,
        src: &Bitmap,
        mode: BlendMode,
        opacity: f32,
        clip: Option<&Bitmap>,
    ) {
        assert_eq!((self.width, self.height), (src.width, src.height));
        let clip = clip.filter(|c| (c.width, c.height) == (src.width, src.height));
        for (i, (d, s)) in self
            .data
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(src.data.as_chunks::<4>().0)
            .enumerate()
        {
            if s[3] == 0 {
                continue;
            }
            let mut s = *s;
            if let Some(clip) = clip {
                let ca = u16::from(clip.data[i * 4 + 3]);
                s[3] = ((u16::from(s[3]) * ca + 127) / 255) as u8;
                if s[3] == 0 {
                    continue;
                }
            }
            *d = blend_pixel(*d, s, mode, opacity);
        }
    }

    /// Pastes `src` with its top-left corner at `(x, y)`, overwriting (including transparent
    /// pixels). Parts outside are cut off.
    pub fn blit(&mut self, src: &Bitmap, x: i32, y: i32) {
        for sy in 0..src.height as i32 {
            for sx in 0..src.width as i32 {
                if let Some(c) = src.get(sx, sy) {
                    self.set(x + sx, y + sy, c);
                }
            }
        }
    }

    /// Draws `src` over this bitmap at `(x, y)` (source-over).
    pub fn draw(&mut self, src: &Bitmap, x: i32, y: i32) {
        for sy in 0..src.height as i32 {
            for sx in 0..src.width as i32 {
                if let (Some(c), Some(d)) = (src.get(sx, sy), self.get(x + sx, y + sy)) {
                    self.set(x + sx, y + sy, blend_pixel(d, c, BlendMode::Normal, 1.0));
                }
            }
        }
    }

    /// The pixels inside `rect` (parts outside the bitmap come out transparent).
    pub fn crop(&self, rect: Rect) -> Bitmap {
        let mut out = Bitmap::new(rect.w.max(0) as u32, rect.h.max(0) as u32);
        for y in 0..rect.h {
            for x in 0..rect.w {
                if let Some(c) = self.get(rect.x + x, rect.y + y) {
                    out.set(x, y, c);
                }
            }
        }
        out
    }

    /// A copy with a new canvas size; the old top-left corner lands at `(dx, dy)`.
    pub fn resized_offset(&self, width: u32, height: u32, dx: i32, dy: i32) -> Bitmap {
        let mut out = Bitmap::new(width, height);
        out.blit(self, dx, dy);
        out
    }

    /// Rotated a quarter turn clockwise (or counter-clockwise).
    pub fn rotated_90(&self, clockwise: bool) -> Bitmap {
        let (w, h) = (self.width as i32, self.height as i32);
        let mut out = Bitmap::new(self.height, self.width);
        for y in 0..h {
            for x in 0..w {
                let c = self.get(x, y).unwrap_or(TRANSPARENT);
                if clockwise {
                    out.set(h - 1 - y, x, c);
                } else {
                    out.set(y, w - 1 - x, c);
                }
            }
        }
        out
    }

    pub fn rotated_180(&self) -> Bitmap {
        self.flipped_horizontal().flipped_vertical()
    }

    /// The bounding box of non-transparent pixels.
    pub fn opaque_bounds(&self) -> Option<Rect> {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                if self.get(x, y).is_some_and(|c| c[3] > 0) {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        (x1 >= x0).then(|| Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1))
    }

    /// Whether every pixel is fully transparent.
    pub fn is_clear(&self) -> bool {
        self.data.as_chunks::<4>().0.iter().all(|p| p[3] == 0)
    }

    /// Applies `f` to every pixel.
    pub fn map_pixels(&mut self, mut f: impl FnMut(i32, i32, Rgba) -> Rgba) {
        let w = self.width as usize;
        for (i, px) in self.data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *px = f((i % w) as i32, (i / w) as i32, *px);
        }
    }

    /// Every pixel as `[r, g, b, a]`, row by row.
    pub fn pixels(&self) -> impl Iterator<Item = Rgba> + '_ {
        self.data.as_chunks::<4>().0.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_from_points_is_inclusive_and_order_independent() {
        let r = Rect::from_points((5, 2), (1, 4));
        assert_eq!(
            r,
            Rect {
                x: 1,
                y: 2,
                w: 5,
                h: 3
            }
        );
        assert!(r.contains(5, 4) && !r.contains(6, 4));
        assert_eq!(
            r.intersect(&Rect::new(4, 0, 10, 3)),
            Some(Rect::new(4, 2, 2, 1))
        );
        assert_eq!(r.intersect(&Rect::new(6, 0, 2, 2)), None);
    }

    #[test]
    fn patch_round_trip_and_diff() {
        let mut a = Bitmap::new(4, 4);
        let b = a.clone();
        a.set(1, 1, [1, 2, 3, 4]);
        a.set(2, 3, [5, 6, 7, 8]);
        let rect = a.diff_bounds(&b).unwrap();
        assert_eq!(rect, Rect::new(1, 1, 2, 3));
        let mut c = b.clone();
        c.apply_patch(rect, &a.patch(rect));
        assert_eq!(c, a);
        assert_eq!(a.diff_bounds(&c), None);
    }

    #[test]
    fn out_of_bounds_access_is_safe() {
        let mut a = Bitmap::new(2, 2);
        a.set(-1, 0, [9; 4]);
        a.set(2, 0, [9; 4]);
        assert_eq!(a, Bitmap::new(2, 2));
        assert_eq!(a.get(0, 2), None);
    }

    #[test]
    fn composite_blends_half_opacity() {
        let mut dst = Bitmap::filled(1, 1, [0, 0, 0, 255]);
        dst.composite(&Bitmap::filled(1, 1, [255, 255, 255, 255]), 0.5);
        assert_eq!(dst.get(0, 0), Some([128, 128, 128, 255]));
    }

    #[test]
    fn composite_onto_transparent_copies_exactly() {
        let mut dst = Bitmap::new(1, 1);
        dst.composite(&Bitmap::filled(1, 1, [10, 20, 30, 77]), 1.0);
        assert_eq!(dst.get(0, 0), Some([10, 20, 30, 77]));
    }
}

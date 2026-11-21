//! Pixel-art friendly scaling and rotation.

use serde::{Deserialize, Serialize};

use crate::{Bitmap, Rgba, TRANSPARENT};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScaleMethod {
    /// Nearest neighbour: every pixel becomes a block.
    #[default]
    Nearest,
    /// Scale2x/EPX edge smoothing at whole-number factors (2x, 3x, 4x, 8x...), nearest
    /// neighbour for the rest.
    Smooth,
    /// RotSprite-style: Scale2x three times, then nearest neighbour to the target.
    RotSprite,
}

impl ScaleMethod {
    pub const ALL: [ScaleMethod; 3] = [Self::Nearest, Self::Smooth, Self::RotSprite];

    pub fn name(self) -> &'static str {
        match self {
            Self::Nearest => "Nearest neighbour",
            Self::Smooth => "Smooth edges (Scale2x)",
            Self::RotSprite => "RotSprite",
        }
    }
}

/// Nearest-neighbour resize to an arbitrary size.
pub fn nearest(src: &Bitmap, width: u32, height: u32) -> Bitmap {
    let mut out = Bitmap::new(width, height);
    if src.width() == 0 || src.height() == 0 {
        return out;
    }
    for y in 0..height {
        let sy = (u64::from(y) * u64::from(src.height()) / u64::from(height.max(1))) as i32;
        for x in 0..width {
            let sx = (u64::from(x) * u64::from(src.width()) / u64::from(width.max(1))) as i32;
            out.set(x as i32, y as i32, src.get(sx, sy).unwrap_or(TRANSPARENT));
        }
    }
    out
}

/// The Scale2x (EPX) algorithm: doubles the size, smoothing diagonal edges.
pub fn scale2x(src: &Bitmap) -> Bitmap {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let mut out = Bitmap::new(src.width() * 2, src.height() * 2);
    let at = |x: i32, y: i32| {
        src.get(x.clamp(0, w - 1), y.clamp(0, h - 1))
            .unwrap_or(TRANSPARENT)
    };
    for y in 0..h {
        for x in 0..w {
            let p = at(x, y);
            let (a, b, c, d) = (at(x, y - 1), at(x + 1, y), at(x - 1, y), at(x, y + 1));
            let mut e = [p; 4];
            if c == a && c != d && a != b {
                e[0] = a;
            }
            if a == b && a != c && b != d {
                e[1] = b;
            }
            if d == c && d != b && c != a {
                e[2] = c;
            }
            if b == d && b != a && d != c {
                e[3] = d;
            }
            out.set(x * 2, y * 2, e[0]);
            out.set(x * 2 + 1, y * 2, e[1]);
            out.set(x * 2, y * 2 + 1, e[2]);
            out.set(x * 2 + 1, y * 2 + 1, e[3]);
        }
    }
    out
}

/// The Scale3x algorithm: triples the size, smoothing diagonal edges.
pub fn scale3x(src: &Bitmap) -> Bitmap {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let mut out = Bitmap::new(src.width() * 3, src.height() * 3);
    let at = |x: i32, y: i32| {
        src.get(x.clamp(0, w - 1), y.clamp(0, h - 1))
            .unwrap_or(TRANSPARENT)
    };
    for y in 0..h {
        for x in 0..w {
            let (a, b, c) = (at(x - 1, y - 1), at(x, y - 1), at(x + 1, y - 1));
            let (d, e, f) = (at(x - 1, y), at(x, y), at(x + 1, y));
            let (g, hh, i) = (at(x - 1, y + 1), at(x, y + 1), at(x + 1, y + 1));
            let mut o = [e; 9];
            if b != hh && d != f {
                o[0] = if d == b { d } else { e };
                o[1] = if (d == b && e != c) || (b == f && e != a) {
                    b
                } else {
                    e
                };
                o[2] = if b == f { f } else { e };
                o[3] = if (d == b && e != g) || (d == hh && e != a) {
                    d
                } else {
                    e
                };
                o[4] = e;
                o[5] = if (b == f && e != i) || (hh == f && e != c) {
                    f
                } else {
                    e
                };
                o[6] = if d == hh { d } else { e };
                o[7] = if (d == hh && e != i) || (hh == f && e != g) {
                    hh
                } else {
                    e
                };
                o[8] = if hh == f { f } else { e };
            }
            for (k, color) in o.iter().enumerate() {
                out.set(x * 3 + (k % 3) as i32, y * 3 + (k / 3) as i32, *color);
            }
        }
    }
    out
}

/// Resizes with the chosen method.
pub fn scale(src: &Bitmap, width: u32, height: u32, method: ScaleMethod) -> Bitmap {
    if width == 0 || height == 0 {
        return Bitmap::new(width, height);
    }
    match method {
        ScaleMethod::Nearest => nearest(src, width, height),
        ScaleMethod::Smooth => {
            let mut img = src.clone();
            // Grow with Scale2x/3x while that does not overshoot, then finish with nearest.
            loop {
                let (w, h) = (img.width(), img.height());
                if w * 3 <= width
                    && h * 3 <= height
                    && width.is_multiple_of(w * 3)
                    && height.is_multiple_of(h * 3)
                {
                    img = scale3x(&img);
                } else if w * 2 <= width && h * 2 <= height {
                    img = scale2x(&img);
                } else {
                    break;
                }
            }
            nearest(&img, width, height)
        }
        ScaleMethod::RotSprite => {
            let big = scale2x(&scale2x(&scale2x(src)));
            nearest(&big, width, height)
        }
    }
}

/// Rotates by an arbitrary angle (degrees, clockwise) around the centre, keeping the
/// canvas size. `smooth` uses the RotSprite approach (upscale 8x with Scale2x, rotate with
/// nearest neighbour, sample back down) so edges stay crisp but less jagged.
pub fn rotate(src: &Bitmap, degrees: f32, smooth: bool) -> Bitmap {
    let (w, h) = (src.width(), src.height());
    let factor = if smooth { 8 } else { 1 };
    let big = if smooth {
        scale2x(&scale2x(&scale2x(src)))
    } else {
        src.clone()
    };
    let (rad_sin, rad_cos) = degrees.to_radians().sin_cos();
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut out = Bitmap::new(w, h);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            // Inverse-rotate the destination pixel centre into the source.
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let sx = dx * rad_cos + dy * rad_sin + cx;
            let sy = -dx * rad_sin + dy * rad_cos + cy;
            let (bx, by) = (
                (sx * factor as f32).floor() as i32,
                (sy * factor as f32).floor() as i32,
            );
            // Sample the middle of the corresponding 8x8 block.
            let c: Rgba = big.get(bx, by).unwrap_or(TRANSPARENT);
            out.set(x, y, c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_doubles_exactly() {
        let mut src = Bitmap::new(2, 1);
        src.set(1, 0, [255, 0, 0, 255]);
        let out = nearest(&src, 4, 2);
        assert_eq!(out.get(2, 1), Some([255, 0, 0, 255]));
        assert_eq!(out.get(1, 1), Some(TRANSPARENT));
        assert_eq!(nearest(&out, 2, 1), src);
    }

    #[test]
    fn scale2x_smooths_a_diagonal() {
        let mut src = Bitmap::new(2, 2);
        let r = [255, 0, 0, 255];
        src.set(0, 0, r);
        src.set(1, 1, r);
        let out = scale2x(&src);
        assert_eq!((out.width(), out.height()), (4, 4));
        // The empty pixel between both red pixels gets its inner corner filled in.
        assert_eq!(out.get(2, 1), Some(r));
        assert_eq!(out.get(3, 0), Some(TRANSPARENT));
        assert_eq!(scale(&src, 6, 6, ScaleMethod::Smooth).width(), 6);
    }

    #[test]
    fn rotating_a_full_turn_is_identity() {
        let mut src = Bitmap::new(4, 4);
        src.set(1, 0, [1, 2, 3, 255]);
        assert_eq!(rotate(&src, 360.0, false), src);
        assert_eq!(rotate(&src, 90.0, false), src.rotated_90(true));
    }
}

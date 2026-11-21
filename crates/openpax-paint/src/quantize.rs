//! Colour reduction and dithering.

use serde::{Deserialize, Serialize};

use crate::color::nearest_index;
use crate::{Bitmap, Rgba};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dither {
    #[default]
    None,
    Bayer2,
    Bayer4,
    Bayer8,
    FloydSteinberg,
    Atkinson,
}

impl Dither {
    pub const ALL: [Dither; 6] = [
        Self::None,
        Self::Bayer2,
        Self::Bayer4,
        Self::Bayer8,
        Self::FloydSteinberg,
        Self::Atkinson,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Bayer2 => "Ordered (Bayer 2×2)",
            Self::Bayer4 => "Ordered (Bayer 4×4)",
            Self::Bayer8 => "Ordered (Bayer 8×8)",
            Self::FloydSteinberg => "Floyd–Steinberg",
            Self::Atkinson => "Atkinson",
        }
    }
}

/// The threshold of a Bayer matrix of size `n` (2, 4 or 8) at a pixel, in 0..1.
pub fn bayer(n: u32, x: i32, y: i32) -> f32 {
    fn value(n: u32, x: u32, y: u32) -> u32 {
        if n == 1 {
            return 0;
        }
        let half = n / 2;
        let q = match (x >= half, y >= half) {
            (false, false) => 0,
            (true, true) => 1,
            (true, false) => 2,
            (false, true) => 3,
        };
        4 * value(half, x % half, y % half) + q
    }
    let n = n.max(2);
    let v = value(
        n,
        x.rem_euclid(n as i32) as u32,
        y.rem_euclid(n as i32) as u32,
    );
    (v as f32 + 0.5) / (n * n) as f32
}

/// Picks up to `count` representative colours with the median-cut algorithm (visible
/// pixels only).
pub fn median_cut(image: &Bitmap, count: usize) -> Vec<Rgba> {
    let mut pixels: Vec<Rgba> = image.pixels().filter(|c| c[3] > 0).collect();
    if pixels.is_empty() || count == 0 {
        return Vec::new();
    }
    let mut distinct = pixels.clone();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() <= count {
        return distinct;
    }
    let mut boxes: Vec<Vec<Rgba>> = vec![std::mem::take(&mut pixels)];
    while boxes.len() < count {
        // Split the box with the widest channel range.
        let (index, channel, range) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| {
                let (c, r) = (0..4)
                    .map(|c| {
                        let min = b.iter().map(|p| p[c]).min().unwrap_or(0);
                        let max = b.iter().map(|p| p[c]).max().unwrap_or(0);
                        (c, max - min)
                    })
                    .max_by_key(|(_, r)| *r)
                    .unwrap_or((0, 0));
                (i, c, r)
            })
            .max_by_key(|(_, _, r)| *r)
            .unwrap_or((0, 0, 0));
        if range == 0 {
            break;
        }
        let mut b = boxes.swap_remove(index);
        b.sort_unstable_by_key(|p| p[channel]);
        let upper = b.split_off(b.len() / 2);
        boxes.push(b);
        boxes.push(upper);
    }
    boxes
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let n = b.len() as u64;
            let sum = b.iter().fold([0u64; 4], |mut acc, p| {
                for c in 0..4 {
                    acc[c] += u64::from(p[c]);
                }
                acc
            });
            sum.map(|s| ((s + n / 2) / n) as u8)
        })
        .collect()
}

/// Maps every visible pixel to the nearest palette colour, optionally dithered.
pub fn remap(image: &Bitmap, palette: &[Rgba], dither: Dither) -> Bitmap {
    let mut out = image.clone();
    if palette.is_empty() {
        return out;
    }
    let (w, h) = (image.width() as usize, image.height() as usize);
    match dither {
        Dither::None | Dither::Bayer2 | Dither::Bayer4 | Dither::Bayer8 => {
            let n = match dither {
                Dither::Bayer2 => 2,
                Dither::Bayer4 => 4,
                Dither::Bayer8 => 8,
                _ => 0,
            };
            // Typical spread between palette colours, for scaling the ordered pattern.
            let spread = 255.0 / (palette.len() as f32).cbrt().max(1.0);
            out.map_pixels(|x, y, c| {
                if c[3] == 0 {
                    return c;
                }
                let c = if n > 0 {
                    let t = (bayer(n, x, y) - 0.5) * spread;
                    [c[0], c[1], c[2]].map(|v| (f32::from(v) + t).clamp(0.0, 255.0) as u8)
                } else {
                    [c[0], c[1], c[2]]
                };
                let src = [c[0], c[1], c[2], 255];
                let i = nearest_index(palette, src).unwrap_or(0);
                let p = palette[i];
                [
                    p[0],
                    p[1],
                    p[2],
                    if p[3] == 255 {
                        image.get(x, y).map_or(255, |o| o[3])
                    } else {
                        p[3]
                    },
                ]
            });
        }
        Dither::FloydSteinberg | Dither::Atkinson => {
            let mut err = vec![[0f32; 3]; w * h];
            let kernel: &[(i32, i32, f32)] = if dither == Dither::FloydSteinberg {
                &[
                    (1, 0, 7.0 / 16.0),
                    (-1, 1, 3.0 / 16.0),
                    (0, 1, 5.0 / 16.0),
                    (1, 1, 1.0 / 16.0),
                ]
            } else {
                &[
                    (1, 0, 1.0 / 8.0),
                    (2, 0, 1.0 / 8.0),
                    (-1, 1, 1.0 / 8.0),
                    (0, 1, 1.0 / 8.0),
                    (1, 1, 1.0 / 8.0),
                    (0, 2, 1.0 / 8.0),
                ]
            };
            for y in 0..h {
                for x in 0..w {
                    let c = image.get(x as i32, y as i32).unwrap_or([0; 4]);
                    if c[3] == 0 {
                        continue;
                    }
                    let e = err[y * w + x];
                    let want = [0, 1, 2].map(|i| (f32::from(c[i]) + e[i]).clamp(0.0, 255.0));
                    let src = [want[0] as u8, want[1] as u8, want[2] as u8, 255];
                    let p = palette[nearest_index(palette, src).unwrap_or(0)];
                    out.set(x as i32, y as i32, [p[0], p[1], p[2], c[3]]);
                    let diff = [0, 1, 2].map(|i| want[i] - f32::from(p[i]));
                    for (dx, dy, k) in kernel {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        if nx >= 0 && (nx as usize) < w && (ny as usize) < h {
                            let cell = &mut err[ny as usize * w + nx as usize];
                            for i in 0..3 {
                                cell[i] += diff[i] * k;
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// Reduces an image to `count` colours (median cut), optionally dithered.
pub fn quantize(image: &Bitmap, count: usize, dither: Dither) -> (Bitmap, Vec<Rgba>) {
    let palette = median_cut(image, count);
    (remap(image, &palette, dither), palette)
}

/// Fills `image` with a pattern dither between two colours at a density of 0..=1, using a
/// custom tile (opaque pixels of `pattern` take `a`) or a Bayer matrix.
pub fn pattern_fill(image: &mut Bitmap, a: Rgba, b: Rgba, density: f32, pattern: Option<&Bitmap>) {
    image.map_pixels(|x, y, _| match pattern {
        Some(p) if p.width() > 0 && p.height() > 0 => {
            let c = p.get(
                x.rem_euclid(p.width() as i32),
                y.rem_euclid(p.height() as i32),
            );
            if c.is_some_and(|c| c[3] > 0) { a } else { b }
        }
        _ => {
            if bayer(4, x, y) < density {
                a
            } else {
                b
            }
        }
    });
}

/// Distinct visible colours in an image.
pub fn count_colors(image: &Bitmap) -> usize {
    let mut v: Vec<Rgba> = image.pixels().filter(|c| c[3] > 0).collect();
    v.sort_unstable();
    v.dedup();
    v.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient() -> Bitmap {
        let mut b = Bitmap::new(32, 4);
        b.map_pixels(|x, y, _| [(x * 8) as u8, (y * 60) as u8, 100, 255]);
        b
    }

    #[test]
    fn median_cut_limits_the_colour_count() {
        for n in [2, 4, 8] {
            let (img, pal) = quantize(&gradient(), n, Dither::None);
            assert!(pal.len() <= n);
            assert!(count_colors(&img) <= n);
        }
    }

    #[test]
    fn dithering_stays_within_the_palette() {
        let pal = vec![[0, 0, 0, 255], [255, 255, 255, 255]];
        for d in Dither::ALL {
            let img = remap(&gradient(), &pal, d);
            assert!(img.pixels().all(|c| pal.contains(&c)), "{d:?}");
        }
    }

    #[test]
    fn bayer_thresholds_cover_the_range() {
        let mut v: Vec<f32> = (0..4)
            .flat_map(|y| (0..4).map(move |x| bayer(4, x, y)))
            .collect();
        v.sort_by(f32::total_cmp);
        v.dedup();
        assert_eq!(v.len(), 16);
    }
}

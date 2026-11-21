//! Image filters, usable destructively or as non-destructive layer effects.

use serde::{Deserialize, Serialize};

use crate::color::{hsv_to_rgb, rgb_to_hsv};
use crate::{Bitmap, Rgba, TRANSPARENT};

/// Which side of the opaque shape an outline goes on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OutlinePlace {
    /// Around the shape, on transparent pixels.
    #[default]
    Outside,
    /// On the shape's own edge pixels.
    Inside,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Outline {
        color: Rgba,
        place: OutlinePlace,
        /// Also count diagonal neighbours (a thicker, rounder outline).
        diagonal: bool,
    },
    DropShadow {
        color: Rgba,
        dx: i32,
        dy: i32,
    },
    HueSaturation {
        /// Degrees to rotate the hue by.
        hue: f32,
        /// -1..=1, added to saturation.
        saturation: f32,
        /// -1..=1, added to value.
        value: f32,
    },
    BrightnessContrast {
        /// -1..=1.
        brightness: f32,
        /// -1..=1.
        contrast: f32,
    },
    ColorOverlay {
        color: Rgba,
        /// 0..=1.
        amount: f32,
    },
    Invert,
    Desaturate,
}

/// A layer effect that can be switched off without removing it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerEffect {
    pub enabled: bool,
    pub effect: Effect,
}

impl Effect {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Outline { .. } => "Outline",
            Self::DropShadow { .. } => "Drop Shadow",
            Self::HueSaturation { .. } => "Hue / Saturation",
            Self::BrightnessContrast { .. } => "Brightness / Contrast",
            Self::ColorOverlay { .. } => "Colour Overlay",
            Self::Invert => "Invert",
            Self::Desaturate => "Desaturate",
        }
    }

    /// Default-valued instances of every effect, for "add effect" menus.
    pub fn presets() -> Vec<Effect> {
        vec![
            Self::Outline {
                color: [0, 0, 0, 255],
                place: OutlinePlace::Outside,
                diagonal: false,
            },
            Self::DropShadow {
                color: [0, 0, 0, 128],
                dx: 1,
                dy: 1,
            },
            Self::HueSaturation {
                hue: 0.0,
                saturation: 0.0,
                value: 0.0,
            },
            Self::BrightnessContrast {
                brightness: 0.0,
                contrast: 0.0,
            },
            Self::ColorOverlay {
                color: [255, 255, 255, 255],
                amount: 0.5,
            },
            Self::Invert,
            Self::Desaturate,
        ]
    }

    /// Applies the effect to a whole bitmap.
    pub fn apply(&self, src: &Bitmap) -> Bitmap {
        match *self {
            Self::Outline {
                color,
                place,
                diagonal,
            } => outline(src, color, place, diagonal),
            Self::DropShadow { color, dx, dy } => {
                let mut out = Bitmap::new(src.width(), src.height());
                for y in 0..src.height() as i32 {
                    for x in 0..src.width() as i32 {
                        if src.get(x - dx, y - dy).is_some_and(|c| c[3] > 0) {
                            out.set(x, y, color);
                        }
                    }
                }
                out.draw(src, 0, 0);
                out
            }
            Self::HueSaturation {
                hue,
                saturation,
                value,
            } => map(src, |c| {
                let (h, s, v) = rgb_to_hsv(c);
                let rgb = hsv_to_rgb(
                    (h + hue).rem_euclid(360.0),
                    (s + saturation).clamp(0.0, 1.0),
                    (v + value).clamp(0.0, 1.0),
                );
                [rgb[0], rgb[1], rgb[2], c[3]]
            }),
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => {
                let k = if contrast >= 0.0 {
                    1.0 / (1.0 - contrast.min(0.99))
                } else {
                    (1.0 + contrast).max(0.0)
                };
                map(src, |c| {
                    let f = |v: u8| {
                        let x = f32::from(v) / 255.0;
                        let x = (x - 0.5) * k + 0.5 + brightness;
                        (x.clamp(0.0, 1.0) * 255.0).round() as u8
                    };
                    [f(c[0]), f(c[1]), f(c[2]), c[3]]
                })
            }
            Self::ColorOverlay { color, amount } => map(src, |c| {
                let t = amount.clamp(0.0, 1.0) * f32::from(color[3]) / 255.0;
                let mix = |a: u8, b: u8| {
                    (f32::from(a) * (1.0 - t) + f32::from(b) * t).round() as u8
                };
                [mix(c[0], color[0]), mix(c[1], color[1]), mix(c[2], color[2]), c[3]]
            }),
            Self::Invert => map(src, |c| [255 - c[0], 255 - c[1], 255 - c[2], c[3]]),
            Self::Desaturate => map(src, |c| {
                let l = (0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2]))
                    .round() as u8;
                [l, l, l, c[3]]
            }),
        }
    }
}

/// Applies `f` to every visible pixel.
fn map(src: &Bitmap, f: impl Fn(Rgba) -> Rgba) -> Bitmap {
    let mut out = src.clone();
    out.map_pixels(|_, _, c| if c[3] == 0 { c } else { f(c) });
    out
}

/// An outline around (or along the edge of) the opaque pixels.
pub fn outline(src: &Bitmap, color: Rgba, place: OutlinePlace, diagonal: bool) -> Bitmap {
    let mut out = src.clone();
    let opaque = |x: i32, y: i32| src.get(x, y).is_some_and(|c| c[3] > 0);
    let neighbours: &[(i32, i32)] = if diagonal {
        &[(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)]
    } else {
        &[(1, 0), (-1, 0), (0, 1), (0, -1)]
    };
    for y in 0..src.height() as i32 {
        for x in 0..src.width() as i32 {
            let here = opaque(x, y);
            let hit = match place {
                OutlinePlace::Outside => !here && neighbours.iter().any(|(dx, dy)| opaque(x + dx, y + dy)),
                OutlinePlace::Inside => here && neighbours.iter().any(|(dx, dy)| !opaque(x + dx, y + dy)),
            };
            if hit {
                out.set(x, y, color);
            }
        }
    }
    out
}

/// Applies a list of enabled effects in order.
pub fn apply_all(src: &Bitmap, effects: &[LayerEffect]) -> Bitmap {
    let mut img = src.clone();
    for e in effects.iter().filter(|e| e.enabled) {
        img = e.effect.apply(&img);
    }
    img
}

/// Replaces every pixel within `tolerance` of `from` by `to`.
pub fn replace_color(src: &mut Bitmap, from: Rgba, to: Rgba, tolerance: u8) {
    src.map_pixels(|_, _, c| {
        if crate::colors_match(c, from, tolerance) {
            to
        } else {
            c
        }
    });
}

/// Maps each colour found in `from` to the colour at the same index in `to`.
pub fn palette_swap(src: &mut Bitmap, from: &[Rgba], to: &[Rgba]) {
    src.map_pixels(|_, _, c| {
        if c[3] == 0 {
            return c;
        }
        match from.iter().position(|f| *f == c) {
            Some(i) if i < to.len() => to[i],
            _ => c,
        }
    });
}

/// Clears every pixel; used by "clear" style filters.
pub fn clear(src: &mut Bitmap) {
    src.map_pixels(|_, _, _| TRANSPARENT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_outside_and_inside() {
        let mut src = Bitmap::new(3, 3);
        src.set(1, 1, [255, 0, 0, 255]);
        let out = outline(&src, [0, 0, 0, 255], OutlinePlace::Outside, false);
        assert_eq!(out.get(1, 0), Some([0, 0, 0, 255]));
        assert_eq!(out.get(0, 0), Some(TRANSPARENT));
        assert_eq!(out.get(1, 1), Some([255, 0, 0, 255]));
        let inside = outline(&src, [0, 0, 0, 255], OutlinePlace::Inside, false);
        assert_eq!(inside.get(1, 1), Some([0, 0, 0, 255]));
    }

    #[test]
    fn swap_and_replace() {
        let mut b = Bitmap::filled(2, 1, [1, 1, 1, 255]);
        b.set(1, 0, [2, 2, 2, 255]);
        palette_swap(&mut b, &[[1, 1, 1, 255], [2, 2, 2, 255]], &[[9, 9, 9, 255], [8, 8, 8, 255]]);
        assert_eq!(b.get(0, 0), Some([9, 9, 9, 255]));
        assert_eq!(b.get(1, 0), Some([8, 8, 8, 255]));
        replace_color(&mut b, [9, 9, 9, 255], [0, 0, 0, 255], 1);
        assert_eq!(b.get(1, 0), Some([0, 0, 0, 255]), "within tolerance");
    }

    #[test]
    fn invert_keeps_alpha() {
        let b = Bitmap::filled(1, 1, [0, 10, 255, 77]);
        assert_eq!(Effect::Invert.apply(&b).get(0, 0), Some([255, 245, 0, 77]));
    }
}

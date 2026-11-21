//! Layer blend modes (W3C compositing formulas on straight alpha).

use serde::{Deserialize, Serialize};

use crate::Rgba;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Add,
    Subtract,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    pub const ALL: [BlendMode; 18] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Darken,
        Self::Lighten,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::HardLight,
        Self::SoftLight,
        Self::Difference,
        Self::Exclusion,
        Self::Add,
        Self::Subtract,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Overlay => "Overlay",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::ColorDodge => "Color Dodge",
            Self::ColorBurn => "Color Burn",
            Self::HardLight => "Hard Light",
            Self::SoftLight => "Soft Light",
            Self::Difference => "Difference",
            Self::Exclusion => "Exclusion",
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Color => "Color",
            Self::Luminosity => "Luminosity",
        }
    }

    /// The blended colour `B(backdrop, source)` with channels in 0..=1.
    fn mix(self, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
        let sep = |f: fn(f32, f32) -> f32| [f(b[0], s[0]), f(b[1], s[1]), f(b[2], s[2])];
        match self {
            Self::Normal => s,
            Self::Multiply => sep(|b, s| b * s),
            Self::Screen => sep(screen),
            Self::Overlay => sep(|b, s| hard_light(s, b)),
            Self::Darken => sep(f32::min),
            Self::Lighten => sep(f32::max),
            Self::ColorDodge => sep(|b, s| {
                if b <= 0.0 {
                    0.0
                } else if s >= 1.0 {
                    1.0
                } else {
                    (b / (1.0 - s)).min(1.0)
                }
            }),
            Self::ColorBurn => sep(|b, s| {
                if b >= 1.0 {
                    1.0
                } else if s <= 0.0 {
                    0.0
                } else {
                    1.0 - ((1.0 - b) / s).min(1.0)
                }
            }),
            Self::HardLight => sep(hard_light),
            Self::SoftLight => sep(|b, s| {
                if s <= 0.5 {
                    b - (1.0 - 2.0 * s) * b * (1.0 - b)
                } else {
                    let d = if b <= 0.25 {
                        ((16.0 * b - 12.0) * b + 4.0) * b
                    } else {
                        b.sqrt()
                    };
                    b + (2.0 * s - 1.0) * (d - b)
                }
            }),
            Self::Difference => sep(|b, s| (b - s).abs()),
            Self::Exclusion => sep(|b, s| b + s - 2.0 * b * s),
            Self::Add => sep(|b, s| (b + s).min(1.0)),
            Self::Subtract => sep(|b, s| (b - s).max(0.0)),
            Self::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
            Self::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
            Self::Color => set_lum(s, lum(b)),
            Self::Luminosity => set_lum(b, lum(s)),
        }
    }
}

fn screen(b: f32, s: f32) -> f32 {
    b + s - b * s
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b * 2.0 * s
    } else {
        screen(b, 2.0 * s - 1.0)
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        for v in &mut out {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 {
        for v in &mut out {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    out
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max - min <= 0.0 {
        return [0.0; 3];
    }
    c.map(|v| (v - min) * s / (max - min))
}

/// Composites one source pixel onto a backdrop pixel with a blend mode and opacity.
pub fn blend_pixel(dst: Rgba, src: Rgba, mode: BlendMode, opacity: f32) -> Rgba {
    let sa = f32::from(src[3]) / 255.0 * opacity.clamp(0.0, 1.0);
    if sa <= 0.0 {
        return dst;
    }
    let da = f32::from(dst[3]) / 255.0;
    if mode == BlendMode::Normal && (da == 0.0 || sa >= 1.0) && opacity >= 1.0 {
        return src;
    }
    let to_f = |c: Rgba| [c[0], c[1], c[2]].map(|v| f32::from(v) / 255.0);
    let (cb, cs) = (to_f(dst), to_f(src));
    let mixed = mode.mix(cb, cs);
    // The source colour as seen through a partly transparent backdrop.
    let cs2 = [0, 1, 2].map(|i| (1.0 - da) * cs[i] + da * mixed[i]);
    let out_a = sa + da * (1.0 - sa);
    let mut out = [0u8; 4];
    for i in 0..3 {
        let v = (cs2[i] * sa + cb[i] * da * (1.0 - sa)) / out_a;
        out[i] = (v * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    out[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREY: Rgba = [128, 128, 128, 255];

    #[test]
    fn known_values() {
        let white = [255, 255, 255, 255];
        let black = [0, 0, 0, 255];
        assert_eq!(blend_pixel(GREY, white, BlendMode::Multiply, 1.0), GREY);
        assert_eq!(blend_pixel(GREY, black, BlendMode::Screen, 1.0), GREY);
        assert_eq!(blend_pixel(GREY, GREY, BlendMode::Difference, 1.0), black);
        assert_eq!(
            blend_pixel(GREY, GREY, BlendMode::Add, 1.0),
            [255, 255, 255, 255]
        );
        assert_eq!(blend_pixel(GREY, white, BlendMode::Subtract, 1.0), black);
        assert_eq!(blend_pixel(black, white, BlendMode::Normal, 0.5)[0], 128);
    }

    #[test]
    fn blending_onto_transparent_shows_the_source() {
        let src = [10, 200, 30, 255];
        for mode in BlendMode::ALL {
            assert_eq!(blend_pixel([0, 0, 0, 0], src, mode, 1.0), src, "{mode:?}");
        }
    }
}

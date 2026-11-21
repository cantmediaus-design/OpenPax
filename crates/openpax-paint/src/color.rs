//! Colour model conversions and helpers.

use crate::Rgba;

/// RGB to HSV: hue in degrees 0..360, saturation and value 0..=1.
pub fn rgb_to_hsv(c: Rgba) -> (f32, f32, f32) {
    let [r, g, b] = [c[0], c[1], c[2]].map(|v| f32::from(v) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max <= 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r, g, b].map(|t| ((t + m).clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// RGB to HSL: hue in degrees, saturation and lightness 0..=1.
pub fn rgb_to_hsl(c: Rgba) -> (f32, f32, f32) {
    let [r, g, b] = [c[0], c[1], c[2]].map(|v| f32::from(v) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d <= 0.0 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    let (h, _, _) = rgb_to_hsv(c);
    (h, s.clamp(0.0, 1.0), l)
}

pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r, g, b].map(|t| ((t + m).clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// `#rrggbb` or `#rrggbbaa`.
pub fn to_hex(c: Rgba) -> String {
    if c[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }
}

/// Parses `rgb`, `rrggbb` or `rrggbbaa`, with or without a leading `#`.
pub fn parse_hex(text: &str) -> Option<Rgba> {
    let t = text.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(t.get(i..i + 2)?, 16).ok();
    match t.len() {
        3 => {
            let n = |i: usize| u8::from_str_radix(t.get(i..=i)?, 16).ok().map(|v| v * 17);
            Some([n(0)?, n(1)?, n(2)?, 255])
        }
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

/// Perceived brightness 0..=1 (Rec. 601 luma).
pub fn luma(c: Rgba) -> f32 {
    (0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2])) / 255.0
}

/// Squared distance in RGBA space, weighted towards how eyes see differences.
pub fn distance(a: Rgba, b: Rgba) -> u32 {
    let d = |i: usize| (i32::from(a[i]) - i32::from(b[i])).unsigned_abs();
    let (r, g, bl, al) = (d(0), d(1), d(2), d(3));
    2 * r * r + 4 * g * g + 3 * bl * bl + 2 * al * al
}

/// The palette entry nearest to `c` (index), or `None` for an empty palette.
pub fn nearest_index(palette: &[Rgba], c: Rgba) -> Option<usize> {
    palette
        .iter()
        .enumerate()
        .min_by_key(|(_, p)| distance(**p, c))
        .map(|(i, _)| i)
}

/// Kinds of colour-vision simulation for previews.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VisionSim {
    #[default]
    Normal,
    Protanopia,
    Deuteranopia,
    Tritanopia,
    Grayscale,
}

impl VisionSim {
    pub const ALL: [VisionSim; 5] = [
        Self::Normal,
        Self::Protanopia,
        Self::Deuteranopia,
        Self::Tritanopia,
        Self::Grayscale,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "Normal vision",
            Self::Protanopia => "Protanopia (red-blind)",
            Self::Deuteranopia => "Deuteranopia (green-blind)",
            Self::Tritanopia => "Tritanopia (blue-blind)",
            Self::Grayscale => "Greyscale",
        }
    }

    /// Simulates how a colour looks with this kind of vision (Machado et al. matrices).
    pub fn apply(self, c: Rgba) -> Rgba {
        let m: [[f32; 3]; 3] = match self {
            Self::Normal => return c,
            Self::Grayscale => {
                let l = (luma(c) * 255.0).round() as u8;
                return [l, l, l, c[3]];
            }
            Self::Protanopia => [
                [0.152, 1.053, -0.205],
                [0.115, 0.786, 0.099],
                [-0.004, -0.048, 1.052],
            ],
            Self::Deuteranopia => [
                [0.367, 0.861, -0.228],
                [0.280, 0.673, 0.047],
                [-0.012, 0.043, 0.969],
            ],
            Self::Tritanopia => [
                [1.256, -0.077, -0.179],
                [-0.078, 0.931, 0.148],
                [0.005, 0.691, 0.304],
            ],
        };
        let v = [c[0], c[1], c[2]].map(f32::from);
        let out = m.map(|row| (row[0] * v[0] + row[1] * v[1] + row[2] * v[2]).round().clamp(0.0, 255.0) as u8);
        [out[0], out[1], out[2], c[3]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trip() {
        for c in [[255, 0, 0, 255], [12, 200, 99, 255], [0, 0, 0, 255], [255, 255, 255, 255]] {
            let (h, s, v) = rgb_to_hsv(c);
            let rgb = hsv_to_rgb(h, s, v);
            assert_eq!([rgb[0], rgb[1], rgb[2], 255], c);
            let (h, s, l) = rgb_to_hsl(c);
            let rgb = hsl_to_rgb(h, s, l);
            assert_eq!([rgb[0], rgb[1], rgb[2], 255], c);
        }
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#ff0080"), Some([255, 0, 128, 255]));
        assert_eq!(parse_hex("f08"), Some([255, 0, 136, 255]));
        assert_eq!(parse_hex("00000080"), Some([0, 0, 0, 128]));
        assert_eq!(parse_hex("zz"), None);
        assert_eq!(to_hex([1, 2, 3, 255]), "#010203");
    }
}

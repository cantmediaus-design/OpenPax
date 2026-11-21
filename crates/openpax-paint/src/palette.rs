//! Palettes: swatch lists, groups, sorting and the common palette file formats.

use serde::{Deserialize, Serialize};

use crate::color::{luma, rgb_to_hsv};
use crate::{Bitmap, Rgba};

/// An ordered list of swatches, optionally split into named groups.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<Rgba>,
    /// `(first index, label)` of each group, in order.
    pub groups: Vec<(usize, String)>,
}

impl Default for Palette {
    fn default() -> Self {
        const HEX: [u32; 24] = [
            0x000000, 0x3b3b3b, 0x7f7f7f, 0xbfbfbf, 0xffffff, 0x5c2e0e, 0x8f563b, 0xd9a066,
            0xac3232, 0xe04040, 0xf2853a, 0xfbd24c, 0x4b692f, 0x6abe30, 0x99e550, 0x37946e,
            0x306082, 0x5b6ee1, 0x5fcde4, 0xcbdbfc, 0x45283c, 0x76428a, 0xd77bba, 0xf7b5c8,
        ];
        Self {
            name: "Default".into(),
            colors: HEX
                .iter()
                .map(|h| [(h >> 16) as u8, (h >> 8) as u8, *h as u8, 255])
                .collect(),
            groups: Vec::new(),
        }
    }
}

/// Palette file formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaletteFormat {
    /// GIMP / Aseprite `.gpl`.
    Gpl,
    /// JASC (Paint Shop Pro) `.pal`.
    JascPal,
    /// Adobe Swatch Exchange `.ase`.
    Ase,
    /// One `rrggbb` per line, `.hex` (Lospec).
    Hex,
    /// A strip of 1-pixel swatches, `.png`.
    Png,
}

impl PaletteFormat {
    pub const ALL: [PaletteFormat; 5] = [Self::Gpl, Self::JascPal, Self::Ase, Self::Hex, Self::Png];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Gpl => "gpl",
            Self::JascPal => "pal",
            Self::Ase => "ase",
            Self::Hex => "hex",
            Self::Png => "png",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Gpl => "GIMP palette (.gpl)",
            Self::JascPal => "JASC palette (.pal)",
            Self::Ase => "Adobe Swatch Exchange (.ase)",
            Self::Hex => "Hex list (.hex)",
            Self::Png => "PNG swatches (.png)",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| {
            f.extension()
                .eq_ignore_ascii_case(ext.trim_start_matches('.'))
        })
    }
}

impl Palette {
    pub fn new(name: impl Into<String>, colors: Vec<Rgba>) -> Self {
        Self {
            name: name.into(),
            colors,
            groups: Vec::new(),
        }
    }

    /// The distinct visible colours of a bitmap, in order of first appearance, up to `max`.
    pub fn from_bitmap(bitmap: &Bitmap, max: usize) -> Self {
        let mut colors: Vec<Rgba> = Vec::new();
        for color in bitmap.pixels() {
            if color[3] != 0 && !colors.contains(&color) {
                if colors.len() == max {
                    break;
                }
                colors.push(color);
            }
        }
        Self::new("From image", colors)
    }

    /// Appends a colour unless it is already present.
    pub fn add(&mut self, color: Rgba) {
        if !self.colors.contains(&color) {
            self.colors.push(color);
        }
    }

    /// Inserts a colour at an index (shifting groups after it).
    pub fn insert(&mut self, index: usize, color: Rgba) {
        let index = index.min(self.colors.len());
        self.colors.insert(index, color);
        for (start, _) in &mut self.groups {
            if *start > index {
                *start += 1;
            }
        }
    }

    /// Removes a colour (shifting groups after it).
    pub fn remove(&mut self, index: usize) {
        if index < self.colors.len() {
            self.colors.remove(index);
            for (start, _) in &mut self.groups {
                if *start > index {
                    *start -= 1;
                }
            }
            self.groups.retain(|(s, _)| *s < self.colors.len().max(1));
        }
    }

    /// The group label that starts at `index`, if any.
    pub fn group_at(&self, index: usize) -> Option<&str> {
        self.groups
            .iter()
            .find(|(s, _)| *s == index)
            .map(|(_, l)| l.as_str())
    }

    pub fn sort_by_hue(&mut self) {
        self.colors.sort_by(|a, b| {
            let (ha, sa, va) = rgb_to_hsv(*a);
            let (hb, sb, vb) = rgb_to_hsv(*b);
            // Greys first, then by hue, then by brightness.
            (sa > 0.08)
                .cmp(&(sb > 0.08))
                .then(ha.total_cmp(&hb))
                .then(va.total_cmp(&vb))
        });
        self.groups.clear();
    }

    pub fn sort_by_luma(&mut self) {
        self.colors.sort_by(|a, b| luma(*a).total_cmp(&luma(*b)));
        self.groups.clear();
    }

    pub fn encode(&self, format: PaletteFormat) -> Vec<u8> {
        match format {
            PaletteFormat::Gpl => {
                let mut s = format!("GIMP Palette\nName: {}\nColumns: 8\n#\n", self.name);
                for (i, c) in self.colors.iter().enumerate() {
                    let label = self.group_at(i).unwrap_or("");
                    s.push_str(&format!(
                        "{:3} {:3} {:3}\t{}\n",
                        c[0],
                        c[1],
                        c[2],
                        if label.is_empty() {
                            crate::color::to_hex(*c)
                        } else {
                            label.to_owned()
                        }
                    ));
                }
                s.into_bytes()
            }
            PaletteFormat::JascPal => {
                let mut s = format!("JASC-PAL\r\n0100\r\n{}\r\n", self.colors.len());
                for c in &self.colors {
                    s.push_str(&format!("{} {} {}\r\n", c[0], c[1], c[2]));
                }
                s.into_bytes()
            }
            PaletteFormat::Hex => self
                .colors
                .iter()
                .map(|c| format!("{:02x}{:02x}{:02x}\n", c[0], c[1], c[2]))
                .collect::<String>()
                .into_bytes(),
            PaletteFormat::Ase => encode_ase(self),
            PaletteFormat::Png => {
                let mut strip = Bitmap::new(self.colors.len().max(1) as u32, 1);
                for (i, c) in self.colors.iter().enumerate() {
                    strip.set(i as i32, 0, *c);
                }
                crate::encode_png(&strip).unwrap_or_default()
            }
        }
    }

    pub fn decode(bytes: &[u8], format: PaletteFormat) -> Result<Self, String> {
        let text = || String::from_utf8_lossy(bytes).into_owned();
        let colors = match format {
            PaletteFormat::Gpl => {
                let t = text();
                let mut lines = t.lines();
                if !lines
                    .next()
                    .is_some_and(|l| l.trim().starts_with("GIMP Palette"))
                {
                    return Err("not a GIMP palette".into());
                }
                let mut name = String::from("Imported");
                let mut colors = Vec::new();
                for line in lines {
                    let line = line.trim();
                    if let Some(n) = line.strip_prefix("Name:") {
                        name = n.trim().to_owned();
                        continue;
                    }
                    if line.is_empty() || line.starts_with('#') || line.starts_with("Columns:") {
                        continue;
                    }
                    let nums: Vec<u8> = line
                        .split_whitespace()
                        .take(3)
                        .filter_map(|v| v.parse().ok())
                        .collect();
                    if let [r, g, b] = nums[..] {
                        colors.push([r, g, b, 255]);
                    }
                }
                return Ok(Self::new(name, colors));
            }
            PaletteFormat::JascPal => {
                let t = text();
                let mut lines = t.lines();
                if !lines.next().is_some_and(|l| l.trim() == "JASC-PAL") {
                    return Err("not a JASC palette".into());
                }
                lines
                    .skip(2)
                    .filter_map(|l| {
                        let nums: Vec<u8> = l
                            .split_whitespace()
                            .filter_map(|v| v.parse().ok())
                            .collect();
                        match nums[..] {
                            [r, g, b] => Some([r, g, b, 255]),
                            [r, g, b, a] => Some([r, g, b, a]),
                            _ => None,
                        }
                    })
                    .collect()
            }
            PaletteFormat::Hex => text()
                .lines()
                .filter_map(|l| crate::color::parse_hex(l.trim()))
                .collect(),
            PaletteFormat::Ase => return decode_ase(bytes),
            PaletteFormat::Png => {
                let img = crate::decode_image(bytes).map_err(|e| e.to_string())?;
                let mut colors: Vec<Rgba> = Vec::new();
                for c in img.pixels() {
                    if c[3] > 0 && !colors.contains(&c) {
                        colors.push(c);
                        if colors.len() >= 256 {
                            break;
                        }
                    }
                }
                colors
            }
        };
        Ok(Self::new("Imported", colors))
    }
}

fn encode_ase(p: &Palette) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"ASEF");
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(p.colors.len() as u32).to_be_bytes());
    for c in &p.colors {
        let name: Vec<u16> = crate::color::to_hex(*c).encode_utf16().chain([0]).collect();
        let mut block = Vec::new();
        block.extend_from_slice(&(name.len() as u16).to_be_bytes());
        for u in &name {
            block.extend_from_slice(&u.to_be_bytes());
        }
        block.extend_from_slice(b"RGB ");
        for v in &c[..3] {
            block.extend_from_slice(&(f32::from(*v) / 255.0).to_be_bytes());
        }
        block.extend_from_slice(&2u16.to_be_bytes()); // normal colour
        out.extend_from_slice(&0x0001u16.to_be_bytes());
        out.extend_from_slice(&(block.len() as u32).to_be_bytes());
        out.extend_from_slice(&block);
    }
    out
}

fn decode_ase(bytes: &[u8]) -> Result<Palette, String> {
    let err = || "not a valid .ase file".to_owned();
    if bytes.get(0..4) != Some(b"ASEF") {
        return Err(err());
    }
    let u16_at = |i: usize| {
        bytes
            .get(i..i + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
    };
    let u32_at = |i: usize| {
        bytes
            .get(i..i + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let f32_at = |i: usize| {
        bytes
            .get(i..i + 4)
            .map(|b| f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let count = u32_at(8).ok_or_else(err)?;
    let mut pos = 12;
    let mut palette = Palette::new("Imported", Vec::new());
    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    for _ in 0..count {
        let kind = u16_at(pos).ok_or_else(err)?;
        let len = u32_at(pos + 2).ok_or_else(err)? as usize;
        let body = pos + 6;
        if kind == 0x0001 {
            let name_len = u16_at(body).ok_or_else(err)? as usize;
            let model_at = body + 2 + name_len * 2;
            let model = bytes.get(model_at..model_at + 4).ok_or_else(err)?;
            let v = model_at + 4;
            let color = match model {
                b"RGB " => [
                    to_u8(f32_at(v).ok_or_else(err)?),
                    to_u8(f32_at(v + 4).ok_or_else(err)?),
                    to_u8(f32_at(v + 8).ok_or_else(err)?),
                    255,
                ],
                b"Gray" => {
                    let g = to_u8(f32_at(v).ok_or_else(err)?);
                    [g, g, g, 255]
                }
                b"CMYK" => {
                    let c = f32_at(v).ok_or_else(err)?;
                    let m = f32_at(v + 4).ok_or_else(err)?;
                    let y = f32_at(v + 8).ok_or_else(err)?;
                    let k = f32_at(v + 12).ok_or_else(err)?;
                    [
                        to_u8((1.0 - c) * (1.0 - k)),
                        to_u8((1.0 - m) * (1.0 - k)),
                        to_u8((1.0 - y) * (1.0 - k)),
                        255,
                    ]
                }
                _ => {
                    pos = body + len;
                    continue;
                }
            };
            palette.colors.push(color);
        } else if kind == 0xC001 {
            // Group start: its name labels the colours that follow.
            let name_len = u16_at(body).ok_or_else(err)? as usize;
            let units: Vec<u16> = (0..name_len.saturating_sub(1))
                .filter_map(|i| u16_at(body + 2 + i * 2))
                .collect();
            palette
                .groups
                .push((palette.colors.len(), String::from_utf16_lossy(&units)));
        }
        pos = body + len;
    }
    Ok(palette)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_distinct_visible_colours() {
        let mut bitmap = Bitmap::new(2, 2);
        bitmap.set(0, 0, [1, 1, 1, 255]);
        bitmap.set(1, 0, [2, 2, 2, 255]);
        bitmap.set(0, 1, [1, 1, 1, 255]);
        let palette = Palette::from_bitmap(&bitmap, 16);
        assert_eq!(palette.colors, vec![[1, 1, 1, 255], [2, 2, 2, 255]]);
        assert_eq!(Palette::from_bitmap(&bitmap, 1).colors.len(), 1);
    }

    #[test]
    fn every_format_round_trips() {
        let p = Palette::new(
            "Test",
            vec![[255, 0, 0, 255], [0, 128, 255, 255], [10, 20, 30, 255]],
        );
        for format in PaletteFormat::ALL {
            let back = Palette::decode(&p.encode(format), format).unwrap();
            assert_eq!(back.colors, p.colors, "{format:?}");
        }
        assert_eq!(
            Palette::decode(&p.encode(PaletteFormat::Gpl), PaletteFormat::Gpl)
                .unwrap()
                .name,
            "Test"
        );
        assert!(Palette::decode(b"junk", PaletteFormat::Ase).is_err());
    }

    #[test]
    fn insert_remove_and_groups() {
        let mut p = Palette::new("x", vec![[0; 4], [1; 4]]);
        p.groups.push((1, "B".into()));
        p.insert(0, [9; 4]);
        assert_eq!(p.group_at(2), Some("B"));
        p.remove(0);
        assert_eq!(p.group_at(1), Some("B"));
        p.sort_by_luma();
        assert!(p.groups.is_empty());
    }
}

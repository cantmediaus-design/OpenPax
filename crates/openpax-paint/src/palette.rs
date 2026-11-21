use crate::{Bitmap, Rgba};

/// An ordered list of swatches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    pub colors: Vec<Rgba>,
}

impl Default for Palette {
    fn default() -> Self {
        const HEX: [u32; 24] = [
            0x000000, 0x3b3b3b, 0x7f7f7f, 0xbfbfbf, 0xffffff, 0x5c2e0e, 0x8f563b, 0xd9a066,
            0xac3232, 0xe04040, 0xf2853a, 0xfbd24c, 0x4b692f, 0x6abe30, 0x99e550, 0x37946e,
            0x306082, 0x5b6ee1, 0x5fcde4, 0xcbdbfc, 0x45283c, 0x76428a, 0xd77bba, 0xf7b5c8,
        ];
        Self {
            colors: HEX
                .iter()
                .map(|h| [(h >> 16) as u8, (h >> 8) as u8, *h as u8, 255])
                .collect(),
        }
    }
}

impl Palette {
    /// The distinct visible colours of a bitmap, in order of first appearance, up to `max`.
    pub fn from_bitmap(bitmap: &Bitmap, max: usize) -> Self {
        let mut colors: Vec<Rgba> = Vec::new();
        for px in bitmap.data().as_chunks::<4>().0 {
            let color = [px[0], px[1], px[2], px[3]];
            if color[3] != 0 && !colors.contains(&color) {
                if colors.len() == max {
                    break;
                }
                colors.push(color);
            }
        }
        Self { colors }
    }

    /// Appends a colour unless it is already present.
    pub fn add(&mut self, color: Rgba) {
        if !self.colors.contains(&color) {
            self.colors.push(color);
        }
    }
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
}

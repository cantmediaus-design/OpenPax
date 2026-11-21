//! Quality checks: tile seams, palette constraints and stray pixels.

use serde::{Deserialize, Serialize};

use crate::{Bitmap, Rgba, colors_match};

/// One problem found in an image.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub kind: IssueKind,
    pub x: i32,
    pub y: i32,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IssueKind {
    Seam,
    OffPalette,
    TooManyColors,
    StrayPixel,
}

impl IssueKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Seam => "Tile seam",
            Self::OffPalette => "Colour not in palette",
            Self::TooManyColors => "Too many colours",
            Self::StrayPixel => "Stray pixel",
        }
    }
}

/// Settings for [`check`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckOptions {
    pub seams: bool,
    /// Colour difference across a wrap edge that counts as a seam.
    pub seam_tolerance: u8,
    /// Every visible colour must be one of these (empty: not checked).
    pub palette: Vec<Rgba>,
    /// Most distinct visible colours allowed (0: not checked).
    pub max_colors: usize,
    pub strays: bool,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            seams: true,
            seam_tolerance: 48,
            palette: Vec::new(),
            max_colors: 0,
            strays: true,
        }
    }
}

fn diff(a: Rgba, b: Rgba) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| u32::from(x.abs_diff(*y)))
        .max()
        .unwrap_or(0)
}

/// Pixels on the image's edges that clash with the pixel on the opposite edge, where the
/// texture repeats.
pub fn seams(image: &Bitmap, tolerance: u8) -> Vec<Issue> {
    let (w, h) = (image.width() as i32, image.height() as i32);
    let mut out = Vec::new();
    if w < 2 || h < 2 {
        return out;
    }
    // How much the image changes between neighbouring pixels inside, as a baseline.
    let tol = u32::from(tolerance);
    for y in 0..h {
        let (l, r) = (
            image.get(0, y).unwrap_or([0; 4]),
            image.get(w - 1, y).unwrap_or([0; 4]),
        );
        if diff(l, r) > tol {
            out.push(Issue {
                kind: IssueKind::Seam,
                x: 0,
                y,
                message: format!("Left and right edges differ at row {y}"),
            });
        }
    }
    for x in 0..w {
        let (t, b) = (
            image.get(x, 0).unwrap_or([0; 4]),
            image.get(x, h - 1).unwrap_or([0; 4]),
        );
        if diff(t, b) > tol {
            out.push(Issue {
                kind: IssueKind::Seam,
                x,
                y: 0,
                message: format!("Top and bottom edges differ at column {x}"),
            });
        }
    }
    out
}

/// A 0..=1 score of how seamlessly the image tiles (1 is perfect): the average difference
/// across the wrap edges compared with the average difference inside.
pub fn seam_score(image: &Bitmap) -> f32 {
    let (w, h) = (image.width() as i32, image.height() as i32);
    if w < 2 || h < 2 {
        return 1.0;
    }
    let px = |x: i32, y: i32| image.get(x, y).unwrap_or([0; 4]);
    let (mut edge, mut edges) = (0u64, 0u64);
    for y in 0..h {
        edge += u64::from(diff(px(0, y), px(w - 1, y)));
        edges += 1;
    }
    for x in 0..w {
        edge += u64::from(diff(px(x, 0), px(x, h - 1)));
        edges += 1;
    }
    let (mut inner, mut inners) = (0u64, 0u64);
    for y in 0..h {
        for x in 1..w {
            inner += u64::from(diff(px(x - 1, y), px(x, y)));
            inners += 1;
        }
    }
    for y in 1..h {
        for x in 0..w {
            inner += u64::from(diff(px(x, y - 1), px(x, y)));
            inners += 1;
        }
    }
    let edge = edge as f32 / edges.max(1) as f32;
    let inner = inner as f32 / inners.max(1) as f32;
    if edge <= inner {
        1.0
    } else {
        (1.0 - (edge - inner) / 255.0).clamp(0.0, 1.0)
    }
}

/// Visible pixels whose colour is not in the palette.
pub fn off_palette(image: &Bitmap, palette: &[Rgba]) -> Vec<Issue> {
    let mut out = Vec::new();
    for y in 0..image.height() as i32 {
        for x in 0..image.width() as i32 {
            let c = image.get(x, y).unwrap_or([0; 4]);
            if c[3] > 0 && !palette.iter().any(|p| colors_match(*p, c, 0)) {
                out.push(Issue {
                    kind: IssueKind::OffPalette,
                    x,
                    y,
                    message: format!("{} is not in the palette", crate::color::to_hex(c)),
                });
            }
        }
    }
    out
}

/// Visible pixels with no orthogonal neighbour of the same colour (isolated specks).
pub fn stray_pixels(image: &Bitmap) -> Vec<Issue> {
    let mut out = Vec::new();
    for y in 0..image.height() as i32 {
        for x in 0..image.width() as i32 {
            let c = image.get(x, y).unwrap_or([0; 4]);
            if c[3] == 0 {
                continue;
            }
            let neighbours = [(1, 0), (-1, 0), (0, 1), (0, -1)];
            let same = neighbours
                .iter()
                .any(|(dx, dy)| image.get(x + dx, y + dy) == Some(c));
            // Diagonal-only neighbours in the same colour are often deliberate (lines).
            let diagonal = [(1, 1), (-1, -1), (1, -1), (-1, 1)]
                .iter()
                .any(|(dx, dy)| image.get(x + dx, y + dy) == Some(c));
            let surrounded_by_one_colour = {
                let n: Vec<Rgba> = neighbours
                    .iter()
                    .filter_map(|(dx, dy)| image.get(x + dx, y + dy))
                    .collect();
                n.len() >= 3 && n.windows(2).all(|p| p[0] == p[1])
            };
            if !same && !diagonal && surrounded_by_one_colour {
                out.push(Issue {
                    kind: IssueKind::StrayPixel,
                    x,
                    y,
                    message: format!("Isolated {} pixel", crate::color::to_hex(c)),
                });
            }
        }
    }
    out
}

/// Runs the checks the options ask for.
pub fn check(image: &Bitmap, options: &CheckOptions) -> Vec<Issue> {
    let mut out = Vec::new();
    if options.seams {
        out.extend(seams(image, options.seam_tolerance));
    }
    if !options.palette.is_empty() {
        out.extend(off_palette(image, &options.palette));
    }
    if options.max_colors > 0 {
        let n = crate::quantize::count_colors(image);
        if n > options.max_colors {
            out.push(Issue {
                kind: IssueKind::TooManyColors,
                x: 0,
                y: 0,
                message: format!("{n} colours used; the limit is {}", options.max_colors),
            });
        }
    }
    if options.strays {
        out.extend(stray_pixels(image));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seamless_images_have_no_seams() {
        let flat = Bitmap::filled(4, 4, [9, 9, 9, 255]);
        assert!(seams(&flat, 0).is_empty());
        assert_eq!(seam_score(&flat), 1.0);
        let mut b = flat.clone();
        for y in 0..4 {
            b.set(3, y, [255, 255, 255, 255]);
        }
        assert_eq!(
            seams(&b, 10).len(),
            4,
            "one per row; columns still match top to bottom"
        );
        assert!(seam_score(&b) < 1.0);
    }

    #[test]
    fn strays_and_palette() {
        let mut b = Bitmap::filled(3, 3, [0, 0, 0, 255]);
        b.set(1, 1, [255, 0, 0, 255]);
        let strays = stray_pixels(&b);
        assert_eq!(strays.len(), 1);
        assert_eq!((strays[0].x, strays[0].y), (1, 1));
        assert_eq!(off_palette(&b, &[[0, 0, 0, 255]]).len(), 1);
        let options = CheckOptions {
            max_colors: 1,
            seams: false,
            ..Default::default()
        };
        assert!(
            check(&b, &options)
                .iter()
                .any(|i| i.kind == IssueKind::TooManyColors)
        );
    }
}

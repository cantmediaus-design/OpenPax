//! Animated image formats, sprite sheets, texture atlases and their metadata.

use std::io::Cursor;

use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::codecs::png::PngDecoder;
use image::codecs::webp::{WebPDecoder, WebPEncoder};
use image::{AnimationDecoder, Delay, ExtendedColorType, ImageEncoder, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::document::{Document, PlayDirection};
use crate::resample::{self, ScaleMethod};
use crate::{Bitmap, Error, Rect};

/// One frame of an animation.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimFrame {
    pub image: Bitmap,
    pub duration_ms: u32,
}

fn to_image(b: &Bitmap) -> RgbaImage {
    RgbaImage::from_raw(b.width(), b.height(), b.data().to_vec()).expect("size matches")
}

fn from_image(img: RgbaImage) -> Bitmap {
    let (w, h) = img.dimensions();
    Bitmap::from_rgba(w, h, img.into_raw()).expect("size matches")
}

/// The frames of a document (or of one tag, in its play direction), flattened.
pub fn document_frames(doc: &Document, tag: Option<usize>) -> Vec<AnimFrame> {
    let order: Vec<usize> = match tag.and_then(|t| doc.tags().get(t)) {
        Some(t) => t.direction.order(t.from, t.to),
        None => (0..doc.frame_count()).collect(),
    };
    order
        .into_iter()
        .map(|f| AnimFrame {
            image: doc.flatten_frame(f),
            duration_ms: doc.frames()[f].duration_ms,
        })
        .collect()
}

/// Scales every frame by a whole factor with nearest neighbour.
pub fn scale_frames(frames: &[AnimFrame], factor: u32) -> Vec<AnimFrame> {
    if factor <= 1 {
        return frames.to_vec();
    }
    frames
        .iter()
        .map(|f| AnimFrame {
            image: resample::scale(
                &f.image,
                f.image.width() * factor,
                f.image.height() * factor,
                ScaleMethod::Nearest,
            ),
            duration_ms: f.duration_ms,
        })
        .collect()
}

/// An animated GIF. `loops` of `None` repeats forever.
pub fn encode_gif(frames: &[AnimFrame], loops: Option<u16>) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new_with_speed(&mut out, 10);
        enc.set_repeat(loops.map_or(Repeat::Infinite, Repeat::Finite))?;
        for f in frames {
            let delay = Delay::from_numer_denom_ms(f.duration_ms.max(10), 1);
            enc.encode_frame(image::Frame::from_parts(to_image(&f.image), 0, 0, delay))?;
        }
    }
    Ok(out)
}

/// An animated PNG. `loops` of `None` repeats forever.
pub fn encode_apng(frames: &[AnimFrame], loops: Option<u16>) -> Result<Vec<u8>, Error> {
    let first = frames
        .first()
        .ok_or_else(|| Error::Other("no frames".into()))?;
    let (w, h) = (first.image.width(), first.image.height());
    let mut out = Vec::new();
    let map = |e: png::EncodingError| Error::Other(e.to_string());
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_animated(frames.len() as u32, u32::from(loops.unwrap_or(0)))
            .map_err(map)?;
        let mut writer = enc.write_header().map_err(map)?;
        for f in frames {
            let image = if (f.image.width(), f.image.height()) == (w, h) {
                f.image.clone()
            } else {
                f.image.resized(w, h)
            };
            writer
                .set_frame_delay(f.duration_ms.min(65535) as u16, 1000)
                .map_err(map)?;
            writer.write_image_data(image.data()).map_err(map)?;
        }
        writer.finish().map_err(map)?;
    }
    Ok(out)
}

/// A lossless WebP still image.
pub fn encode_webp(image: &Bitmap) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    WebPEncoder::new_lossless(&mut out).write_image(
        image.data(),
        image.width(),
        image.height(),
        ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

/// Reads every frame of a GIF, APNG or animated WebP (a still image gives one frame).
pub fn decode_animation(bytes: &[u8]) -> Result<Vec<AnimFrame>, Error> {
    let collect = |frames: image::Frames| -> Result<Vec<AnimFrame>, Error> {
        frames
            .map(|f| {
                let f = f?;
                let (n, d) = f.delay().numer_denom_ms();
                Ok(AnimFrame {
                    duration_ms: (n / d.max(1)).max(1),
                    image: from_image(f.into_buffer()),
                })
            })
            .collect()
    };
    match image::guess_format(bytes)? {
        image::ImageFormat::Gif => collect(GifDecoder::new(Cursor::new(bytes))?.into_frames()),
        image::ImageFormat::Png => {
            let dec = PngDecoder::new(Cursor::new(bytes))?;
            if dec.is_apng()? {
                collect(dec.apng()?.into_frames())
            } else {
                Ok(vec![AnimFrame {
                    image: crate::decode_png(bytes)?,
                    duration_ms: 100,
                }])
            }
        }
        image::ImageFormat::WebP => {
            let dec = WebPDecoder::new(Cursor::new(bytes))?;
            if dec.has_animation() {
                collect(dec.into_frames())
            } else {
                Ok(vec![AnimFrame {
                    image: crate::decode_image(bytes)?,
                    duration_ms: 100,
                }])
            }
        }
        _ => Ok(vec![AnimFrame {
            image: crate::decode_image(bytes)?,
            duration_ms: 100,
        }]),
    }
}

/// Builds a document with one frame per animation frame (all the size of the largest).
pub fn document_from_frames(frames: Vec<AnimFrame>) -> Option<Document> {
    let w = frames.iter().map(|f| f.image.width()).max()?;
    let h = frames.iter().map(|f| f.image.height()).max()?;
    let durations: Vec<u32> = frames.iter().map(|f| f.duration_ms).collect();
    let images: Vec<Bitmap> = frames.into_iter().map(|f| f.image.resized(w, h)).collect();
    let mut doc = Document::from_frames(w, h, images);
    for (i, d) in durations.into_iter().enumerate() {
        doc.set_frame_duration(i, d);
        doc.end_coalescing();
    }
    // Durations set while building are not edits the user made.
    let mut parts = doc.parts();
    parts.current = 0;
    Some(Document::from_parts(parts))
}

/// Orders numbered file names (`walk_2.png` before `walk_10.png`).
pub fn sort_sequence(names: &mut [String]) {
    fn key(s: &str) -> (String, u64) {
        let stem = s.rsplit_once('.').map_or(s, |(a, _)| a);
        let digits: String = stem
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect();
        let number: String = digits.chars().rev().collect();
        (
            stem[..stem.len() - number.len()].to_owned(),
            number.parse().unwrap_or(0),
        )
    }
    names.sort_by_key(|n| key(n));
}

/// `{name}` and `{frame}` (with optional zero padding like `{frame:3}`) in a file name
/// template.
pub fn frame_file_name(template: &str, name: &str, frame: usize, tag: &str) -> String {
    let mut out = template.replace("{name}", name).replace("{tag}", tag);
    while let Some(start) = out.find("{frame") {
        let Some(end) = out[start..].find('}') else {
            break;
        };
        let spec = &out[start + 6..start + end];
        let width: usize = spec.trim_start_matches(':').parse().unwrap_or(0);
        out.replace_range(start..=start + end, &format!("{frame:0width$}"));
    }
    out
}

// ---- sprite sheets and atlases ----

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SheetLayout {
    #[default]
    Horizontal,
    Vertical,
    /// Rows of a fixed number of columns.
    Grid {
        columns: u32,
    },
    /// Tightly packed (MaxRects), for atlases.
    Packed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SheetOptions {
    pub layout: SheetLayout,
    /// Cut away transparent borders of each sprite.
    pub trim: bool,
    /// Transparent pixels between sprites.
    pub spacing: u32,
    /// Transparent pixels around the whole sheet.
    pub border: u32,
    /// Repeat each sprite's edge pixels outwards (stops texture bleeding).
    pub extrude: u32,
    /// Whole-number scale factor.
    pub scale: u32,
    /// Make packed sheets a power of two in each direction.
    pub power_of_two: bool,
    /// Store identical sprites once.
    pub dedupe: bool,
}

impl Default for SheetOptions {
    fn default() -> Self {
        Self {
            layout: SheetLayout::Horizontal,
            trim: false,
            spacing: 0,
            border: 0,
            extrude: 0,
            scale: 1,
            power_of_two: false,
            dedupe: false,
        }
    }
}

/// A sprite to put on a sheet.
#[derive(Clone, Debug)]
pub struct Sprite {
    pub name: String,
    pub image: Bitmap,
    pub duration_ms: u32,
}

/// Where a sprite ended up on the sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub name: String,
    /// The sprite's pixels on the sheet (without extrusion).
    pub frame: Rect,
    pub trimmed: bool,
    /// The kept part relative to the untrimmed sprite.
    pub sprite_source: Rect,
    /// Size of the untrimmed sprite.
    pub source_size: (u32, u32),
    pub duration_ms: u32,
}

impl Serialize for Rect {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        json!({"x": self.x, "y": self.y, "w": self.w, "h": self.h}).serialize(s)
    }
}

impl<'de> Deserialize<'de> for Rect {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct R {
            x: i32,
            y: i32,
            w: i32,
            h: i32,
        }
        let r = R::deserialize(d)?;
        Ok(Rect::new(r.x, r.y, r.w, r.h))
    }
}

/// A simple MaxRects bin packer (best short side fit).
struct Packer {
    width: i32,
    height: i32,
    free: Vec<Rect>,
}

impl Packer {
    fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            free: vec![Rect::new(0, 0, width, height)],
        }
    }

    fn insert(&mut self, w: i32, h: i32) -> Option<(i32, i32)> {
        let best = self
            .free
            .iter()
            .filter(|f| f.w >= w && f.h >= h)
            .min_by_key(|f| ((f.w - w).min(f.h - h), (f.w - w).max(f.h - h)))
            .copied()?;
        let placed = Rect::new(best.x, best.y, w, h);
        let mut next = Vec::new();
        for f in self.free.drain(..) {
            if f.intersect(&placed).is_none() {
                next.push(f);
                continue;
            }
            // Split the free rectangle around the placed one.
            if placed.x > f.x {
                next.push(Rect::new(f.x, f.y, placed.x - f.x, f.h));
            }
            if placed.x + placed.w < f.x + f.w {
                next.push(Rect::new(
                    placed.x + placed.w,
                    f.y,
                    f.x + f.w - placed.x - placed.w,
                    f.h,
                ));
            }
            if placed.y > f.y {
                next.push(Rect::new(f.x, f.y, f.w, placed.y - f.y));
            }
            if placed.y + placed.h < f.y + f.h {
                next.push(Rect::new(
                    f.x,
                    placed.y + placed.h,
                    f.w,
                    f.y + f.h - placed.y - placed.h,
                ));
            }
        }
        // Drop free rectangles contained in others.
        let contains = |a: &Rect, b: &Rect| {
            b.x >= a.x && b.y >= a.y && b.x + b.w <= a.x + a.w && b.y + b.h <= a.y + a.h
        };
        let mut pruned: Vec<Rect> = Vec::new();
        for (i, r) in next.iter().enumerate() {
            let inside = next
                .iter()
                .enumerate()
                .any(|(j, o)| i != j && contains(o, r) && (r != o || j < i));
            if !inside && !r.is_empty() {
                pruned.push(*r);
            }
        }
        self.free = pruned;
        let _ = (self.width, self.height);
        Some((placed.x, placed.y))
    }
}

/// Packs `sizes` into the smallest square-ish bin it can find; returns positions and size.
pub fn pack(sizes: &[(u32, u32)], power_of_two: bool) -> (Vec<(i32, i32)>, u32, u32) {
    let area: u64 = sizes
        .iter()
        .map(|(w, h)| u64::from(*w) * u64::from(*h))
        .sum();
    let max_w = sizes.iter().map(|s| s.0).max().unwrap_or(1);
    let max_h = sizes.iter().map(|s| s.1).max().unwrap_or(1);
    let mut side = ((area as f64).sqrt().ceil() as u32)
        .max(max_w)
        .max(max_h)
        .max(1);
    // Largest first packs better.
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|i| std::cmp::Reverse(sizes[*i].0.max(sizes[*i].1)));
    loop {
        let (w, h) = if power_of_two {
            (side.next_power_of_two(), side.next_power_of_two())
        } else {
            (side, side)
        };
        let mut packer = Packer::new(w as i32, h as i32);
        let mut pos = vec![(0, 0); sizes.len()];
        let ok = order.iter().all(
            |&i| match packer.insert(sizes[i].0 as i32, sizes[i].1 as i32) {
                Some(p) => {
                    pos[i] = p;
                    true
                }
                None => false,
            },
        );
        if ok {
            // Shrink to what is used.
            let used_w = (0..sizes.len())
                .map(|i| pos[i].0 as u32 + sizes[i].0)
                .max()
                .unwrap_or(1);
            let used_h = (0..sizes.len())
                .map(|i| pos[i].1 as u32 + sizes[i].1)
                .max()
                .unwrap_or(1);
            let (w, h) = if power_of_two {
                (used_w.next_power_of_two(), used_h.next_power_of_two())
            } else {
                (used_w, used_h)
            };
            return (pos, w, h);
        }
        side += (side / 8).max(1);
    }
}

/// Lays sprites out on one image.
pub fn build_sheet(sprites: &[Sprite], options: &SheetOptions) -> (Bitmap, Vec<Placed>) {
    let scale = options.scale.max(1);
    let pad = options.extrude as i32;
    // Prepare each sprite: scale, trim.
    struct Prepared {
        image: Bitmap,
        source: Rect,
        size: (u32, u32),
        trimmed: bool,
    }
    let prepared: Vec<Prepared> = sprites
        .iter()
        .map(|s| {
            let scaled = resample::scale(
                &s.image,
                s.image.width() * scale,
                s.image.height() * scale,
                ScaleMethod::Nearest,
            );
            let full = scaled.bounds();
            let source = if options.trim {
                scaled.opaque_bounds().unwrap_or(Rect::new(0, 0, 1, 1))
            } else {
                full
            };
            Prepared {
                image: scaled.crop(source),
                trimmed: source != full,
                size: (scaled.width(), scaled.height()),
                source,
            }
        })
        .collect();
    // Identical sprites can share one spot.
    let mut unique: Vec<usize> = Vec::new();
    let mut slot = vec![0usize; prepared.len()];
    for (i, p) in prepared.iter().enumerate() {
        let same = options
            .dedupe
            .then(|| unique.iter().position(|u| prepared[*u].image == p.image))
            .flatten();
        slot[i] = match same {
            Some(s) => s,
            None => {
                unique.push(i);
                unique.len() - 1
            }
        };
    }
    let cell = |i: usize| {
        let img = &prepared[unique[i]].image;
        (
            img.width() + 2 * options.extrude + options.spacing,
            img.height() + 2 * options.extrude + options.spacing,
        )
    };
    let n = unique.len();
    let border = options.border as i32;
    let (positions, mut w, mut h): (Vec<(i32, i32)>, u32, u32) = match options.layout {
        SheetLayout::Packed => {
            let sizes: Vec<(u32, u32)> = (0..n).map(cell).collect();
            pack(&sizes, options.power_of_two)
        }
        layout => {
            let columns = match layout {
                SheetLayout::Horizontal => n.max(1) as u32,
                SheetLayout::Vertical => 1,
                SheetLayout::Grid { columns } => columns.max(1),
                SheetLayout::Packed => unreachable!(),
            };
            let cw = (0..n).map(|i| cell(i).0).max().unwrap_or(1);
            let ch = (0..n).map(|i| cell(i).1).max().unwrap_or(1);
            let rows = (n as u32).div_ceil(columns).max(1);
            let pos = (0..n)
                .map(|i| {
                    (
                        ((i as u32 % columns) * cw) as i32,
                        ((i as u32 / columns) * ch) as i32,
                    )
                })
                .collect();
            (pos, columns.min(n.max(1) as u32) * cw, rows * ch)
        }
    };
    w = (w + 2 * options.border)
        .saturating_sub(options.spacing)
        .max(1);
    h = (h + 2 * options.border)
        .saturating_sub(options.spacing)
        .max(1);
    let mut sheet = Bitmap::new(w, h);
    let mut frames_at: Vec<Rect> = Vec::with_capacity(n);
    for (k, &i) in unique.iter().enumerate() {
        let img = &prepared[i].image;
        let (x, y) = (positions[k].0 + border + pad, positions[k].1 + border + pad);
        // Extrude edges first, then the sprite itself.
        for e in 1..=pad {
            for sx in 0..img.width() as i32 {
                let top = img.get(sx, 0).unwrap_or([0; 4]);
                let bottom = img.get(sx, img.height() as i32 - 1).unwrap_or([0; 4]);
                sheet.set(x + sx, y - e, top);
                sheet.set(x + sx, y + img.height() as i32 - 1 + e, bottom);
            }
            for sy in -pad..img.height() as i32 + pad {
                let cy = sy.clamp(0, img.height() as i32 - 1);
                sheet.set(x - e, y + sy, img.get(0, cy).unwrap_or([0; 4]));
                sheet.set(
                    x + img.width() as i32 - 1 + e,
                    y + sy,
                    img.get(img.width() as i32 - 1, cy).unwrap_or([0; 4]),
                );
            }
        }
        sheet.blit(img, x, y);
        frames_at.push(Rect::new(x, y, img.width() as i32, img.height() as i32));
    }
    let placed = sprites
        .iter()
        .enumerate()
        .map(|(i, s)| Placed {
            name: s.name.clone(),
            frame: frames_at[slot[i]],
            trimmed: prepared[i].trimmed,
            sprite_source: prepared[i].source,
            source_size: prepared[i].size,
            duration_ms: s.duration_ms,
        })
        .collect();
    (sheet, placed)
}

/// Sprite sheet metadata in Aseprite's JSON layout ("array" form), which most engines and
/// tools understand.
pub fn sheet_json(
    placed: &[Placed],
    sheet: &Bitmap,
    image_name: &str,
    doc: Option<&Document>,
    scale: u32,
) -> Value {
    let frames: Vec<Value> = placed
        .iter()
        .map(|p| {
            json!({
                "filename": p.name,
                "frame": p.frame,
                "rotated": false,
                "trimmed": p.trimmed,
                "spriteSourceSize": p.sprite_source,
                "sourceSize": {"w": p.source_size.0, "h": p.source_size.1},
                "duration": p.duration_ms,
            })
        })
        .collect();
    let mut meta = json!({
        "app": "OpenPax",
        "version": env!("CARGO_PKG_VERSION"),
        "image": image_name,
        "format": "RGBA8888",
        "size": {"w": sheet.width(), "h": sheet.height()},
        "scale": scale.max(1).to_string(),
    });
    if let Some(doc) = doc {
        meta["frameTags"] = Value::Array(
            doc.tags()
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name, "from": t.from, "to": t.to,
                        "direction": match t.direction {
                            PlayDirection::Forward => "forward",
                            PlayDirection::Reverse => "reverse",
                            PlayDirection::PingPong => "pingpong",
                        },
                        "color": crate::color::to_hex(t.color),
                    })
                })
                .collect(),
        );
        meta["layers"] = Value::Array(
            doc.layers()
                .iter()
                .map(|l| json!({"name": l.name, "opacity": (l.opacity * 255.0).round() as u8, "blendMode": l.blend.name().to_lowercase()}))
                .collect(),
        );
        let s = scale.max(1) as i32;
        let mut slices = Vec::new();
        if let Some((px, py)) = doc.extras.pivot {
            slices.push(json!({
                "name": "pivot", "color": "#0000ffff",
                "keys": [{"frame": 0, "bounds": {"x": 0, "y": 0, "w": doc.width() as i32 * s, "h": doc.height() as i32 * s}, "pivot": {"x": px * s, "y": py * s}}],
            }));
        }
        for a in &doc.extras.anchors {
            slices.push(json!({
                "name": a.name, "color": "#ff0000ff",
                "keys": [{"frame": 0, "bounds": {"x": a.x * s, "y": a.y * s, "w": s, "h": s}, "pivot": {"x": 0, "y": 0}}],
            }));
        }
        meta["slices"] = Value::Array(slices);
    }
    json!({"frames": frames, "meta": meta})
}

/// A saved set of export settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportPreset {
    pub name: String,
    pub format: ExportFormat,
    pub scale: u32,
    /// File name template: `{name}`, `{tag}`, `{frame}` / `{frame:3}`.
    pub file_name: String,
    pub sheet: SheetOptions,
    /// Also write a JSON file with frame and tag data (sheets).
    pub json: bool,
    /// Export only this tag's frames.
    pub tag: Option<String>,
}

impl Default for ExportPreset {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            format: ExportFormat::Png,
            scale: 1,
            file_name: "{name}".into(),
            sheet: SheetOptions::default(),
            json: true,
            tag: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExportFormat {
    /// The current frame (or one PNG per frame with `{frame}` in the name).
    #[default]
    Png,
    PngSequence,
    Gif,
    Apng,
    WebP,
    SpriteSheet,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 6] = [
        Self::Png,
        Self::PngSequence,
        Self::Gif,
        Self::Apng,
        Self::WebP,
        Self::SpriteSheet,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG image",
            Self::PngSequence => "PNG sequence (one file per frame)",
            Self::Gif => "Animated GIF",
            Self::Apng => "Animated PNG",
            Self::WebP => "WebP image",
            Self::SpriteSheet => "Sprite sheet (PNG + JSON)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png | Self::PngSequence | Self::Apng | Self::SpriteSheet => "png",
            Self::Gif => "gif",
            Self::WebP => "webp",
        }
    }
}

/// Renders a document with a preset into `(file name, bytes)` pairs.
pub fn export(
    doc: &Document,
    name: &str,
    preset: &ExportPreset,
) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let tag_index = preset
        .tag
        .as_ref()
        .and_then(|t| doc.tags().iter().position(|x| &x.name == t));
    let tag_name = tag_index.map_or("", |i| doc.tags()[i].name.as_str());
    let frames = scale_frames(&document_frames(doc, tag_index), preset.scale.max(1));
    let base = frame_file_name(&preset.file_name, name, 0, tag_name).replace("{frame}", "");
    let ext = preset.format.extension();
    let mut out = Vec::new();
    match preset.format {
        ExportFormat::Png => {
            let f = frames.get(doc.current_frame().min(frames.len().saturating_sub(1)));
            if let Some(f) = f {
                out.push((format!("{base}.{ext}"), crate::encode_png(&f.image)?));
            }
        }
        ExportFormat::PngSequence => {
            let template = if preset.file_name.contains("{frame") {
                preset.file_name.clone()
            } else {
                format!("{}_{{frame:3}}", preset.file_name)
            };
            for (i, f) in frames.iter().enumerate() {
                let file = frame_file_name(&template, name, i, tag_name);
                out.push((format!("{file}.{ext}"), crate::encode_png(&f.image)?));
            }
        }
        ExportFormat::Gif => out.push((format!("{base}.{ext}"), encode_gif(&frames, None)?)),
        ExportFormat::Apng => out.push((format!("{base}.{ext}"), encode_apng(&frames, None)?)),
        ExportFormat::WebP => {
            if let Some(f) = frames.first() {
                out.push((format!("{base}.{ext}"), encode_webp(&f.image)?));
            }
        }
        ExportFormat::SpriteSheet => {
            let sprites: Vec<Sprite> = frames
                .iter()
                .enumerate()
                .map(|(i, f)| Sprite {
                    name: format!("{name} {i}"),
                    image: f.image.clone(),
                    duration_ms: f.duration_ms,
                })
                .collect();
            // Frames are already scaled.
            let options = SheetOptions {
                scale: 1,
                ..preset.sheet.clone()
            };
            let (sheet, placed) = build_sheet(&sprites, &options);
            let image_name = format!("{base}.{ext}");
            out.push((image_name.clone(), crate::encode_png(&sheet)?));
            if preset.json {
                let json = sheet_json(&placed, &sheet, &image_name, Some(doc), preset.scale);
                out.push((format!("{base}.json"), serde_json::to_vec_pretty(&json)?));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames() -> Vec<AnimFrame> {
        (0..3)
            .map(|i| {
                let mut b = Bitmap::new(4, 4);
                b.set(i, i, [255, 0, 0, 255]);
                AnimFrame {
                    image: b,
                    duration_ms: 100 + i as u32 * 50,
                }
            })
            .collect()
    }

    #[test]
    fn gif_round_trip_keeps_frames_and_delays() {
        let bytes = encode_gif(&frames(), None).unwrap();
        let back = decode_animation(&bytes).unwrap();
        assert_eq!(back.len(), 3);
        assert_eq!(back[1].duration_ms, 150);
        assert_eq!(back[2].image.get(2, 2), Some([255, 0, 0, 255]));
    }

    #[test]
    fn apng_round_trip_is_exact() {
        let bytes = encode_apng(&frames(), None).unwrap();
        let back = decode_animation(&bytes).unwrap();
        assert_eq!(back, frames());
    }

    #[test]
    fn webp_still_is_lossless() {
        let f = &frames()[1];
        let back = decode_animation(&encode_webp(&f.image).unwrap()).unwrap();
        assert_eq!(back[0].image, f.image);
    }

    #[test]
    fn sheets_place_every_sprite() {
        let sprites: Vec<Sprite> = frames()
            .into_iter()
            .enumerate()
            .map(|(i, f)| Sprite {
                name: format!("s{i}"),
                image: f.image,
                duration_ms: f.duration_ms,
            })
            .collect();
        let (sheet, placed) = build_sheet(&sprites, &SheetOptions::default());
        assert_eq!((sheet.width(), sheet.height()), (12, 4));
        assert_eq!(placed[2].frame, Rect::new(8, 0, 4, 4));

        let trimmed = SheetOptions {
            trim: true,
            spacing: 1,
            layout: SheetLayout::Grid { columns: 2 },
            ..Default::default()
        };
        let (sheet, placed) = build_sheet(&sprites, &trimmed);
        assert!(placed.iter().all(|p| p.frame.w == 1 && p.trimmed));
        assert_eq!(
            sheet.get(placed[1].frame.x, placed[1].frame.y),
            Some([255, 0, 0, 255])
        );

        let packed = SheetOptions {
            layout: SheetLayout::Packed,
            extrude: 1,
            ..Default::default()
        };
        let (sheet, placed) = build_sheet(&sprites, &packed);
        for (i, a) in placed.iter().enumerate() {
            for b in &placed[i + 1..] {
                assert!(a.frame.intersect(&b.frame).is_none(), "overlap");
            }
            assert!(a.frame.x >= 1 && a.frame.x + a.frame.w < sheet.width() as i32);
        }
        let json = sheet_json(&placed, &sheet, "s.png", None, 1);
        assert_eq!(json["frames"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn packer_fits_many_rects() {
        let sizes: Vec<(u32, u32)> = (1..20).map(|i| (i % 7 + 1, i % 5 + 2)).collect();
        let (pos, w, h) = pack(&sizes, true);
        assert!(w.is_power_of_two() && h.is_power_of_two());
        for (i, (p, s)) in pos.iter().zip(&sizes).enumerate() {
            let a = Rect::new(p.0, p.1, s.0 as i32, s.1 as i32);
            assert!(a.x + a.w <= w as i32 && a.y + a.h <= h as i32);
            for (q, t) in pos.iter().zip(&sizes).skip(i + 1) {
                assert!(
                    a.intersect(&Rect::new(q.0, q.1, t.0 as i32, t.1 as i32))
                        .is_none()
                );
            }
        }
    }

    #[test]
    fn names_and_sequences() {
        assert_eq!(
            frame_file_name("{name}_{frame:3}", "walk", 7, ""),
            "walk_007"
        );
        let mut names = vec![
            "a_10.png".to_owned(),
            "a_2.png".to_owned(),
            "a_1.png".to_owned(),
        ];
        sort_sequence(&mut names);
        assert_eq!(names, ["a_1.png", "a_2.png", "a_10.png"]);
    }

    #[test]
    fn export_presets_render_files() {
        let mut doc = Document::new(2, 2);
        doc.add_frame(false);
        let preset = ExportPreset {
            format: ExportFormat::SpriteSheet,
            scale: 2,
            ..Default::default()
        };
        let files = export(&doc, "hero", &preset).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "hero.png");
        let sheet = crate::decode_png(&files[0].1).unwrap();
        assert_eq!((sheet.width(), sheet.height()), (8, 4));
        let seq = ExportPreset {
            format: ExportFormat::PngSequence,
            ..Default::default()
        };
        let files = export(&doc, "hero", &seq).unwrap();
        assert_eq!(files[1].0, "hero_001.png");
    }
}

use std::collections::BTreeMap;
use std::io::Cursor;

use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::document::{CelId, Extras, Frame, Layer, Parts, Tag};
use crate::{Bitmap, Document, Rect};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("invalid layer data: {0}")]
    Layers(String),
    #[error("invalid layer metadata: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

pub fn decode_png(bytes: &[u8]) -> Result<Bitmap, Error> {
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Png)?.to_rgba8();
    let (width, height) = image.dimensions();
    Ok(Bitmap::from_rgba(width, height, image.into_raw()).expect("RGBA8 buffer matches its size"))
}

/// Decodes any supported still image (PNG, GIF's first frame, WebP, ...).
pub fn decode_image(bytes: &[u8]) -> Result<Bitmap, Error> {
    let image = image::load_from_memory(bytes)?.to_rgba8();
    let (width, height) = image.dimensions();
    Ok(Bitmap::from_rgba(width, height, image.into_raw()).expect("RGBA8 buffer matches its size"))
}

pub fn encode_png(bitmap: &Bitmap) -> Result<Vec<u8>, Error> {
    let image = RgbaImage::from_raw(bitmap.width(), bitmap.height(), bitmap.data().to_vec())
        .expect("bitmap buffer matches its size");
    let mut out = Vec::new();
    image.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)?;
    Ok(out)
}

// ---- version 1: plain layers, no frames ----

#[derive(Serialize, Deserialize)]
struct LayerInfoV1 {
    name: String,
    visible: bool,
    opacity: f32,
}

#[derive(Serialize, Deserialize)]
struct HeaderV1 {
    version: u32,
    width: u32,
    height: u32,
    active: usize,
    /// Bottom layer first; layer `i` occupies rows `i * height ..` of the strip image.
    layers: Vec<LayerInfoV1>,
}

// ---- version 2: layers × frames of cels, tags, extras ----

#[derive(Serialize, Deserialize)]
struct HeaderV2 {
    version: u32,
    width: u32,
    height: u32,
    active: usize,
    #[serde(default)]
    current: usize,
    frames: Vec<Frame>,
    #[serde(default)]
    tags: Vec<Tag>,
    layers: Vec<Layer>,
    /// Cel ids in the order they are stacked in the image.
    cels: Vec<CelId>,
    #[serde(default)]
    extras: Extras,
    /// Columns of the tileset atlas stored under the cels.
    #[serde(default)]
    tile_columns: u32,
}

#[derive(Deserialize)]
struct VersionOnly {
    version: u32,
}

const TILE_COLUMNS: u32 = 16;

/// Serialises a document as a JSON header plus a PNG with every cel stacked vertically
/// (and the tileset's tiles under them).
pub fn encode_layered(doc: &Document) -> Result<(String, Vec<u8>), Error> {
    let parts = doc.parts();
    let (width, height) = (parts.width, parts.height);
    let order: Vec<CelId> = parts.cels.keys().copied().collect();
    let atlas = parts
        .extras
        .tileset
        .as_ref()
        .filter(|t| !t.tiles.is_empty())
        .map(|t| t.atlas(TILE_COLUMNS));
    let strip_w = width.max(atlas.as_ref().map_or(0, Bitmap::width));
    let strip_h = height * order.len() as u32 + atlas.as_ref().map_or(0, Bitmap::height);
    let mut strip = Bitmap::new(strip_w, strip_h.max(1));
    for (i, id) in order.iter().enumerate() {
        strip.blit(&parts.cels[id], 0, (i as u32 * height) as i32);
    }
    if let Some(atlas) = &atlas {
        strip.blit(atlas, 0, (order.len() as u32 * height) as i32);
    }
    let header = HeaderV2 {
        version: 2,
        width,
        height,
        active: parts.active,
        current: parts.current,
        frames: parts.frames,
        tags: parts.tags,
        layers: parts.layers,
        cels: order,
        extras: parts.extras,
        tile_columns: TILE_COLUMNS,
    };
    Ok((serde_json::to_string_pretty(&header)?, encode_png(&strip)?))
}

/// Inverse of [`encode_layered`] (also reads version 1 files). The returned document has
/// an empty undo history.
pub fn decode_layered(header_json: &str, strip_png: &[u8]) -> Result<Document, Error> {
    let version: VersionOnly = serde_json::from_str(header_json)?;
    let strip = decode_png(strip_png)?;
    match version.version {
        1 => decode_v1(header_json, &strip),
        2 => decode_v2(header_json, &strip),
        v => Err(Error::Layers(format!("unsupported version {v}"))),
    }
}

fn decode_v1(header_json: &str, strip: &Bitmap) -> Result<Document, Error> {
    let header: HeaderV1 = serde_json::from_str(header_json)?;
    let count = header.layers.len() as u32;
    if count == 0
        || header.width == 0
        || header.height == 0
        || strip.width() != header.width
        || strip.height() != header.height * count
    {
        return Err(Error::Layers("layer image does not match its header".into()));
    }
    let mut cels = BTreeMap::new();
    let layers = header
        .layers
        .into_iter()
        .enumerate()
        .map(|(i, info)| {
            let rect = Rect::new(
                0,
                (i as u32 * header.height) as i32,
                header.width as i32,
                header.height as i32,
            );
            let id = i as CelId + 1;
            cels.insert(id, strip.crop(rect));
            Layer {
                id: 1000 + i as u32,
                name: info.name,
                visible: info.visible,
                opacity: info.opacity.clamp(0.0, 1.0),
                cels: vec![id],
                ..Default::default()
            }
        })
        .collect();
    Ok(Document::from_parts(Parts {
        width: header.width,
        height: header.height,
        layers,
        active: header.active,
        frames: vec![Frame::default()],
        current: 0,
        tags: Vec::new(),
        cels,
        extras: Extras::default(),
    }))
}

fn decode_v2(header_json: &str, strip: &Bitmap) -> Result<Document, Error> {
    let mut header: HeaderV2 = serde_json::from_str(header_json)?;
    let (w, h) = (header.width, header.height);
    let n = header.cels.len() as u32;
    if w == 0 || h == 0 || header.layers.is_empty() || strip.width() < w || strip.height() < h * n {
        return Err(Error::Layers("layer image does not match its header".into()));
    }
    let mut cels = BTreeMap::new();
    for (i, id) in header.cels.iter().enumerate() {
        cels.insert(*id, strip.crop(Rect::new(0, (i as u32 * h) as i32, w as i32, h as i32)));
    }
    if let Some(set) = &mut header.extras.tileset {
        let top = (n * h) as i32;
        let atlas = strip.crop(Rect::new(0, top, strip.width() as i32, strip.height() as i32 - top));
        set.load_atlas(&atlas, header.tile_columns.max(1));
    }
    Ok(Document::from_parts(Parts {
        width: w,
        height: h,
        layers: header.layers,
        active: header.active,
        frames: header.frames,
        current: header.current,
        tags: header.tags,
        cels,
        extras: header.extras,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::PlayDirection;
    use crate::tiles::{GridKind, TileRef, Tilemap, Tileset};
    use crate::{BlendMode, ToolKind, ToolSettings};

    fn red() -> ToolSettings {
        ToolSettings {
            color: [255, 0, 0, 255],
            ..Default::default()
        }
    }

    #[test]
    fn png_round_trip_is_exact() {
        let mut bitmap = Bitmap::new(3, 2);
        bitmap.set(0, 0, [1, 2, 3, 255]);
        bitmap.set(2, 1, [200, 100, 50, 7]);
        assert_eq!(decode_png(&encode_png(&bitmap).unwrap()).unwrap(), bitmap);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode_png(b"not a png").is_err());
    }

    #[test]
    fn layered_round_trip() {
        let mut doc = Document::new(4, 2);
        doc.stroke_begin(ToolKind::Pencil, &red(), (0, 0));
        doc.stroke_end();
        doc.add_layer();
        doc.stroke_begin(ToolKind::Pencil, &red(), (3, 1));
        doc.stroke_end();
        doc.set_layer_opacity(1, 0.5);
        doc.set_layer_name(1, "Glow".into());
        doc.set_layer_blend(1, BlendMode::Multiply);
        doc.set_layer_visible(0, false);
        doc.add_frame(true);
        doc.add_linked_frame();
        doc.add_tag(crate::Tag {
            name: "idle".into(),
            from: 0,
            to: 1,
            direction: PlayDirection::PingPong,
            color: [1, 2, 3, 255],
        });

        let (json, png) = encode_layered(&doc).unwrap();
        let loaded = decode_layered(&json, &png).unwrap();
        assert_eq!(loaded.layers(), doc.layers());
        assert_eq!(loaded.frame_count(), 3);
        assert!(loaded.is_linked(0, 1) && loaded.is_linked(0, 2));
        assert_eq!(loaded.tags(), doc.tags());
        assert_eq!(loaded.active(), 1);
        for f in 0..3 {
            assert_eq!(loaded.flatten_frame(f), doc.flatten_frame(f));
        }
        assert!(!loaded.can_undo());
    }

    #[test]
    fn version_1_sidecars_still_load() {
        let json = r#"{"version":1,"width":2,"height":1,"active":1,
            "layers":[{"name":"A","visible":true,"opacity":1.0},{"name":"B","visible":false,"opacity":0.5}]}"#;
        let mut strip = Bitmap::new(2, 2);
        strip.set(0, 0, [9, 9, 9, 255]);
        strip.set(1, 1, [8, 8, 8, 255]);
        let doc = decode_layered(json, &encode_png(&strip).unwrap()).unwrap();
        assert_eq!(doc.layers().len(), 2);
        assert_eq!(doc.layers()[1].name, "B");
        assert!(!doc.layers()[1].visible);
        assert_eq!(doc.layer_pixels(1).get(1, 0), Some([8, 8, 8, 255]));
        assert_eq!(doc.active(), 1);
    }

    #[test]
    fn tilesets_survive_a_round_trip() {
        let mut doc = Document::new(4, 4);
        let mut set = Tileset::new(2, 2);
        set.add_tile(Bitmap::filled(2, 2, [5, 6, 7, 255]));
        doc.edit_extras("Tileset", |e| e.tileset = Some(set));
        let mut map = Tilemap::new(GridKind::Rectangular, 2, 2);
        map.set(1, 1, Some(TileRef::new(0)));
        doc.add_tilemap_layer(map);
        assert_eq!(doc.flatten().get(3, 3), Some([5, 6, 7, 255]));
        let (json, png) = encode_layered(&doc).unwrap();
        let loaded = decode_layered(&json, &png).unwrap();
        assert_eq!(loaded.flatten(), doc.flatten());
    }

    #[test]
    fn layered_rejects_mismatched_strip() {
        let doc = Document::new(4, 2);
        let (json, _) = encode_layered(&doc).unwrap();
        let wrong = encode_png(&Bitmap::new(4, 1)).unwrap();
        assert!(matches!(decode_layered(&json, &wrong), Err(Error::Layers(_))));
    }
}

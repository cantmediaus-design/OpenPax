use std::io::Cursor;

use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::{Bitmap, Document, Layer, Rect};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("invalid layer data: {0}")]
    Layers(String),
    #[error("invalid layer metadata: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn decode_png(bytes: &[u8]) -> Result<Bitmap, Error> {
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Png)?.to_rgba8();
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

#[derive(Serialize, Deserialize)]
struct LayerInfo {
    name: String,
    visible: bool,
    opacity: f32,
}

#[derive(Serialize, Deserialize)]
struct LayeredHeader {
    version: u32,
    width: u32,
    height: u32,
    active: usize,
    /// Bottom layer first; layer `i` occupies rows `i * height ..` of the strip image.
    layers: Vec<LayerInfo>,
}

/// Serialises a document's layers as a JSON header plus a PNG with the layers stacked vertically.
pub fn encode_layered(doc: &Document) -> Result<(String, Vec<u8>), Error> {
    let (width, height) = (doc.width(), doc.height());
    let mut data = Vec::with_capacity(doc.layers().len() * (width * height * 4) as usize);
    for layer in doc.layers() {
        data.extend_from_slice(layer.pixels.data());
    }
    let strip = Bitmap::from_rgba(width, height * doc.layers().len() as u32, data)
        .expect("all layers share the document size");
    let header = LayeredHeader {
        version: 1,
        width,
        height,
        active: doc.active(),
        layers: doc
            .layers()
            .iter()
            .map(|l| LayerInfo {
                name: l.name.clone(),
                visible: l.visible,
                opacity: l.opacity,
            })
            .collect(),
    };
    Ok((serde_json::to_string_pretty(&header)?, encode_png(&strip)?))
}

/// Inverse of [`encode_layered`]. The returned document has an empty undo history.
pub fn decode_layered(header_json: &str, strip_png: &[u8]) -> Result<Document, Error> {
    let header: LayeredHeader = serde_json::from_str(header_json)?;
    let strip = decode_png(strip_png)?;
    let count = header.layers.len() as u32;
    if header.version != 1 {
        return Err(Error::Layers(format!(
            "unsupported version {}",
            header.version
        )));
    }
    if count == 0
        || header.width == 0
        || header.height == 0
        || strip.width() != header.width
        || strip.height() != header.height * count
    {
        return Err(Error::Layers(
            "layer image does not match its header".into(),
        ));
    }
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
            let pixels = Bitmap::from_rgba(header.width, header.height, strip.patch(rect))
                .expect("patch matches rect size");
            Layer {
                name: info.name,
                visible: info.visible,
                opacity: info.opacity.clamp(0.0, 1.0),
                pixels,
            }
        })
        .collect();
    Ok(Document::from_layers(
        header.width,
        header.height,
        layers,
        header.active,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToolKind, ToolSettings};

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
        let red = ToolSettings {
            color: [255, 0, 0, 255],
            ..Default::default()
        };
        doc.stroke_begin(ToolKind::Pencil, red, (0, 0));
        doc.stroke_end();
        doc.add_layer();
        doc.stroke_begin(ToolKind::Pencil, red, (3, 1));
        doc.stroke_end();
        doc.set_layer_opacity(1, 0.5);
        doc.set_layer_name(1, "Glow".into());
        doc.set_layer_visible(0, false);

        let (json, png) = encode_layered(&doc).unwrap();
        let loaded = decode_layered(&json, &png).unwrap();
        assert_eq!(loaded.layers(), doc.layers());
        assert_eq!(loaded.active(), 1);
        assert_eq!(loaded.flatten(), doc.flatten());
        assert!(!loaded.can_undo());
    }

    #[test]
    fn layered_rejects_mismatched_strip() {
        let doc = Document::new(4, 2);
        let (json, _) = encode_layered(&doc).unwrap();
        let wrong = encode_png(&Bitmap::new(4, 3)).unwrap();
        assert!(matches!(
            decode_layered(&json, &wrong),
            Err(Error::Layers(_))
        ));
    }
}

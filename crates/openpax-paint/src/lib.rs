//! Headless pixel-art editing: bitmaps, layered and animated documents, tools, filters,
//! palettes, tiles, file formats and undo history.
//!
//! Nothing here depends on a UI toolkit, so every tool can be unit tested and the same code
//! powers the command-line tool.

mod bitmap;
pub mod blend;
pub mod color;
mod document;
pub mod effects;
mod io;
pub mod mask;
mod palette;
pub mod raster;
pub mod resample;
pub mod tiles;
mod tools;

pub use bitmap::{Bitmap, Rect, Rgba, TRANSPARENT};
pub use blend::BlendMode;
pub use document::{
    Anchor, CelId, Document, Extras, Floating, Frame, Guide, Layer, LayerId, LayerKind, Parts,
    PlayDirection, RenderOptions, SavedSelection, Scope, Tag, Transform,
};
pub use io::{Error, decode_image, decode_layered, decode_png, encode_layered, encode_png};
pub use mask::{Mask, SelectMode};
pub use palette::Palette;
pub use resample::ScaleMethod;
pub use tools::{BrushShape, Symmetry, ToolKind, ToolSettings, colors_match, region_mask};

//! Headless pixel-art editing: bitmaps, layered documents, tools, and undo history.
//!
//! Nothing here depends on a UI toolkit, so every tool can be unit tested.

mod bitmap;
mod document;
mod io;
mod palette;
mod tools;

pub use bitmap::{Bitmap, Rect, Rgba, TRANSPARENT};
pub use document::{Document, Layer};
pub use io::{Error, decode_layered, decode_png, encode_layered, encode_png};
pub use palette::Palette;
pub use tools::{ToolKind, ToolSettings};

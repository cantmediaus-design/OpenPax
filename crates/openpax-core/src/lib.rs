//! Headless model of a Minecraft: Java Edition resource pack.
//!
//! This crate has no UI dependencies. It knows how to open, create, index, and export packs,
//! and takes care never to drop JSON fields it does not understand.

mod archive;
mod error;
mod file_ops;
mod formats;
mod pack;
mod pack_meta;
mod progress;
pub mod remix;
mod resource_location;
mod texture_meta;
pub mod vanilla;

pub use archive::{export_zip, import_zip};
pub use error::{Error, Result};
pub use file_ops::TRASH_DIR;
pub use formats::{RESOURCE_FORMATS, version_for_format};
pub use pack::{AssetEntry, AssetIndex, AssetKind, Pack, SIDECAR_DIR};
pub use pack_meta::{PackMeta, PackSection};
pub use progress::Progress;
pub use resource_location::ResourceLocation;
pub use texture_meta::{Animation, Frame, TextureMeta};

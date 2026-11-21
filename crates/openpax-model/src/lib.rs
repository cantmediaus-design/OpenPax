//! Headless model of a Minecraft: Java Edition block/item model (`models/**/*.json`).
//!
//! Covers the JSON format (keeping fields it does not understand), an undoable document,
//! mesh generation for rendering, and ray picking. Nothing here depends on a UI or GPU.

mod document;
mod geometry;
mod json;
mod pretty;

pub use document::ModelDocument;
pub use geometry::{FaceMesh, Hit, Vertex, build_mesh, element_corners, pick};
pub use json::{Axis, Direction, Element, Face, ModelFile, Rotation};
pub use pretty::to_pretty_json;

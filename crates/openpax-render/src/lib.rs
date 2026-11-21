//! A small wgpu renderer for the OpenPax 3D viewport.
//!
//! It draws textured quads and coloured lines into an off-screen texture with its own depth
//! buffer, so the result can be shown by any UI toolkit as a plain image. It knows nothing
//! about Minecraft models; the app turns those into [`Quad`]s and [`LineVertex`]es.

mod camera;
mod renderer;

pub use camera::Camera;
pub use renderer::{COLOR_FORMAT, LineVertex, MeshVertex, Quad, Renderer, TextureImage};

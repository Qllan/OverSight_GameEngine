//! OverSight 2D: a 2D game runtime for games written in C++, C# or Rust.
//!
//! The engine draws sprites, shapes, text and tile maps with a camera,
//! sorts them by layer and depth (y-sorting for top-down and isometric
//! views), reads keyboard and mouse input, moves boxes through tile maps
//! and plays sounds. C++ and C# use it through the C API in `ffi`
//! (include/oversight.h).

pub mod camera;
pub mod engine;
pub mod render;
pub mod text;
pub mod texture;
pub mod types;

pub use camera::Camera;
pub use engine::{Engine, DEFAULT_FONT_ID};
pub use types::{Color, Rect, Space, Sprite, Vec2};

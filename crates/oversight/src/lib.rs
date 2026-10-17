//! OverSight 2D: a 2D game runtime for games written in C++, C# or Rust.
//!
//! The engine draws sprites, shapes, text and tile maps with a camera,
//! sorts them by layer and depth (y-sorting for top-down and isometric
//! views), reads keyboard and mouse input, moves boxes through tile maps
//! and plays sounds. C++ and C# use it through the C API in `ffi`
//! (include/oversight.h).

pub mod anim;
#[cfg(feature = "audio")]
pub mod audio;
pub mod camera;
pub mod collision;
pub mod engine;
pub mod ffi;
pub mod input;
pub mod render;
pub mod text;
pub mod texture;
pub mod tiled;
pub mod tilemap;
pub mod time;
pub mod types;
#[cfg(feature = "window")]
pub mod window;

pub use anim::{AnimMode, AnimPlayer, Animation, SpriteSheet};
pub use camera::Camera;
pub use collision::{Contacts, MoveResult, RayHit};
pub use engine::{Engine, InputEvent, DEFAULT_FONT_ID};
pub use input::{Input, Key, MouseButton};
pub use tilemap::{
    LayerDraw, MapObject, ObjectLayer, Orientation, Properties, Property, TileLayer, Tilemap,
    Tileset,
};
pub use time::Time;
pub use types::{Color, Rect, Space, Sprite, Vec2};
#[cfg(feature = "window")]
pub use window::WindowConfig;

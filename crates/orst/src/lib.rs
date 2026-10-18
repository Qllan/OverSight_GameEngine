//! ORST Game Engine: a 2D game runtime for games written in C++, C# or Rust.
//!
//! The engine draws sprites, shapes, text and tile maps with a camera,
//! sorts them by layer and depth (y-sorting for top-down and isometric
//! views), reads keyboard and mouse input, moves boxes through tile maps
//! and plays sounds. C++ and C# use it through the C API in `ffi`
//! (include/orst.h).

pub mod actions;
pub mod anim;
pub mod asset_db;
#[cfg(feature = "audio")]
pub mod audio;
pub mod camera;
pub mod collision;
pub mod collisions;
pub mod components;
pub mod debug;
pub mod ecs;
pub mod engine;
pub mod ffi;
pub mod gamepad;
#[cfg(feature = "gpu")]
pub mod gpu;
/// The wgpu the engine draws with, for hosts that share its device
#[cfg(feature = "gpu")]
pub use wgpu;
pub mod hotreload;
pub mod input;
pub mod light;
pub mod locale;
pub mod particles;
pub mod pathfind;
#[cfg(feature = "physics")]
pub mod physics;
#[cfg(not(feature = "physics"))]
#[path = "physics_off.rs"]
pub mod physics;
pub mod post;
pub mod prefab;
pub mod render;
pub mod rng;
pub mod scene_file;
pub mod scenes;
pub mod schema;
pub mod shapes;
pub mod skeleton;
pub mod strings;
pub mod text;
pub mod texture;
pub mod tiled;
pub mod tilemap;
pub mod time;
pub mod tween;
pub mod types;
pub mod ui;
#[cfg(feature = "window")]
pub mod window;

pub use actions::{Actions, Binding};
pub use anim::{AnimMode, AnimPlayer, Animation, SpriteSheet};
pub use camera::Camera;
pub use collision::{Contacts, MoveResult, RayHit};
pub use engine::{Engine, InputEvent, DEFAULT_FONT_ID};
pub use gamepad::{GamepadAxis, GamepadButton, Gamepads};
pub use input::{Input, Key, MouseButton};
pub use particles::{Emitter, EmitterConfig};
pub use scenes::{Scenes, Transition};
pub use tilemap::{
    LayerDraw, MapObject, ObjectLayer, Orientation, Properties, Property, TileLayer, Tilemap,
    Tileset,
};
pub use time::Time;
pub use tween::{ease, Ease, Repeat, Tween};
pub use types::{Color, Rect, Space, Sprite, Vec2};
#[cfg(feature = "window")]
pub use window::WindowConfig;

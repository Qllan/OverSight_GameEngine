//! Components the engine itself understands, and the systems that use
//! them: motion (velocity, with tile collisions for bodies) and drawing
//! sprites of entities. Every engine registers them first, so their ids
//! are fixed: games and the C API can use the constants below.

use crate::collision::Contacts;
use crate::ecs::World;
use crate::types::{Color, Rect, Vec2};

pub const TRANSFORM: u32 = 1;
pub const VELOCITY: u32 = 2;
pub const SPRITE_RENDERER: u32 = 3;
pub const BODY: u32 = 4;

/// Where an entity is
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub pos: Vec2,
    /// Radians, clockwise
    pub rotation: f32,
    pub scale: Vec2,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            pos: Vec2::default(),
            rotation: 0.0,
            scale: Vec2::new(1.0, 1.0),
        }
    }
}

/// How fast it moves: units and radians per second
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Velocity {
    pub linear: Vec2,
    pub angular: f32,
}

/// A sprite drawn at the entity's transform
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteRenderer {
    pub texture: u32,
    /// Texture region; zero size: the whole texture
    pub src: Rect,
    /// Point of the sprite placed at the position (and rotated around)
    pub origin: Vec2,
    pub tint: Color,
    pub layer: i32,
    pub flip_x: u8,
    pub flip_y: u8,
    pub hidden: u8,
    /// Order within the layer by y (top-down games)
    pub y_sort: u8,
}

impl Default for SpriteRenderer {
    fn default() -> Self {
        SpriteRenderer {
            texture: 0,
            src: Rect::default(),
            origin: Vec2::default(),
            tint: Color::WHITE,
            layer: 0,
            flip_x: 0,
            flip_y: 0,
            hidden: 0,
            y_sort: 0,
        }
    }
}

/// A box that collides with a tile map while the entity moves
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Body {
    /// Relative to the position: (-5, -14, 10, 14) is a 10x14 box standing
    /// on the position
    pub bounds: Rect,
    /// The map it collides with; 0: none, it moves freely
    pub tilemap: u32,
    /// What it stopped at or rests against after the last move
    pub contacts: Contacts,
}

/// Register the built-in components so they get the ids above
pub fn register(world: &mut World) {
    let size = |n: usize| n as u32;
    let ids = [
        world.component("Transform", size(std::mem::size_of::<Transform>()), 4),
        world.component("Velocity", size(std::mem::size_of::<Velocity>()), 4),
        world.component(
            "SpriteRenderer",
            size(std::mem::size_of::<SpriteRenderer>()),
            4,
        ),
        world.component("Body", size(std::mem::size_of::<Body>()), 4),
    ];
    let ids: Vec<u32> = ids
        .into_iter()
        .map(|r| r.expect("built-in components"))
        .collect();
    debug_assert_eq!(
        ids,
        [TRANSFORM, VELOCITY, SPRITE_RENDERER, BODY],
        "built-ins come first"
    );
}

/// The default value of a built-in component as bytes (scale 1, white
/// tint), for setting one without data; None for other components
pub fn default_bytes(component: u32) -> Option<Vec<u8>> {
    fn bytes<T: Copy>(v: T) -> Vec<u8> {
        // Safety: plain repr(C) data
        unsafe { std::slice::from_raw_parts(&v as *const T as *const u8, std::mem::size_of::<T>()) }
            .to_vec()
    }
    match component {
        TRANSFORM => Some(bytes(Transform::default())),
        VELOCITY => Some(bytes(Velocity::default())),
        SPRITE_RENDERER => Some(bytes(SpriteRenderer::default())),
        BODY => Some(bytes(Body::default())),
        _ => None,
    }
}

/// Copy a component out of the world
pub fn read<T: Copy>(world: &World, entity: u64, component: u32) -> Option<T> {
    let bytes = world.get(entity, component)?;
    debug_assert_eq!(bytes.len(), std::mem::size_of::<T>());
    // Safety: the built-ins are plain repr(C) data of exactly this size
    Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const T) })
}

pub fn write<T: Copy>(world: &mut World, entity: u64, component: u32, value: &T) -> bool {
    let bytes = unsafe {
        std::slice::from_raw_parts(value as *const T as *const u8, std::mem::size_of::<T>())
    };
    world.set(entity, component, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_the_c_header() {
        // oversight.h and the C# structs rely on these
        assert_eq!(std::mem::size_of::<Transform>(), 20);
        assert_eq!(std::mem::size_of::<Velocity>(), 12);
        assert_eq!(std::mem::size_of::<SpriteRenderer>(), 40);
        assert_eq!(std::mem::size_of::<Body>(), 24);
        let mut w = World::new();
        register(&mut w);
        assert_eq!(w.component_id("Body"), Some(BODY));
    }
}

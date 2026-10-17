//! Components the engine itself understands, and the systems that use
//! them: motion (velocity, with tile collisions for bodies) and drawing
//! sprites of entities. Every engine registers them first, so their ids
//! are fixed: games and the C API can use the constants below.

use crate::collision::Contacts;
use crate::ecs::World;
use crate::shapes::{rotate, Shape};
use crate::types::{Color, Rect, Vec2};

pub const TRANSFORM: u32 = 1;
pub const VELOCITY: u32 = 2;
pub const SPRITE_RENDERER: u32 = 3;
pub const BODY: u32 = 4;
pub const COLLIDER: u32 = 5;
pub const RIGIDBODY: u32 = 6;
pub const LIGHT: u32 = 7;

pub const BODY_DYNAMIC: u32 = 0;
pub const BODY_FIXED: u32 = 1;
pub const BODY_KINEMATIC: u32 = 2;

pub const COLLIDER_BOX: u32 = 0;
pub const COLLIDER_CIRCLE: u32 = 1;
pub const COLLIDER_POLYGON: u32 = 2;

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

/// A shape that collides with other colliders. It moves, turns and
/// scales with the entity's Transform.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Collider {
    /// COLLIDER_BOX, COLLIDER_CIRCLE or COLLIDER_POLYGON
    pub kind: u32,
    /// A polygon made with create_polygon (COLLIDER_POLYGON)
    pub polygon: u32,
    /// Centre of the shape from the position
    pub offset: Vec2,
    /// Box width and height
    pub size: Vec2,
    pub radius: f32,
    /// What it is (bits)
    pub layer: u32,
    /// What it collides with: two colliders meet when each one's mask has
    /// a bit of the other's layer
    pub mask: u32,
    /// Reports overlaps but never pushes
    pub trigger: u8,
    pub _pad: [u8; 3],
}

impl Default for Collider {
    fn default() -> Self {
        Collider {
            kind: COLLIDER_BOX,
            polygon: 0,
            offset: Vec2::default(),
            size: Vec2::default(),
            radius: 0.0,
            layer: 1,
            mask: u32::MAX,
            trigger: 0,
            _pad: [0; 3],
        }
    }
}

impl Collider {
    /// Whether the two can collide at all
    pub fn meets(&self, other: &Collider) -> bool {
        self.mask & other.layer != 0 && other.mask & self.layer != 0
    }

    /// The shape in the world for this transform; None for a polygon id
    /// that does not exist
    pub fn shape(
        &self,
        t: &Transform,
        polygon: impl Fn(u32) -> Option<Vec<Vec2>>,
    ) -> Option<Shape> {
        let off = rotate(
            Vec2::new(self.offset.x * t.scale.x, self.offset.y * t.scale.y),
            t.rotation,
        );
        let center = Vec2::new(t.pos.x + off.x, t.pos.y + off.y);
        match self.kind {
            COLLIDER_CIRCLE => Some(Shape::Circle {
                center,
                radius: self.radius * t.scale.x.abs().max(t.scale.y.abs()),
            }),
            COLLIDER_POLYGON => {
                polygon(self.polygon).map(|p| Shape::polygon(&p, center, t.rotation, t.scale))
            }
            _ => Some(Shape::rect(
                center,
                Vec2::new(self.size.x * t.scale.x.abs(), self.size.y * t.scale.y.abs()),
                t.rotation,
            )),
        }
    }
}

/// A body with mass for the physics step (with a Transform and a
/// Collider). Dynamic bodies fall and bounce; fixed ones are walls;
/// kinematic ones go where the game puts them (or by their Velocity) and
/// push dynamic ones out of the way.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RigidBody {
    /// BODY_DYNAMIC, BODY_FIXED or BODY_KINEMATIC
    pub kind: u32,
    /// Mass per square unit of the collider
    pub density: f32,
    pub friction: f32,
    /// Bounciness, 0..1
    pub restitution: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub gravity_scale: f32,
    /// Never turns (characters)
    pub fixed_rotation: u8,
    /// Fast: checked along its whole move so it does not pass through
    /// thin walls (continuous collision detection)
    pub bullet: u8,
    pub _pad: [u8; 2],
}

impl Default for RigidBody {
    fn default() -> Self {
        RigidBody {
            kind: BODY_DYNAMIC,
            density: 1.0,
            friction: 0.5,
            restitution: 0.0,
            linear_damping: 0.0,
            angular_damping: 0.0,
            gravity_scale: 1.0,
            fixed_rotation: 0,
            bullet: 0,
            _pad: [0; 2],
        }
    }
}

/// A light at the entity's position, seen by targets (or the screen)
/// with the lighting effect. Colliders on the shadow layers and solid
/// cells of shadow maps cast shadows.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// From the position (not turned with the entity)
    pub offset: Vec2,
    pub color: Color,
    pub intensity: f32,
    /// World units it reaches
    pub radius: f32,
    /// How high above the ground it is, as a share of the radius: low
    /// lights bring out normal maps more
    pub height: f32,
    /// Shadow edge blur in pixels (0: hard)
    pub softness: f32,
    pub shadows: u8,
    pub _pad: [u8; 3],
}

impl Default for Light {
    fn default() -> Self {
        Light {
            offset: Vec2::default(),
            color: Color::WHITE,
            intensity: 1.0,
            radius: 160.0,
            height: 0.5,
            softness: 4.0,
            shadows: 1,
            _pad: [0; 3],
        }
    }
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
        world.component("Collider", size(std::mem::size_of::<Collider>()), 4),
        world.component("RigidBody", size(std::mem::size_of::<RigidBody>()), 4),
        world.component("Light", size(std::mem::size_of::<Light>()), 4),
    ];
    let ids: Vec<u32> = ids
        .into_iter()
        .map(|r| r.expect("built-in components"))
        .collect();
    debug_assert_eq!(
        ids,
        [
            TRANSFORM,
            VELOCITY,
            SPRITE_RENDERER,
            BODY,
            COLLIDER,
            RIGIDBODY,
            LIGHT
        ],
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
        COLLIDER => Some(bytes(Collider::default())),
        RIGIDBODY => Some(bytes(RigidBody::default())),
        LIGHT => Some(bytes(Light::default())),
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
        assert_eq!(std::mem::size_of::<Collider>(), 40);
        assert_eq!(std::mem::size_of::<RigidBody>(), 32);
        assert_eq!(std::mem::size_of::<Light>(), 32);
        let mut w = World::new();
        register(&mut w);
        assert_eq!(w.component_id("Body"), Some(BODY));
    }
}

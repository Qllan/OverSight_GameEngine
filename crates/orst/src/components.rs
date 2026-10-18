//! Components the engine itself understands, and the systems that use
//! them: motion (velocity, with tile collisions for bodies) and drawing
//! sprites of entities. Every engine registers them first, so their ids
//! are fixed: games and the C API can use the constants below.

use crate::collision::Contacts;
use crate::ecs::World;
use crate::schema::{Field, FieldType, FLAG_ANGLE, FLAG_LAYERS};
use crate::shapes::{rotate, Shape};
use crate::types::{Color, Rect, Vec2};

pub const TRANSFORM: u32 = 1;
pub const VELOCITY: u32 = 2;
pub const SPRITE_RENDERER: u32 = 3;
pub const BODY: u32 = 4;
pub const COLLIDER: u32 = 5;
pub const RIGIDBODY: u32 = 6;
pub const LIGHT: u32 = 7;
pub const NAME: u32 = 8;
pub const SPRITE_ANIMATOR: u32 = 9;
pub const CAMERA_2D: u32 = 10;
pub const PARTICLE_EMITTER: u32 = 11;
pub const TILEMAP_RENDERER: u32 = 12;
pub const PREFAB: u32 = 13;
/// Ids up to this one are the engine's own
pub const LAST_BUILTIN: u32 = PREFAB;

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

/// What an entity is called: the editor's hierarchy shows it and games
/// find entities by it. Up to 64 bytes of UTF-8, the rest zeros.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Name {
    pub text: [u8; NAME_BYTES],
}

pub const NAME_BYTES: usize = 64;

impl Name {
    /// Cut to 64 bytes at a character boundary
    pub fn new(name: &str) -> Name {
        Name {
            text: crate::schema::text_bytes(name),
        }
    }

    pub fn as_str(&self) -> &str {
        crate::schema::text_str(&self.text)
    }
}

impl Default for Name {
    fn default() -> Self {
        Name {
            text: [0; NAME_BYTES],
        }
    }
}

/// Plays frames of a sprite sheet on the entity's SpriteRenderer: frame
/// `first + i` of a grid of `frame_w` x `frame_h` cells sets its `src`.
/// Engine::step advances it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteAnimator {
    pub frame_w: u32,
    pub frame_h: u32,
    /// Frames in a row of the sheet; 0: as many as fit the texture
    pub columns: u32,
    pub first: u32,
    pub count: u32,
    pub fps: f32,
    /// ANIM_LOOP, ANIM_ONCE or ANIM_PING_PONG (as AnimMode)
    pub mode: u32,
    /// Seconds into the clip; Once clips stop at the end
    pub time: f32,
    pub playing: u8,
    pub _pad: [u8; 3],
}

impl Default for SpriteAnimator {
    fn default() -> Self {
        SpriteAnimator {
            frame_w: 32,
            frame_h: 32,
            columns: 0,
            first: 0,
            count: 1,
            fps: 10.0,
            mode: 0,
            time: 0.0,
            playing: 1,
            _pad: [0; 3],
        }
    }
}

impl SpriteAnimator {
    /// The frame shown now, counted from `first`
    pub fn frame(&self) -> u32 {
        let count = self.count.max(1);
        let n = (self.time.max(0.0) * self.fps.max(0.0)).floor() as u64;
        match self.mode {
            1 => n.min(count as u64 - 1) as u32,
            2 if count > 1 => {
                // 0 1 2 3 2 1 0 1 ...
                let period = 2 * (count as u64 - 1);
                let k = n % period;
                (if k < count as u64 { k } else { period - k }) as u32
            }
            _ => (n % count as u64) as u32,
        }
    }

    /// Where the current frame is in a texture `texture_w` wide
    pub fn src(&self, texture_w: u32) -> Rect {
        let columns = match self.columns {
            0 => (texture_w / self.frame_w.max(1)).max(1),
            c => c,
        };
        let i = self.first + self.frame();
        Rect::new(
            ((i % columns) * self.frame_w) as f32,
            ((i / columns) * self.frame_h) as f32,
            self.frame_w as f32,
            self.frame_h as f32,
        )
    }
}

/// Makes the entity the camera: Engine::step puts the view's centre at its
/// position, turned with it, at this zoom. The first `current` one wins.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera2D {
    /// Screen pixels per world unit
    pub zoom: f32,
    pub current: u8,
    pub _pad: [u8; 3],
}

impl Default for Camera2D {
    fn default() -> Self {
        Camera2D {
            zoom: 1.0,
            current: 1,
            _pad: [0; 3],
        }
    }
}

/// Particles that follow the entity: Engine::step runs an emitter at its
/// position, its direction turned with the entity, and draw_entities
/// draws them. Living particles are not part of the world (they are
/// looks, not game state), so a restore does not bring them back.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleEmitter {
    pub config: crate::particles::EmitterConfig,
    pub emitting: u8,
    pub _pad: [u8; 3],
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        ParticleEmitter {
            config: Default::default(),
            emitting: 1,
            _pad: [0; 3],
        }
    }
}

/// Draws a tile map: every tile layer of `map`, the first at draw layer
/// `layer` and each next one above it. The map lies where its file puts
/// it (cell 0, 0 at the world origin), as collisions and paths see it, so
/// the entity's Transform does not move it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TilemapRenderer {
    pub map: u32,
    pub layer: i32,
    pub tint: Color,
    /// Sort tiles with sprites by the bottom of their cell
    pub y_sort: u8,
    pub hidden: u8,
    pub _pad: [u8; 2],
}

impl Default for TilemapRenderer {
    fn default() -> Self {
        TilemapRenderer {
            map: 0,
            layer: 0,
            tint: Color::WHITE,
            y_sort: 0,
            hidden: 0,
            _pad: [0; 2],
        }
    }
}

/// Marks an entity as an instance of a prefab (prefab.rs): the scene file
/// stores only what the instance changes. It has no fields, so the
/// inspector and scene files treat it apart, like Name.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PrefabInstance {
    /// The prefab's id in `Engine::prefabs`
    pub prefab: u32,
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
        world.component("Name", size(std::mem::size_of::<Name>()), 1),
        world.component(
            "SpriteAnimator",
            size(std::mem::size_of::<SpriteAnimator>()),
            4,
        ),
        world.component("Camera2D", size(std::mem::size_of::<Camera2D>()), 4),
        world.component(
            "ParticleEmitter",
            size(std::mem::size_of::<ParticleEmitter>()),
            4,
        ),
        world.component(
            "TilemapRenderer",
            size(std::mem::size_of::<TilemapRenderer>()),
            4,
        ),
        world.component("Prefab", size(std::mem::size_of::<PrefabInstance>()), 4),
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
            LIGHT,
            NAME,
            SPRITE_ANIMATOR,
            CAMERA_2D,
            PARTICLE_EMITTER,
            TILEMAP_RENDERER,
            PREFAB
        ],
        "built-ins come first"
    );
    for (id, fields) in builtin_fields() {
        world.set_fields(id, fields).expect("built-in fields");
        if let Some(s) = world.storage_mut(id) {
            s.defaults = default_bytes(id).unwrap_or_default();
        }
    }
}

/// What the editor shows of the built-ins. Body contacts are left out:
/// the engine works them out every move.
fn builtin_fields() -> Vec<(u32, Vec<Field>)> {
    use std::mem::offset_of;
    use FieldType as T;
    let at = |offset: usize| offset as u32;
    vec![
        (
            TRANSFORM,
            vec![
                Field::new("pos", T::Vec2, at(offset_of!(Transform, pos))),
                Field::new("rotation", T::F32, at(offset_of!(Transform, rotation)))
                    .flags(FLAG_ANGLE),
                Field::new("scale", T::Vec2, at(offset_of!(Transform, scale))),
            ],
        ),
        (
            VELOCITY,
            vec![
                Field::new("linear", T::Vec2, at(offset_of!(Velocity, linear))),
                Field::new("angular", T::F32, at(offset_of!(Velocity, angular))).flags(FLAG_ANGLE),
            ],
        ),
        (
            SPRITE_RENDERER,
            vec![
                Field::new(
                    "texture",
                    T::Texture,
                    at(offset_of!(SpriteRenderer, texture)),
                ),
                Field::new("src", T::Rect, at(offset_of!(SpriteRenderer, src))),
                Field::new("origin", T::Vec2, at(offset_of!(SpriteRenderer, origin))),
                Field::new("tint", T::Color, at(offset_of!(SpriteRenderer, tint))),
                Field::new("layer", T::I32, at(offset_of!(SpriteRenderer, layer))),
                Field::new("flip_x", T::Bool, at(offset_of!(SpriteRenderer, flip_x))),
                Field::new("flip_y", T::Bool, at(offset_of!(SpriteRenderer, flip_y))),
                Field::new("hidden", T::Bool, at(offset_of!(SpriteRenderer, hidden))),
                Field::new("y_sort", T::Bool, at(offset_of!(SpriteRenderer, y_sort))),
            ],
        ),
        (
            BODY,
            vec![
                Field::new("bounds", T::Rect, at(offset_of!(Body, bounds))),
                Field::new("tilemap", T::Tilemap, at(offset_of!(Body, tilemap))),
            ],
        ),
        (
            COLLIDER,
            vec![
                Field::new("kind", T::U32, at(offset_of!(Collider, kind)))
                    .options(&["Box", "Circle", "Polygon"]),
                Field::new("polygon", T::Polygon, at(offset_of!(Collider, polygon))),
                Field::new("offset", T::Vec2, at(offset_of!(Collider, offset))),
                Field::new("size", T::Vec2, at(offset_of!(Collider, size))),
                Field::new("radius", T::F32, at(offset_of!(Collider, radius))),
                Field::new("layer", T::U32, at(offset_of!(Collider, layer))).flags(FLAG_LAYERS),
                Field::new("mask", T::U32, at(offset_of!(Collider, mask))).flags(FLAG_LAYERS),
                Field::new("trigger", T::Bool, at(offset_of!(Collider, trigger))),
            ],
        ),
        (
            RIGIDBODY,
            vec![
                Field::new("kind", T::U32, at(offset_of!(RigidBody, kind))).options(&[
                    "Dynamic",
                    "Fixed",
                    "Kinematic",
                ]),
                Field::new("density", T::F32, at(offset_of!(RigidBody, density))),
                Field::new("friction", T::F32, at(offset_of!(RigidBody, friction))).range(0.0, 1.0),
                Field::new(
                    "restitution",
                    T::F32,
                    at(offset_of!(RigidBody, restitution)),
                )
                .range(0.0, 1.0),
                Field::new(
                    "linear_damping",
                    T::F32,
                    at(offset_of!(RigidBody, linear_damping)),
                ),
                Field::new(
                    "angular_damping",
                    T::F32,
                    at(offset_of!(RigidBody, angular_damping)),
                ),
                Field::new(
                    "gravity_scale",
                    T::F32,
                    at(offset_of!(RigidBody, gravity_scale)),
                ),
                Field::new(
                    "fixed_rotation",
                    T::Bool,
                    at(offset_of!(RigidBody, fixed_rotation)),
                ),
                Field::new("bullet", T::Bool, at(offset_of!(RigidBody, bullet))),
            ],
        ),
        (
            LIGHT,
            vec![
                Field::new("offset", T::Vec2, at(offset_of!(Light, offset))),
                Field::new("color", T::Color, at(offset_of!(Light, color))),
                Field::new("intensity", T::F32, at(offset_of!(Light, intensity))),
                Field::new("radius", T::F32, at(offset_of!(Light, radius))),
                Field::new("height", T::F32, at(offset_of!(Light, height))).range(0.0, 1.0),
                Field::new("softness", T::F32, at(offset_of!(Light, softness))),
                Field::new("shadows", T::Bool, at(offset_of!(Light, shadows))),
            ],
        ),
        (NAME, vec![Field::new("text", T::Text, 0)]),
        (
            SPRITE_ANIMATOR,
            vec![
                Field::new("frame_w", T::U32, at(offset_of!(SpriteAnimator, frame_w))),
                Field::new("frame_h", T::U32, at(offset_of!(SpriteAnimator, frame_h))),
                Field::new("columns", T::U32, at(offset_of!(SpriteAnimator, columns))),
                Field::new("first", T::U32, at(offset_of!(SpriteAnimator, first))),
                Field::new("count", T::U32, at(offset_of!(SpriteAnimator, count))),
                Field::new("fps", T::F32, at(offset_of!(SpriteAnimator, fps))),
                Field::new("mode", T::U32, at(offset_of!(SpriteAnimator, mode)))
                    .options(&["Loop", "Once", "PingPong"]),
                Field::new("time", T::F32, at(offset_of!(SpriteAnimator, time))),
                Field::new("playing", T::Bool, at(offset_of!(SpriteAnimator, playing))),
            ],
        ),
        (
            CAMERA_2D,
            vec![
                Field::new("zoom", T::F32, at(offset_of!(Camera2D, zoom))),
                Field::new("current", T::Bool, at(offset_of!(Camera2D, current))),
            ],
        ),
        (PARTICLE_EMITTER, emitter_fields()),
        (
            TILEMAP_RENDERER,
            vec![
                Field::new("map", T::Tilemap, at(offset_of!(TilemapRenderer, map))),
                Field::new("layer", T::I32, at(offset_of!(TilemapRenderer, layer))),
                Field::new("tint", T::Color, at(offset_of!(TilemapRenderer, tint))),
                Field::new("y_sort", T::Bool, at(offset_of!(TilemapRenderer, y_sort))),
                Field::new("hidden", T::Bool, at(offset_of!(TilemapRenderer, hidden))),
            ],
        ),
    ]
}

fn emitter_fields() -> Vec<Field> {
    use crate::particles::EmitterConfig as C;
    use std::mem::offset_of;
    use FieldType as T;
    let base = offset_of!(ParticleEmitter, config);
    let at = |offset: usize| (base + offset) as u32;
    vec![
        Field::new(
            "emitting",
            T::Bool,
            offset_of!(ParticleEmitter, emitting) as u32,
        ),
        Field::new("texture", T::Texture, at(offset_of!(C, texture))),
        Field::new("src", T::Rect, at(offset_of!(C, src))),
        Field::new("rate", T::F32, at(offset_of!(C, rate))),
        Field::new("max", T::U32, at(offset_of!(C, max))),
        Field::new("life_min", T::F32, at(offset_of!(C, life_min))),
        Field::new("life_max", T::F32, at(offset_of!(C, life_max))),
        Field::new("speed_min", T::F32, at(offset_of!(C, speed_min))),
        Field::new("speed_max", T::F32, at(offset_of!(C, speed_max))),
        Field::new("angle", T::F32, at(offset_of!(C, angle))).flags(FLAG_ANGLE),
        Field::new("spread", T::F32, at(offset_of!(C, spread))).flags(FLAG_ANGLE),
        Field::new("gravity", T::Vec2, at(offset_of!(C, gravity))),
        Field::new("drag", T::F32, at(offset_of!(C, drag))).range(0.0, 10.0),
        Field::new("size_start", T::F32, at(offset_of!(C, size_start))),
        Field::new("size_end", T::F32, at(offset_of!(C, size_end))),
        Field::new("spin_min", T::F32, at(offset_of!(C, spin_min))).flags(FLAG_ANGLE),
        Field::new("spin_max", T::F32, at(offset_of!(C, spin_max))).flags(FLAG_ANGLE),
        Field::new("color_start", T::Color, at(offset_of!(C, color_start))),
        Field::new("color_end", T::Color, at(offset_of!(C, color_end))),
        Field::new("area", T::Vec2, at(offset_of!(C, area))),
        Field::new("layer", T::I32, at(offset_of!(C, layer))),
        Field::new("seed", T::U32, at(offset_of!(C, seed))),
    ]
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
        NAME => Some(bytes(Name::default())),
        SPRITE_ANIMATOR => Some(bytes(SpriteAnimator::default())),
        CAMERA_2D => Some(bytes(Camera2D::default())),
        PARTICLE_EMITTER => Some(bytes(ParticleEmitter::default())),
        TILEMAP_RENDERER => Some(bytes(TilemapRenderer::default())),
        PREFAB => Some(bytes(PrefabInstance::default())),
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
    fn animators_pick_frames_from_the_sheet() {
        let mut a = SpriteAnimator {
            frame_w: 16,
            frame_h: 8,
            first: 2,
            count: 4,
            fps: 10.0,
            ..Default::default()
        };
        let frames = |a: &mut SpriteAnimator| {
            (0..8)
                .map(|i| {
                    a.time = i as f32 * 0.1 + 0.01;
                    a.frame()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(frames(&mut a), [0, 1, 2, 3, 0, 1, 2, 3]);
        a.mode = 1;
        assert_eq!(frames(&mut a), [0, 1, 2, 3, 3, 3, 3, 3]);
        a.mode = 2;
        assert_eq!(frames(&mut a), [0, 1, 2, 3, 2, 1, 0, 1]);
        // A 64 px wide sheet holds 4 frames a row: frame 2 + 3 = 5 is the
        // second of the second row
        a.time = 0.35;
        a.mode = 0;
        assert_eq!(a.src(64), Rect::new(16.0, 8.0, 16.0, 8.0));
        a.columns = 2;
        assert_eq!(a.src(64), Rect::new(16.0, 16.0, 16.0, 8.0));
    }

    #[test]
    fn layouts_match_the_c_header() {
        // orst.h and the C# structs rely on these
        assert_eq!(std::mem::size_of::<Transform>(), 20);
        assert_eq!(std::mem::size_of::<Velocity>(), 12);
        assert_eq!(std::mem::size_of::<SpriteRenderer>(), 40);
        assert_eq!(std::mem::size_of::<Body>(), 24);
        assert_eq!(std::mem::size_of::<Collider>(), 40);
        assert_eq!(std::mem::size_of::<RigidBody>(), 32);
        assert_eq!(std::mem::size_of::<Light>(), 32);
        assert_eq!(std::mem::size_of::<Name>(), 64);
        assert_eq!(std::mem::size_of::<SpriteAnimator>(), 36);
        assert_eq!(std::mem::size_of::<Camera2D>(), 8);
        assert_eq!(std::mem::size_of::<ParticleEmitter>(), 108);
        let mut w = World::new();
        register(&mut w);
        assert_eq!(w.component_id("Body"), Some(BODY));
    }

    #[test]
    fn built_ins_describe_their_fields() {
        use crate::schema::Value;
        let mut w = World::new();
        register(&mut w);
        let e = w.spawn();
        write(&mut w, e, COLLIDER, &Collider::default());
        assert!(w.set_field(e, COLLIDER, "kind", Value::U32(COLLIDER_CIRCLE)));
        assert!(w.set_field(e, COLLIDER, "radius", Value::F32(6.0)));
        let c: Collider = read(&w, e, COLLIDER).unwrap();
        assert_eq!((c.kind, c.radius, c.mask), (COLLIDER_CIRCLE, 6.0, u32::MAX));
        write(&mut w, e, SPRITE_RENDERER, &SpriteRenderer::default());
        assert_eq!(
            w.field(e, SPRITE_RENDERER, "tint"),
            Some(Value::Color(Color::WHITE))
        );
        assert!(!w.set_field(e, SPRITE_RENDERER, "tint", Value::F32(1.0)));
        assert!(!w.set_field(e, SPRITE_RENDERER, "nope", Value::F32(1.0)));
        let mut saved = Vec::new();
        w.save(&mut saved);
        w.load(&saved).unwrap();
        assert_eq!(w.fields(LIGHT).len(), 7, "a restore keeps the fields");
        write(&mut w, e, NAME, &Name::new("Ёлка"));
        assert_eq!(w.find_named("Ёлка"), Some(e));
        assert_eq!(read::<Name>(&w, e, NAME).unwrap().as_str(), "Ёлка");
        let long = "я".repeat(40); // 80 bytes: cut to 32 letters
        assert_eq!(Name::new(&long).as_str(), "я".repeat(32));
        // Every byte is described but padding and Body's contacts
        let left = [0, 0, 0, 4, 3, 2, 3, 0, 3, 3, 3, 2, 4];
        for (id, left) in (TRANSFORM..=PREFAB).zip(left) {
            let described: u32 = w.fields(id).iter().map(|f| f.ty.size()).sum();
            assert_eq!(
                w.storage(id).unwrap().size - described,
                left,
                "component {id}"
            );
        }
    }
}

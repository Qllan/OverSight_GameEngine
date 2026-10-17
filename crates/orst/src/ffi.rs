//! C API (include/orst.h). Every function takes the engine handle
//! made by `orst_engine_create`; resources are numeric ids, 0 meaning
//! "none" or "failed". When a call fails, `orst_last_error` says why.
//!
//! Strings are UTF-8, NUL-terminated. Strings the engine returns stay
//! valid until the next call that returns a string of the same kind.
//!
//! The handle contract above applies to every function, so they are not
//! each marked `unsafe` for Rust callers.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr, CString};

use crate::actions::Binding;
use crate::anim::{AnimMode, Animation, SpriteSheet};
use crate::camera::Camera;
use crate::collision::{self, Contacts, MoveResult, RayHit};
use crate::components::Transform;
use crate::engine::{Engine, InputEvent};
use crate::gamepad::{GamepadAxis, GamepadButton};
use crate::input::{key_from_u32, Key, MouseButton};
use crate::particles::EmitterConfig;
use crate::schema::{Field, FieldType};
use crate::tilemap::{LayerDraw, Orientation, Property, Tilemap};
use crate::tween::{self, Ease, Repeat};
use crate::types::{Color, Rect, Space, Sprite, Vec2};

/// The engine plus strings handed out to C
pub struct OrstEngine {
    engine: Engine,
    text: CString,
    /// Name and options of the field orst_get_field returned last
    field_text: (CString, CString),
}

#[repr(C)]
pub struct OrstConfig {
    pub title: *const c_char,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub resizable: bool,
    pub fullscreen: bool,
    pub integer_scale: bool,
    pub target_fps: u32,
    /// No window: render offscreen (tests, servers, CI)
    pub headless: bool,
    /// 0 auto (the graphics card for a window, falling back to the CPU;
    /// the CPU without one), 1 always the CPU, 2 always the graphics card
    pub renderer: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct OrstTilemapInfo {
    pub orientation: Orientation,
    pub width: u32,
    pub height: u32,
    pub tile_w: u32,
    pub tile_h: u32,
    pub layer_count: u32,
    pub object_count: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct OrstMapObject {
    pub id: u32,
    pub rect: Rect,
    pub point: bool,
    pub visible: bool,
    pub rotation: f32,
    pub gid: u32,
    /// Index of its object layer
    pub layer: u32,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(message: impl Into<String>) {
    let message = message.into().replace('\0', " ");
    LAST_ERROR.with(|e| *e.borrow_mut() = CString::new(message).unwrap_or_default());
}

fn result<T>(r: Result<T, String>, failed: T) -> T {
    r.unwrap_or_else(|e| {
        set_error(e);
        failed
    })
}

/// # Safety
/// `s` is NULL or a NUL-terminated string
unsafe fn string<'a>(s: *const c_char) -> &'a str {
    if s.is_null() {
        return "";
    }
    CStr::from_ptr(s).to_str().unwrap_or("")
}

/// Copy into a caller buffer; returns the full length in bytes (without
/// the NUL), so a too-small buffer can be retried
unsafe fn copy_out(s: &str, buf: *mut c_char, size: usize) -> usize {
    if !buf.is_null() && size > 0 {
        let n = s.len().min(size - 1);
        std::ptr::copy_nonoverlapping(s.as_ptr() as *const c_char, buf, n);
        *buf.add(n) = 0;
    }
    s.len()
}

// Handles from C are trusted to be live pointers from orst_engine_create;
// NULL is tolerated everywhere and treated as a no-op.
macro_rules! engine {
    ($e:expr, $default:expr) => {
        match unsafe { $e.as_mut() } {
            Some(h) => &mut h.engine,
            None => return $default,
        }
    };
    ($e:expr) => {
        engine!($e, ())
    };
}

fn space(value: i32) -> Space {
    if value == 1 {
        Space::Screen
    } else {
        Space::World
    }
}

fn mouse_button(value: i32) -> Option<MouseButton> {
    match value {
        0 => Some(MouseButton::Left),
        1 => Some(MouseButton::Right),
        2 => Some(MouseButton::Middle),
        _ => None,
    }
}

// --- engine and frame ------------------------------------------------------

#[no_mangle]
pub extern "C" fn orst_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// # Safety
/// `config` is NULL (defaults) or a valid config
#[no_mangle]
pub unsafe extern "C" fn orst_engine_create(config: *const OrstConfig) -> *mut OrstEngine {
    let c = config.as_ref();
    let width = c.map_or(960, |c| c.width);
    let height = c.map_or(540, |c| c.height);
    let headless = c.is_some_and(|c| c.headless);
    let renderer = c.map_or(0, |c| c.renderer);
    let engine = if renderer == 2 && (headless || cfg!(not(feature = "window"))) {
        #[cfg(feature = "gpu")]
        {
            Engine::headless_gpu(width, height)
        }
        #[cfg(not(feature = "gpu"))]
        Err("this build has no GPU renderer".to_string())
    } else if headless || cfg!(not(feature = "window")) {
        Engine::headless(width, height)
    } else {
        #[cfg(feature = "window")]
        {
            let mut wc = crate::window::WindowConfig {
                width,
                height,
                ..Default::default()
            };
            if let Some(c) = c {
                if !c.title.is_null() {
                    wc.title = string(c.title).to_string();
                }
                wc.scale = if c.scale > 0.0 { c.scale } else { 1.0 };
                wc.resizable = c.resizable;
                wc.fullscreen = c.fullscreen;
                wc.integer_scale = c.integer_scale;
                wc.target_fps = c.target_fps;
                wc.software = c.renderer == 1;
            }
            match Engine::windowed(wc) {
                // Asked for the graphics card and did not get it
                Ok(mut e) if renderer == 2 && !e.renderer_name().starts_with("gpu") => {
                    Err(e.take_error().unwrap_or_else(|| "no GPU".to_string()))
                }
                other => other,
            }
        }
        #[cfg(not(feature = "window"))]
        unreachable!()
    };
    match engine {
        Ok(engine) => Box::into_raw(Box::new(OrstEngine {
            engine,
            text: CString::default(),
            field_text: Default::default(),
        })),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `e` is NULL or a handle from `orst_engine_create`, not used afterwards
#[no_mangle]
pub unsafe extern "C" fn orst_engine_destroy(e: *mut OrstEngine) {
    if !e.is_null() {
        drop(Box::from_raw(e));
    }
}

#[no_mangle]
pub extern "C" fn orst_poll(e: *mut OrstEngine) -> bool {
    let engine = engine!(e, false);
    let alive = engine.poll();
    // Errors the engine collected (presenting, sound device) go to C
    if let Some(err) = engine.take_error() {
        set_error(err);
    }
    alive
}

#[no_mangle]
pub extern "C" fn orst_quit(e: *mut OrstEngine) {
    engine!(e).quit();
}

#[no_mangle]
pub extern "C" fn orst_dt(e: *mut OrstEngine) -> f32 {
    engine!(e, 0.0).time.dt
}

#[no_mangle]
pub extern "C" fn orst_time(e: *mut OrstEngine) -> f64 {
    engine!(e, 0.0).time.total
}

#[no_mangle]
pub extern "C" fn orst_fps(e: *mut OrstEngine) -> f32 {
    engine!(e, 0.0).time.fps
}

#[no_mangle]
pub extern "C" fn orst_frame(e: *mut OrstEngine) -> u64 {
    engine!(e, 0).time.frame
}

#[no_mangle]
pub extern "C" fn orst_set_tick_rate(e: *mut OrstEngine, rate: u32) {
    engine!(e).time.set_tick_rate(rate);
}

#[no_mangle]
pub extern "C" fn orst_ticks(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).time.ticks
}

#[no_mangle]
pub extern "C" fn orst_tick_dt(e: *mut OrstEngine) -> f32 {
    engine!(e, 0.0).time.tick_dt()
}

#[no_mangle]
pub extern "C" fn orst_tick_alpha(e: *mut OrstEngine) -> f32 {
    engine!(e, 0.0).time.tick_alpha()
}

#[no_mangle]
pub extern "C" fn orst_tick_count(e: *mut OrstEngine) -> u64 {
    engine!(e, 0).time.tick_count
}

#[no_mangle]
pub extern "C" fn orst_random_seed(e: *mut OrstEngine, seed: u64) {
    engine!(e).rng = crate::rng::Rng::new(seed);
}

#[no_mangle]
pub extern "C" fn orst_random_u32(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).rng.next_u32()
}

#[no_mangle]
pub extern "C" fn orst_random_float(e: *mut OrstEngine) -> f32 {
    engine!(e, 0.0).rng.float()
}

#[no_mangle]
pub extern "C" fn orst_random_range(e: *mut OrstEngine, min: f32, max: f32) -> f32 {
    engine!(e, 0.0).rng.range(min, max)
}

#[no_mangle]
pub extern "C" fn orst_random_int(e: *mut OrstEngine, min: i32, max: i32) -> i32 {
    engine!(e, 0).rng.int(min, max)
}

// --- entities and components ---

/// # Safety
/// `name` is NULL or a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_component(
    e: *mut OrstEngine,
    name: *const c_char,
    size: u32,
    align: u32,
) -> u32 {
    let engine = engine!(e, 0);
    result(engine.world.component(string(name), size, align), 0)
}

#[no_mangle]
pub extern "C" fn orst_spawn(e: *mut OrstEngine) -> u64 {
    engine!(e, 0).world.spawn()
}

#[no_mangle]
pub extern "C" fn orst_despawn(e: *mut OrstEngine, entity: u64) -> bool {
    engine!(e, false).world.despawn(entity)
}

#[no_mangle]
pub extern "C" fn orst_alive(e: *mut OrstEngine, entity: u64) -> bool {
    engine!(e, false).world.alive(entity)
}

#[no_mangle]
pub extern "C" fn orst_entity_count(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).world.count()
}

/// # Safety
/// `data` is NULL or points at the component's size in bytes
#[no_mangle]
pub unsafe extern "C" fn orst_set(
    e: *mut OrstEngine,
    entity: u64,
    component: u32,
    data: *const c_void,
) -> bool {
    let world = &mut engine!(e, false).world;
    let size = match world.storage(component) {
        Some(s) => s.size as usize,
        None => return false,
    };
    if data.is_null() {
        // Built-ins get their defaults (scale 1, white tint), others zeroes
        if let Some(d) = crate::components::default_bytes(component) {
            return world.set(entity, component, &d);
        }
    }
    let bytes = if data.is_null() || size == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(data as *const u8, size)
    };
    world.set(entity, component, bytes)
}

#[no_mangle]
pub extern "C" fn orst_get(e: *mut OrstEngine, entity: u64, component: u32) -> *mut c_void {
    let world = &mut engine!(e, std::ptr::null_mut()).world;
    world
        .ptr(entity, component)
        .map_or(std::ptr::null_mut(), |p| p as *mut c_void)
}

#[no_mangle]
pub extern "C" fn orst_has(e: *mut OrstEngine, entity: u64, component: u32) -> bool {
    engine!(e, false).world.has(entity, component)
}

#[no_mangle]
pub extern "C" fn orst_remove(e: *mut OrstEngine, entity: u64, component: u32) -> bool {
    engine!(e, false).world.remove(entity, component)
}

/// # Safety
/// `components` holds `count` ids (or is NULL when count is 0); `out` is
/// NULL or has room for `capacity` entities
#[no_mangle]
pub unsafe extern "C" fn orst_query(
    e: *mut OrstEngine,
    components: *const u32,
    count: u32,
    out: *mut u64,
    capacity: u32,
) -> u32 {
    let world = &engine!(e, 0).world;
    let ids = if components.is_null() || count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(components, count as usize)
    };
    let found = world.query(ids);
    if !out.is_null() {
        let n = found.len().min(capacity as usize);
        std::ptr::copy_nonoverlapping(found.as_ptr(), out, n);
    }
    found.len() as u32
}

/// # Safety
/// `buf` is NULL or has room for `capacity` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_snapshot(e: *mut OrstEngine, buf: *mut u8, capacity: usize) -> usize {
    let bytes = engine!(e, 0).snapshot();
    if !buf.is_null() && capacity >= bytes.len() {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
    }
    bytes.len()
}

/// # Safety
/// `data` points at `size` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_restore(e: *mut OrstEngine, data: *const u8, size: usize) -> bool {
    let engine = engine!(e, false);
    let bytes = if data.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(data, size)
    };
    result(engine.restore(bytes).map(|_| true), false)
}

#[no_mangle]
pub extern "C" fn orst_state_hash(e: *mut OrstEngine) -> u64 {
    engine!(e, 0).state_hash()
}

#[no_mangle]
pub extern "C" fn orst_update_motion(e: *mut OrstEngine, dt: f32) {
    engine!(e).update_motion(dt);
}

#[no_mangle]
pub extern "C" fn orst_draw_entities(e: *mut OrstEngine) {
    engine!(e).draw_entities();
}

fn transition(t: i32) -> Result<crate::scenes::Transition, String> {
    crate::scenes::Transition::from_i32(t).ok_or_else(|| format!("unknown transition {t}"))
}

#[no_mangle]
pub extern "C" fn orst_scene_go(
    e: *mut OrstEngine,
    scene: u32,
    transition_kind: i32,
    duration: f32,
    color: Color,
) -> bool {
    let engine = engine!(e, false);
    let r = transition(transition_kind).map(|t| engine.scenes.go(scene, t, duration, color));
    result(r, false)
}

#[no_mangle]
pub extern "C" fn orst_scene_push(
    e: *mut OrstEngine,
    scene: u32,
    transition_kind: i32,
    duration: f32,
    color: Color,
) -> bool {
    let engine = engine!(e, false);
    let r = transition(transition_kind).map(|t| engine.scenes.push(scene, t, duration, color));
    result(r, false)
}

#[no_mangle]
pub extern "C" fn orst_scene_pop(
    e: *mut OrstEngine,
    transition_kind: i32,
    duration: f32,
    color: Color,
) -> bool {
    let engine = engine!(e, false);
    let r = transition(transition_kind).map(|t| engine.scenes.pop(t, duration, color));
    result(r, false)
}

#[no_mangle]
pub extern "C" fn orst_scene(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).scenes.current()
}

#[no_mangle]
pub extern "C" fn orst_scene_entered(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).scenes.entered()
}

#[no_mangle]
pub extern "C" fn orst_scene_left(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).scenes.left()
}

#[no_mangle]
pub extern "C" fn orst_scene_depth(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).scenes.depth() as u32
}

#[no_mangle]
pub extern "C" fn orst_scene_transitioning(e: *mut OrstEngine) -> bool {
    engine!(e, false).scenes.transitioning()
}

#[no_mangle]
pub extern "C" fn orst_scene_cover(e: *mut OrstEngine) -> f32 {
    engine!(e, 0.0).scenes.cover()
}

/// # Safety
/// `points` is NULL or has `count` points
#[no_mangle]
pub unsafe extern "C" fn orst_create_polygon(
    e: *mut OrstEngine,
    points: *const Vec2,
    count: u32,
) -> u32 {
    let engine = engine!(e, 0);
    let pts = if points.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(points, count as usize)
    };
    result(engine.create_polygon(pts), 0)
}

#[no_mangle]
pub extern "C" fn orst_free_polygon(e: *mut OrstEngine, polygon: u32) {
    engine!(e).polygons.remove(polygon);
}

/// # Safety
/// `out` is NULL or points to an OrstOverlap
#[no_mangle]
pub unsafe extern "C" fn orst_entity_overlap(
    e: *mut OrstEngine,
    a: u64,
    b: u64,
    out: *mut crate::shapes::Overlap,
) -> bool {
    let Some(o) = engine!(e, false).entity_overlap(a, b) else {
        return false;
    };
    if let Some(out) = out.as_mut() {
        *out = o;
    }
    true
}

/// # Safety
/// `out` is NULL or has room for `capacity` points
#[no_mangle]
pub unsafe extern "C" fn orst_find_path(
    e: *mut OrstEngine,
    map: u32,
    from: Vec2,
    to: Vec2,
    diagonal: bool,
    max_cost: f32,
    out: *mut Vec2,
    capacity: u32,
) -> u32 {
    let Some(path) = engine!(e, 0).find_path(map, from, to, diagonal, max_cost) else {
        return 0;
    };
    if !out.is_null() {
        std::ptr::copy_nonoverlapping(path.as_ptr(), out, path.len().min(capacity as usize));
    }
    path.len() as u32
}

// Physics: built without the `physics` feature these do nothing and
// orst_last_error says why

/// Run `f` on the physics with bodies made for new entities first
fn with_physics<T>(
    e: *mut OrstEngine,
    failed: T,
    f: impl FnOnce(&mut crate::physics::Physics) -> T,
) -> T {
    let engine = engine!(e, failed);
    if let Some(why) = crate::physics::Physics::missing() {
        set_error(why);
        return failed;
    }
    engine.physics_sync();
    f(&mut engine.physics)
}

#[no_mangle]
pub extern "C" fn orst_physics_step(e: *mut OrstEngine, dt: f32) {
    engine!(e).physics_step(dt);
}

#[no_mangle]
pub extern "C" fn orst_set_gravity(e: *mut OrstEngine, gravity: Vec2) {
    engine!(e).physics.set_gravity(gravity);
}

#[no_mangle]
pub extern "C" fn orst_set_physics_scale(e: *mut OrstEngine, units_per_metre: f32) {
    engine!(e).physics.set_scale(units_per_metre);
}

#[no_mangle]
pub extern "C" fn orst_apply_impulse(e: *mut OrstEngine, entity: u64, impulse: Vec2) -> bool {
    with_physics(e, false, |p| p.apply_impulse(entity, impulse))
}

/// Pushes during the next step only
#[no_mangle]
pub extern "C" fn orst_apply_force(e: *mut OrstEngine, entity: u64, force: Vec2) -> bool {
    with_physics(e, false, |p| p.apply_force(entity, force))
}

#[no_mangle]
pub extern "C" fn orst_apply_torque_impulse(e: *mut OrstEngine, entity: u64, torque: f32) -> bool {
    with_physics(e, false, |p| p.apply_torque_impulse(entity, torque))
}

#[no_mangle]
pub extern "C" fn orst_body_mass(e: *mut OrstEngine, entity: u64) -> f32 {
    with_physics(e, 0.0, |p| p.mass(entity))
}

#[no_mangle]
pub extern "C" fn orst_joint_pin(e: *mut OrstEngine, a: u64, b: u64, at: Vec2) -> u32 {
    with_physics(e, 0, |p| p.pin(a, b, at).unwrap_or(0))
}

#[no_mangle]
pub extern "C" fn orst_joint_weld(e: *mut OrstEngine, a: u64, b: u64) -> u32 {
    with_physics(e, 0, |p| p.weld(a, b).unwrap_or(0))
}

#[no_mangle]
pub extern "C" fn orst_joint_rope(e: *mut OrstEngine, a: u64, b: u64, length: f32) -> u32 {
    with_physics(e, 0, |p| p.rope(a, b, length).unwrap_or(0))
}

#[no_mangle]
pub extern "C" fn orst_joint_spring(
    e: *mut OrstEngine,
    a: u64,
    b: u64,
    rest: f32,
    stiffness: f32,
    damping: f32,
) -> u32 {
    with_physics(e, 0, |p| {
        p.spring(a, b, rest, stiffness, damping).unwrap_or(0)
    })
}

#[no_mangle]
pub extern "C" fn orst_remove_joint(e: *mut OrstEngine, joint: u32) -> bool {
    with_physics(e, false, |p| p.remove_joint(joint))
}

#[no_mangle]
pub extern "C" fn orst_update_collisions(e: *mut OrstEngine) {
    engine!(e).update_collisions();
}

#[no_mangle]
pub extern "C" fn orst_set_collision_cell(e: *mut OrstEngine, size: f32) {
    engine!(e).collisions.cell = size.max(1.0);
}

#[no_mangle]
pub extern "C" fn orst_contact_count(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).collisions.contacts().len() as u32
}

/// # Safety
/// `out` is NULL or points to an OrstContact
#[no_mangle]
pub unsafe extern "C" fn orst_contact(
    e: *mut OrstEngine,
    index: u32,
    out: *mut crate::collisions::Contact,
) -> bool {
    match (
        engine!(e, false).collisions.contacts().get(index as usize),
        out.as_mut(),
    ) {
        (Some(c), Some(out)) => {
            *out = *c;
            true
        }
        _ => false,
    }
}

/// Copy found entities out; returns how many there are
unsafe fn entities_out(found: &[u64], out: *mut u64, capacity: u32) -> u32 {
    if !out.is_null() {
        let n = found.len().min(capacity as usize);
        std::ptr::copy_nonoverlapping(found.as_ptr(), out, n);
    }
    found.len() as u32
}

/// # Safety
/// `out` is NULL or has room for `capacity` entities
#[no_mangle]
pub unsafe extern "C" fn orst_query_point(
    e: *mut OrstEngine,
    point: Vec2,
    mask: u32,
    out: *mut u64,
    capacity: u32,
) -> u32 {
    let shape = crate::shapes::Shape::Circle {
        center: point,
        radius: 0.0,
    };
    entities_out(&engine!(e, 0).query_shape(&shape, mask), out, capacity)
}

/// # Safety
/// `out` is NULL or has room for `capacity` entities
#[no_mangle]
pub unsafe extern "C" fn orst_query_rect(
    e: *mut OrstEngine,
    rect: Rect,
    mask: u32,
    out: *mut u64,
    capacity: u32,
) -> u32 {
    let center = Vec2::new(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5);
    let shape = crate::shapes::Shape::rect(center, Vec2::new(rect.w, rect.h), 0.0);
    entities_out(&engine!(e, 0).query_shape(&shape, mask), out, capacity)
}

/// # Safety
/// `out` is NULL or has room for `capacity` entities
#[no_mangle]
pub unsafe extern "C" fn orst_query_circle(
    e: *mut OrstEngine,
    center: Vec2,
    radius: f32,
    mask: u32,
    out: *mut u64,
    capacity: u32,
) -> u32 {
    let shape = crate::shapes::Shape::Circle {
        center,
        radius: radius.max(0.0),
    };
    entities_out(&engine!(e, 0).query_shape(&shape, mask), out, capacity)
}

/// # Safety
/// `out` is NULL or points to an OrstEntityHit
#[no_mangle]
pub unsafe extern "C" fn orst_raycast_entities(
    e: *mut OrstEngine,
    from: Vec2,
    to: Vec2,
    mask: u32,
    out: *mut crate::collisions::EntityHit,
) -> bool {
    let Some(hit) = engine!(e, false).raycast_entities(from, to, mask) else {
        return false;
    };
    if let Some(out) = out.as_mut() {
        *out = hit;
    }
    true
}

#[no_mangle]
pub extern "C" fn orst_debug_colliders(e: *mut OrstEngine) {
    engine!(e).debug_colliders();
}

#[no_mangle]
pub extern "C" fn orst_world_clear(e: *mut OrstEngine) {
    engine!(e).world.clear();
}

/// # Safety
/// `name` is NULL or a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_find_named(e: *mut OrstEngine, name: *const c_char) -> u64 {
    engine!(e, 0).world.find_named(string(name)).unwrap_or(0)
}

#[repr(C)]
pub struct OrstField {
    pub name: *const c_char,
    pub ty: u32,
    pub offset: u32,
    pub flags: u32,
    pub min: f32,
    pub max: f32,
    pub options: *const c_char,
}

/// # Safety
/// `fields` points at `count` fields whose strings are NULL or
/// NUL-terminated
#[no_mangle]
pub unsafe extern "C" fn orst_set_fields(
    e: *mut OrstEngine,
    component: u32,
    fields: *const OrstField,
    count: u32,
) -> bool {
    let engine = engine!(e, false);
    let fields = if fields.is_null() || count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(fields, count as usize)
    };
    let mut out = Vec::with_capacity(fields.len());
    for f in fields {
        let Some(ty) = FieldType::from_u32(f.ty) else {
            set_error(format!("field {:?}: unknown type {}", string(f.name), f.ty));
            return false;
        };
        let mut field = Field::new(string(f.name), ty, f.offset)
            .flags(f.flags)
            .range(f.min, f.max);
        let options = string(f.options);
        if !options.is_empty() {
            field.options = options.split('|').map(str::to_string).collect();
        }
        out.push(field);
    }
    result(engine.world.set_fields(component, out).map(|_| true), false)
}

#[no_mangle]
pub extern "C" fn orst_field_count(e: *mut OrstEngine, component: u32) -> u32 {
    engine!(e, 0).world.fields(component).len() as u32
}

/// # Safety
/// `out` is NULL or points at a field to fill
#[no_mangle]
pub unsafe extern "C" fn orst_get_field(
    e: *mut OrstEngine,
    component: u32,
    index: u32,
    out: *mut OrstField,
) -> bool {
    let Some(h) = e.as_mut() else { return false };
    let Some(f) = h.engine.world.fields(component).get(index as usize) else {
        return false;
    };
    let text = |s: &str| CString::new(s.replace('\0', "")).unwrap_or_default();
    h.field_text = (text(&f.name), text(&f.options.join("|")));
    if let Some(out) = out.as_mut() {
        *out = OrstField {
            name: h.field_text.0.as_ptr(),
            ty: f.ty as u32,
            offset: f.offset,
            flags: f.flags,
            min: f.min,
            max: f.max,
            options: if f.options.is_empty() {
                std::ptr::null()
            } else {
                h.field_text.1.as_ptr()
            },
        };
    }
    true
}

#[no_mangle]
pub extern "C" fn orst_begin_frame(e: *mut OrstEngine, clear: Color) {
    engine!(e).begin_frame(clear);
}

#[no_mangle]
pub extern "C" fn orst_end_frame(e: *mut OrstEngine) {
    let engine = engine!(e);
    engine.end_frame();
    if let Some(err) = engine.take_error() {
        set_error(err);
    }
}

/// # Safety
/// `title` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_set_title(e: *mut OrstEngine, title: *const c_char) {
    engine!(e).set_title(string(title));
}

#[no_mangle]
pub extern "C" fn orst_set_fullscreen(e: *mut OrstEngine, on: bool) {
    engine!(e).set_fullscreen(on);
}

#[no_mangle]
pub extern "C" fn orst_is_fullscreen(e: *mut OrstEngine) -> bool {
    engine!(e, false).fullscreen()
}

#[no_mangle]
pub extern "C" fn orst_set_target_fps(e: *mut OrstEngine, fps: u32) {
    engine!(e).set_target_fps(fps);
}

#[no_mangle]
pub extern "C" fn orst_has_window(e: *mut OrstEngine) -> bool {
    engine!(e, false).has_window()
}

/// Canvas size (the logical resolution)
///
/// # Safety
/// `w` and `h` are NULL or valid
#[no_mangle]
pub unsafe extern "C" fn orst_canvas_size(e: *mut OrstEngine, w: *mut u32, h: *mut u32) {
    let (cw, ch) = engine!(e).size();
    if let Some(w) = w.as_mut() {
        *w = cw;
    }
    if let Some(h) = h.as_mut() {
        *h = ch;
    }
}

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_save_screenshot(e: *mut OrstEngine, path: *const c_char) -> bool {
    let engine = engine!(e, false);
    result(engine.save_screenshot(string(path)).map(|_| true), false)
}

/// Colour of a canvas pixel of the last frame (transparent outside)
#[no_mangle]
pub extern "C" fn orst_pixel(e: *mut OrstEngine, x: u32, y: u32) -> Color {
    engine!(e, Color::TRANSPARENT)
        .pixel(x, y)
        .unwrap_or(Color::TRANSPARENT)
}

// --- textures and fonts ------------------------------------------------------

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_load_texture(e: *mut OrstEngine, path: *const c_char) -> u32 {
    let engine = engine!(e, 0);
    result(engine.load_texture(string(path)), 0)
}

/// # Safety
/// `rgba` points to `width * height * 4` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_texture_from_rgba(
    e: *mut OrstEngine,
    width: u32,
    height: u32,
    rgba: *const u8,
) -> u32 {
    let engine = engine!(e, 0);
    if rgba.is_null() {
        set_error("no pixel data");
        return 0;
    }
    let data = std::slice::from_raw_parts(rgba, width as usize * height as usize * 4);
    result(engine.texture_from_rgba(width, height, data), 0)
}

/// # Safety
/// `w` and `h` are NULL or valid
#[no_mangle]
pub unsafe extern "C" fn orst_texture_size(
    e: *mut OrstEngine,
    texture: u32,
    w: *mut u32,
    h: *mut u32,
) -> bool {
    let Some((tw, th)) = engine!(e, false).texture_size(texture) else {
        return false;
    };
    if let Some(w) = w.as_mut() {
        *w = tw;
    }
    if let Some(h) = h.as_mut() {
        *h = th;
    }
    true
}

#[no_mangle]
pub extern "C" fn orst_set_texture_smooth(e: *mut OrstEngine, texture: u32, smooth: bool) {
    engine!(e).set_texture_smooth(texture, smooth);
}

#[no_mangle]
pub extern "C" fn orst_free_texture(e: *mut OrstEngine, texture: u32) {
    engine!(e).free_texture(texture);
}

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_load_font(e: *mut OrstEngine, path: *const c_char) -> u32 {
    let engine = engine!(e, 0);
    result(engine.load_font(string(path)), 0)
}

#[no_mangle]
pub extern "C" fn orst_free_font(e: *mut OrstEngine, font: u32) {
    engine!(e).free_font(font);
}

/// # Safety
/// `text` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_measure_text(
    e: *mut OrstEngine,
    font: u32,
    text: *const c_char,
    size: f32,
) -> Vec2 {
    engine!(e, Vec2::default()).measure_text(font, string(text), size)
}

// --- render targets, post effects, shaders and shake ---------------------------

#[no_mangle]
pub extern "C" fn orst_create_target(e: *mut OrstEngine, width: u32, height: u32) -> u32 {
    result(engine!(e, 0).create_target(width, height), 0)
}

#[no_mangle]
pub extern "C" fn orst_set_target(e: *mut OrstEngine, target: u32) -> bool {
    result(engine!(e, false).set_target(target).map(|_| true), false)
}

#[no_mangle]
pub extern "C" fn orst_get_target(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).target()
}

#[no_mangle]
pub extern "C" fn orst_clear_target(e: *mut OrstEngine, target: u32, color: Color) {
    engine!(e).clear_target(target, color);
}

/// # Safety
/// `camera` is NULL (back to the main camera) or valid
#[no_mangle]
pub unsafe extern "C" fn orst_set_target_camera(
    e: *mut OrstEngine,
    target: u32,
    camera: *const Camera,
) {
    engine!(e).set_target_camera(target, camera.as_ref().copied());
}

/// # Safety
/// `effects` is NULL or has `count` effects
#[no_mangle]
pub unsafe extern "C" fn orst_set_effects(
    e: *mut OrstEngine,
    target: u32,
    effects: *const crate::post::Effect,
    count: u32,
) -> bool {
    let engine = engine!(e, false);
    let list = if effects.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(effects, count as usize)
    };
    result(engine.set_effects(target, list).map(|_| true), false)
}

/// # Safety
/// `wgsl` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_create_shader(e: *mut OrstEngine, wgsl: *const c_char) -> u32 {
    result(engine!(e, 0).create_shader(string(wgsl)), 0)
}

#[no_mangle]
pub extern "C" fn orst_free_shader(e: *mut OrstEngine, shader: u32) {
    engine!(e).free_shader(shader);
}

#[no_mangle]
pub extern "C" fn orst_shake(e: *mut OrstEngine, strength: f32, duration: f32) {
    engine!(e).shake(strength, duration);
}

#[no_mangle]
pub extern "C" fn orst_set_normal_map(e: *mut OrstEngine, texture: u32, normal_map: u32) {
    engine!(e).set_normal_map(texture, normal_map);
}

#[no_mangle]
pub extern "C" fn orst_set_shadow_layers(e: *mut OrstEngine, layers: u32) {
    engine!(e).set_shadow_layers(layers);
}

#[no_mangle]
pub extern "C" fn orst_set_shadow_tilemap(e: *mut OrstEngine, map: u32, on: bool) {
    engine!(e).set_shadow_tilemap(map, on);
}

// --- camera and draw state ---------------------------------------------------

#[no_mangle]
pub extern "C" fn orst_get_camera(e: *mut OrstEngine) -> Camera {
    engine!(e, Camera::default()).camera()
}

#[no_mangle]
pub extern "C" fn orst_set_camera(e: *mut OrstEngine, camera: Camera) {
    engine!(e).set_camera(camera);
}

#[no_mangle]
pub extern "C" fn orst_world_to_screen(e: *mut OrstEngine, p: Vec2) -> Vec2 {
    engine!(e, p).world_to_screen(p)
}

#[no_mangle]
pub extern "C" fn orst_screen_to_world(e: *mut OrstEngine, p: Vec2) -> Vec2 {
    engine!(e, p).screen_to_world(p)
}

#[no_mangle]
pub extern "C" fn orst_set_layer(e: *mut OrstEngine, layer: i32) {
    engine!(e).set_layer(layer);
}

#[no_mangle]
pub extern "C" fn orst_set_depth(e: *mut OrstEngine, depth: f32) {
    engine!(e).set_depth(depth);
}

/// 0: world (moved by the camera), 1: screen pixels
#[no_mangle]
pub extern "C" fn orst_set_space(e: *mut OrstEngine, space_: i32) {
    engine!(e).set_space(space(space_));
}

#[no_mangle]
pub extern "C" fn orst_set_antialias(e: *mut OrstEngine, on: bool) {
    engine!(e).set_antialias(on);
}

// --- drawing -----------------------------------------------------------------

/// # Safety
/// `sprite` is a valid sprite
#[no_mangle]
pub unsafe extern "C" fn orst_draw_sprite(e: *mut OrstEngine, texture: u32, sprite: *const Sprite) {
    let engine = engine!(e);
    if let Some(s) = sprite.as_ref() {
        engine.draw_sprite(texture, s);
    }
}

#[no_mangle]
pub extern "C" fn orst_draw_texture(e: *mut OrstEngine, texture: u32, x: f32, y: f32) {
    engine!(e).draw_texture(texture, x, y);
}

/// # Safety
/// `sheet` and `sprite` are valid
#[no_mangle]
pub unsafe extern "C" fn orst_draw_frame(
    e: *mut OrstEngine,
    texture: u32,
    sheet: *const SpriteSheet,
    frame: u32,
    sprite: *const Sprite,
) {
    let engine = engine!(e);
    if let (Some(sheet), Some(sprite)) = (sheet.as_ref(), sprite.as_ref()) {
        engine.draw_frame(texture, sheet, frame, sprite);
    }
}

#[no_mangle]
pub extern "C" fn orst_fill_rect(e: *mut OrstEngine, rect: Rect, color: Color) {
    engine!(e).fill_rect(rect, color);
}

#[no_mangle]
pub extern "C" fn orst_draw_rect(e: *mut OrstEngine, rect: Rect, thickness: f32, color: Color) {
    engine!(e).draw_rect(rect, thickness, color);
}

#[no_mangle]
pub extern "C" fn orst_draw_line(
    e: *mut OrstEngine,
    from: Vec2,
    to: Vec2,
    thickness: f32,
    color: Color,
) {
    engine!(e).draw_line(from, to, thickness, color);
}

#[no_mangle]
pub extern "C" fn orst_fill_circle(e: *mut OrstEngine, center: Vec2, radius: f32, color: Color) {
    engine!(e).fill_circle(center, radius, color);
}

#[no_mangle]
pub extern "C" fn orst_draw_circle(
    e: *mut OrstEngine,
    center: Vec2,
    radius: f32,
    thickness: f32,
    color: Color,
) {
    engine!(e).draw_circle(center, radius, thickness, color);
}

/// # Safety
/// `text` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_draw_text(
    e: *mut OrstEngine,
    font: u32,
    text: *const c_char,
    pos: Vec2,
    size: f32,
    color: Color,
) {
    engine!(e).draw_text(font, string(text), pos, size, color);
}

// --- input -------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn orst_key_down(e: *mut OrstEngine, key: i32) -> bool {
    engine!(e, false).input.key_down(key_from_u32(key as u32))
}

#[no_mangle]
pub extern "C" fn orst_key_pressed(e: *mut OrstEngine, key: i32) -> bool {
    engine!(e, false)
        .input
        .key_pressed(key_from_u32(key as u32))
}

#[no_mangle]
pub extern "C" fn orst_key_released(e: *mut OrstEngine, key: i32) -> bool {
    engine!(e, false)
        .input
        .key_released(key_from_u32(key as u32))
}

/// -1, 0 or 1 from two keys (e.g. A and D)
#[no_mangle]
pub extern "C" fn orst_axis(e: *mut OrstEngine, negative: i32, positive: i32) -> f32 {
    let engine = engine!(e, 0.0);
    engine
        .input
        .axis(key_from_u32(negative as u32), key_from_u32(positive as u32))
}

#[no_mangle]
pub extern "C" fn orst_mouse_down(e: *mut OrstEngine, button: i32) -> bool {
    let engine = engine!(e, false);
    mouse_button(button).is_some_and(|b| engine.input.mouse_down(b))
}

#[no_mangle]
pub extern "C" fn orst_mouse_pressed(e: *mut OrstEngine, button: i32) -> bool {
    let engine = engine!(e, false);
    mouse_button(button).is_some_and(|b| engine.input.mouse_pressed(b))
}

#[no_mangle]
pub extern "C" fn orst_mouse_released(e: *mut OrstEngine, button: i32) -> bool {
    let engine = engine!(e, false);
    mouse_button(button).is_some_and(|b| engine.input.mouse_released(b))
}

/// Mouse in canvas pixels
#[no_mangle]
pub extern "C" fn orst_mouse_pos(e: *mut OrstEngine) -> Vec2 {
    engine!(e, Vec2::default()).input.mouse
}

#[no_mangle]
pub extern "C" fn orst_mouse_world(e: *mut OrstEngine) -> Vec2 {
    engine!(e, Vec2::default()).mouse_world()
}

#[no_mangle]
pub extern "C" fn orst_mouse_wheel(e: *mut OrstEngine) -> f32 {
    engine!(e, 0.0).input.wheel
}

/// Text typed this frame (valid until the next call)
///
/// # Safety
/// `e` is NULL or a live handle
#[no_mangle]
pub unsafe extern "C" fn orst_text_input(e: *mut OrstEngine) -> *const c_char {
    static EMPTY: &[u8] = b"\0";
    let Some(h) = e.as_mut() else {
        return EMPTY.as_ptr() as *const c_char;
    };
    h.text = CString::new(h.engine.input.text.replace('\0', "")).unwrap_or_default();
    h.text.as_ptr()
}

/// Feed a key change, applied at the next `orst_poll` (tests, replays)
#[no_mangle]
pub extern "C" fn orst_inject_key(e: *mut OrstEngine, key: i32, down: bool) {
    engine!(e).inject(InputEvent::Key(key_from_u32(key as u32), down));
}

#[no_mangle]
pub extern "C" fn orst_inject_mouse_button(e: *mut OrstEngine, button: i32, down: bool) {
    let engine = engine!(e);
    if let Some(b) = mouse_button(button) {
        engine.inject(InputEvent::MouseButton(b, down));
    }
}

#[no_mangle]
pub extern "C" fn orst_inject_mouse_move(e: *mut OrstEngine, pos: Vec2) {
    engine!(e).inject(InputEvent::MouseMove(pos));
}

// --- gamepads -------------------------------------------------------------------

/// Slot from C; a negative one maps past the end, which reads as "off"
/// An input bound to an action, as the C API sees it
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrstBinding {
    /// 0 none, 1 key, 2 mouse button, 3 pad button, 4 pad axis
    pub kind: i32,
    pub code: i32,
    /// Pad slot, -1 any
    pub pad: i32,
    /// Pad axis direction: 1 or -1
    pub direction: i32,
}

impl OrstBinding {
    fn to_binding(self) -> Result<Binding, String> {
        let pad = if self.pad < 0 {
            crate::actions::ANY_PAD
        } else {
            self.pad
        };
        let bad = || format!("unknown binding {self:?}");
        match self.kind {
            1 => match key_from_u32(self.code as u32) {
                Key::Unknown => Err(bad()),
                k => Ok(Binding::Key(k)),
            },
            2 => mouse_button(self.code).map(Binding::Mouse).ok_or_else(bad),
            3 => GamepadButton::from_i32(self.code)
                .map(|b| Binding::PadButton(pad, b))
                .ok_or_else(bad),
            4 if self.direction != 0 => GamepadAxis::from_i32(self.code)
                .map(|a| Binding::PadAxis(pad, a, self.direction.signum()))
                .ok_or_else(bad),
            _ => Err(bad()),
        }
    }

    fn from_binding(b: Binding) -> OrstBinding {
        let (kind, code, pad, direction) = match b {
            Binding::Key(k) => (1, k as i32, -1, 0),
            Binding::Mouse(m) => (2, m as i32, -1, 0),
            Binding::PadButton(p, b) => (3, b as i32, p, 0),
            Binding::PadAxis(p, a, d) => (4, a as i32, p, d),
        };
        OrstBinding {
            kind,
            code,
            pad,
            direction,
        }
    }
}

/// # Safety
/// `name` is NULL or a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_action(e: *mut OrstEngine, name: *const c_char) -> u32 {
    let engine = engine!(e, 0);
    match string(name) {
        "" => {
            set_error("an action needs a name");
            0
        }
        n => engine.actions.action(n),
    }
}

#[no_mangle]
pub extern "C" fn orst_bind(e: *mut OrstEngine, action: u32, binding: OrstBinding) -> bool {
    let engine = engine!(e, false);
    let r = binding.to_binding().map(|b| engine.actions.bind(action, b));
    result(r, false)
}

#[no_mangle]
pub extern "C" fn orst_set_binding(
    e: *mut OrstEngine,
    action: u32,
    index: u32,
    binding: OrstBinding,
) -> bool {
    let engine = engine!(e, false);
    let r = binding
        .to_binding()
        .map(|b| engine.actions.set_binding(action, index as usize, b));
    result(r, false)
}

#[no_mangle]
pub extern "C" fn orst_remove_binding(e: *mut OrstEngine, action: u32, index: u32) -> bool {
    engine!(e, false)
        .actions
        .remove_binding(action, index as usize)
}

#[no_mangle]
pub extern "C" fn orst_clear_bindings(e: *mut OrstEngine, action: u32) {
    engine!(e).actions.clear_bindings(action);
}

#[no_mangle]
pub extern "C" fn orst_binding_count(e: *mut OrstEngine, action: u32) -> u32 {
    engine!(e, 0).actions.bindings(action).len() as u32
}

/// # Safety
/// `out` is NULL or points to an OrstBinding
#[no_mangle]
pub unsafe extern "C" fn orst_get_binding(
    e: *mut OrstEngine,
    action: u32,
    index: u32,
    out: *mut OrstBinding,
) -> bool {
    let engine = engine!(e, false);
    match (
        engine.actions.bindings(action).get(index as usize),
        out.as_mut(),
    ) {
        (Some(b), Some(out)) => {
            *out = OrstBinding::from_binding(*b);
            true
        }
        _ => false,
    }
}

/// # Safety
/// `buf` is NULL or has room for `size` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_binding_text(
    e: *mut OrstEngine,
    binding: OrstBinding,
    buf: *mut c_char,
    size: usize,
) -> usize {
    let _ = engine!(e, 0);
    let text = binding
        .to_binding()
        .map(|b| b.to_text())
        .unwrap_or_default();
    copy_out(&text, buf, size)
}

#[no_mangle]
pub extern "C" fn orst_action_down(e: *mut OrstEngine, action: u32) -> bool {
    engine!(e, false).actions.down(action)
}

#[no_mangle]
pub extern "C" fn orst_action_pressed(e: *mut OrstEngine, action: u32) -> bool {
    engine!(e, false).actions.pressed(action)
}

#[no_mangle]
pub extern "C" fn orst_action_released(e: *mut OrstEngine, action: u32) -> bool {
    engine!(e, false).actions.released(action)
}

#[no_mangle]
pub extern "C" fn orst_action_value(e: *mut OrstEngine, action: u32) -> f32 {
    engine!(e, 0.0).actions.value(action)
}

#[no_mangle]
pub extern "C" fn orst_action_axis(e: *mut OrstEngine, negative: u32, positive: u32) -> f32 {
    let a = &engine!(e, 0.0).actions;
    a.value(positive) - a.value(negative)
}

/// # Safety
/// `out` is NULL or points to an OrstBinding
#[no_mangle]
pub unsafe extern "C" fn orst_captured_binding(e: *mut OrstEngine, out: *mut OrstBinding) -> bool {
    let engine = engine!(e, false);
    let Some(b) = engine.actions.captured() else {
        return false;
    };
    if let Some(out) = out.as_mut() {
        *out = OrstBinding::from_binding(b);
    }
    true
}

/// # Safety
/// `buf` is NULL or has room for `size` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_bindings_save(
    e: *mut OrstEngine,
    buf: *mut c_char,
    size: usize,
) -> usize {
    let text = engine!(e, 0).actions.save();
    copy_out(&text, buf, size)
}

/// # Safety
/// `text` is NULL or a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_bindings_load(e: *mut OrstEngine, text: *const c_char) -> bool {
    let engine = engine!(e, false);
    let r = engine.actions.load(string(text)).map(|_| true);
    result(r, false)
}

fn pad(v: i32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

#[no_mangle]
pub extern "C" fn orst_gamepad_connected(e: *mut OrstEngine, gamepad: i32) -> bool {
    engine!(e, false).input.gamepads.connected(pad(gamepad))
}

/// # Safety
/// `buf` is NULL or has room for `size` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_gamepad_name(
    e: *mut OrstEngine,
    gamepad: i32,
    buf: *mut c_char,
    size: usize,
) -> usize {
    let pads = &engine!(e, 0).input.gamepads;
    let name = pads.pads.get(pad(gamepad)).map_or("", |p| p.name.as_str());
    copy_out(name, buf, size)
}

#[no_mangle]
pub extern "C" fn orst_gamepad_down(e: *mut OrstEngine, gamepad: i32, button: i32) -> bool {
    let pads = &engine!(e, false).input.gamepads;
    GamepadButton::from_i32(button).is_some_and(|b| pads.down(pad(gamepad), b))
}

#[no_mangle]
pub extern "C" fn orst_gamepad_pressed(e: *mut OrstEngine, gamepad: i32, button: i32) -> bool {
    let pads = &engine!(e, false).input.gamepads;
    GamepadButton::from_i32(button).is_some_and(|b| pads.pressed(pad(gamepad), b))
}

#[no_mangle]
pub extern "C" fn orst_gamepad_released(e: *mut OrstEngine, gamepad: i32, button: i32) -> bool {
    let pads = &engine!(e, false).input.gamepads;
    GamepadButton::from_i32(button).is_some_and(|b| pads.released(pad(gamepad), b))
}

/// Stick -1..1 (y down) or trigger 0..1, after the dead zone
#[no_mangle]
pub extern "C" fn orst_gamepad_axis(e: *mut OrstEngine, gamepad: i32, axis: i32) -> f32 {
    let pads = &engine!(e, 0.0).input.gamepads;
    GamepadAxis::from_i32(axis).map_or(0.0, |a| pads.axis(pad(gamepad), a))
}

#[no_mangle]
pub extern "C" fn orst_set_gamepad_deadzone(e: *mut OrstEngine, deadzone: f32) {
    engine!(e).input.gamepads.deadzone = deadzone;
}

#[no_mangle]
pub extern "C" fn orst_inject_gamepad_connected(e: *mut OrstEngine, gamepad: i32, connected: bool) {
    engine!(e).inject(InputEvent::GamepadConnected(pad(gamepad), connected));
}

#[no_mangle]
pub extern "C" fn orst_inject_gamepad_button(
    e: *mut OrstEngine,
    gamepad: i32,
    button: i32,
    down: bool,
) {
    let engine = engine!(e);
    if let Some(b) = GamepadButton::from_i32(button) {
        engine.inject(InputEvent::GamepadButton(pad(gamepad), b, down));
    }
}

/// Raw value before the dead zone: stick -1..1 with y down, trigger 0..1
#[no_mangle]
pub extern "C" fn orst_inject_gamepad_axis(
    e: *mut OrstEngine,
    gamepad: i32,
    axis: i32,
    value: f32,
) {
    let engine = engine!(e);
    if let Some(a) = GamepadAxis::from_i32(axis) {
        engine.inject(InputEvent::GamepadAxis(pad(gamepad), a, value));
    }
}

// --- sprite sheets and animations --------------------------------------------

#[no_mangle]
pub extern "C" fn orst_sheet_grid(
    texture_w: u32,
    texture_h: u32,
    frame_w: u32,
    frame_h: u32,
    margin: u32,
    spacing: u32,
) -> SpriteSheet {
    SpriteSheet::grid(texture_w, texture_h, frame_w, frame_h, margin, spacing)
}

#[no_mangle]
pub extern "C" fn orst_sheet_frame(sheet: SpriteSheet, index: u32) -> Rect {
    sheet.frame(index)
}

/// Frame of an animation at `time` seconds; mode 0 loop, 1 once,
/// 2 ping-pong
///
/// # Safety
/// `frames` points to `count` values
#[no_mangle]
pub unsafe extern "C" fn orst_anim_frame_at(
    frames: *const u32,
    count: u32,
    fps: f32,
    mode: i32,
    time: f32,
) -> u32 {
    if frames.is_null() || count == 0 {
        return 0;
    }
    let mode = match mode {
        1 => AnimMode::Once,
        2 => AnimMode::PingPong,
        _ => AnimMode::Loop,
    };
    let frames = std::slice::from_raw_parts(frames, count as usize);
    Animation::new(frames.to_vec(), fps, mode).frame_at(time)
}

// --- tweens and easing --------------------------------------------------------

/// Eased progress for `t` in 0..1; unknown curves are linear
#[no_mangle]
pub extern "C" fn orst_ease(ease: i32, t: f32) -> f32 {
    tween::ease(Ease::from_i32(ease), t)
}

/// Eased progress 0..1 of a tween `time` seconds after its start
/// (negative time is the delay); repeat 0 once, 1 loop, 2 ping-pong
#[no_mangle]
pub extern "C" fn orst_tween_progress(time: f32, duration: f32, ease: i32, repeat: i32) -> f32 {
    tween::progress(
        time,
        duration,
        Ease::from_i32(ease),
        Repeat::from_i32(repeat),
    )
}

#[no_mangle]
pub extern "C" fn orst_lerp_color(a: Color, b: Color, t: f32) -> Color {
    tween::lerp_color(a, b, t)
}

/// "gpu: <graphics card>" or "software"
///
/// # Safety
/// `buf` is NULL or has room for `size` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_renderer_name(
    e: *mut OrstEngine,
    buf: *mut c_char,
    size: usize,
) -> usize {
    copy_out(&engine!(e, 0).renderer_name(), buf, size)
}

// --- hot reload ----------------------------------------------------------------

/// While on, `orst_poll` reloads textures and maps whose files changed
/// (checked twice a second); ids stay the same
#[no_mangle]
pub extern "C" fn orst_set_hot_reload(e: *mut OrstEngine, on: bool) {
    engine!(e).hot_reload.enabled = on;
}

#[no_mangle]
pub extern "C" fn orst_hot_reload_enabled(e: *mut OrstEngine) -> bool {
    engine!(e, false).hot_reload.enabled
}

/// Check the files now; returns how many resources were reloaded
#[no_mangle]
pub extern "C" fn orst_reload_changed(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).reload_changed()
}

/// Resources reloaded by the last `orst_poll` (e.g. to re-read map objects)
#[no_mangle]
pub extern "C" fn orst_reloaded(e: *mut OrstEngine) -> u32 {
    engine!(e, 0).hot_reload.reloaded
}

// --- tile maps -----------------------------------------------------------------

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_load_tilemap(e: *mut OrstEngine, path: *const c_char) -> u32 {
    let engine = engine!(e, 0);
    result(engine.load_tilemap(string(path)), 0)
}

/// An empty map to fill with `orst_tilemap_set_tile`; it has `layers`
/// tile layers and no tilesets (add one with `orst_tilemap_add_tileset`)
#[no_mangle]
pub extern "C" fn orst_create_tilemap(
    e: *mut OrstEngine,
    orientation: i32,
    width: u32,
    height: u32,
    tile_w: u32,
    tile_h: u32,
    layers: u32,
) -> u32 {
    let engine = engine!(e, 0);
    let orientation = if orientation == 1 {
        Orientation::Isometric
    } else {
        Orientation::Orthogonal
    };
    let mut map = Tilemap::new(orientation, width, height, tile_w, tile_h);
    for i in 0..layers {
        map.add_layer(&format!("layer{}", i));
    }
    engine.add_tilemap(map)
}

/// Tiles of `texture` cut by `sheet`; tile 0 of the set gets id `first_gid`
#[no_mangle]
pub extern "C" fn orst_tilemap_add_tileset(
    e: *mut OrstEngine,
    map: u32,
    first_gid: u32,
    texture: u32,
    sheet: SpriteSheet,
) -> bool {
    let Some(m) = engine!(e, false).tilemap_mut(map) else {
        return false;
    };
    m.add_tileset(crate::tilemap::Tileset::new(first_gid, texture, sheet));
    true
}

/// Give a tile of a tileset a true/false property, e.g. "solid"
///
/// # Safety
/// `name` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_tilemap_set_tile_flag(
    e: *mut OrstEngine,
    map: u32,
    gid: u32,
    name: *const c_char,
    value: bool,
) -> bool {
    let Some(m) = engine!(e, false).tilemap_mut(map) else {
        return false;
    };
    let gid = gid & crate::tilemap::GID_MASK;
    let Some(set) = m
        .tilesets
        .iter_mut()
        .rev()
        .find(|t| t.first_gid <= gid && gid > 0)
    else {
        return false;
    };
    let local = gid - set.first_gid;
    set.tile_properties
        .entry(local)
        .or_default()
        .insert(string(name).to_string(), Property::Bool(value));
    true
}

#[no_mangle]
pub extern "C" fn orst_free_tilemap(e: *mut OrstEngine, map: u32) {
    engine!(e).free_tilemap(map);
}

/// # Safety
/// `info` is valid
#[no_mangle]
pub unsafe extern "C" fn orst_tilemap_info(
    e: *mut OrstEngine,
    map: u32,
    info: *mut OrstTilemapInfo,
) -> bool {
    let (Some(m), Some(info)) = (engine!(e, false).tilemap(map), info.as_mut()) else {
        return false;
    };
    *info = OrstTilemapInfo {
        orientation: m.orientation,
        width: m.width,
        height: m.height,
        tile_w: m.tile_w,
        tile_h: m.tile_h,
        layer_count: m.layers.len() as u32,
        object_count: m.object_layers.iter().map(|l| l.objects.len() as u32).sum(),
    };
    true
}

/// Index of a tile layer by name, -1 if there is none
///
/// # Safety
/// `name` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_tilemap_layer_index(
    e: *mut OrstEngine,
    map: u32,
    name: *const c_char,
) -> i32 {
    engine!(e, -1)
        .tilemap(map)
        .and_then(|m| m.layer_index(string(name)))
        .map_or(-1, |i| i as i32)
}

#[no_mangle]
pub extern "C" fn orst_tilemap_get_tile(
    e: *mut OrstEngine,
    map: u32,
    layer: u32,
    x: i32,
    y: i32,
) -> u32 {
    engine!(e, 0)
        .tilemap(map)
        .and_then(|m| m.layers.get(layer as usize))
        .map_or(0, |l| l.get(x, y))
}

#[no_mangle]
pub extern "C" fn orst_tilemap_set_tile(
    e: *mut OrstEngine,
    map: u32,
    layer: u32,
    x: i32,
    y: i32,
    gid: u32,
) {
    if let Some(l) = engine!(e)
        .tilemap_mut(map)
        .and_then(|m| m.layers.get_mut(layer as usize))
    {
        l.set(x, y, gid);
    }
}

#[no_mangle]
pub extern "C" fn orst_tilemap_is_solid(e: *mut OrstEngine, map: u32, x: i32, y: i32) -> bool {
    engine!(e, false)
        .tilemap(map)
        .is_some_and(|m| m.is_solid(x, y))
}

#[no_mangle]
pub extern "C" fn orst_tilemap_tile_to_world(e: *mut OrstEngine, map: u32, tile: Vec2) -> Vec2 {
    engine!(e, Vec2::default())
        .tilemap(map)
        .map_or(Vec2::default(), |m| m.tile_to_world(tile.x, tile.y))
}

#[no_mangle]
pub extern "C" fn orst_tilemap_world_to_tile(e: *mut OrstEngine, map: u32, p: Vec2) -> Vec2 {
    engine!(e, Vec2::default())
        .tilemap(map)
        .map_or(Vec2::default(), |m| m.world_to_tile(p))
}

#[no_mangle]
pub extern "C" fn orst_tilemap_cell_center(e: *mut OrstEngine, map: u32, x: i32, y: i32) -> Vec2 {
    engine!(e, Vec2::default())
        .tilemap(map)
        .map_or(Vec2::default(), |m| m.cell_center(x, y))
}

/// Every tile layer, layer i on draw layer `base + i`
#[no_mangle]
pub extern "C" fn orst_draw_tilemap(e: *mut OrstEngine, map: u32, base: i32) {
    engine!(e).draw_tilemap(map, base);
}

/// One tile layer; with `y_sort` each tile's depth is the bottom of its
/// cell, so it sorts with sprites whose depth is their feet's y
#[no_mangle]
pub extern "C" fn orst_draw_tilemap_layer(
    e: *mut OrstEngine,
    map: u32,
    layer: u32,
    draw_layer: i32,
    depth: f32,
    y_sort: bool,
    tint: Color,
) {
    let opts = LayerDraw {
        layer: draw_layer,
        depth,
        y_sort,
        tint,
    };
    engine!(e).draw_tilemap_layer(map, layer as usize, &opts);
}

fn object(m: &Tilemap, index: u32) -> Option<(u32, &crate::tilemap::MapObject)> {
    m.object_layers
        .iter()
        .enumerate()
        .flat_map(|(li, l)| l.objects.iter().map(move |o| (li as u32, o)))
        .nth(index as usize)
}

/// Object `index` counting through all object layers in order
///
/// # Safety
/// `out` is valid
#[no_mangle]
pub unsafe extern "C" fn orst_tilemap_object(
    e: *mut OrstEngine,
    map: u32,
    index: u32,
    out: *mut OrstMapObject,
) -> bool {
    let engine = engine!(e, false);
    let (Some((layer, o)), Some(out)) = (
        engine.tilemap(map).and_then(|m| object(m, index)),
        out.as_mut(),
    ) else {
        return false;
    };
    *out = OrstMapObject {
        id: o.id,
        rect: o.rect,
        point: o.point,
        visible: o.visible,
        rotation: o.rotation,
        gid: o.gid,
        layer,
    };
    true
}

/// Index of the first object with this name, -1 if there is none
///
/// # Safety
/// `name` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_tilemap_find_object(
    e: *mut OrstEngine,
    map: u32,
    name: *const c_char,
) -> i32 {
    let name = string(name);
    let Some(m) = engine!(e, -1).tilemap(map) else {
        return -1;
    };
    m.object_layers
        .iter()
        .flat_map(|l| l.objects.iter())
        .position(|o| o.name == name)
        .map_or(-1, |i| i as i32)
}

/// Copy an object's name; returns its length (call with size 0 to ask)
///
/// # Safety
/// `buf` holds `size` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_tilemap_object_name(
    e: *mut OrstEngine,
    map: u32,
    index: u32,
    buf: *mut c_char,
    size: usize,
) -> usize {
    let name = engine!(e, 0)
        .tilemap(map)
        .and_then(|m| object(m, index))
        .map_or("", |(_, o)| o.name.as_str());
    copy_out(name, buf, size)
}

/// Copy an object's class (Tiled's "class" or "type")
///
/// # Safety
/// `buf` holds `size` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_tilemap_object_class(
    e: *mut OrstEngine,
    map: u32,
    index: u32,
    buf: *mut c_char,
    size: usize,
) -> usize {
    let class = engine!(e, 0)
        .tilemap(map)
        .and_then(|m| object(m, index))
        .map_or("", |(_, o)| o.class.as_str());
    copy_out(class, buf, size)
}

/// A number property of an object (bools give 0 or 1), or `fallback`
///
/// # Safety
/// `name` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_tilemap_object_number(
    e: *mut OrstEngine,
    map: u32,
    index: u32,
    name: *const c_char,
    fallback: f64,
) -> f64 {
    let name = string(name);
    engine!(e, fallback)
        .tilemap(map)
        .and_then(|m| object(m, index))
        .and_then(|(_, o)| o.properties.get(name))
        .and_then(|p| match p {
            Property::Bool(b) => Some(*b as i32 as f64),
            other => other.as_f64(),
        })
        .unwrap_or(fallback)
}

/// A string property of an object; returns its length, or -1 if missing
///
/// # Safety
/// `name` is a NUL-terminated string, `buf` holds `size` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_tilemap_object_string(
    e: *mut OrstEngine,
    map: u32,
    index: u32,
    name: *const c_char,
    buf: *mut c_char,
    size: usize,
) -> i64 {
    let name = string(name);
    let value = engine!(e, -1)
        .tilemap(map)
        .and_then(|m| object(m, index))
        .and_then(|(_, o)| o.properties.get(name))
        .and_then(Property::as_str);
    match value {
        Some(v) => copy_out(v, buf, size) as i64,
        None => -1,
    }
}

// --- collisions ----------------------------------------------------------------

#[no_mangle]
pub extern "C" fn orst_move_box(
    e: *mut OrstEngine,
    map: u32,
    rect: Rect,
    delta: Vec2,
) -> MoveResult {
    let fallback = MoveResult {
        rect: Rect::new(rect.x + delta.x, rect.y + delta.y, rect.w, rect.h),
        contacts: Contacts::default(),
    };
    engine!(e, fallback).move_box(map, rect, delta)
}

#[no_mangle]
pub extern "C" fn orst_touching(e: *mut OrstEngine, map: u32, rect: Rect) -> Contacts {
    engine!(e, Contacts::default())
        .tilemap(map)
        .map_or(Contacts::default(), |m| collision::touching(m, rect))
}

/// # Safety
/// `hit` is NULL or valid
#[no_mangle]
pub unsafe extern "C" fn orst_raycast(
    e: *mut OrstEngine,
    map: u32,
    from: Vec2,
    to: Vec2,
    hit: *mut RayHit,
) -> bool {
    match engine!(e, false).raycast(map, from, to) {
        Some(h) => {
            if let Some(out) = hit.as_mut() {
                *out = h;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn orst_point_solid(e: *mut OrstEngine, map: u32, p: Vec2) -> bool {
    engine!(e, false)
        .tilemap(map)
        .is_some_and(|m| collision::point_solid(m, p))
}

#[no_mangle]
pub extern "C" fn orst_rect_solid(e: *mut OrstEngine, map: u32, rect: Rect) -> bool {
    engine!(e, false)
        .tilemap(map)
        .is_some_and(|m| collision::rect_solid(m, rect))
}

#[no_mangle]
pub extern "C" fn orst_rects_overlap(a: Rect, b: Rect) -> bool {
    a.overlaps(&b)
}

// --- debug overlay -------------------------------------------------------------

/// On while debugging: draws the stats panel and the debug shapes
#[no_mangle]
pub extern "C" fn orst_set_debug(e: *mut OrstEngine, on: bool) {
    engine!(e).set_debug(on);
}

#[no_mangle]
pub extern "C" fn orst_debug_enabled(e: *mut OrstEngine) -> bool {
    engine!(e, false).debug.enabled
}

/// Key that toggles the overlay (F3 by default; 0 = none)
#[no_mangle]
pub extern "C" fn orst_set_debug_key(e: *mut OrstEngine, key: i32) {
    engine!(e).debug.toggle_key = (key != 0).then(|| key_from_u32(key as u32));
}

#[no_mangle]
pub extern "C" fn orst_debug_rect(e: *mut OrstEngine, rect: Rect, color: Color) {
    engine!(e).debug_rect(rect, color);
}

#[no_mangle]
pub extern "C" fn orst_debug_line(e: *mut OrstEngine, from: Vec2, to: Vec2, color: Color) {
    engine!(e).debug_line(from, to, color);
}

#[no_mangle]
pub extern "C" fn orst_debug_circle(e: *mut OrstEngine, center: Vec2, radius: f32, color: Color) {
    engine!(e).debug_circle(center, radius, color);
}

/// # Safety
/// `text` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_debug_text(
    e: *mut OrstEngine,
    text: *const c_char,
    pos: Vec2,
    color: Color,
) {
    engine!(e).debug_text(string(text), pos, color);
}

#[no_mangle]
pub extern "C" fn orst_debug_tilemap(e: *mut OrstEngine, map: u32) {
    engine!(e).debug_tilemap(map);
}

// --- particles -----------------------------------------------------------------

#[no_mangle]
pub extern "C" fn orst_default_emitter() -> EmitterConfig {
    EmitterConfig::default()
}

/// # Safety
/// `config` is NULL (defaults) or a valid config
#[no_mangle]
pub unsafe extern "C" fn orst_create_emitter(
    e: *mut OrstEngine,
    config: *const EmitterConfig,
    pos: Vec2,
) -> u32 {
    let config = config.as_ref().copied().unwrap_or_default();
    engine!(e, 0).create_emitter(config, pos)
}

#[no_mangle]
pub extern "C" fn orst_free_emitter(e: *mut OrstEngine, emitter: u32) {
    engine!(e).free_emitter(emitter);
}

/// Config of an emitter (the defaults for an unknown id)
#[no_mangle]
pub extern "C" fn orst_emitter_config(e: *mut OrstEngine, emitter: u32) -> EmitterConfig {
    engine!(e, EmitterConfig::default())
        .emitter(emitter)
        .map(|em| em.config)
        .unwrap_or_default()
}

/// # Safety
/// `config` is a valid config
#[no_mangle]
pub unsafe extern "C" fn orst_set_emitter_config(
    e: *mut OrstEngine,
    emitter: u32,
    config: *const EmitterConfig,
) -> bool {
    let (Some(em), Some(c)) = (engine!(e, false).emitter_mut(emitter), config.as_ref()) else {
        return false;
    };
    em.set_config(*c);
    true
}

#[no_mangle]
pub extern "C" fn orst_set_emitter_pos(e: *mut OrstEngine, emitter: u32, pos: Vec2) {
    if let Some(em) = engine!(e).emitter_mut(emitter) {
        em.pos = pos;
    }
}

#[no_mangle]
pub extern "C" fn orst_emitter_pos(e: *mut OrstEngine, emitter: u32) -> Vec2 {
    engine!(e, Vec2::default())
        .emitter(emitter)
        .map_or(Vec2::default(), |em| em.pos)
}

/// Spawn at the config's rate (on by default); bursts work either way
#[no_mangle]
pub extern "C" fn orst_set_emitter_emitting(e: *mut OrstEngine, emitter: u32, on: bool) {
    if let Some(em) = engine!(e).emitter_mut(emitter) {
        em.emitting = on;
    }
}

#[no_mangle]
pub extern "C" fn orst_emitter_emitting(e: *mut OrstEngine, emitter: u32) -> bool {
    engine!(e, false)
        .emitter(emitter)
        .is_some_and(|em| em.emitting)
}

#[no_mangle]
pub extern "C" fn orst_emitter_burst(e: *mut OrstEngine, emitter: u32, count: u32) {
    if let Some(em) = engine!(e).emitter_mut(emitter) {
        em.burst(count);
    }
}

/// Particles alive now
#[no_mangle]
pub extern "C" fn orst_emitter_count(e: *mut OrstEngine, emitter: u32) -> u32 {
    engine!(e, 0)
        .emitter(emitter)
        .map_or(0, |em| em.particles.len() as u32)
}

#[no_mangle]
pub extern "C" fn orst_emitter_clear(e: *mut OrstEngine, emitter: u32) {
    if let Some(em) = engine!(e).emitter_mut(emitter) {
        em.clear();
    }
}

#[no_mangle]
pub extern "C" fn orst_update_emitter(e: *mut OrstEngine, emitter: u32, dt: f32) {
    if let Some(em) = engine!(e).emitter_mut(emitter) {
        em.update(dt);
    }
}

#[no_mangle]
pub extern "C" fn orst_update_emitters(e: *mut OrstEngine, dt: f32) {
    engine!(e).update_emitters(dt);
}

#[no_mangle]
pub extern "C" fn orst_draw_emitter(e: *mut OrstEngine, emitter: u32) {
    engine!(e).draw_emitter(emitter);
}

// --- GPU emitters: the same configs, for huge numbers of particles ----------------

/// # Safety
/// `config` is NULL (defaults) or a valid config
#[no_mangle]
pub unsafe extern "C" fn orst_create_gpu_emitter(
    e: *mut OrstEngine,
    config: *const EmitterConfig,
    pos: Vec2,
) -> u32 {
    let config = config.as_ref().copied().unwrap_or_default();
    engine!(e, 0).create_gpu_emitter(config, pos)
}

#[no_mangle]
pub extern "C" fn orst_free_gpu_emitter(e: *mut OrstEngine, emitter: u32) {
    engine!(e).free_gpu_emitter(emitter);
}

#[no_mangle]
pub extern "C" fn orst_gpu_emitter_config(e: *mut OrstEngine, emitter: u32) -> EmitterConfig {
    engine!(e, EmitterConfig::default())
        .gpu_emitter(emitter)
        .map(|em| em.config)
        .unwrap_or_default()
}

/// # Safety
/// `config` is a valid config
#[no_mangle]
pub unsafe extern "C" fn orst_set_gpu_emitter_config(
    e: *mut OrstEngine,
    emitter: u32,
    config: *const EmitterConfig,
) -> bool {
    let (Some(em), Some(c)) = (engine!(e, false).gpu_emitter_mut(emitter), config.as_ref()) else {
        return false;
    };
    em.set_config(*c);
    true
}

#[no_mangle]
pub extern "C" fn orst_set_gpu_emitter_pos(e: *mut OrstEngine, emitter: u32, pos: Vec2) {
    if let Some(em) = engine!(e).gpu_emitter_mut(emitter) {
        em.pos = pos;
    }
}

#[no_mangle]
pub extern "C" fn orst_gpu_emitter_pos(e: *mut OrstEngine, emitter: u32) -> Vec2 {
    engine!(e, Vec2::default())
        .gpu_emitter(emitter)
        .map_or(Vec2::default(), |em| em.pos)
}

#[no_mangle]
pub extern "C" fn orst_set_gpu_emitter_emitting(e: *mut OrstEngine, emitter: u32, on: bool) {
    if let Some(em) = engine!(e).gpu_emitter_mut(emitter) {
        em.emitting = on;
    }
}

#[no_mangle]
pub extern "C" fn orst_gpu_emitter_burst(e: *mut OrstEngine, emitter: u32, count: u32) {
    if let Some(em) = engine!(e).gpu_emitter_mut(emitter) {
        em.burst(count);
    }
}

#[no_mangle]
pub extern "C" fn orst_gpu_emitter_count(e: *mut OrstEngine, emitter: u32) -> u32 {
    engine!(e, 0)
        .gpu_emitter(emitter)
        .map_or(0, |em| em.count())
}

#[no_mangle]
pub extern "C" fn orst_gpu_emitter_clear(e: *mut OrstEngine, emitter: u32) {
    if let Some(em) = engine!(e).gpu_emitter_mut(emitter) {
        em.clear();
    }
}

#[no_mangle]
pub extern "C" fn orst_update_gpu_emitters(e: *mut OrstEngine, dt: f32) {
    engine!(e).update_gpu_emitters(dt);
}

#[no_mangle]
pub extern "C" fn orst_draw_gpu_emitter(e: *mut OrstEngine, emitter: u32) {
    engine!(e).draw_gpu_emitter(emitter);
}

// --- skeletons -----------------------------------------------------------------

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_load_skeleton(e: *mut OrstEngine, path: *const c_char) -> u32 {
    result(engine!(e, 0).load_skeleton(string(path)), 0)
}

/// # Safety
/// `json` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_skeleton_from_json(e: *mut OrstEngine, json: *const c_char) -> u32 {
    result(engine!(e, 0).skeleton_from_json(string(json)), 0)
}

#[no_mangle]
pub extern "C" fn orst_free_skeleton_data(e: *mut OrstEngine, data: u32) {
    engine!(e).free_skeleton_data(data);
}

#[no_mangle]
pub extern "C" fn orst_create_skeleton(e: *mut OrstEngine, data: u32) -> u32 {
    result(engine!(e, 0).create_skeleton(data), 0)
}

#[no_mangle]
pub extern "C" fn orst_free_skeleton(e: *mut OrstEngine, skeleton: u32) {
    engine!(e).free_skeleton(skeleton);
}

fn skeleton(e: &mut Engine, id: u32) -> Result<&mut crate::skeleton::Skeleton, String> {
    e.skeleton_mut(id)
        .ok_or_else(|| format!("no skeleton {id}"))
}

/// # Safety
/// `name` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_skeleton_play(
    e: *mut OrstEngine,
    skeleton_id: u32,
    name: *const c_char,
    looped: bool,
    mix: f32,
) -> bool {
    let r =
        skeleton(engine!(e, false), skeleton_id).and_then(|s| s.play(string(name), looped, mix));
    result(r.map(|_| true), false)
}

#[no_mangle]
pub extern "C" fn orst_skeleton_stop(e: *mut OrstEngine, skeleton: u32) {
    if let Some(s) = engine!(e).skeleton_mut(skeleton) {
        s.stop();
    }
}

/// Name of the current animation ("" for none)
///
/// # Safety
/// `buf` is NULL or has room for `size` bytes
#[no_mangle]
pub unsafe extern "C" fn orst_skeleton_animation(
    e: *mut OrstEngine,
    skeleton: u32,
    buf: *mut c_char,
    size: usize,
) -> usize {
    let name = engine!(e, 0)
        .skeleton(skeleton)
        .and_then(|s| s.current())
        .unwrap_or("");
    copy_out(name, buf, size)
}

#[no_mangle]
pub extern "C" fn orst_skeleton_done(e: *mut OrstEngine, skeleton: u32) -> bool {
    engine!(e, false)
        .skeleton(skeleton)
        .is_some_and(|s| s.done())
}

/// # Safety
/// `event` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_skeleton_fired(
    e: *mut OrstEngine,
    skeleton: u32,
    event: *const c_char,
) -> bool {
    engine!(e, false)
        .skeleton(skeleton)
        .is_some_and(|s| s.fired(string(event)))
}

#[no_mangle]
pub extern "C" fn orst_update_skeletons(e: *mut OrstEngine, dt: f32) {
    engine!(e).update_skeletons(dt);
}

#[no_mangle]
pub extern "C" fn orst_set_skeleton_root(e: *mut OrstEngine, skeleton: u32, root: Transform) {
    if let Some(s) = engine!(e).skeleton_mut(skeleton) {
        s.set_root(root.pos, root.rotation, root.scale);
    }
}

#[no_mangle]
pub extern "C" fn orst_set_skeleton_tint(e: *mut OrstEngine, skeleton: u32, tint: Color) {
    if let Some(s) = engine!(e).skeleton_mut(skeleton) {
        s.tint = tint;
    }
}

#[no_mangle]
pub extern "C" fn orst_set_skeleton_speed(e: *mut OrstEngine, skeleton: u32, speed: f32) {
    if let Some(s) = engine!(e).skeleton_mut(skeleton) {
        s.speed = speed;
    }
}

#[no_mangle]
pub extern "C" fn orst_draw_skeleton(e: *mut OrstEngine, skeleton: u32, texture: u32) {
    engine!(e).draw_skeleton(skeleton, texture);
}

/// A bone's index, -1 if there is none by that name
///
/// # Safety
/// `name` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_skeleton_bone(
    e: *mut OrstEngine,
    skeleton: u32,
    name: *const c_char,
) -> i32 {
    engine!(e, -1)
        .skeleton(skeleton)
        .and_then(|s| s.data.bone(string(name)))
        .map_or(-1, |b| b as i32)
}

/// Where a bone is in the world (a default Transform for an unknown one)
#[no_mangle]
pub extern "C" fn orst_skeleton_bone_world(
    e: *mut OrstEngine,
    skeleton: u32,
    bone: i32,
) -> Transform {
    engine!(e, Transform::default())
        .skeleton(skeleton)
        .and_then(|s| s.bone_world(usize::try_from(bone).ok()?))
        .map_or(Transform::default(), |(pos, rotation, scale)| Transform {
            pos,
            rotation,
            scale,
        })
}

/// A point in a bone's own space, in the world
#[no_mangle]
pub extern "C" fn orst_skeleton_bone_point(
    e: *mut OrstEngine,
    skeleton: u32,
    bone: i32,
    local: Vec2,
) -> Vec2 {
    engine!(e, Vec2::default())
        .skeleton(skeleton)
        .and_then(|s| s.bone_point(usize::try_from(bone).ok()?, local))
        .unwrap_or_default()
}

/// Turn a bone by code, blended over the animation by `weight`
/// (0 gives it back)
#[no_mangle]
pub extern "C" fn orst_set_bone_override(
    e: *mut OrstEngine,
    skeleton: u32,
    bone: i32,
    rotation: f32,
    weight: f32,
) {
    if let (Some(s), Ok(b)) = (engine!(e).skeleton_mut(skeleton), usize::try_from(bone)) {
        s.set_override(b, rotation, weight);
    }
}

/// Show another attachment in a slot; NULL hides it
///
/// # Safety
/// `slot` is a NUL-terminated string, `attachment` NULL or one
#[no_mangle]
pub unsafe extern "C" fn orst_skeleton_set_attachment(
    e: *mut OrstEngine,
    skeleton_id: u32,
    slot: *const c_char,
    attachment: *const c_char,
) -> bool {
    let which = (!attachment.is_null()).then(|| string(attachment));
    let r = skeleton(engine!(e, false), skeleton_id)
        .and_then(|s| s.set_attachment(string(slot), which));
    result(r.map(|_| true), false)
}

// --- sound ---------------------------------------------------------------------
// Built without the "audio" feature these do nothing and return 0/false.

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_load_sound(e: *mut OrstEngine, path: *const c_char) -> u32 {
    let _engine = engine!(e, 0);
    #[cfg(feature = "audio")]
    return result(_engine.audio.load_sound(string(path)), 0);
    #[cfg(not(feature = "audio"))]
    {
        let _ = path;
        set_error("built without sound");
        0
    }
}

#[no_mangle]
pub extern "C" fn orst_free_sound(e: *mut OrstEngine, _sound: u32) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.free_sound(_sound);
}

/// Returns a voice id for `orst_stop_voice` (0 when nothing plays)
#[no_mangle]
pub extern "C" fn orst_play_sound(
    e: *mut OrstEngine,
    _sound: u32,
    _volume: f32,
    _pitch: f32,
) -> u32 {
    let _engine = engine!(e, 0);
    #[cfg(feature = "audio")]
    return _engine.audio.play_sound(_sound, _volume, _pitch);
    #[cfg(not(feature = "audio"))]
    0
}

#[no_mangle]
pub extern "C" fn orst_stop_voice(e: *mut OrstEngine, _voice: u32) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.stop_voice(_voice);
}

#[no_mangle]
pub extern "C" fn orst_voice_playing(e: *mut OrstEngine, _voice: u32) -> bool {
    let _engine = engine!(e, false);
    #[cfg(feature = "audio")]
    return _engine.audio.voice_playing(_voice);
    #[cfg(not(feature = "audio"))]
    false
}

#[no_mangle]
pub extern "C" fn orst_stop_all_sounds(e: *mut OrstEngine) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.stop_all_sounds();
}

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn orst_play_music(
    e: *mut OrstEngine,
    path: *const c_char,
    _volume: f32,
    _looped: bool,
) -> bool {
    let _engine = engine!(e, false);
    #[cfg(feature = "audio")]
    return result(
        _engine
            .audio
            .play_music(string(path), _volume, _looped)
            .map(|_| true),
        false,
    );
    #[cfg(not(feature = "audio"))]
    {
        let _ = path;
        set_error("built without sound");
        false
    }
}

#[no_mangle]
pub extern "C" fn orst_stop_music(e: *mut OrstEngine) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.stop_music();
}

#[no_mangle]
pub extern "C" fn orst_pause_music(e: *mut OrstEngine, _paused: bool) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.pause_music(_paused);
}

#[no_mangle]
pub extern "C" fn orst_music_playing(e: *mut OrstEngine) -> bool {
    let _engine = engine!(e, false);
    #[cfg(feature = "audio")]
    return _engine.audio.music_playing();
    #[cfg(not(feature = "audio"))]
    false
}

#[no_mangle]
pub extern "C" fn orst_set_music_volume(e: *mut OrstEngine, _volume: f32) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.set_music_volume(_volume);
}

#[no_mangle]
pub extern "C" fn orst_set_master_volume(e: *mut OrstEngine, _volume: f32) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.set_master_volume(_volume);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headless(w: u32, h: u32) -> *mut OrstEngine {
        let config = OrstConfig {
            title: std::ptr::null(),
            width: w,
            height: h,
            scale: 1.0,
            resizable: false,
            fullscreen: false,
            integer_scale: false,
            target_fps: 0,
            headless: true,
            renderer: 0,
        };
        unsafe { orst_engine_create(&config) }
    }

    #[test]
    fn draws_moves_and_reports_errors_through_c() {
        let e = headless(32, 32);
        assert!(!e.is_null());
        unsafe {
            assert_eq!(orst_load_texture(e, c"missing.png".as_ptr()), 0);
            let err = CStr::from_ptr(orst_last_error()).to_str().unwrap();
            assert!(err.contains("missing.png"), "{}", err);

            let white = orst_texture_from_rgba(e, 8, 8, [255u8; 256].as_ptr());
            let map = orst_create_tilemap(e, 0, 4, 4, 8, 8, 1);
            assert!(orst_tilemap_add_tileset(
                e,
                map,
                1,
                white,
                orst_sheet_grid(8, 8, 8, 8, 0, 0)
            ));
            assert!(orst_tilemap_set_tile_flag(
                e,
                map,
                1,
                c"solid".as_ptr(),
                true
            ));
            for x in 0..4 {
                orst_tilemap_set_tile(e, map, 0, x, 3, 1);
            }
            let r = orst_move_box(e, map, Rect::new(2.0, 2.0, 4.0, 4.0), Vec2::new(0.0, 50.0));
            assert!(r.contacts.ground && r.rect.bottom() == 24.0);
            let mut hit = RayHit::default();
            assert!(orst_raycast(
                e,
                map,
                Vec2::new(4.0, 0.0),
                Vec2::new(4.0, 40.0),
                &mut hit
            ));
            assert_eq!(hit.cell_y, 3);

            assert!(orst_poll(e));
            orst_set_camera(
                e,
                Camera {
                    center: Vec2::new(16.0, 16.0),
                    ..Camera::default()
                },
            );
            orst_begin_frame(e, Color::BLACK);
            orst_draw_tilemap(e, map, 0);
            orst_set_space(e, 1);
            orst_fill_rect(
                e,
                Rect::new(0.0, 0.0, 4.0, 4.0),
                Color::rgba(255, 0, 0, 255),
            );
            orst_end_frame(e);
            assert_eq!(orst_pixel(e, 1, 1), Color::rgba(255, 0, 0, 255));
            assert_eq!(orst_pixel(e, 10, 28), Color::WHITE);

            let frames = [3u32, 4, 5];
            assert_eq!(orst_anim_frame_at(frames.as_ptr(), 3, 10.0, 0, 0.15), 4);
            assert_eq!(
                std::mem::size_of::<EmitterConfig>(),
                104,
                "matches OrstEmitter"
            );
            let em = orst_create_emitter(e, std::ptr::null(), Vec2::new(4.0, 4.0));
            orst_emitter_burst(e, em, 3);
            assert_eq!(orst_emitter_count(e, em), 3);
            let mut c = orst_emitter_config(e, em);
            c.max = 1;
            assert!(orst_set_emitter_config(e, em, &c));
            orst_emitter_clear(e, em);
            orst_emitter_burst(e, em, 3);
            assert_eq!(orst_emitter_count(e, em), 1, "new max applies");
            orst_free_emitter(e, em);
            assert_eq!(orst_emitter_count(e, em), 0);
            assert_eq!(orst_ease(1, 0.5), 0.25);
            assert_eq!(orst_ease(-5, 0.5), 0.5, "unknown curve is linear");
            assert_eq!(orst_tween_progress(-0.2, 1.0, 0, 0), 0.0, "delay");
            assert!((orst_tween_progress(1.25, 1.0, 0, 2) - 0.75).abs() < 1e-5);
            orst_inject_key(e, 1, true); // A
            orst_poll(e);
            assert!(orst_key_pressed(e, 1));
            assert_eq!(orst_axis(e, 1, 4), -1.0);

            orst_inject_gamepad_connected(e, 2, true);
            orst_inject_gamepad_button(e, 2, 0, true);
            orst_inject_gamepad_axis(e, 2, 0, 1.0);
            orst_poll(e);
            assert!(orst_gamepad_connected(e, 2) && !orst_gamepad_connected(e, 0));
            assert!(orst_gamepad_pressed(e, 2, 0) && orst_gamepad_down(e, 2, 0));
            assert_eq!(orst_gamepad_axis(e, 2, 0), 1.0);
            assert!(!orst_gamepad_down(e, -1, 0) && !orst_gamepad_down(e, 2, 99));
            let mut name = [0 as c_char; 4];
            assert_eq!(orst_gamepad_name(e, 2, name.as_mut_ptr(), 4), 8, "Injected");
            orst_inject_gamepad_connected(e, 2, false);
            orst_poll(e);
            assert!(orst_gamepad_released(e, 2, 0) && orst_gamepad_axis(e, 2, 0) == 0.0);
            orst_engine_destroy(e);
        }
    }

    #[test]
    fn actions_follow_their_bindings_and_rebind() {
        let e = headless(8, 8);
        unsafe {
            let jump = orst_action(e, c"jump".as_ptr());
            assert!(jump != 0 && orst_action(e, c"".as_ptr()) == 0);
            let key = OrstBinding {
                kind: 1,
                code: Key::Space as i32,
                pad: -1,
                direction: 0,
            };
            assert!(orst_bind(e, jump, key));
            assert!(
                !orst_bind(
                    e,
                    jump,
                    OrstBinding {
                        kind: 4,
                        code: 0,
                        pad: -1,
                        direction: 0
                    }
                ),
                "an axis needs a direction"
            );
            orst_inject_key(e, Key::Space as i32, true);
            orst_poll(e);
            assert!(orst_action_pressed(e, jump) && orst_action_value(e, jump) == 1.0);
            // Rebinding: the next input pressed replaces the key
            assert!(orst_captured_binding(e, std::ptr::null_mut()));
            orst_inject_key(e, Key::Space as i32, false);
            orst_poll(e);
            let mut got = OrstBinding::default();
            assert!(!orst_captured_binding(e, &mut got) && orst_action_released(e, jump));
            orst_inject_gamepad_connected(e, 0, true);
            orst_inject_gamepad_button(e, 0, 0, true);
            orst_poll(e);
            assert!(orst_captured_binding(e, &mut got));
            assert_eq!(
                got,
                OrstBinding {
                    kind: 3,
                    code: 0,
                    pad: 0,
                    direction: 0
                }
            );
            assert!(orst_set_binding(e, jump, 0, got) && orst_binding_count(e, jump) == 1);
            let mut buf = [0 as c_char; 64];
            orst_binding_text(e, got, buf.as_mut_ptr(), buf.len());
            assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "pad0:South");
            let n = orst_bindings_save(e, std::ptr::null_mut(), 0);
            assert_eq!(n, "jump = pad0:South\n".len());
            assert!(!orst_bindings_load(e, c"jump = key:Nope".as_ptr()));
            assert!(orst_bindings_load(e, c"jump = key:Enter".as_ptr()));
            orst_get_binding(e, jump, 0, &mut got);
            assert_eq!(got.kind, 1);
            orst_engine_destroy(e);
        }
    }

    #[test]
    fn colliders_follow_their_transforms() {
        use crate::components::*;
        let e = headless(64, 64);
        unsafe {
            let put = |pos: Vec2, rotation: f32, c: Collider| {
                let ent = orst_spawn(e);
                let t = Transform {
                    pos,
                    rotation,
                    ..Transform::default()
                };
                orst_set(e, ent, TRANSFORM, &t as *const _ as *const c_void);
                orst_set(e, ent, COLLIDER, &c as *const _ as *const c_void);
                ent
            };
            let wall = put(
                Vec2::new(10.0, 10.0),
                0.0,
                Collider {
                    size: Vec2::new(10.0, 10.0),
                    ..Collider::default()
                },
            );
            let ball = put(
                Vec2::new(17.0, 10.0),
                0.0,
                Collider {
                    kind: COLLIDER_CIRCLE,
                    radius: 3.0,
                    ..Collider::default()
                },
            );
            let mut o = crate::shapes::Overlap::default();
            assert!(orst_entity_overlap(e, wall, ball, &mut o));
            assert!(
                (o.normal.x - 1.0).abs() < 1e-4 && (o.depth - 1.0).abs() < 1e-4,
                "{o:?}"
            );

            // A triangle: the offset turns with the entity
            let tri = [
                Vec2::new(0.0, -2.0),
                Vec2::new(2.0, 2.0),
                Vec2::new(-2.0, 2.0),
            ];
            let id = orst_create_polygon(e, tri.as_ptr(), 3);
            assert!(id != 0);
            let bent = [
                Vec2::new(0.0, 0.0),
                Vec2::new(4.0, 0.0),
                Vec2::new(2.0, 1.0),
                Vec2::new(4.0, 4.0),
            ];
            assert_eq!(orst_create_polygon(e, bent.as_ptr(), 4), 0, "concave");
            let poly = Collider {
                kind: COLLIDER_POLYGON,
                polygon: id,
                offset: Vec2::new(10.0, 0.0),
                ..Collider::default()
            };
            let spun = put(Vec2::new(40.0, 40.0), std::f32::consts::FRAC_PI_2, poly);
            let probe = put(
                Vec2::new(40.0, 50.0),
                0.0,
                Collider {
                    kind: COLLIDER_CIRCLE,
                    radius: 1.0,
                    ..Collider::default()
                },
            );
            assert!(
                orst_entity_overlap(e, spun, probe, std::ptr::null_mut()),
                "offset (10, 0) turned a quarter is (0, 10)"
            );
            assert!(!orst_entity_overlap(e, spun, wall, std::ptr::null_mut()));

            // Outlines in the debug overlay
            orst_set_debug(e, true);
            orst_begin_frame(e, Color::BLACK);
            orst_debug_colliders(e);
            orst_end_frame(e);
            assert_ne!(orst_pixel(e, 5, 8), Color::BLACK, "the wall's left edge");
            orst_engine_destroy(e);
        }
    }

    #[test]
    fn collision_pass_reports_and_survives_a_snapshot() {
        use crate::collisions::Contact;
        use crate::components::*;
        let e = headless(8, 8);
        unsafe {
            let put = |x: f32, c: Collider| {
                let ent = orst_spawn(e);
                let t = Transform {
                    pos: Vec2::new(x, 0.0),
                    ..Transform::default()
                };
                orst_set(e, ent, TRANSFORM, &t as *const _ as *const c_void);
                orst_set(e, ent, COLLIDER, &c as *const _ as *const c_void);
                ent
            };
            let coin = put(
                0.0,
                Collider {
                    kind: COLLIDER_CIRCLE,
                    radius: 4.0,
                    trigger: 1,
                    layer: 8,
                    ..Collider::default()
                },
            );
            let hero = put(
                6.0,
                Collider {
                    kind: COLLIDER_CIRCLE,
                    radius: 4.0,
                    ..Collider::default()
                },
            );
            orst_update_collisions(e);
            assert_eq!(orst_contact_count(e), 1);
            let mut c: Contact = std::mem::zeroed();
            assert!(
                orst_contact(e, 0, &mut c)
                    && c.a == coin
                    && c.b == hero
                    && c.state == 0
                    && c.trigger == 1
            );

            // Restoring a snapshot keeps the pair touching: no second begin
            let mut snap = vec![0u8; orst_snapshot(e, std::ptr::null_mut(), 0)];
            orst_snapshot(e, snap.as_mut_ptr(), snap.len());
            orst_update_collisions(e);
            assert!(orst_restore(e, snap.as_ptr(), snap.len()));
            orst_update_collisions(e);
            assert!(
                orst_contact(e, 0, &mut c) && c.state == 1,
                "stay after restore"
            );

            let mut found = [0u64; 4];
            assert_eq!(
                orst_query_point(e, Vec2::new(-1.0, 0.0), u32::MAX, found.as_mut_ptr(), 4),
                1
            );
            assert_eq!(found[0], coin);
            assert_eq!(
                orst_query_rect(
                    e,
                    Rect::new(-10.0, -10.0, 30.0, 20.0),
                    1,
                    std::ptr::null_mut(),
                    0
                ),
                1,
                "layer 1: the hero only"
            );
            assert_eq!(
                orst_query_circle(
                    e,
                    Vec2::new(20.0, 0.0),
                    11.0,
                    u32::MAX,
                    found.as_mut_ptr(),
                    4
                ),
                1
            );
            let mut hit = crate::collisions::EntityHit::default();
            assert!(orst_raycast_entities(
                e,
                Vec2::new(30.0, 0.0),
                Vec2::new(-30.0, 0.0),
                u32::MAX,
                &mut hit
            ));
            assert_eq!(hit.entity, hero);
            orst_engine_destroy(e);
        }
    }

    #[test]
    fn paths_go_around_solid_tiles() {
        let e = headless(8, 8);
        unsafe {
            let white = orst_texture_from_rgba(e, 8, 8, [255u8; 256].as_ptr());
            let map = orst_create_tilemap(e, 0, 5, 3, 8, 8, 1);
            orst_tilemap_add_tileset(e, map, 1, white, orst_sheet_grid(8, 8, 8, 8, 0, 0));
            orst_tilemap_set_tile_flag(e, map, 1, c"solid".as_ptr(), true);
            for y in 0..2 {
                orst_tilemap_set_tile(e, map, 0, 2, y, 1);
            }
            let (from, to) = (Vec2::new(4.0, 4.0), Vec2::new(36.0, 4.0));
            let n = orst_find_path(e, map, from, to, false, 0.0, std::ptr::null_mut(), 0);
            assert_eq!(n, 9, "down to the gap in row 2 and back up");
            let mut path = vec![Vec2::default(); n as usize];
            orst_find_path(e, map, from, to, false, 0.0, path.as_mut_ptr(), n);
            assert_eq!((path[0], path[8]), (from, to));
            assert!(path.contains(&Vec2::new(20.0, 20.0)));
            assert_eq!(
                orst_find_path(
                    e,
                    map,
                    from,
                    Vec2::new(20.0, 4.0),
                    true,
                    0.0,
                    std::ptr::null_mut(),
                    0
                ),
                0,
                "a wall"
            );
            assert_eq!(
                orst_find_path(e, 99, from, to, true, 0.0, std::ptr::null_mut(), 0),
                0,
                "no such map"
            );
            orst_engine_destroy(e);
        }
    }

    #[cfg(feature = "physics")]
    #[test]
    fn physics_bodies_fall_and_replay_from_a_snapshot() {
        use crate::components::*;
        let e = headless(8, 8);
        unsafe {
            orst_set_gravity(e, Vec2::new(0.0, 400.0));
            let put = |pos: Vec2, size: Vec2, kind: u32| {
                let ent = orst_spawn(e);
                let t = Transform {
                    pos,
                    ..Transform::default()
                };
                orst_set(e, ent, TRANSFORM, &t as *const _ as *const c_void);
                let c = Collider {
                    size,
                    ..Collider::default()
                };
                orst_set(e, ent, COLLIDER, &c as *const _ as *const c_void);
                let rb = RigidBody {
                    kind,
                    ..RigidBody::default()
                };
                orst_set(e, ent, RIGIDBODY, &rb as *const _ as *const c_void);
                ent
            };
            put(Vec2::new(0.0, 100.0), Vec2::new(300.0, 20.0), BODY_FIXED);
            let a = put(Vec2::new(0.0, 0.0), Vec2::new(16.0, 16.0), BODY_DYNAMIC);
            let b = put(Vec2::new(4.0, -30.0), Vec2::new(16.0, 16.0), BODY_DYNAMIC);
            // Bodies exist before the first step for joints and impulses
            assert!(orst_body_mass(e, a) > 0.0);
            assert!(orst_joint_rope(e, a, b, 40.0) != 0);
            assert!(orst_apply_impulse(e, b, Vec2::new(500.0, 0.0)));
            for _ in 0..20 {
                orst_physics_step(e, 1.0 / 60.0);
            }
            let mut snap = vec![0u8; orst_snapshot(e, std::ptr::null_mut(), 0)];
            orst_snapshot(e, snap.as_mut_ptr(), snap.len());
            let mut hashes = Vec::new();
            for _ in 0..60 {
                orst_physics_step(e, 1.0 / 60.0);
                hashes.push(orst_state_hash(e));
            }
            let t = *(orst_get(e, a, TRANSFORM) as *const Transform);
            assert!(
                t.pos.y > 60.0 && t.pos.y < 95.0,
                "fell onto the ground: {:?}",
                t.pos
            );
            assert!(orst_restore(e, snap.as_ptr(), snap.len()));
            for (i, h) in hashes.iter().enumerate() {
                orst_physics_step(e, 1.0 / 60.0);
                assert_eq!(orst_state_hash(e), *h, "tick {i} after the restore");
            }
            orst_engine_destroy(e);
        }
    }

    #[test]
    fn scenes_fade_over_the_frame() {
        let e = headless(8, 8);
        let white = Color::rgba(255, 255, 255, 255);
        let frame = |e: *mut OrstEngine| {
            assert!(orst_poll(e));
            orst_begin_frame(e, white);
            orst_end_frame(e);
        };
        assert!(orst_scene_go(e, 1, 0, 0.0, Color::BLACK));
        frame(e);
        assert_eq!((orst_scene(e), orst_scene_entered(e)), (1, 1));
        assert!(
            !orst_scene_go(e, 2, 9, 1.0, Color::BLACK),
            "unknown transition"
        );
        // A 1 s fade at 60 fps: covered and switched at frame 30
        assert!(orst_scene_go(e, 2, 1, 1.0, Color::BLACK));
        let mut entered_at = 0;
        for i in 1..=70 {
            frame(e);
            if orst_scene_entered(e) == 2 {
                entered_at = i;
                assert_eq!(orst_pixel(e, 4, 4), Color::BLACK, "covered at the switch");
            }
        }
        assert!(
            (29..=31).contains(&entered_at),
            "switched at frame {entered_at}"
        );
        assert!(!orst_scene_transitioning(e) && orst_pixel(e, 4, 4) == white);
        assert!(
            orst_scene_push(e, 3, 2, 0.5, Color::BLACK) && !orst_scene_pop(e, 0, 0.0, Color::BLACK)
        );
        for _ in 0..40 {
            frame(e);
        }
        assert_eq!((orst_scene(e), orst_scene_depth(e)), (3, 2));
        unsafe { orst_engine_destroy(e) };
    }

    #[test]
    fn built_in_components_move_and_draw() {
        use crate::components::*;
        let e = headless(32, 32);
        unsafe {
            let white = orst_texture_from_rgba(e, 8, 8, [255u8; 256].as_ptr());
            let map = orst_create_tilemap(e, 0, 4, 4, 8, 8, 1);
            orst_tilemap_add_tileset(e, map, 1, white, orst_sheet_grid(8, 8, 8, 8, 0, 0));
            orst_tilemap_set_tile_flag(e, map, 1, c"solid".as_ptr(), true);
            for x in 0..4 {
                orst_tilemap_set_tile(e, map, 0, x, 3, 1);
            }
            // A falling box with a body on the map, and a free spinner
            let faller = orst_spawn(e);
            let t = Transform {
                pos: Vec2::new(8.0, 4.0),
                ..Transform::default()
            };
            orst_set(e, faller, TRANSFORM, &t as *const _ as *const c_void);
            let v = Velocity {
                linear: Vec2::new(0.0, 100.0),
                angular: 0.0,
            };
            orst_set(e, faller, VELOCITY, &v as *const _ as *const c_void);
            let b = Body {
                bounds: Rect::new(-2.0, -4.0, 4.0, 4.0),
                tilemap: map,
                ..Body::default()
            };
            orst_set(e, faller, BODY, &b as *const _ as *const c_void);
            let spinner = orst_spawn(e);
            orst_set(e, spinner, TRANSFORM, std::ptr::null());
            let spin = Velocity {
                linear: Vec2::new(10.0, 0.0),
                angular: 1.0,
            };
            orst_set(e, spinner, VELOCITY, &spin as *const _ as *const c_void);
            for _ in 0..30 {
                orst_update_motion(e, 1.0 / 60.0);
            }
            let t = *(orst_get(e, faller, TRANSFORM) as *const Transform);
            let v = *(orst_get(e, faller, VELOCITY) as *const Velocity);
            let b = *(orst_get(e, faller, BODY) as *const Body);
            assert_eq!(t.pos.y, 24.0, "stands on the floor row");
            assert!(b.contacts.ground && v.linear.y == 0.0);
            let s = *(orst_get(e, spinner, TRANSFORM) as *const Transform);
            assert_eq!(s.scale, Vec2::new(1.0, 1.0), "NULL data gives the defaults");
            assert!((s.pos.x - 5.0).abs() < 1e-4 && (s.rotation - 0.5).abs() < 1e-4);

            // Sprites of entities draw at their transform
            let red = orst_texture_from_rgba(e, 1, 1, [255, 0, 0, 255].as_ptr());
            let dot = orst_spawn(e);
            let t = Transform {
                pos: Vec2::new(20.0, 2.0),
                scale: Vec2::new(4.0, 4.0),
                ..Transform::default()
            };
            orst_set(e, dot, TRANSFORM, &t as *const _ as *const c_void);
            let r = SpriteRenderer {
                texture: red,
                ..SpriteRenderer::default()
            };
            orst_set(e, dot, SPRITE_RENDERER, &r as *const _ as *const c_void);
            orst_begin_frame(e, Color::BLACK);
            orst_set_space(e, 1);
            orst_draw_entities(e);
            orst_end_frame(e);
            assert_eq!(orst_pixel(e, 21, 3), Color::rgba(255, 0, 0, 255));
            assert_eq!(orst_pixel(e, 19, 3), Color::BLACK);
            orst_engine_destroy(e);
        }
    }

    #[test]
    fn map_objects_and_strings() {
        let dir = std::env::temp_dir().join(format!("orst_ffi_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.tmj");
        std::fs::write(
            &path,
            r#"{"width": 2, "height": 2, "tilewidth": 8, "tileheight": 8, "layers": [
                {"type": "objectgroup", "name": "o", "objects": [
                    {"id": 3, "name": "door", "class": "trigger", "x": 4, "y": 6, "width": 8, "height": 2,
                     "properties": [{"name": "to", "type": "string", "value": "level2"},
                                    {"name": "locked", "type": "bool", "value": true}]}]}]}"#,
        )
        .unwrap();
        let e = headless(8, 8);
        unsafe {
            let cpath = CString::new(path.to_string_lossy().as_bytes()).unwrap();
            let map = orst_load_tilemap(e, cpath.as_ptr());
            std::fs::remove_dir_all(&dir).ok();
            assert!(map != 0);
            let mut info = OrstTilemapInfo::default();
            assert!(orst_tilemap_info(e, map, &mut info));
            assert_eq!((info.width, info.object_count), (2, 1));
            let i = orst_tilemap_find_object(e, map, c"door".as_ptr());
            assert_eq!(i, 0);
            let mut o = OrstMapObject::default();
            assert!(orst_tilemap_object(e, map, 0, &mut o));
            assert_eq!((o.id, o.rect), (3, Rect::new(4.0, 6.0, 8.0, 2.0)));
            let mut buf = [0 as c_char; 4];
            assert_eq!(orst_tilemap_object_class(e, map, 0, buf.as_mut_ptr(), 4), 7);
            assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "tri");
            let mut buf = [0 as c_char; 16];
            assert_eq!(
                orst_tilemap_object_string(e, map, 0, c"to".as_ptr(), buf.as_mut_ptr(), 16),
                6
            );
            assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "level2");
            assert_eq!(
                orst_tilemap_object_string(e, map, 0, c"nope".as_ptr(), buf.as_mut_ptr(), 16),
                -1
            );
            assert_eq!(
                orst_tilemap_object_number(e, map, 0, c"locked".as_ptr(), 0.0),
                1.0
            );
            assert_eq!(orst_tilemap_find_object(e, map, c"window".as_ptr()), -1);
            orst_engine_destroy(e);
            // NULL handles are harmless
            assert!(!orst_poll(std::ptr::null_mut()));
            orst_engine_destroy(std::ptr::null_mut());
        }
    }

    #[test]
    fn targets_effects_and_shake_through_c() {
        let e = headless(16, 16);
        unsafe {
            let t = orst_create_target(e, 4, 4);
            assert!(t != 0);
            assert!(!orst_set_target(e, 999));
            let red = Color::rgba(255, 0, 0, 255);
            orst_begin_frame(e, Color::BLACK);
            assert!(orst_set_target(e, t));
            assert_eq!(orst_get_target(e), t);
            orst_clear_target(e, t, red);
            assert!(orst_set_target(e, 0));
            orst_set_space(e, 1);
            orst_draw_texture(e, t, 0.0, 0.0);
            orst_end_frame(e);
            assert_eq!(orst_pixel(e, 2, 2), red);
            assert_eq!(orst_pixel(e, 8, 8), Color::BLACK);

            let cam = Camera::default();
            orst_set_target_camera(e, t, &cam);
            orst_set_target_camera(e, t, std::ptr::null());
            let grey = crate::post::Effect::grade(0.0, 1.0, 0.0, 1.0, Color::WHITE);
            assert!(orst_set_effects(e, 0, &grey, 1));
            orst_begin_frame(e, red);
            orst_end_frame(e);
            let p = orst_pixel(e, 0, 0);
            assert!(p.r == p.g && p.g == p.b, "{:?}", p);
            assert!(orst_set_effects(e, 0, std::ptr::null(), 0));
            let bad = crate::post::Effect {
                kind: 42,
                ..Default::default()
            };
            assert!(!orst_set_effects(e, 0, &bad, 1));
            let err = CStr::from_ptr(orst_last_error()).to_str().unwrap();
            assert!(err.contains("42"), "{}", err);

            let id = orst_create_shader(
                e,
                c"fn effect(uv: vec2<f32>, pixel: vec2<f32>) -> vec4<f32> { return load(vec2<i32>(pixel)); }".as_ptr(),
            );
            assert!(id != 0);
            orst_free_shader(e, id);
            orst_shake(e, 4.0, 0.2);

            // Lighting: a default light (white, radius 160) at the centre
            let lit = crate::post::Effect::lighting(Color::BLACK, 1.0);
            assert!(orst_set_effects(e, 0, &lit, 1));
            let lamp = orst_spawn(e);
            assert!(orst_set(
                e,
                lamp,
                crate::components::TRANSFORM,
                std::ptr::null()
            ));
            assert!(orst_set(
                e,
                lamp,
                crate::components::LIGHT,
                std::ptr::null()
            ));
            orst_set_shadow_layers(e, 1);
            orst_set_shadow_tilemap(e, 1, true);
            orst_set_normal_map(e, t, 0);
            orst_set_space(e, 1);
            orst_begin_frame(e, Color::WHITE);
            orst_end_frame(e);
            assert!(orst_pixel(e, 8, 8).r > 200, "lit near the light");
            orst_engine_destroy(e);
        }
    }
}

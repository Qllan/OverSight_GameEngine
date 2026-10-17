//! C API (include/oversight.h). Every function takes the engine handle
//! made by `ov_engine_create`; resources are numeric ids, 0 meaning
//! "none" or "failed". When a call fails, `ov_last_error` says why.
//!
//! Strings are UTF-8, NUL-terminated. Strings the engine returns stay
//! valid until the next call that returns a string of the same kind.
//!
//! The handle contract above applies to every function, so they are not
//! each marked `unsafe` for Rust callers.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};

use crate::anim::{AnimMode, Animation, SpriteSheet};
use crate::camera::Camera;
use crate::collision::{self, Contacts, MoveResult, RayHit};
use crate::engine::{Engine, InputEvent};
use crate::input::{key_from_u32, MouseButton};
use crate::tilemap::{LayerDraw, Orientation, Property, Tilemap};
use crate::types::{Color, Rect, Space, Sprite, Vec2};

/// The engine plus strings handed out to C
pub struct OvEngine {
    engine: Engine,
    text: CString,
}

#[repr(C)]
pub struct OvConfig {
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
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct OvTilemapInfo {
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
pub struct OvMapObject {
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

// Handles from C are trusted to be live pointers from ov_engine_create;
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
pub extern "C" fn ov_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// # Safety
/// `config` is NULL (defaults) or a valid config
#[no_mangle]
pub unsafe extern "C" fn ov_engine_create(config: *const OvConfig) -> *mut OvEngine {
    let c = config.as_ref();
    let width = c.map_or(960, |c| c.width);
    let height = c.map_or(540, |c| c.height);
    let headless = c.is_some_and(|c| c.headless);
    let engine = if headless || cfg!(not(feature = "window")) {
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
            }
            Engine::windowed(wc)
        }
        #[cfg(not(feature = "window"))]
        unreachable!()
    };
    match engine {
        Ok(engine) => Box::into_raw(Box::new(OvEngine {
            engine,
            text: CString::default(),
        })),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `e` is NULL or a handle from `ov_engine_create`, not used afterwards
#[no_mangle]
pub unsafe extern "C" fn ov_engine_destroy(e: *mut OvEngine) {
    if !e.is_null() {
        drop(Box::from_raw(e));
    }
}

#[no_mangle]
pub extern "C" fn ov_poll(e: *mut OvEngine) -> bool {
    let engine = engine!(e, false);
    let alive = engine.poll();
    // Errors the engine collected (presenting, sound device) go to C
    if let Some(err) = engine.take_error() {
        set_error(err);
    }
    alive
}

#[no_mangle]
pub extern "C" fn ov_quit(e: *mut OvEngine) {
    engine!(e).quit();
}

#[no_mangle]
pub extern "C" fn ov_dt(e: *mut OvEngine) -> f32 {
    engine!(e, 0.0).time.dt
}

#[no_mangle]
pub extern "C" fn ov_time(e: *mut OvEngine) -> f64 {
    engine!(e, 0.0).time.total
}

#[no_mangle]
pub extern "C" fn ov_fps(e: *mut OvEngine) -> f32 {
    engine!(e, 0.0).time.fps
}

#[no_mangle]
pub extern "C" fn ov_frame(e: *mut OvEngine) -> u64 {
    engine!(e, 0).time.frame
}

#[no_mangle]
pub extern "C" fn ov_begin_frame(e: *mut OvEngine, clear: Color) {
    engine!(e).begin_frame(clear);
}

#[no_mangle]
pub extern "C" fn ov_end_frame(e: *mut OvEngine) {
    let engine = engine!(e);
    engine.end_frame();
    if let Some(err) = engine.take_error() {
        set_error(err);
    }
}

/// # Safety
/// `title` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn ov_set_title(e: *mut OvEngine, title: *const c_char) {
    engine!(e).set_title(string(title));
}

#[no_mangle]
pub extern "C" fn ov_set_fullscreen(e: *mut OvEngine, on: bool) {
    engine!(e).set_fullscreen(on);
}

#[no_mangle]
pub extern "C" fn ov_is_fullscreen(e: *mut OvEngine) -> bool {
    engine!(e, false).fullscreen()
}

#[no_mangle]
pub extern "C" fn ov_set_target_fps(e: *mut OvEngine, fps: u32) {
    engine!(e).set_target_fps(fps);
}

#[no_mangle]
pub extern "C" fn ov_has_window(e: *mut OvEngine) -> bool {
    engine!(e, false).has_window()
}

/// Canvas size (the logical resolution)
///
/// # Safety
/// `w` and `h` are NULL or valid
#[no_mangle]
pub unsafe extern "C" fn ov_canvas_size(e: *mut OvEngine, w: *mut u32, h: *mut u32) {
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
pub unsafe extern "C" fn ov_save_screenshot(e: *mut OvEngine, path: *const c_char) -> bool {
    let engine = engine!(e, false);
    result(engine.save_screenshot(string(path)).map(|_| true), false)
}

/// Colour of a canvas pixel of the last frame (transparent outside)
#[no_mangle]
pub extern "C" fn ov_pixel(e: *mut OvEngine, x: u32, y: u32) -> Color {
    engine!(e, Color::TRANSPARENT)
        .pixel(x, y)
        .unwrap_or(Color::TRANSPARENT)
}

// --- textures and fonts ------------------------------------------------------

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn ov_load_texture(e: *mut OvEngine, path: *const c_char) -> u32 {
    let engine = engine!(e, 0);
    result(engine.load_texture(string(path)), 0)
}

/// # Safety
/// `rgba` points to `width * height * 4` bytes
#[no_mangle]
pub unsafe extern "C" fn ov_texture_from_rgba(
    e: *mut OvEngine,
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
pub unsafe extern "C" fn ov_texture_size(
    e: *mut OvEngine,
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
pub extern "C" fn ov_set_texture_smooth(e: *mut OvEngine, texture: u32, smooth: bool) {
    engine!(e).set_texture_smooth(texture, smooth);
}

#[no_mangle]
pub extern "C" fn ov_free_texture(e: *mut OvEngine, texture: u32) {
    engine!(e).free_texture(texture);
}

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn ov_load_font(e: *mut OvEngine, path: *const c_char) -> u32 {
    let engine = engine!(e, 0);
    result(engine.load_font(string(path)), 0)
}

#[no_mangle]
pub extern "C" fn ov_free_font(e: *mut OvEngine, font: u32) {
    engine!(e).free_font(font);
}

/// # Safety
/// `text` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn ov_measure_text(
    e: *mut OvEngine,
    font: u32,
    text: *const c_char,
    size: f32,
) -> Vec2 {
    engine!(e, Vec2::default()).measure_text(font, string(text), size)
}

// --- camera and draw state ---------------------------------------------------

#[no_mangle]
pub extern "C" fn ov_get_camera(e: *mut OvEngine) -> Camera {
    engine!(e, Camera::default()).camera()
}

#[no_mangle]
pub extern "C" fn ov_set_camera(e: *mut OvEngine, camera: Camera) {
    engine!(e).set_camera(camera);
}

#[no_mangle]
pub extern "C" fn ov_world_to_screen(e: *mut OvEngine, p: Vec2) -> Vec2 {
    engine!(e, p).world_to_screen(p)
}

#[no_mangle]
pub extern "C" fn ov_screen_to_world(e: *mut OvEngine, p: Vec2) -> Vec2 {
    engine!(e, p).screen_to_world(p)
}

#[no_mangle]
pub extern "C" fn ov_set_layer(e: *mut OvEngine, layer: i32) {
    engine!(e).set_layer(layer);
}

#[no_mangle]
pub extern "C" fn ov_set_depth(e: *mut OvEngine, depth: f32) {
    engine!(e).set_depth(depth);
}

/// 0: world (moved by the camera), 1: screen pixels
#[no_mangle]
pub extern "C" fn ov_set_space(e: *mut OvEngine, space_: i32) {
    engine!(e).set_space(space(space_));
}

#[no_mangle]
pub extern "C" fn ov_set_antialias(e: *mut OvEngine, on: bool) {
    engine!(e).set_antialias(on);
}

// --- drawing -----------------------------------------------------------------

/// # Safety
/// `sprite` is a valid sprite
#[no_mangle]
pub unsafe extern "C" fn ov_draw_sprite(e: *mut OvEngine, texture: u32, sprite: *const Sprite) {
    let engine = engine!(e);
    if let Some(s) = sprite.as_ref() {
        engine.draw_sprite(texture, s);
    }
}

#[no_mangle]
pub extern "C" fn ov_draw_texture(e: *mut OvEngine, texture: u32, x: f32, y: f32) {
    engine!(e).draw_texture(texture, x, y);
}

/// # Safety
/// `sheet` and `sprite` are valid
#[no_mangle]
pub unsafe extern "C" fn ov_draw_frame(
    e: *mut OvEngine,
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
pub extern "C" fn ov_fill_rect(e: *mut OvEngine, rect: Rect, color: Color) {
    engine!(e).fill_rect(rect, color);
}

#[no_mangle]
pub extern "C" fn ov_draw_rect(e: *mut OvEngine, rect: Rect, thickness: f32, color: Color) {
    engine!(e).draw_rect(rect, thickness, color);
}

#[no_mangle]
pub extern "C" fn ov_draw_line(
    e: *mut OvEngine,
    from: Vec2,
    to: Vec2,
    thickness: f32,
    color: Color,
) {
    engine!(e).draw_line(from, to, thickness, color);
}

#[no_mangle]
pub extern "C" fn ov_fill_circle(e: *mut OvEngine, center: Vec2, radius: f32, color: Color) {
    engine!(e).fill_circle(center, radius, color);
}

#[no_mangle]
pub extern "C" fn ov_draw_circle(
    e: *mut OvEngine,
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
pub unsafe extern "C" fn ov_draw_text(
    e: *mut OvEngine,
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
pub extern "C" fn ov_key_down(e: *mut OvEngine, key: i32) -> bool {
    engine!(e, false).input.key_down(key_from_u32(key as u32))
}

#[no_mangle]
pub extern "C" fn ov_key_pressed(e: *mut OvEngine, key: i32) -> bool {
    engine!(e, false)
        .input
        .key_pressed(key_from_u32(key as u32))
}

#[no_mangle]
pub extern "C" fn ov_key_released(e: *mut OvEngine, key: i32) -> bool {
    engine!(e, false)
        .input
        .key_released(key_from_u32(key as u32))
}

/// -1, 0 or 1 from two keys (e.g. A and D)
#[no_mangle]
pub extern "C" fn ov_axis(e: *mut OvEngine, negative: i32, positive: i32) -> f32 {
    let engine = engine!(e, 0.0);
    engine
        .input
        .axis(key_from_u32(negative as u32), key_from_u32(positive as u32))
}

#[no_mangle]
pub extern "C" fn ov_mouse_down(e: *mut OvEngine, button: i32) -> bool {
    let engine = engine!(e, false);
    mouse_button(button).is_some_and(|b| engine.input.mouse_down(b))
}

#[no_mangle]
pub extern "C" fn ov_mouse_pressed(e: *mut OvEngine, button: i32) -> bool {
    let engine = engine!(e, false);
    mouse_button(button).is_some_and(|b| engine.input.mouse_pressed(b))
}

#[no_mangle]
pub extern "C" fn ov_mouse_released(e: *mut OvEngine, button: i32) -> bool {
    let engine = engine!(e, false);
    mouse_button(button).is_some_and(|b| engine.input.mouse_released(b))
}

/// Mouse in canvas pixels
#[no_mangle]
pub extern "C" fn ov_mouse_pos(e: *mut OvEngine) -> Vec2 {
    engine!(e, Vec2::default()).input.mouse
}

#[no_mangle]
pub extern "C" fn ov_mouse_world(e: *mut OvEngine) -> Vec2 {
    engine!(e, Vec2::default()).mouse_world()
}

#[no_mangle]
pub extern "C" fn ov_mouse_wheel(e: *mut OvEngine) -> f32 {
    engine!(e, 0.0).input.wheel
}

/// Text typed this frame (valid until the next call)
///
/// # Safety
/// `e` is NULL or a live handle
#[no_mangle]
pub unsafe extern "C" fn ov_text_input(e: *mut OvEngine) -> *const c_char {
    static EMPTY: &[u8] = b"\0";
    let Some(h) = e.as_mut() else {
        return EMPTY.as_ptr() as *const c_char;
    };
    h.text = CString::new(h.engine.input.text.replace('\0', "")).unwrap_or_default();
    h.text.as_ptr()
}

/// Feed a key change, applied at the next `ov_poll` (tests, replays)
#[no_mangle]
pub extern "C" fn ov_inject_key(e: *mut OvEngine, key: i32, down: bool) {
    engine!(e).inject(InputEvent::Key(key_from_u32(key as u32), down));
}

#[no_mangle]
pub extern "C" fn ov_inject_mouse_button(e: *mut OvEngine, button: i32, down: bool) {
    let engine = engine!(e);
    if let Some(b) = mouse_button(button) {
        engine.inject(InputEvent::MouseButton(b, down));
    }
}

#[no_mangle]
pub extern "C" fn ov_inject_mouse_move(e: *mut OvEngine, pos: Vec2) {
    engine!(e).inject(InputEvent::MouseMove(pos));
}

// --- sprite sheets and animations --------------------------------------------

#[no_mangle]
pub extern "C" fn ov_sheet_grid(
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
pub extern "C" fn ov_sheet_frame(sheet: SpriteSheet, index: u32) -> Rect {
    sheet.frame(index)
}

/// Frame of an animation at `time` seconds; mode 0 loop, 1 once,
/// 2 ping-pong
///
/// # Safety
/// `frames` points to `count` values
#[no_mangle]
pub unsafe extern "C" fn ov_anim_frame_at(
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

// --- tile maps -----------------------------------------------------------------

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn ov_load_tilemap(e: *mut OvEngine, path: *const c_char) -> u32 {
    let engine = engine!(e, 0);
    result(engine.load_tilemap(string(path)), 0)
}

/// An empty map to fill with `ov_tilemap_set_tile`; it has `layers`
/// tile layers and no tilesets (add one with `ov_tilemap_add_tileset`)
#[no_mangle]
pub extern "C" fn ov_create_tilemap(
    e: *mut OvEngine,
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
pub extern "C" fn ov_tilemap_add_tileset(
    e: *mut OvEngine,
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
pub unsafe extern "C" fn ov_tilemap_set_tile_flag(
    e: *mut OvEngine,
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
pub extern "C" fn ov_free_tilemap(e: *mut OvEngine, map: u32) {
    engine!(e).free_tilemap(map);
}

/// # Safety
/// `info` is valid
#[no_mangle]
pub unsafe extern "C" fn ov_tilemap_info(
    e: *mut OvEngine,
    map: u32,
    info: *mut OvTilemapInfo,
) -> bool {
    let (Some(m), Some(info)) = (engine!(e, false).tilemap(map), info.as_mut()) else {
        return false;
    };
    *info = OvTilemapInfo {
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
pub unsafe extern "C" fn ov_tilemap_layer_index(
    e: *mut OvEngine,
    map: u32,
    name: *const c_char,
) -> i32 {
    engine!(e, -1)
        .tilemap(map)
        .and_then(|m| m.layer_index(string(name)))
        .map_or(-1, |i| i as i32)
}

#[no_mangle]
pub extern "C" fn ov_tilemap_get_tile(
    e: *mut OvEngine,
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
pub extern "C" fn ov_tilemap_set_tile(
    e: *mut OvEngine,
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
pub extern "C" fn ov_tilemap_is_solid(e: *mut OvEngine, map: u32, x: i32, y: i32) -> bool {
    engine!(e, false)
        .tilemap(map)
        .is_some_and(|m| m.is_solid(x, y))
}

#[no_mangle]
pub extern "C" fn ov_tilemap_tile_to_world(e: *mut OvEngine, map: u32, tile: Vec2) -> Vec2 {
    engine!(e, Vec2::default())
        .tilemap(map)
        .map_or(Vec2::default(), |m| m.tile_to_world(tile.x, tile.y))
}

#[no_mangle]
pub extern "C" fn ov_tilemap_world_to_tile(e: *mut OvEngine, map: u32, p: Vec2) -> Vec2 {
    engine!(e, Vec2::default())
        .tilemap(map)
        .map_or(Vec2::default(), |m| m.world_to_tile(p))
}

#[no_mangle]
pub extern "C" fn ov_tilemap_cell_center(e: *mut OvEngine, map: u32, x: i32, y: i32) -> Vec2 {
    engine!(e, Vec2::default())
        .tilemap(map)
        .map_or(Vec2::default(), |m| m.cell_center(x, y))
}

/// Every tile layer, layer i on draw layer `base + i`
#[no_mangle]
pub extern "C" fn ov_draw_tilemap(e: *mut OvEngine, map: u32, base: i32) {
    engine!(e).draw_tilemap(map, base);
}

/// One tile layer; with `y_sort` each tile's depth is the bottom of its
/// cell, so it sorts with sprites whose depth is their feet's y
#[no_mangle]
pub extern "C" fn ov_draw_tilemap_layer(
    e: *mut OvEngine,
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
pub unsafe extern "C" fn ov_tilemap_object(
    e: *mut OvEngine,
    map: u32,
    index: u32,
    out: *mut OvMapObject,
) -> bool {
    let engine = engine!(e, false);
    let (Some((layer, o)), Some(out)) = (
        engine.tilemap(map).and_then(|m| object(m, index)),
        out.as_mut(),
    ) else {
        return false;
    };
    *out = OvMapObject {
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
pub unsafe extern "C" fn ov_tilemap_find_object(
    e: *mut OvEngine,
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
pub unsafe extern "C" fn ov_tilemap_object_name(
    e: *mut OvEngine,
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
pub unsafe extern "C" fn ov_tilemap_object_class(
    e: *mut OvEngine,
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
pub unsafe extern "C" fn ov_tilemap_object_number(
    e: *mut OvEngine,
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
pub unsafe extern "C" fn ov_tilemap_object_string(
    e: *mut OvEngine,
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
pub extern "C" fn ov_move_box(e: *mut OvEngine, map: u32, rect: Rect, delta: Vec2) -> MoveResult {
    let fallback = MoveResult {
        rect: Rect::new(rect.x + delta.x, rect.y + delta.y, rect.w, rect.h),
        contacts: Contacts::default(),
    };
    engine!(e, fallback).move_box(map, rect, delta)
}

#[no_mangle]
pub extern "C" fn ov_touching(e: *mut OvEngine, map: u32, rect: Rect) -> Contacts {
    engine!(e, Contacts::default())
        .tilemap(map)
        .map_or(Contacts::default(), |m| collision::touching(m, rect))
}

/// # Safety
/// `hit` is NULL or valid
#[no_mangle]
pub unsafe extern "C" fn ov_raycast(
    e: *mut OvEngine,
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
pub extern "C" fn ov_point_solid(e: *mut OvEngine, map: u32, p: Vec2) -> bool {
    engine!(e, false)
        .tilemap(map)
        .is_some_and(|m| collision::point_solid(m, p))
}

#[no_mangle]
pub extern "C" fn ov_rect_solid(e: *mut OvEngine, map: u32, rect: Rect) -> bool {
    engine!(e, false)
        .tilemap(map)
        .is_some_and(|m| collision::rect_solid(m, rect))
}

#[no_mangle]
pub extern "C" fn ov_rects_overlap(a: Rect, b: Rect) -> bool {
    a.overlaps(&b)
}

// --- sound ---------------------------------------------------------------------
// Built without the "audio" feature these do nothing and return 0/false.

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn ov_load_sound(e: *mut OvEngine, path: *const c_char) -> u32 {
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
pub extern "C" fn ov_free_sound(e: *mut OvEngine, _sound: u32) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.free_sound(_sound);
}

/// Returns a voice id for `ov_stop_voice` (0 when nothing plays)
#[no_mangle]
pub extern "C" fn ov_play_sound(e: *mut OvEngine, _sound: u32, _volume: f32, _pitch: f32) -> u32 {
    let _engine = engine!(e, 0);
    #[cfg(feature = "audio")]
    return _engine.audio.play_sound(_sound, _volume, _pitch);
    #[cfg(not(feature = "audio"))]
    0
}

#[no_mangle]
pub extern "C" fn ov_stop_voice(e: *mut OvEngine, _voice: u32) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.stop_voice(_voice);
}

#[no_mangle]
pub extern "C" fn ov_voice_playing(e: *mut OvEngine, _voice: u32) -> bool {
    let _engine = engine!(e, false);
    #[cfg(feature = "audio")]
    return _engine.audio.voice_playing(_voice);
    #[cfg(not(feature = "audio"))]
    false
}

#[no_mangle]
pub extern "C" fn ov_stop_all_sounds(e: *mut OvEngine) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.stop_all_sounds();
}

/// # Safety
/// `path` is a NUL-terminated string
#[no_mangle]
pub unsafe extern "C" fn ov_play_music(
    e: *mut OvEngine,
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
pub extern "C" fn ov_stop_music(e: *mut OvEngine) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.stop_music();
}

#[no_mangle]
pub extern "C" fn ov_pause_music(e: *mut OvEngine, _paused: bool) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.pause_music(_paused);
}

#[no_mangle]
pub extern "C" fn ov_music_playing(e: *mut OvEngine) -> bool {
    let _engine = engine!(e, false);
    #[cfg(feature = "audio")]
    return _engine.audio.music_playing();
    #[cfg(not(feature = "audio"))]
    false
}

#[no_mangle]
pub extern "C" fn ov_set_music_volume(e: *mut OvEngine, _volume: f32) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.set_music_volume(_volume);
}

#[no_mangle]
pub extern "C" fn ov_set_master_volume(e: *mut OvEngine, _volume: f32) {
    let _engine = engine!(e);
    #[cfg(feature = "audio")]
    _engine.audio.set_master_volume(_volume);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headless(w: u32, h: u32) -> *mut OvEngine {
        let config = OvConfig {
            title: std::ptr::null(),
            width: w,
            height: h,
            scale: 1.0,
            resizable: false,
            fullscreen: false,
            integer_scale: false,
            target_fps: 0,
            headless: true,
        };
        unsafe { ov_engine_create(&config) }
    }

    #[test]
    fn draws_moves_and_reports_errors_through_c() {
        let e = headless(32, 32);
        assert!(!e.is_null());
        unsafe {
            assert_eq!(ov_load_texture(e, c"missing.png".as_ptr()), 0);
            let err = CStr::from_ptr(ov_last_error()).to_str().unwrap();
            assert!(err.contains("missing.png"), "{}", err);

            let white = ov_texture_from_rgba(e, 8, 8, [255u8; 256].as_ptr());
            let map = ov_create_tilemap(e, 0, 4, 4, 8, 8, 1);
            assert!(ov_tilemap_add_tileset(
                e,
                map,
                1,
                white,
                ov_sheet_grid(8, 8, 8, 8, 0, 0)
            ));
            assert!(ov_tilemap_set_tile_flag(e, map, 1, c"solid".as_ptr(), true));
            for x in 0..4 {
                ov_tilemap_set_tile(e, map, 0, x, 3, 1);
            }
            let r = ov_move_box(e, map, Rect::new(2.0, 2.0, 4.0, 4.0), Vec2::new(0.0, 50.0));
            assert!(r.contacts.ground && r.rect.bottom() == 24.0);
            let mut hit = RayHit::default();
            assert!(ov_raycast(
                e,
                map,
                Vec2::new(4.0, 0.0),
                Vec2::new(4.0, 40.0),
                &mut hit
            ));
            assert_eq!(hit.cell_y, 3);

            assert!(ov_poll(e));
            ov_set_camera(
                e,
                Camera {
                    center: Vec2::new(16.0, 16.0),
                    ..Camera::default()
                },
            );
            ov_begin_frame(e, Color::BLACK);
            ov_draw_tilemap(e, map, 0);
            ov_set_space(e, 1);
            ov_fill_rect(
                e,
                Rect::new(0.0, 0.0, 4.0, 4.0),
                Color::rgba(255, 0, 0, 255),
            );
            ov_end_frame(e);
            assert_eq!(ov_pixel(e, 1, 1), Color::rgba(255, 0, 0, 255));
            assert_eq!(ov_pixel(e, 10, 28), Color::WHITE);

            let frames = [3u32, 4, 5];
            assert_eq!(ov_anim_frame_at(frames.as_ptr(), 3, 10.0, 0, 0.15), 4);
            ov_inject_key(e, 1, true); // A
            ov_poll(e);
            assert!(ov_key_pressed(e, 1));
            assert_eq!(ov_axis(e, 1, 4), -1.0);
            ov_engine_destroy(e);
        }
    }

    #[test]
    fn map_objects_and_strings() {
        let dir = std::env::temp_dir().join(format!("oversight_ffi_{}", std::process::id()));
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
            let map = ov_load_tilemap(e, cpath.as_ptr());
            std::fs::remove_dir_all(&dir).ok();
            assert!(map != 0);
            let mut info = OvTilemapInfo::default();
            assert!(ov_tilemap_info(e, map, &mut info));
            assert_eq!((info.width, info.object_count), (2, 1));
            let i = ov_tilemap_find_object(e, map, c"door".as_ptr());
            assert_eq!(i, 0);
            let mut o = OvMapObject::default();
            assert!(ov_tilemap_object(e, map, 0, &mut o));
            assert_eq!((o.id, o.rect), (3, Rect::new(4.0, 6.0, 8.0, 2.0)));
            let mut buf = [0 as c_char; 4];
            assert_eq!(ov_tilemap_object_class(e, map, 0, buf.as_mut_ptr(), 4), 7);
            assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "tri");
            let mut buf = [0 as c_char; 16];
            assert_eq!(
                ov_tilemap_object_string(e, map, 0, c"to".as_ptr(), buf.as_mut_ptr(), 16),
                6
            );
            assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "level2");
            assert_eq!(
                ov_tilemap_object_string(e, map, 0, c"nope".as_ptr(), buf.as_mut_ptr(), 16),
                -1
            );
            assert_eq!(
                ov_tilemap_object_number(e, map, 0, c"locked".as_ptr(), 0.0),
                1.0
            );
            assert_eq!(ov_tilemap_find_object(e, map, c"window".as_ptr()), -1);
            ov_engine_destroy(e);
            // NULL handles are harmless
            assert!(!ov_poll(std::ptr::null_mut()));
            ov_engine_destroy(std::ptr::null_mut());
        }
    }
}

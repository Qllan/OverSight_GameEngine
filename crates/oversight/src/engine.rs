//! Engine: the object a game talks to. It owns the renderer, the loaded
//! resources, the window, input and timing.
//!
//! A game runs its own loop:
//!
//! ```no_run
//! # use oversight::*;
//! let mut engine = Engine::windowed(WindowConfig::default()).unwrap();
//! while engine.poll() {
//!     let dt = engine.time.dt;
//!     engine.begin_frame(Color::BLACK);
//!     // update with dt, then draw
//!     engine.end_frame();
//! }
//! ```

use crate::anim::SpriteSheet;
#[cfg(feature = "audio")]
use crate::audio::Audio;
use crate::camera::Camera;
use crate::collision::{self, MoveResult, RayHit};
use crate::debug::{DebugOverlay, Stats};
use crate::ecs::World;
use crate::gamepad::{GamepadAxis, GamepadButton};
use crate::hotreload::{HotReload, MapFiles, Watched};
use crate::input::{Input, Key, MouseButton};
use crate::particles::{Emitter, EmitterConfig};
use crate::post::Effect;
use crate::render::{DrawKind, Renderer, Shape};
use crate::rng::Rng;
use crate::skeleton::{Skeleton, SkeletonData};
use crate::text::{Font, DEFAULT_FONT};
use crate::texture::{Store, Texture};
use crate::tilemap::{LayerDraw, Tilemap};
use crate::time::Time;
use crate::types::{Color, Rect, Space, Sprite, Vec2};
#[cfg(feature = "window")]
use crate::window::{Window, WindowConfig};

/// Font id of the built-in DejaVu Sans
pub const DEFAULT_FONT_ID: u32 = 1;

/// Layer, depth and space used by shape and text calls
#[derive(Debug, Clone, Copy, Default)]
pub struct DrawState {
    pub layer: i32,
    pub depth: f32,
    pub space: Space,
}

/// Start of every snapshot; the digit changes when the format does
const SNAPSHOT_MAGIC: &[u8; 4] = b"OVS2";

pub struct Engine {
    pub renderer: Renderer,
    pub textures: Store<Texture>,
    pub fonts: Store<Font>,
    pub tilemaps: Store<Tilemap>,
    pub emitters: Store<Emitter>,
    /// Loaded skeleton files, shared by the skeletons made from them
    pub skeleton_data: Store<std::sync::Arc<SkeletonData>>,
    pub skeletons: Store<Skeleton>,
    /// Convex polygons for colliders, in local units
    pub polygons: Store<Vec<Vec2>>,
    /// WGSL sources of post effect shaders
    pub shaders: Store<String>,
    /// Collider layers that cast shadows
    shadow_layers: u32,
    /// Maps whose solid cells cast shadows
    shadow_maps: std::collections::BTreeSet<u32>,
    pub state: DrawState,
    pub input: Input,
    /// Named actions bound to keys, buttons and sticks
    pub actions: crate::actions::Actions,
    pub time: Time,
    /// Seeded random numbers for game logic (same seed, same game)
    pub rng: Rng,
    /// Entities and their components
    pub world: World,
    /// Which scene is on, and the transition between scenes
    pub scenes: crate::scenes::Scenes,
    /// Overlaps between colliders from the last collision pass
    pub collisions: crate::collisions::Collisions,
    /// Bodies with mass and joints (Rapier)
    pub physics: crate::physics::Physics,
    pub debug: DebugOverlay,
    pub hot_reload: HotReload,
    #[cfg(feature = "audio")]
    pub audio: Audio,
    #[cfg(feature = "window")]
    pub window: Option<Window>,
    /// Real controllers; only a windowed engine reads them, so tests and
    /// headless runs depend on injected input alone
    #[cfg(feature = "gamepad")]
    gamepads: Option<crate::gamepad::Backend>,
    /// Draws frames on the graphics card; None: the software renderer
    #[cfg(feature = "gpu")]
    gpu: Option<crate::gpu::Gpu>,
    /// The software canvas is behind the GPU's frame (read it back before
    /// screenshots and pixel reads)
    canvas_stale: bool,
    /// Input events injected by code, applied at the next poll
    injected: Vec<InputEvent>,
    quit: bool,
    last_error: Option<String>,
}

/// An input change injected by code (tests, replays, bots)
#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    Key(Key, bool),
    MouseButton(MouseButton, bool),
    MouseMove(Vec2),
    Wheel(f32),
    /// Slot 0..3 plugged in or unplugged
    GamepadConnected(usize, bool),
    GamepadButton(usize, GamepadButton, bool),
    /// Raw value, before the dead zone (y down, triggers 0..1)
    GamepadAxis(usize, GamepadAxis, f32),
}

impl Engine {
    /// An engine without a window: frames are rendered to an offscreen
    /// canvas (tests, servers, screenshots)
    pub fn headless(width: u32, height: u32) -> Result<Engine, String> {
        let mut fonts = Store::default();
        fonts.insert(Font::from_bytes(DEFAULT_FONT)?);
        Ok(Engine {
            renderer: Renderer::new(width, height)?,
            textures: Store::default(),
            fonts,
            tilemaps: Store::default(),
            emitters: Store::default(),
            skeleton_data: Store::default(),
            skeletons: Store::default(),
            polygons: Store::default(),
            shaders: Store::default(),
            shadow_layers: 0,
            shadow_maps: Default::default(),
            state: DrawState::default(),
            input: Input::default(),
            actions: Default::default(),
            time: Time::new(Some(1.0 / 60.0)),
            rng: Rng::default(),
            world: {
                let mut w = World::new();
                crate::components::register(&mut w);
                w
            },
            scenes: Default::default(),
            collisions: Default::default(),
            physics: Default::default(),
            debug: DebugOverlay::default(),
            hot_reload: HotReload::default(),
            #[cfg(feature = "audio")]
            audio: Audio::silent(),
            #[cfg(feature = "window")]
            window: None,
            #[cfg(feature = "gamepad")]
            gamepads: None,
            #[cfg(feature = "gpu")]
            gpu: None,
            canvas_stale: false,
            injected: Vec::new(),
            quit: false,
            last_error: None,
        })
    }

    /// An engine with a window; the canvas has the configured size and is
    /// scaled to fit the window
    #[cfg(feature = "window")]
    pub fn windowed(config: WindowConfig) -> Result<Engine, String> {
        let mut engine = Engine::headless(config.width, config.height)?;
        let software = config.software;
        engine.window = Some(Window::open(config)?);
        // The graphics card when there is one; the CPU otherwise, and the
        // error says why
        #[cfg(feature = "gpu")]
        if !software {
            let handle = engine.window.as_ref().and_then(|w| w.handle());
            let size = engine.renderer.size();
            match handle.map(|h| crate::gpu::Gpu::for_window(h, size)) {
                Some(Ok(gpu)) => engine.gpu = Some(gpu),
                Some(Err(e)) => engine.last_error = Some(format!("drawing in software: {}", e)),
                None => {}
            }
        }
        #[cfg(not(feature = "gpu"))]
        let _ = software;
        engine.time = Time::new(None);
        // A game without sound still runs; the error says why it is quiet
        #[cfg(feature = "audio")]
        match Audio::open() {
            Ok(audio) => engine.audio = audio,
            Err(e) => engine.last_error = Some(format!("no sound: {}", e)),
        }
        #[cfg(feature = "gamepad")]
        match crate::gamepad::Backend::new(&mut engine.input.gamepads) {
            Ok(b) => engine.gamepads = Some(b),
            Err(e) => {
                let msg = format!("no gamepads: {}", e);
                engine.last_error.get_or_insert(msg);
            }
        }
        Ok(engine)
    }

    /// An engine without a window that draws on the graphics card (or the
    /// system's software adapter); for comparing the two renderers
    #[cfg(feature = "gpu")]
    pub fn headless_gpu(width: u32, height: u32) -> Result<Engine, String> {
        let mut engine = Engine::headless(width, height)?;
        engine.gpu = Some(crate::gpu::Gpu::headless((width, height))?);
        Ok(engine)
    }

    /// "gpu: <adapter>" or "software"
    pub fn renderer_name(&self) -> String {
        #[cfg(feature = "gpu")]
        if let Some(gpu) = &self.gpu {
            return format!("gpu: {}", gpu.adapter_name);
        }
        "software".to_string()
    }

    /// Start a frame: read window events and injected input and advance
    /// the clock. Returns false when the game should end (window closed
    /// or `quit` called).
    pub fn poll(&mut self) -> bool {
        self.input.next_frame();
        #[cfg(feature = "window")]
        if let Some(window) = self.window.as_mut() {
            let canvas = self.renderer.size();
            if !window.pump(&mut self.input, canvas) {
                self.quit = true;
            }
        }
        #[cfg(feature = "gamepad")]
        if let Some(b) = self.gamepads.as_mut() {
            b.pump(&mut self.input.gamepads);
        }
        for event in std::mem::take(&mut self.injected) {
            let pads = &mut self.input.gamepads;
            match event {
                InputEvent::GamepadConnected(pad, true) => pads.connect(pad, "Injected"),
                InputEvent::GamepadConnected(pad, false) => pads.disconnect(pad),
                InputEvent::GamepadButton(pad, b, down) => pads.set_button(pad, b, down),
                InputEvent::GamepadAxis(pad, axis, v) => pads.set_axis(pad, axis, v),
                InputEvent::Key(key, down) => self.input.set_key(key, down),
                InputEvent::MouseButton(b, down) => self.input.set_mouse_button(b, down),
                InputEvent::MouseMove(p) => self.input.mouse = p,
                InputEvent::Wheel(w) => self.input.wheel += w,
            }
        }
        self.actions.update(&self.input);
        if let Some(key) = self.debug.toggle_key {
            if self.input.key_pressed(key) {
                self.debug.enabled = !self.debug.enabled;
            }
        }
        #[cfg(feature = "audio")]
        self.audio.update();
        self.time.tick();
        self.scenes.update(self.time.dt);
        self.hot_reload.reloaded = 0;
        if self.hot_reload.enabled {
            self.hot_reload.timer += self.time.dt;
            if self.hot_reload.timer >= self.hot_reload.interval {
                self.hot_reload.timer = 0.0;
                self.hot_reload.reloaded = self.reload_changed();
            }
        }
        !self.quit
    }

    /// The game state as bytes: the entities and components, the random
    /// generator and the tick counter. Restoring it continues the game
    /// exactly from there (saves, rollback, replays). Textures, maps and
    /// sounds are not in it: they are assets the game loads by itself.
    pub fn snapshot(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(SNAPSHOT_MAGIC);
        out.extend_from_slice(&self.rng.state().to_le_bytes());
        out.extend_from_slice(&self.time.tick_count.to_le_bytes());
        // Which pairs touch, so the next pass reports the same begins
        let touching = self.collisions.touching();
        out.extend_from_slice(&(touching.len() as u32).to_le_bytes());
        for &(a, b) in touching {
            out.extend_from_slice(&a.to_le_bytes());
            out.extend_from_slice(&b.to_le_bytes());
        }
        // The physics bodies, contacts and joints, so a restored game
        // steps on exactly as before
        #[cfg(feature = "physics")]
        let physics = if self.physics.body_count() > 0 {
            self.physics.save()
        } else {
            Vec::new()
        };
        #[cfg(not(feature = "physics"))]
        let physics: Vec<u8> = Vec::new();
        out.extend_from_slice(&(physics.len() as u32).to_le_bytes());
        out.extend_from_slice(&physics);
        self.world.save(&mut out);
        out
    }

    /// Go back to a snapshot; on an error nothing changes
    pub fn restore(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut r = crate::ecs::Reader { bytes, at: 0 };
        if r.take(SNAPSHOT_MAGIC.len()).ok() != Some(&SNAPSHOT_MAGIC[..]) {
            return Err("not an OverSight snapshot (or from another version)".into());
        }
        let rng = r.u64()?;
        let ticks = r.u64()?;
        let n = r.u32()? as usize;
        r.need(n.saturating_mul(16))?;
        let mut touching = Vec::with_capacity(n);
        for _ in 0..n {
            touching.push((r.u64()?, r.u64()?));
        }
        let size = r.u32()? as usize;
        let physics_bytes = r.take(size)?;
        let physics = if size == 0 {
            crate::physics::Physics::default_like(&self.physics)
        } else {
            crate::physics::Physics::load(physics_bytes)?
        };
        self.world.load(&bytes[r.at..])?;
        self.physics = physics;
        self.rng = Rng::new(rng);
        self.time.tick_count = ticks;
        self.collisions.set_touching(touching);
        Ok(())
    }

    /// A 64-bit fingerprint of the snapshot: two machines running the same
    /// game in step have the same hash each tick, so a difference shows
    /// where they went apart
    pub fn state_hash(&self) -> u64 {
        // FNV-1a
        self.snapshot().iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
            (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// Move every entity with a Transform and a Velocity by dt (a tick's
    /// dt for fixed ticks). With a Body on a tile map it slides along the
    /// walls, its contacts are updated, and the velocity stops on the
    /// blocked axis (no speed builds up while standing on the floor).
    pub fn update_motion(&mut self, dt: f32) {
        use crate::components::{
            read, write, Body, Transform, Velocity, BODY, TRANSFORM, VELOCITY,
        };
        for ent in self.world.query(&[TRANSFORM, VELOCITY]) {
            if self.world.has(ent, crate::components::RIGIDBODY) {
                continue; // the physics step moves these
            }
            let (Some(mut t), Some(mut v)) = (
                read::<Transform>(&self.world, ent, TRANSFORM),
                read::<Velocity>(&self.world, ent, VELOCITY),
            ) else {
                continue;
            };
            let delta = Vec2::new(v.linear.x * dt, v.linear.y * dt);
            match read::<Body>(&self.world, ent, BODY) {
                Some(mut body) if body.tilemap != 0 => {
                    let b = body.bounds;
                    let start = Rect::new(t.pos.x + b.x, t.pos.y + b.y, b.w, b.h);
                    let moved = self.move_box(body.tilemap, start, delta);
                    t.pos = Vec2::new(moved.rect.x - b.x, moved.rect.y - b.y);
                    // Only a real stop zeroes the velocity, but a resting body
                    // does not move, so its contacts also count what the
                    // final box touches
                    let stop = moved.contacts;
                    let mut c = stop;
                    if let Some(m) = self.tilemaps.get(body.tilemap) {
                        let near = collision::touching(m, moved.rect);
                        c.ground |= near.ground;
                        c.ceiling |= near.ceiling;
                        c.wall_left |= near.wall_left;
                        c.wall_right |= near.wall_right;
                    }
                    if (stop.ground && v.linear.y > 0.0) || (stop.ceiling && v.linear.y < 0.0) {
                        v.linear.y = 0.0;
                    }
                    if (stop.wall_left && v.linear.x < 0.0) || (stop.wall_right && v.linear.x > 0.0)
                    {
                        v.linear.x = 0.0;
                    }
                    body.contacts = c;
                    write(&mut self.world, ent, BODY, &body);
                    write(&mut self.world, ent, VELOCITY, &v);
                }
                _ => t.pos = Vec2::new(t.pos.x + delta.x, t.pos.y + delta.y),
            }
            t.rotation += v.angular * dt;
            write(&mut self.world, ent, TRANSFORM, &t);
        }
    }

    /// Find overlaps between colliders, push solid movers apart and work
    /// out begin and end events (after update_motion, once per tick)
    pub fn update_collisions(&mut self) {
        self.collisions.update(&mut self.world, &self.polygons);
    }

    /// Entities whose collider overlaps the shape and has a layer bit in mask
    pub fn query_shape(&self, shape: &crate::shapes::Shape, mask: u32) -> Vec<u64> {
        crate::collisions::overlapping(&self.world, &self.polygons, shape, mask)
    }

    pub fn raycast_entities(
        &self,
        from: Vec2,
        to: Vec2,
        mask: u32,
    ) -> Option<crate::collisions::EntityHit> {
        crate::collisions::raycast(&self.world, &self.polygons, from, to, mask)
    }

    /// Advance the bodies with mass by dt (a tick's dt) and write their
    /// places and speeds back to Transform and Velocity
    pub fn physics_step(&mut self, dt: f32) {
        self.physics.step(&mut self.world, &self.polygons, dt);
    }

    /// Make bodies for new entities now, so impulses and joints can use
    /// them before the first step
    pub fn physics_sync(&mut self) {
        self.physics.sync(&self.world, &self.polygons, 0.0);
    }

    /// A convex polygon for colliders (3 to 64 corners in order)
    pub fn create_polygon(&mut self, points: &[Vec2]) -> Result<u32, String> {
        if points.len() > 64 {
            return Err(format!(
                "a polygon has at most 64 corners, not {}",
                points.len()
            ));
        }
        if !crate::shapes::is_convex(points) {
            return Err("a collider polygon must be convex, with corners in order".into());
        }
        Ok(self.polygons.insert(points.to_vec()))
    }

    /// The entity's collider in the world (needs a Transform and a Collider)
    pub fn collider_shape(&self, entity: u64) -> Option<crate::shapes::Shape> {
        use crate::components::{read, Collider, Transform, COLLIDER, TRANSFORM};
        let t = read::<Transform>(&self.world, entity, TRANSFORM)?;
        let c = read::<Collider>(&self.world, entity, COLLIDER)?;
        c.shape(&t, |id| self.polygons.get(id).cloned())
    }

    /// How the colliders of two entities overlap (layers and masks aside)
    pub fn entity_overlap(&self, a: u64, b: u64) -> Option<crate::shapes::Overlap> {
        self.collider_shape(a)?.overlap(&self.collider_shape(b)?)
    }

    /// Outline every collider in the debug overlay (shown while it is on)
    pub fn debug_colliders(&mut self) {
        use crate::components::{read, Collider, COLLIDER, TRANSFORM};
        use crate::shapes::Shape;
        if !self.debug.enabled {
            return;
        }
        for ent in self.world.query(&[TRANSFORM, COLLIDER]) {
            let trigger =
                read::<Collider>(&self.world, ent, COLLIDER).is_some_and(|c| c.trigger != 0);
            let color = if trigger {
                Color::rgba(255, 200, 40, 255)
            } else {
                Color::rgba(60, 220, 255, 255)
            };
            match self.collider_shape(ent) {
                Some(Shape::Circle { center, radius }) => self.debug_circle(center, radius, color),
                Some(Shape::Polygon(p)) => {
                    for i in 0..p.len() {
                        self.debug_line(p[i], p[(i + 1) % p.len()], color);
                    }
                }
                None => {}
            }
        }
    }

    /// Draw every entity with a Transform and a SpriteRenderer, in the
    /// current space
    pub fn draw_entities(&mut self) {
        use crate::components::{read, SpriteRenderer, Transform, SPRITE_RENDERER, TRANSFORM};
        for ent in self.world.query(&[SPRITE_RENDERER, TRANSFORM]) {
            let (Some(t), Some(s)) = (
                read::<Transform>(&self.world, ent, TRANSFORM),
                read::<SpriteRenderer>(&self.world, ent, SPRITE_RENDERER),
            ) else {
                continue;
            };
            if s.hidden != 0 {
                continue;
            }
            let sprite = Sprite {
                src: s.src,
                pos: t.pos,
                origin: s.origin,
                scale: t.scale,
                rotation: t.rotation,
                flip_x: s.flip_x != 0,
                flip_y: s.flip_y != 0,
                tint: s.tint,
                layer: s.layer,
                depth: if s.y_sort != 0 { t.pos.y } else { 0.0 },
            };
            self.draw_sprite(s.texture, &sprite);
        }
    }

    /// Make the next `poll` return false
    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn inject(&mut self, event: InputEvent) {
        self.injected.push(event);
    }

    /// The last error from loading or presenting, taken (C API reports it)
    pub fn take_error(&mut self) -> Option<String> {
        self.last_error.take()
    }

    pub fn set_error(&mut self, error: String) {
        self.last_error = Some(error);
    }

    pub fn has_window(&self) -> bool {
        #[cfg(feature = "window")]
        return self.window.is_some();
        #[cfg(not(feature = "window"))]
        false
    }

    pub fn set_title(&mut self, _title: &str) {
        #[cfg(feature = "window")]
        if let Some(w) = self.window.as_mut() {
            w.set_title(_title);
        }
    }

    pub fn set_fullscreen(&mut self, _on: bool) {
        #[cfg(feature = "window")]
        if let Some(w) = self.window.as_mut() {
            w.set_fullscreen(_on);
        }
    }

    pub fn fullscreen(&self) -> bool {
        #[cfg(feature = "window")]
        return self.window.as_ref().is_some_and(|w| w.fullscreen());
        #[cfg(not(feature = "window"))]
        false
    }

    /// Frame limit for windowed engines (0 = unlimited)
    pub fn set_target_fps(&mut self, _fps: u32) {
        #[cfg(feature = "window")]
        if let Some(w) = self.window.as_mut() {
            w.set_target_fps(_fps);
        }
    }

    /// Mouse position in world coordinates
    pub fn mouse_world(&self) -> Vec2 {
        self.screen_to_world(self.input.mouse)
    }

    pub fn size(&self) -> (u32, u32) {
        self.renderer.size()
    }

    // --- resources -------------------------------------------------------

    pub fn load_texture(&mut self, path: &str) -> Result<u32, String> {
        let id = self.textures.insert(Texture::load(path)?);
        let file = Watched::new(std::path::Path::new(path));
        self.hot_reload.textures.insert(id, file);
        Ok(id)
    }

    pub fn texture_from_rgba(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<u32, String> {
        Ok(self
            .textures
            .insert(Texture::from_rgba(width, height, rgba)?))
    }

    pub fn texture_size(&self, texture: u32) -> Option<(u32, u32)> {
        self.textures.get(texture).map(|t| (t.width(), t.height()))
    }

    /// Smooth (bilinear) or crisp (nearest, the default) scaling
    pub fn set_texture_smooth(&mut self, texture: u32, smooth: bool) {
        if let Some(t) = self.textures.get_mut(texture) {
            t.smooth = smooth;
        }
    }

    pub fn free_texture(&mut self, texture: u32) {
        self.textures.remove(texture);
        self.hot_reload.textures.remove(&texture);
        self.renderer.targets.remove(&texture);
        if self.renderer.target == texture {
            self.renderer.target = 0;
        }
    }

    // --- render targets, post effects and shaders --------------------------------

    /// A transparent texture draw calls can go into (see `set_target`);
    /// it is drawn like any other texture
    pub fn create_target(&mut self, width: u32, height: u32) -> Result<u32, String> {
        let id = self.textures.insert(Texture::target(width, height)?);
        let state = crate::render::TargetState {
            size: (width, height),
            ..Default::default()
        };
        self.renderer.targets.insert(id, state);
        Ok(id)
    }

    /// Draw calls from now on go into `target` (0: the screen). Every
    /// frame starts on the screen.
    pub fn set_target(&mut self, target: u32) -> Result<(), String> {
        if self.renderer.set_target(target) {
            Ok(())
        } else {
            Err(format!("texture {} is not a render target", target))
        }
    }

    pub fn target(&self) -> u32 {
        self.renderer.target
    }

    /// Clear a target before this frame's draw calls into it; a target
    /// that is not cleared keeps its picture and is drawn over
    pub fn clear_target(&mut self, target: u32, color: Color) {
        self.renderer.clear_target(target, color);
    }

    /// The camera World draw calls into a target use (None: the main one)
    pub fn set_target_camera(&mut self, target: u32, camera: Option<Camera>) {
        if let Some(t) = self.renderer.targets.get_mut(&target) {
            t.camera = camera;
        }
    }

    /// The post effects of a target (0: the screen), run in order every
    /// frame it is drawn
    pub fn set_effects(&mut self, target: u32, effects: &[Effect]) -> Result<(), String> {
        for e in effects {
            match e.kind {
                crate::post::EFFECT_GLOW..=crate::post::EFFECT_CRT => {}
                crate::post::EFFECT_LIGHTING => {}
                crate::post::EFFECT_SHADER if self.shaders.get(e.shader).is_some() => {}
                crate::post::EFFECT_SHADER => return Err(format!("no shader {}", e.shader)),
                k => return Err(format!("unknown effect kind {}", k)),
            }
        }
        let list = if target == 0 {
            &mut self.renderer.effects
        } else {
            match self.renderer.targets.get_mut(&target) {
                Some(t) => &mut t.effects,
                None => return Err(format!("texture {} is not a render target", target)),
            }
        };
        *list = effects.to_vec();
        Ok(())
    }

    /// A post effect shader in WGSL: it defines
    /// `fn effect(uv: vec2<f32>, pixel: vec2<f32>) -> vec4<f32>` and may
    /// use `source` (the picture so far, sampled with `samp` or read
    /// with `load`), `post.size`, `post.time` and `post.p[0..2]` (the
    /// effect's eight numbers). Only the graphics card runs them.
    pub fn create_shader(&mut self, wgsl: &str) -> Result<u32, String> {
        #[cfg(feature = "gpu")]
        crate::gpu::validate_shader(wgsl)?;
        let id = self.shaders.insert(wgsl.to_string());
        #[cfg(feature = "gpu")]
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(e) = gpu.compile_shader(id, wgsl) {
                self.shaders.remove(id);
                return Err(e);
            }
        }
        Ok(id)
    }

    /// Effects that still name it are skipped
    pub fn free_shader(&mut self, shader: u32) {
        self.shaders.remove(shader);
        #[cfg(feature = "gpu")]
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.free_shader(shader);
        }
    }

    /// Lighting uses this texture of the same size as `texture`'s normal
    /// map (red right, green up); 0 removes it
    pub fn set_normal_map(&mut self, texture: u32, normal: u32) {
        if let Some(t) = self.textures.get_mut(texture) {
            t.normal = normal;
        }
    }

    /// Colliders on these layers (bits) cast shadows
    pub fn set_shadow_layers(&mut self, layers: u32) {
        self.shadow_layers = layers;
    }

    /// Solid cells of a stored map cast shadows (or stop)
    pub fn set_shadow_tilemap(&mut self, map: u32, on: bool) {
        if on {
            self.shadow_maps.insert(map);
        } else {
            self.shadow_maps.remove(&map);
        }
    }

    /// What each target with the lighting effect sees of the lights and
    /// shadow casters, in its own pixels
    fn prepare_lighting(&mut self) {
        use crate::components::{read, Light, Transform, LIGHT, TRANSFORM};
        use crate::light::{shape_edges, tilemap_edges, LightPx, LightScene};
        let lit = |fx: &[Effect]| fx.iter().any(|e| e.kind == crate::post::EFFECT_LIGHTING);
        let mut targets: Vec<u32> = self
            .renderer
            .targets
            .iter()
            .filter(|(_, t)| lit(&t.effects))
            .map(|(id, _)| *id)
            .collect();
        if lit(&self.renderer.effects) {
            targets.push(0);
        }
        self.renderer.light_scenes.clear();
        if targets.is_empty() {
            return;
        }
        let lights: Vec<(Vec2, Light)> = self
            .world
            .query(&[LIGHT, TRANSFORM])
            .into_iter()
            .filter_map(|ent| {
                let t = read::<Transform>(&self.world, ent, TRANSFORM)?;
                let l = read::<Light>(&self.world, ent, LIGHT)?;
                Some((Vec2::new(t.pos.x + l.offset.x, t.pos.y + l.offset.y), l))
            })
            .collect();
        let reach = lights.iter().map(|(_, l)| l.radius).fold(0.0, f32::max);
        let casters: Vec<crate::shapes::Shape> = if self.shadow_layers == 0 {
            Vec::new()
        } else {
            self.world
                .query(&[crate::components::COLLIDER, TRANSFORM])
                .into_iter()
                .filter(|ent| {
                    read::<crate::components::Collider>(
                        &self.world,
                        *ent,
                        crate::components::COLLIDER,
                    )
                    .is_some_and(|c| c.layer & self.shadow_layers != 0)
                })
                .filter_map(|ent| self.collider_shape(ent))
                .collect()
        };
        for target in targets {
            let t = self.renderer.space_transform(Space::World, target);
            let size = if target == 0 {
                self.renderer.size()
            } else {
                self.renderer.targets[&target].size
            };
            let scale = (t.sx * t.sy - t.kx * t.ky).abs().sqrt();
            let mut scene = LightScene::default();
            for (pos, l) in &lights {
                let mut p = tiny_skia::Point::from_xy(pos.x, pos.y);
                t.map_point(&mut p);
                let k = l.intensity / 255.0;
                scene.lights.push(LightPx {
                    pos: Vec2::new(p.x, p.y),
                    radius: l.radius * scale,
                    color: [
                        l.color.r as f32 * k,
                        l.color.g as f32 * k,
                        l.color.b as f32 * k,
                    ],
                    height: l.height,
                    softness: l.softness,
                    shadows: l.shadows != 0,
                });
            }
            for shape in &casters {
                shape_edges(shape, t, &mut scene.edges);
            }
            let view = self.renderer.camera_of(target).visible_world(size);
            let view = Rect::new(
                view.x - reach,
                view.y - reach,
                view.w + reach * 2.0,
                view.h + reach * 2.0,
            );
            for map in &self.shadow_maps {
                if let Some(m) = self.tilemaps.get(*map) {
                    tilemap_edges(m, view, t, &mut scene.edges);
                }
            }
            self.renderer.light_scenes.insert(target, scene);
        }
    }

    /// Shake the world on screen by up to `strength` pixels, fading out
    /// over `duration` seconds; the HUD (screen space) stays put
    pub fn shake(&mut self, strength: f32, duration: f32) {
        self.renderer.shake.start(strength, duration);
    }

    pub fn load_font(&mut self, path: &str) -> Result<u32, String> {
        Ok(self.fonts.insert(Font::load(path)?))
    }

    pub fn free_font(&mut self, font: u32) {
        if font != DEFAULT_FONT_ID {
            self.fonts.remove(font);
        }
    }

    pub fn measure_text(&mut self, font: u32, text: &str, size: f32) -> Vec2 {
        self.fonts
            .get_mut(font)
            .map_or(Vec2::default(), |f| f.measure(text, size))
    }

    /// Load a Tiled map (.tmj) and its tileset images
    pub fn load_tilemap(&mut self, path: &str) -> Result<u32, String> {
        let mut files = MapFiles::default();
        let map = self.read_tilemap(std::path::Path::new(path), &mut files.images)?;
        files.files = map.files.iter().map(|f| Watched::new(f)).collect();
        let id = self.add_tilemap(map);
        self.hot_reload.maps.insert(id, files);
        Ok(id)
    }

    /// Read a map file; tileset images already in `images` are reused,
    /// new ones are loaded (and watched like any texture)
    fn read_tilemap(
        &mut self,
        path: &std::path::Path,
        images: &mut std::collections::BTreeMap<std::path::PathBuf, u32>,
    ) -> Result<Tilemap, String> {
        let (textures, watched) = (&mut self.textures, &mut self.hot_reload.textures);
        crate::tiled::load(path, &mut |p| {
            if let Some(&id) = images.get(p) {
                if textures.get(id).is_some() {
                    return Ok(id);
                }
            }
            let id = textures.insert(Texture::load(&p.to_string_lossy())?);
            watched.insert(id, Watched::new(p));
            images.insert(p.to_path_buf(), id);
            Ok(id)
        })
    }

    /// Reload textures and maps whose files changed since they were read;
    /// returns how many were reloaded. Ids stay the same. A file that
    /// fails to load keeps the old version and sets the last error.
    /// `poll` calls this every `hot_reload.interval` seconds while
    /// `hot_reload.enabled` is on.
    pub fn reload_changed(&mut self) -> u32 {
        let mut count = 0;
        let ids: Vec<u32> = self.hot_reload.textures.keys().copied().collect();
        for id in ids {
            let Some(file) = self.hot_reload.textures.get_mut(&id) else {
                continue;
            };
            if !file.changed() {
                continue;
            }
            match Texture::load(&file.path.to_string_lossy()) {
                Ok(mut fresh) => {
                    if let Some(t) = self.textures.get_mut(id) {
                        fresh.smooth = t.smooth;
                        *t = fresh;
                        count += 1;
                    }
                }
                Err(e) => self.last_error = Some(e),
            }
        }
        let ids: Vec<u32> = self.hot_reload.maps.keys().copied().collect();
        for id in ids {
            let Some(entry) = self.hot_reload.maps.get_mut(&id) else {
                continue;
            };
            // Check every file so each change is seen once
            let changed = entry
                .files
                .iter_mut()
                .fold(false, |any, f| f.changed() | any);
            if !changed || entry.files.is_empty() {
                continue;
            }
            let path = entry.files[0].path.clone();
            let mut images = std::mem::take(&mut entry.images);
            let result = self.read_tilemap(&path, &mut images);
            let entry = self.hot_reload.maps.get_mut(&id).expect("checked above");
            entry.images = images;
            match result {
                Ok(map) => {
                    // A tileset file added or removed: watch the new list
                    entry.files = map.files.iter().map(|f| Watched::new(f)).collect();
                    if let Some(m) = self.tilemaps.get_mut(id) {
                        *m = map;
                        count += 1;
                    }
                }
                Err(e) => self.last_error = Some(e),
            }
        }
        count
    }

    /// Keep a map in the engine (the C API refers to maps by id)
    pub fn add_tilemap(&mut self, map: Tilemap) -> u32 {
        self.tilemaps.insert(map)
    }

    pub fn tilemap(&self, map: u32) -> Option<&Tilemap> {
        self.tilemaps.get(map)
    }

    pub fn tilemap_mut(&mut self, map: u32) -> Option<&mut Tilemap> {
        self.tilemaps.get_mut(map)
    }

    /// Frees the map only; its tileset textures stay loaded
    /// Free a map and the tileset textures it loaded
    pub fn free_tilemap(&mut self, map: u32) {
        self.tilemaps.remove(map);
        if let Some(files) = self.hot_reload.maps.remove(&map) {
            for texture in files.images.into_values() {
                self.free_texture(texture);
            }
        }
    }

    /// `collision::move_box` against a stored map (unblocked if there is
    /// no such map)
    pub fn move_box(&self, map: u32, rect: Rect, delta: Vec2) -> MoveResult {
        match self.tilemaps.get(map) {
            Some(m) => collision::move_box(m, rect, delta),
            None => MoveResult {
                rect: Rect::new(rect.x + delta.x, rect.y + delta.y, rect.w, rect.h),
                contacts: Default::default(),
            },
        }
    }

    /// Cell centres from `from` to `to` around solid cells (A*); None when
    /// there is no way or no such map
    pub fn find_path(
        &self,
        map: u32,
        from: Vec2,
        to: Vec2,
        diagonal: bool,
        max_cost: f32,
    ) -> Option<Vec<Vec2>> {
        crate::pathfind::find_world_path(self.tilemaps.get(map)?, from, to, diagonal, max_cost)
    }

    pub fn raycast(&self, map: u32, from: Vec2, to: Vec2) -> Option<RayHit> {
        collision::raycast(self.tilemaps.get(map)?, from, to)
    }

    // --- camera and draw state ------------------------------------------

    pub fn camera(&self) -> Camera {
        self.renderer.camera
    }

    pub fn set_camera(&mut self, camera: Camera) {
        self.renderer.camera = camera;
    }

    pub fn world_to_screen(&self, p: Vec2) -> Vec2 {
        self.renderer.camera.world_to_screen(p, self.size())
    }

    pub fn screen_to_world(&self, p: Vec2) -> Vec2 {
        self.renderer.camera.screen_to_world(p, self.size())
    }

    pub fn set_layer(&mut self, layer: i32) {
        self.state.layer = layer;
    }

    pub fn set_depth(&mut self, depth: f32) {
        self.state.depth = depth;
    }

    pub fn set_space(&mut self, space: Space) {
        self.state.space = space;
    }

    pub fn set_antialias(&mut self, on: bool) {
        self.renderer.antialias = on;
    }

    // --- frame ------------------------------------------------------------

    pub fn begin_frame(&mut self, clear: Color) {
        self.renderer.begin(clear);
    }

    /// Rasterize the frame (and show it, when there is a window)
    pub fn end_frame(&mut self) {
        let stats = Stats {
            fps: self.time.fps,
            dt: self.time.dt,
            commands: self.renderer.pending(),
            sprites: self.renderer.drawn_sprites,
            camera: self.renderer.camera.center,
            mouse_world: self.mouse_world(),
            renderer: self.renderer_name(),
        };
        // The transition cover and the overlay go on the screen
        self.renderer.target = 0;
        self.scenes.draw(&mut self.renderer);
        self.debug.flush(&mut self.renderer, &stats);
        self.prepare_lighting();
        let dt = self.time.dt;
        #[cfg(feature = "gpu")]
        if let Some(gpu) = self.gpu.as_mut() {
            let commands = self.renderer.take_sorted();
            let sprites = gpu.render(
                &self.renderer,
                &commands,
                &mut self.textures,
                &mut self.fonts,
                self.time.total as f32,
            );
            self.renderer.drawn_sprites = sprites;
            self.renderer.recycle(commands);
            self.renderer.shake.update(dt);
            self.canvas_stale = true;
            #[cfg(feature = "window")]
            if let Some(window) = self.window.as_mut() {
                let view = window.viewport(self.renderer.size());
                window.pre_present();
                if let Err(e) = gpu.present(window.size(), (view.x, view.y, view.scale)) {
                    self.last_error = Some(e);
                }
                self.time.limit(window.target_fps());
            }
            return;
        }
        self.renderer.flush(&mut self.textures, &mut self.fonts);
        self.renderer.shake.update(dt);
        #[cfg(feature = "window")]
        if let Some(window) = self.window.as_mut() {
            if let Err(e) = window.present(&self.renderer.canvas) {
                self.last_error = Some(e);
            }
            self.time.limit(window.target_fps());
        }
    }

    /// Bring the software canvas up to the GPU's last frame
    fn sync_canvas(&mut self) {
        if !std::mem::take(&mut self.canvas_stale) {
            return;
        }
        #[cfg(feature = "gpu")]
        if let Some(gpu) = &self.gpu {
            if let Err(e) = gpu.read_canvas(&mut self.renderer.canvas) {
                self.last_error = Some(e);
            }
        }
    }

    pub fn draw_sprite(&mut self, texture: u32, sprite: &Sprite) {
        let (layer, depth, space) = (sprite.layer, sprite.depth, self.state.space);
        self.renderer.push(
            layer,
            depth,
            space,
            DrawKind::Sprite {
                texture,
                sprite: *sprite,
            },
        );
    }

    /// Whole texture at (x, y), using the current layer and depth
    pub fn draw_texture(&mut self, texture: u32, x: f32, y: f32) {
        let sprite = Sprite {
            pos: Vec2::new(x, y),
            layer: self.state.layer,
            depth: self.state.depth,
            ..Sprite::default()
        };
        self.draw_sprite(texture, &sprite);
    }

    /// One frame of a sprite sheet; `sprite.src` is replaced by the frame
    pub fn draw_frame(&mut self, texture: u32, sheet: &SpriteSheet, frame: u32, sprite: &Sprite) {
        let rect = sheet.frame(frame);
        if rect.w <= 0.0 {
            return; // out of range: drawing the whole sheet would be worse
        }
        let sprite = Sprite {
            src: rect,
            ..*sprite
        };
        self.draw_sprite(texture, &sprite);
    }

    /// Visible tiles of one layer of a stored map
    pub fn draw_tilemap_layer(&mut self, map: u32, layer: usize, opts: &LayerDraw) {
        if let Some(map) = self.tilemaps.get(map) {
            map.draw_layer(&mut self.renderer, layer, opts);
        }
    }

    /// Every tile layer of a stored map, layer i on draw layer `base + i`
    pub fn draw_tilemap(&mut self, map: u32, base: i32) {
        if let Some(map) = self.tilemaps.get(map) {
            for i in 0..map.layers.len() {
                let opts = LayerDraw {
                    layer: base + i as i32,
                    ..LayerDraw::default()
                };
                map.draw_layer(&mut self.renderer, i, &opts);
            }
        }
    }

    // --- particles --------------------------------------------------------------

    pub fn create_emitter(&mut self, config: EmitterConfig, pos: Vec2) -> u32 {
        self.emitters.insert(Emitter::new(config, pos))
    }

    pub fn emitter(&self, id: u32) -> Option<&Emitter> {
        self.emitters.get(id)
    }

    pub fn emitter_mut(&mut self, id: u32) -> Option<&mut Emitter> {
        self.emitters.get_mut(id)
    }

    pub fn free_emitter(&mut self, id: u32) {
        self.emitters.remove(id);
    }

    /// Advance every emitter by `dt` seconds
    pub fn update_emitters(&mut self, dt: f32) {
        for e in self.emitters.iter_mut() {
            e.update(dt);
        }
    }

    /// Particles of one emitter on the emitter's layer, at the current
    /// depth and space
    pub fn draw_emitter(&mut self, id: u32) {
        let Some(e) = self.emitters.get(id) else {
            return;
        };
        let mut src = e.config.src;
        if src.w <= 0.0 || src.h <= 0.0 {
            let (w, h) = self
                .textures
                .get(e.config.texture)
                .map_or((0, 0), |t| (t.width(), t.height()));
            src = Rect::new(0.0, 0.0, w as f32, h as f32);
        }
        e.draw(&mut self.renderer, src, self.state.depth, self.state.space);
    }

    // --- skeletons -------------------------------------------------------------

    pub fn load_skeleton(&mut self, path: &str) -> Result<u32, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        self.skeleton_from_json(&text)
            .map_err(|e| format!("{path}: {e}"))
    }

    pub fn skeleton_from_json(&mut self, json: &str) -> Result<u32, String> {
        let data = SkeletonData::from_json(json)?;
        Ok(self.skeleton_data.insert(std::sync::Arc::new(data)))
    }

    /// Skeletons made from it keep working
    pub fn free_skeleton_data(&mut self, data: u32) {
        self.skeleton_data.remove(data);
    }

    /// A skeleton in its setup pose
    pub fn create_skeleton(&mut self, data: u32) -> Result<u32, String> {
        let d = self
            .skeleton_data
            .get(data)
            .ok_or_else(|| format!("no skeleton data {data}"))?;
        Ok(self.skeletons.insert(Skeleton::new(d.clone())))
    }

    pub fn skeleton(&self, id: u32) -> Option<&Skeleton> {
        self.skeletons.get(id)
    }

    pub fn skeleton_mut(&mut self, id: u32) -> Option<&mut Skeleton> {
        self.skeletons.get_mut(id)
    }

    pub fn free_skeleton(&mut self, id: u32) {
        self.skeletons.remove(id);
    }

    /// Advance every skeleton by `dt` seconds
    pub fn update_skeletons(&mut self, dt: f32) {
        for s in self.skeletons.iter_mut() {
            s.update(dt);
        }
    }

    /// A skeleton's attachments as sprites from `texture` (its atlas), at
    /// the current layer, depth and space
    pub fn draw_skeleton(&mut self, id: u32, texture: u32) {
        let Some(s) = self.skeletons.get(id) else {
            return;
        };
        for sprite in s.sprites(self.state.layer, self.state.depth) {
            self.draw_sprite(texture, &sprite);
        }
    }

    // --- debug overlay (drawn only while it is on; F3 toggles it) ------------

    pub fn set_debug(&mut self, on: bool) {
        self.debug.enabled = on;
    }

    /// Outline in the current space (world or screen)
    pub fn debug_rect(&mut self, rect: Rect, color: Color) {
        self.debug.rect(rect, color, self.state.space);
    }

    pub fn debug_line(&mut self, from: Vec2, to: Vec2, color: Color) {
        let line = Shape::Line {
            from,
            to,
            thickness: 1.0,
        };
        self.debug.shape(line, color, self.state.space);
    }

    pub fn debug_circle(&mut self, center: Vec2, radius: f32, color: Color) {
        let circle = Shape::Circle {
            center,
            radius,
            filled: false,
            thickness: 1.0,
        };
        self.debug.shape(circle, color, self.state.space);
    }

    pub fn debug_text(&mut self, text: &str, pos: Vec2, color: Color) {
        self.debug.text(text, pos, color, self.state.space);
    }

    /// Solid cells, platforms and objects of a stored map
    pub fn debug_tilemap(&mut self, map: u32) {
        if !self.debug.enabled {
            return;
        }
        let view = self.renderer.camera.visible_world(self.size());
        if let Some(m) = self.tilemaps.get(map) {
            self.debug.tilemap(m, view);
        }
    }

    fn shape(&mut self, shape: Shape, color: Color) {
        let s = self.state;
        self.renderer
            .push(s.layer, s.depth, s.space, DrawKind::Shape { shape, color });
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.shape(
            Shape::Rect {
                rect,
                filled: true,
                thickness: 0.0,
            },
            color,
        );
    }

    pub fn draw_rect(&mut self, rect: Rect, thickness: f32, color: Color) {
        self.shape(
            Shape::Rect {
                rect,
                filled: false,
                thickness,
            },
            color,
        );
    }

    pub fn draw_line(&mut self, from: Vec2, to: Vec2, thickness: f32, color: Color) {
        self.shape(
            Shape::Line {
                from,
                to,
                thickness,
            },
            color,
        );
    }

    pub fn fill_circle(&mut self, center: Vec2, radius: f32, color: Color) {
        self.shape(
            Shape::Circle {
                center,
                radius,
                filled: true,
                thickness: 0.0,
            },
            color,
        );
    }

    pub fn draw_circle(&mut self, center: Vec2, radius: f32, thickness: f32, color: Color) {
        self.shape(
            Shape::Circle {
                center,
                radius,
                filled: false,
                thickness,
            },
            color,
        );
    }

    /// Text with its top-left corner at `pos`
    pub fn draw_text(&mut self, font: u32, text: &str, pos: Vec2, size: f32, color: Color) {
        let s = self.state;
        let kind = DrawKind::Text {
            font,
            text: text.to_string(),
            pos,
            size,
            color,
        };
        self.renderer.push(s.layer, s.depth, s.space, kind);
    }

    /// Write the last rendered frame to a PNG file
    pub fn save_screenshot(&mut self, path: &str) -> Result<(), String> {
        self.sync_canvas();
        self.renderer
            .canvas
            .save_png(path)
            .map_err(|e| format!("{}: {}", path, e))
    }

    pub fn pixel(&mut self, x: u32, y: u32) -> Option<Color> {
        self.sync_canvas();
        self.renderer.pixel(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny game: entities drift by a random velocity each tick, and one
    /// in ten ticks a new one spawns
    fn simulate(e: &mut Engine, pos: u32, ticks: u32) {
        for _ in 0..ticks {
            e.time.tick_count += 1;
            for ent in e.world.query(&[pos]) {
                let dx = e.rng.range(-1.0, 1.0);
                let p = e.world.ptr(ent, pos).unwrap() as *mut f32;
                unsafe { *p += dx };
            }
            if e.rng.int(0, 9) == 0 {
                let ent = e.world.spawn();
                e.world.set(ent, pos, &[]);
            }
        }
    }

    #[test]
    fn rollback_replays_the_same_game() {
        let mut e = Engine::headless(8, 8).unwrap();
        let pos = e.world.component("Pos", 4, 4).unwrap();
        e.rng = Rng::new(5);
        simulate(&mut e, pos, 50);
        let saved = e.snapshot();
        let saved_hash = e.state_hash();
        simulate(&mut e, pos, 100);
        let first_run = e.state_hash();
        assert_ne!(first_run, saved_hash);

        e.restore(&saved).unwrap();
        assert_eq!(e.state_hash(), saved_hash);
        assert_eq!(e.time.tick_count, 50);
        simulate(&mut e, pos, 100);
        assert_eq!(
            e.state_hash(),
            first_run,
            "the same ticks from the same state"
        );

        // A fresh engine given the snapshot plays the same way
        let mut other = Engine::headless(8, 8).unwrap();
        other.restore(&saved).unwrap();
        simulate(&mut other, pos, 100);
        assert_eq!(other.state_hash(), first_run);

        assert!(e.restore(b"nope").is_err());
        assert!(e.restore(&saved[..saved.len() - 1]).is_err());
        assert_eq!(
            e.state_hash(),
            first_run,
            "a failed restore changes nothing"
        );
    }

    const RED: Color = Color::rgba(255, 0, 0, 255);
    const BLUE: Color = Color::rgba(0, 0, 255, 255);

    /// 2x2 texture: red, green / blue, white
    fn checker(engine: &mut Engine) -> u32 {
        let px = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        engine.texture_from_rgba(2, 2, &px).unwrap()
    }

    #[test]
    fn sprites_regions_flips_and_scale() {
        let mut e = Engine::headless(64, 64).unwrap();
        let tex = checker(&mut e);
        e.begin_frame(Color::BLACK);
        // Top-left texel only, scaled to 8x8, in screen space
        e.set_space(Space::Screen);
        e.draw_sprite(
            tex,
            &Sprite {
                src: Rect::new(0.0, 0.0, 1.0, 1.0),
                pos: Vec2::new(4.0, 4.0),
                scale: Vec2::new(8.0, 8.0),
                ..Sprite::default()
            },
        );
        // Whole texture flipped horizontally, 4x
        e.draw_sprite(
            tex,
            &Sprite {
                pos: Vec2::new(32.0, 32.0),
                scale: Vec2::new(4.0, 4.0),
                flip_x: true,
                ..Sprite::default()
            },
        );
        e.end_frame();
        assert_eq!(e.pixel(5, 5), Some(RED));
        assert_eq!(e.pixel(11, 11), Some(RED));
        assert_eq!(e.pixel(13, 13), Some(Color::BLACK));
        // Flipped: green now top-left, red top-right
        assert_eq!(e.pixel(33, 33), Some(Color::rgba(0, 255, 0, 255)));
        assert_eq!(e.pixel(38, 33), Some(RED));
        assert_eq!(e.pixel(38, 38), Some(BLUE));
    }

    #[test]
    fn layers_then_depth_then_call_order() {
        let mut e = Engine::headless(16, 16).unwrap();
        e.begin_frame(Color::BLACK);
        e.set_space(Space::Screen);
        let full = Rect::new(0.0, 0.0, 16.0, 16.0);
        e.set_layer(1);
        e.fill_rect(full, BLUE); // drawn last: highest layer
        e.set_layer(0);
        e.set_depth(5.0);
        e.fill_rect(full, RED);
        e.set_depth(1.0);
        e.fill_rect(full, Color::WHITE);
        e.end_frame();
        assert_eq!(e.pixel(8, 8), Some(BLUE));

        e.begin_frame(Color::BLACK);
        e.set_layer(0);
        e.set_depth(5.0);
        e.fill_rect(full, RED);
        e.set_depth(1.0);
        e.fill_rect(full, Color::WHITE);
        e.end_frame();
        assert_eq!(
            e.pixel(8, 8),
            Some(RED),
            "higher depth is drawn later (y-sort)"
        );
    }

    #[test]
    fn camera_moves_world_but_not_screen_space() {
        let mut e = Engine::headless(100, 100).unwrap();
        e.set_camera(Camera {
            center: Vec2::new(500.0, 500.0),
            zoom: 2.0,
            rotation: 0.0,
        });
        e.begin_frame(Color::BLACK);
        e.fill_rect(Rect::new(500.0, 500.0, 5.0, 5.0), RED); // world: lands at the centre
        e.set_space(Space::Screen);
        e.fill_rect(Rect::new(0.0, 0.0, 5.0, 5.0), BLUE);
        e.end_frame();
        assert_eq!(e.pixel(51, 51), Some(RED));
        assert_eq!(e.pixel(59, 59), Some(RED), "zoom 2 doubles the size");
        assert_eq!(e.pixel(61, 61), Some(Color::BLACK));
        assert_eq!(e.pixel(2, 2), Some(BLUE));
        let world = e.screen_to_world(Vec2::new(50.0, 50.0));
        assert_eq!(world, Vec2::new(500.0, 500.0));
    }

    #[test]
    fn offscreen_sprites_are_culled_and_text_renders() {
        let mut e = Engine::headless(200, 60).unwrap();
        let tex = checker(&mut e);
        e.begin_frame(Color::BLACK);
        e.draw_texture(tex, 5000.0, 5000.0);
        e.draw_texture(tex, 10.0, 10.0);
        e.set_space(Space::Screen);
        e.draw_text(
            DEFAULT_FONT_ID,
            "Привет, мир!",
            Vec2::new(4.0, 20.0),
            24.0,
            Color::WHITE,
        );
        e.end_frame();
        assert_eq!(e.renderer.drawn_sprites, 1);
        let lit = (0..200)
            .flat_map(|x| (20..50).map(move |y| (x, y)))
            .filter(|&(x, y)| e.pixel(x, y).is_some_and(|c| c.r > 128))
            .count();
        assert!(lit > 150, "text should cover some pixels, got {}", lit);
        let size = e.measure_text(DEFAULT_FONT_ID, "Привет, мир!", 24.0);
        assert!(size.x > 100.0 && size.x < 200.0);
    }

    #[test]
    fn poll_applies_injected_input_and_quit() {
        let mut e = Engine::headless(8, 8).unwrap();
        e.inject(InputEvent::Key(Key::Space, true));
        e.inject(InputEvent::MouseMove(Vec2::new(3.0, 4.0)));
        assert!(e.poll());
        assert!(e.input.key_pressed(Key::Space));
        assert_eq!(e.input.mouse, Vec2::new(3.0, 4.0));
        assert!(e.poll());
        assert!(e.input.key_down(Key::Space) && !e.input.key_pressed(Key::Space));
        assert!((e.time.total - 2.0 / 60.0).abs() < 1e-6);
        e.quit();
        assert!(!e.poll());
    }

    #[test]
    fn draw_frame_picks_the_sheet_cell() {
        let mut e = Engine::headless(8, 8).unwrap();
        let tex = checker(&mut e);
        let sheet = SpriteSheet::grid(2, 2, 1, 1, 0, 0);
        e.begin_frame(Color::BLACK);
        e.set_space(Space::Screen);
        let sprite = Sprite {
            scale: Vec2::new(8.0, 8.0),
            ..Sprite::default()
        };
        e.draw_frame(tex, &sheet, 2, &sprite);
        e.draw_frame(tex, &sheet, 9, &sprite); // out of range: nothing
        e.end_frame();
        assert_eq!(e.pixel(4, 4), Some(BLUE));
        assert_eq!(e.renderer.drawn_sprites, 1);
    }

    #[test]
    fn loads_and_draws_a_tiled_map() {
        let dir = std::env::temp_dir().join(format!("oversight_engine_map_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Tileset image: a red and a blue 4x4 tile
        let mut img = image::RgbaImage::new(8, 4);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 4 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            };
        }
        img.save(dir.join("tiles.png")).unwrap();
        let map = r#"{"orientation": "orthogonal", "width": 2, "height": 1,
            "tilewidth": 4, "tileheight": 4,
            "tilesets": [{"firstgid": 1, "name": "t", "image": "tiles.png",
                "imagewidth": 8, "imageheight": 4, "tilewidth": 4, "tileheight": 4,
                "columns": 2, "tilecount": 2}],
            "layers": [{"type": "tilelayer", "name": "g", "width": 2, "height": 1, "data": [2, 1]}]}"#;
        let path = dir.join("level.tmj");
        std::fs::write(&path, map).unwrap();

        let mut e = Engine::headless(8, 4).unwrap();
        let id = e.load_tilemap(&path.to_string_lossy()).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        e.set_camera(Camera {
            center: Vec2::new(4.0, 2.0),
            ..Camera::default()
        });
        e.begin_frame(Color::BLACK);
        e.draw_tilemap(id, 0);
        e.end_frame();
        assert_eq!(e.pixel(1, 1), Some(BLUE));
        assert_eq!(e.pixel(6, 2), Some(RED));
        assert!(e.load_tilemap("missing.tmj").is_err());
    }

    #[test]
    fn tint_and_alpha() {
        let mut e = Engine::headless(8, 8).unwrap();
        let white = e.texture_from_rgba(1, 1, &[255, 255, 255, 255]).unwrap();
        e.begin_frame(Color::BLACK);
        e.set_space(Space::Screen);
        e.draw_sprite(
            white,
            &Sprite {
                scale: Vec2::new(8.0, 8.0),
                tint: Color::rgba(255, 0, 0, 128),
                ..Sprite::default()
            },
        );
        e.end_frame();
        let p = e.pixel(4, 4).unwrap();
        assert!(
            (126..=130).contains(&p.r) && p.g == 0 && p.b == 0,
            "{:?}",
            p
        );
    }

    #[test]
    fn render_targets_are_drawn_first_and_shown_like_textures() {
        let mut e = Engine::headless(16, 16).unwrap();
        let a = e.create_target(4, 4).unwrap();
        let b = e.create_target(8, 8).unwrap();
        assert!(e.set_target(999).is_err());
        e.begin_frame(Color::BLACK);
        // The screen shows b, b shows a: a is opened first, so it is ready
        e.set_space(Space::Screen);
        e.draw_texture(b, 0.0, 0.0);
        e.set_target(a).unwrap();
        e.clear_target(a, RED);
        e.set_target(b).unwrap();
        e.clear_target(b, BLUE);
        e.draw_texture(a, 2.0, 2.0);
        e.set_target(0).unwrap();
        e.fill_rect(Rect::new(12.0, 12.0, 4.0, 4.0), RED);
        e.end_frame();
        assert_eq!(e.pixel(0, 0), Some(BLUE));
        assert_eq!(e.pixel(3, 3), Some(RED), "a inside b");
        assert_eq!(e.pixel(7, 7), Some(BLUE));
        assert_eq!(e.pixel(10, 10), Some(Color::BLACK), "b is 8x8");
        assert_eq!(e.pixel(13, 13), Some(RED));
        assert_eq!(e.target(), 0, "a frame ends on the screen");

        // Not cleared: a keeps its picture and is drawn over
        e.begin_frame(Color::BLACK);
        assert_eq!(e.target(), 0, "every frame starts on the screen");
        e.set_space(Space::Screen);
        e.set_target(a).unwrap();
        e.fill_rect(Rect::new(0.0, 0.0, 1.0, 1.0), BLUE);
        e.set_target(0).unwrap();
        e.draw_sprite(
            a,
            &Sprite {
                scale: Vec2::new(4.0, 4.0),
                ..Sprite::default()
            },
        );
        e.end_frame();
        assert_eq!(e.pixel(1, 1), Some(BLUE));
        assert_eq!(e.pixel(9, 9), Some(RED), "last frame's red is still there");

        e.free_texture(a);
        assert!(e.set_target(a).is_err());
    }

    #[test]
    fn targets_have_their_own_camera_and_effects() {
        let mut e = Engine::headless(8, 8).unwrap();
        let t = e.create_target(8, 8).unwrap();
        e.set_camera(Camera {
            center: Vec2::new(100.0, 100.0),
            ..Camera::default()
        });
        let frame = |e: &mut Engine| {
            e.begin_frame(Color::BLACK);
            e.set_target(t).unwrap();
            e.clear_target(t, Color::BLACK);
            e.fill_rect(Rect::new(0.0, 0.0, 2.0, 2.0), RED);
            e.set_target(0).unwrap();
            e.set_space(Space::Screen);
            e.draw_texture(t, 0.0, 0.0);
            e.set_space(Space::World);
            e.end_frame();
        };
        frame(&mut e);
        assert_eq!(
            e.pixel(4, 4),
            Some(Color::BLACK),
            "the main camera is far away"
        );
        e.set_target_camera(
            t,
            Some(Camera {
                center: Vec2::new(1.0, 1.0),
                ..Camera::default()
            }),
        );
        frame(&mut e);
        assert_eq!(
            e.pixel(4, 4),
            Some(RED),
            "its own camera looks at the origin"
        );
        let grey = crate::post::Effect::grade(0.0, 1.0, 0.0, 1.0, Color::WHITE);
        e.set_effects(t, &[grey]).unwrap();
        frame(&mut e);
        let p = e.pixel(4, 4).unwrap();
        assert!(p.r == p.g && p.g == p.b && p.r > 0, "{:?}", p);
        assert!(e
            .set_effects(t, &[crate::post::Effect::shader(7, [0.0; 8])])
            .is_err());
        let bad = crate::post::Effect {
            kind: 99,
            ..Default::default()
        };
        assert!(e.set_effects(0, &[bad]).is_err());
        assert!(e.set_effects(12345, &[]).is_err());
    }

    #[test]
    fn shake_moves_the_world_but_not_the_hud() {
        let mut e = Engine::headless(40, 40).unwrap();
        let frame = |e: &mut Engine| {
            e.poll();
            e.begin_frame(Color::BLACK);
            e.set_space(Space::World);
            e.fill_rect(Rect::new(-1.0, -1.0, 2.0, 2.0), RED);
            e.set_space(Space::Screen);
            e.fill_rect(Rect::new(0.0, 0.0, 2.0, 2.0), BLUE);
            e.end_frame();
        };
        e.shake(8.0, 0.5);
        let mut moved = false;
        for _ in 0..10 {
            frame(&mut e);
            moved |= e.pixel(20, 20) != Some(RED);
            assert_eq!(e.pixel(1, 1), Some(BLUE));
        }
        assert!(moved, "the world shook");
        for _ in 0..30 {
            frame(&mut e);
        }
        assert_eq!(e.pixel(20, 20), Some(RED), "and settled");
        assert_eq!(e.screen_to_world(Vec2::new(20.0, 20.0)), Vec2::default());
    }

    #[test]
    fn shaders_are_checked_when_created() {
        let mut e = Engine::headless(4, 4).unwrap();
        let ok = "fn effect(uv: vec2<f32>, pixel: vec2<f32>) -> vec4<f32> {\n    return load(vec2<i32>(pixel));\n}";
        #[cfg(feature = "gpu")]
        {
            let err = e.create_shader("fn effect(uv: vec2<f32>) -> f32 { return 1; }");
            assert!(err.is_err());
        }
        let id = e.create_shader(ok).unwrap();
        e.set_effects(0, &[crate::post::Effect::shader(id, [0.0; 8])])
            .unwrap();
        e.begin_frame(RED);
        e.end_frame();
        assert_eq!(e.pixel(0, 0), Some(RED), "left out in software");
        e.free_shader(id);
        e.begin_frame(RED);
        e.end_frame();
    }

    /// A white world 64x32 seen 1:1, lit by one light at (10, 16), with a
    /// wall at x 28..32 on the shadow layer
    fn lit_world(e: &mut Engine, softness: f32, ambient: Color) {
        use crate::components::{write, Collider, Light, Transform, COLLIDER, LIGHT, TRANSFORM};
        e.set_camera(Camera {
            center: Vec2::new(32.0, 16.0),
            ..Camera::default()
        });
        e.set_effects(0, &[crate::post::Effect::lighting(ambient, 1.0)])
            .unwrap();
        e.set_shadow_layers(2);
        let lamp = e.world.spawn();
        let at = |x, y| Transform {
            pos: Vec2::new(x, y),
            ..Transform::default()
        };
        write(&mut e.world, lamp, TRANSFORM, &at(10.0, 16.0));
        let light = Light {
            radius: 60.0,
            softness,
            ..Light::default()
        };
        write(&mut e.world, lamp, LIGHT, &light);
        let wall = e.world.spawn();
        write(&mut e.world, wall, TRANSFORM, &at(30.0, 16.0));
        let c = Collider {
            size: Vec2::new(4.0, 20.0),
            layer: 2,
            ..Collider::default()
        };
        write(&mut e.world, wall, COLLIDER, &c);
        e.begin_frame(Color::BLACK);
        e.fill_rect(Rect::new(0.0, 0.0, 64.0, 32.0), Color::WHITE);
        e.end_frame();
    }

    #[test]
    fn lights_fade_out_and_walls_cast_shadows() {
        let mut e = Engine::headless(64, 32).unwrap();
        lit_world(&mut e, 0.0, Color::BLACK);
        let r = |e: &mut Engine, x, y| e.pixel(x, y).unwrap().r;
        assert!(r(&mut e, 10, 16) > 240, "at the light");
        let (near, far) = (r(&mut e, 14, 16), r(&mut e, 24, 16));
        assert!(near > far && far > 50, "fades with distance: {near} {far}");
        assert!(r(&mut e, 30, 16) > 0, "the wall's own face is lit");
        assert_eq!(r(&mut e, 40, 16), 0, "behind the wall");
        assert!(r(&mut e, 34, 0) > 0, "past its end");
        assert_eq!(r(&mut e, 63, 31), 0, "out of reach, no ambient");

        let mut e = Engine::headless(64, 32).unwrap();
        lit_world(&mut e, 4.0, Color::rgba(128, 128, 128, 255));
        assert_eq!(r(&mut e, 63, 31), 128, "ambient light");
        let edge = r(&mut e, 34, 3);
        assert!(edge > 128 && edge < 250, "a soft edge: {edge}");
    }

    #[test]
    fn normal_maps_tilt_the_light() {
        use crate::components::{write, Light, Transform, LIGHT, TRANSFORM};
        let shade = |normal: Option<[u8; 4]>, light_x: f32| {
            let mut e = Engine::headless(16, 16).unwrap();
            e.set_camera(Camera {
                center: Vec2::new(8.0, 8.0),
                ..Camera::default()
            });
            let white = e.texture_from_rgba(16, 16, &[255; 16 * 16 * 4]).unwrap();
            if let Some(n) = normal {
                let px: Vec<u8> = n.iter().copied().cycle().take(16 * 16 * 4).collect();
                let map = e.texture_from_rgba(16, 16, &px).unwrap();
                e.set_normal_map(white, map);
            }
            e.set_effects(0, &[crate::post::Effect::lighting(Color::BLACK, 1.0)])
                .unwrap();
            let lamp = e.world.spawn();
            let t = Transform {
                pos: Vec2::new(light_x, 8.0),
                ..Transform::default()
            };
            write(&mut e.world, lamp, TRANSFORM, &t);
            let l = Light {
                radius: 40.0,
                height: 0.3,
                ..Light::default()
            };
            write(&mut e.world, lamp, LIGHT, &l);
            e.begin_frame(Color::BLACK);
            e.draw_texture(white, 0.0, 0.0);
            e.end_frame();
            e.pixel(8, 8).unwrap().r
        };
        let right = Some([230, 128, 200, 255]); // tilted to the right
        let flat = shade(None, 30.0);
        assert!(shade(right, 30.0) > flat + 20, "faces the light");
        assert!(shade(right, -14.0) + 20 < flat, "faces away");
        let level = shade(Some([128, 128, 255, 255]), 30.0);
        assert!(level.abs_diff(flat) <= 2, "a flat map changes nothing");
    }

    #[test]
    fn solid_cells_of_shadow_maps_cast_shadows() {
        use crate::components::{write, Light, Transform, LIGHT, TRANSFORM};
        use crate::tilemap::{Orientation, Property, Tileset};
        let mut e = Engine::headless(64, 16).unwrap();
        e.set_camera(Camera {
            center: Vec2::new(32.0, 8.0),
            ..Camera::default()
        });
        // A wall column at x 24..32
        let mut m = Tilemap::new(Orientation::Orthogonal, 8, 2, 8, 8);
        let mut ts = Tileset::new(1, 0, SpriteSheet::grid(8, 8, 8, 8, 0, 0));
        ts.tile_properties
            .entry(0)
            .or_default()
            .insert("solid".into(), Property::Bool(true));
        m.add_tileset(ts);
        let l = m.add_layer("walls");
        m.layers[l].set(3, 0, 1);
        m.layers[l].set(3, 1, 1);
        let map = e.add_tilemap(m);
        e.set_effects(0, &[crate::post::Effect::lighting(Color::BLACK, 1.0)])
            .unwrap();
        let lamp = e.world.spawn();
        let t = Transform {
            pos: Vec2::new(8.0, 8.0),
            ..Transform::default()
        };
        write(&mut e.world, lamp, TRANSFORM, &t);
        let light = Light {
            radius: 60.0,
            softness: 0.0,
            ..Light::default()
        };
        write(&mut e.world, lamp, LIGHT, &light);
        let frame = |e: &mut Engine| {
            e.begin_frame(Color::BLACK);
            e.fill_rect(Rect::new(0.0, 0.0, 64.0, 16.0), Color::WHITE);
            e.end_frame();
            e.pixel(40, 8).unwrap().r
        };
        assert!(frame(&mut e) > 0, "no shadow map yet");
        e.set_shadow_tilemap(map, true);
        assert_eq!(frame(&mut e), 0, "behind the wall");
        assert!(e.pixel(28, 8).unwrap().r > 0, "the wall's face is lit");
    }

    #[test]
    fn skeletons_draw_their_attachments_where_the_bones_are() {
        let mut e = Engine::headless(48, 48).unwrap();
        e.set_space(Space::Screen);
        let json = r#"{
            "bones": [{"name": "root", "x": 10, "y": 10}, {"name": "arm", "parent": "root"}],
            "slots": [{"name": "arm", "bone": "arm", "attachment": "arm",
                       "attachments": {"arm": {"region": [0, 0, 20, 4], "origin": [0, 2]}}}],
            "animations": {"down": {"bones": {"arm": {"rotate": [{"time": 0, "value": 90}]}}}}
        }"#;
        let data = e.skeleton_from_json(json).unwrap();
        let red: Vec<u8> = [255, 0, 0, 255]
            .iter()
            .copied()
            .cycle()
            .take(20 * 4 * 4)
            .collect();
        let atlas = e.texture_from_rgba(20, 4, &red).unwrap();
        let s = e.create_skeleton(data).unwrap();
        e.free_skeleton_data(data);
        assert!(e.create_skeleton(data).is_err());
        let frame = |e: &mut Engine| {
            e.begin_frame(Color::BLACK);
            e.draw_skeleton(s, atlas);
            e.end_frame();
            (e.pixel(25, 10).unwrap().r, e.pixel(10, 25).unwrap().r)
        };
        assert_eq!(frame(&mut e), (255, 0), "the arm points right");
        e.skeleton_mut(s).unwrap().play("down", true, 0.0).unwrap();
        e.update_skeletons(0.1);
        assert_eq!(frame(&mut e), (0, 255), "then down");
        e.free_skeleton(s);
        assert_eq!(frame(&mut e), (0, 0));
    }
}

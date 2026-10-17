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
use crate::gamepad::{GamepadAxis, GamepadButton};
use crate::hotreload::{HotReload, MapFiles, Watched};
use crate::input::{Input, Key, MouseButton};
use crate::particles::{Emitter, EmitterConfig};
use crate::render::{DrawKind, Renderer, Shape};
use crate::rng::Rng;
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

pub struct Engine {
    pub renderer: Renderer,
    pub textures: Store<Texture>,
    pub fonts: Store<Font>,
    pub tilemaps: Store<Tilemap>,
    pub emitters: Store<Emitter>,
    pub state: DrawState,
    pub input: Input,
    pub time: Time,
    /// Seeded random numbers for game logic (same seed, same game)
    pub rng: Rng,
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
            state: DrawState::default(),
            input: Input::default(),
            time: Time::new(Some(1.0 / 60.0)),
            rng: Rng::default(),
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
        if let Some(key) = self.debug.toggle_key {
            if self.input.key_pressed(key) {
                self.debug.enabled = !self.debug.enabled;
            }
        }
        #[cfg(feature = "audio")]
        self.audio.update();
        self.time.tick();
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
        self.debug.flush(&mut self.renderer, &stats);
        #[cfg(feature = "gpu")]
        if let Some(gpu) = self.gpu.as_mut() {
            let commands = self.renderer.take_sorted();
            let sprites = gpu.render(
                &self.renderer,
                &commands,
                &mut self.textures,
                &mut self.fonts,
            );
            self.renderer.drawn_sprites = sprites;
            self.renderer.recycle(commands);
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
}

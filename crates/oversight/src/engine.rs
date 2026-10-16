//! Engine: the object a game talks to. It owns the renderer, the loaded
//! resources and (later) the window, input and audio.

use crate::camera::Camera;
use crate::render::{DrawKind, Renderer, Shape};
use crate::text::{Font, DEFAULT_FONT};
use crate::texture::{Store, Texture};
use crate::types::{Color, Rect, Space, Sprite, Vec2};

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
    pub state: DrawState,
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
            state: DrawState::default(),
        })
    }

    pub fn size(&self) -> (u32, u32) {
        self.renderer.size()
    }

    // --- resources -------------------------------------------------------

    pub fn load_texture(&mut self, path: &str) -> Result<u32, String> {
        Ok(self.textures.insert(Texture::load(path)?))
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
        self.renderer.flush(&mut self.textures, &mut self.fonts);
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
    pub fn save_screenshot(&self, path: &str) -> Result<(), String> {
        self.renderer
            .canvas
            .save_png(path)
            .map_err(|e| format!("{}: {}", path, e))
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
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

//! Renderer: draw calls are collected during a frame, sorted by layer,
//! depth and call order, then rasterized into the canvas.

use tiny_skia::{FillRule, Paint, PathBuilder, Pattern, Pixmap, SpreadMode, Stroke, Transform};

use crate::camera::Camera;
use crate::text::Font;
use crate::texture::{Store, Texture};
use crate::types::{Color, Rect, Space, Sprite, Vec2};

#[derive(Debug, Clone)]
pub enum Shape {
    Rect {
        rect: Rect,
        filled: bool,
        thickness: f32,
    },
    Line {
        from: Vec2,
        to: Vec2,
        thickness: f32,
    },
    Circle {
        center: Vec2,
        radius: f32,
        filled: bool,
        thickness: f32,
    },
}

#[derive(Debug, Clone)]
pub enum DrawKind {
    Sprite {
        texture: u32,
        sprite: Sprite,
    },
    Shape {
        shape: Shape,
        color: Color,
    },
    Text {
        font: u32,
        text: String,
        pos: Vec2,
        size: f32,
        color: Color,
    },
}

#[derive(Debug, Clone)]
pub struct DrawCmd {
    pub layer: i32,
    pub depth: f32,
    pub space: Space,
    pub kind: DrawKind,
    seq: u32,
}

pub struct Renderer {
    pub canvas: Pixmap,
    pub camera: Camera,
    pub clear_color: Color,
    /// Smooth shape edges (off gives crisp pixel-art shapes)
    pub antialias: bool,
    commands: Vec<DrawCmd>,
    seq: u32,
    /// Sprites drawn in the last frame (after culling)
    pub drawn_sprites: u32,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Result<Renderer, String> {
        let canvas = Pixmap::new(width.max(1), height.max(1))
            .ok_or_else(|| format!("invalid canvas size {}x{}", width, height))?;
        Ok(Renderer {
            canvas,
            camera: Camera::default(),
            clear_color: Color::BLACK,
            antialias: true,
            commands: Vec::new(),
            seq: 0,
            drawn_sprites: 0,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.canvas.width(), self.canvas.height())
    }

    pub fn begin(&mut self, clear: Color) {
        self.clear_color = clear;
        self.commands.clear();
        self.seq = 0;
    }

    pub fn push(&mut self, layer: i32, depth: f32, space: Space, kind: DrawKind) {
        self.commands.push(DrawCmd {
            layer,
            depth,
            space,
            kind,
            seq: self.seq,
        });
        self.seq += 1;
    }

    pub fn pending(&self) -> usize {
        self.commands.len()
    }

    fn space_transform(&self, space: Space) -> Transform {
        match space {
            Space::World => self.camera.transform(self.size()),
            Space::Screen => Transform::identity(),
        }
    }

    /// Sort and rasterize this frame's draw calls into the canvas
    pub fn flush(&mut self, textures: &mut Store<Texture>, fonts: &mut Store<Font>) {
        let mut commands = std::mem::take(&mut self.commands);
        commands.sort_by(|a, b| {
            a.layer
                .cmp(&b.layer)
                .then(a.depth.total_cmp(&b.depth))
                .then(a.seq.cmp(&b.seq))
        });
        self.canvas.fill(self.clear_color.to_skia());
        self.drawn_sprites = 0;
        for cmd in &commands {
            let base = self.space_transform(cmd.space);
            match &cmd.kind {
                DrawKind::Sprite { texture, sprite } => {
                    if let Some(tex) = textures.get_mut(*texture) {
                        if self.draw_sprite(tex, sprite, base) {
                            self.drawn_sprites += 1;
                        }
                    }
                }
                DrawKind::Shape { shape, color } => self.draw_shape(shape, *color, base),
                DrawKind::Text {
                    font,
                    text,
                    pos,
                    size,
                    color,
                } => {
                    if let Some(font) = fonts.get_mut(*font) {
                        self.draw_text(font, text, *pos, *size, *color, base);
                    }
                }
            }
        }
        commands.clear();
        self.commands = commands; // keep the allocation
    }

    /// Returns false when the sprite was culled
    fn draw_sprite(&mut self, tex: &mut Texture, s: &Sprite, base: Transform) -> bool {
        let (tw, th) = (tex.width() as f32, tex.height() as f32);
        let src = if s.src.w > 0.0 && s.src.h > 0.0 {
            s.src
        } else {
            Rect::new(0.0, 0.0, tw, th)
        };
        let mut local = Transform::from_translate(s.pos.x, s.pos.y)
            .pre_concat(Transform::from_rotate(s.rotation.to_degrees()))
            .pre_scale(s.scale.x, s.scale.y)
            .pre_translate(-s.origin.x, -s.origin.y);
        // Flips mirror the image inside its own rectangle
        if s.flip_x {
            local = local.pre_translate(src.w, 0.0).pre_scale(-1.0, 1.0);
        }
        if s.flip_y {
            local = local.pre_translate(0.0, src.h).pre_scale(1.0, -1.0);
        }
        let transform = base.pre_concat(local);
        if !self.visible(Rect::new(0.0, 0.0, src.w, src.h), transform) {
            return false;
        }
        let quality = tex.quality();
        let pixmap = tex.tinted(s.tint);
        let pattern = Pattern::new(
            pixmap.as_ref(),
            SpreadMode::Pad,
            quality,
            s.tint.a as f32 / 255.0,
            Transform::from_translate(-src.x, -src.y),
        );
        let paint = Paint {
            shader: pattern,
            anti_alias: false,
            ..Paint::default()
        };
        if let Some(rect) = tiny_skia::Rect::from_xywh(0.0, 0.0, src.w, src.h) {
            self.canvas.fill_rect(rect, &paint, transform, None);
        }
        true
    }

    /// Whether a local rectangle can touch the canvas
    fn visible(&self, rect: Rect, transform: Transform) -> bool {
        let mut corners = [
            tiny_skia::Point::from_xy(rect.x, rect.y),
            tiny_skia::Point::from_xy(rect.right(), rect.y),
            tiny_skia::Point::from_xy(rect.x, rect.bottom()),
            tiny_skia::Point::from_xy(rect.right(), rect.bottom()),
        ];
        transform.map_points(&mut corners);
        let (w, h) = (self.canvas.width() as f32, self.canvas.height() as f32);
        let min_x = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let max_y = corners
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        max_x >= 0.0 && max_y >= 0.0 && min_x <= w && min_y <= h
    }

    fn draw_shape(&mut self, shape: &Shape, color: Color, transform: Transform) {
        let mut paint = Paint {
            anti_alias: self.antialias,
            ..Paint::default()
        };
        paint.set_color(color.to_skia());
        let (path, filled, thickness) = match shape {
            Shape::Rect {
                rect, filled: true, ..
            } => {
                if let Some(r) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.w, rect.h) {
                    self.canvas.fill_rect(r, &paint, transform, None);
                }
                return;
            }
            Shape::Rect {
                rect,
                filled: false,
                thickness,
            } => (
                tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.w, rect.h)
                    .map(PathBuilder::from_rect),
                false,
                *thickness,
            ),
            Shape::Line {
                from,
                to,
                thickness,
            } => {
                let mut pb = PathBuilder::new();
                pb.move_to(from.x, from.y);
                pb.line_to(to.x, to.y);
                (pb.finish(), false, *thickness)
            }
            Shape::Circle {
                center,
                radius,
                filled,
                thickness,
            } => (
                PathBuilder::from_circle(center.x, center.y, *radius),
                *filled,
                *thickness,
            ),
        };
        let Some(path) = path else { return };
        if filled {
            self.canvas
                .fill_path(&path, &paint, FillRule::Winding, transform, None);
        } else {
            let stroke = Stroke {
                width: thickness.max(0.1),
                ..Stroke::default()
            };
            self.canvas
                .stroke_path(&path, &paint, &stroke, transform, None);
        }
    }

    fn draw_text(
        &mut self,
        font: &mut Font,
        text: &str,
        pos: Vec2,
        size: f32,
        color: Color,
        base: Transform,
    ) {
        let (glyphs, _) = font.layout(text, size, color);
        let transform = base.pre_translate(pos.x, pos.y);
        for placed in glyphs {
            let Some(pixmap) = font.cached(&placed.key).and_then(|g| g.pixmap.as_ref()) else {
                continue;
            };
            let (x, y) = (placed.pos.x.round(), placed.pos.y.round());
            let pattern = Pattern::new(
                pixmap.as_ref(),
                SpreadMode::Pad,
                tiny_skia::FilterQuality::Bilinear,
                1.0,
                Transform::from_translate(x, y),
            );
            let paint = Paint {
                shader: pattern,
                anti_alias: false,
                ..Paint::default()
            };
            let rect =
                tiny_skia::Rect::from_xywh(x, y, pixmap.width() as f32, pixmap.height() as f32);
            if let Some(rect) = rect {
                self.canvas.fill_rect(rect, &paint, transform, None);
            }
        }
    }

    /// RGBA (straight alpha) of one canvas pixel, for tests and tools
    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        let p = self.canvas.pixel(x, y)?.demultiply();
        Some(Color::rgba(p.red(), p.green(), p.blue(), p.alpha()))
    }
}

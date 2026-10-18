//! Renderer: draw calls are collected during a frame, sorted by layer,
//! depth and call order, then rasterized into the canvas here (the
//! software renderer) or drawn by the graphics card (`gpu`).
//!
//! Draw calls can go into a render target (a texture) instead of the
//! screen. Targets are drawn in the order they were first opened in the
//! frame, the screen last, so a target can show another one opened before
//! it. Each target and the screen can run a chain of post effects.

use std::collections::BTreeMap;
use std::ops::Range;

use tiny_skia::{FillRule, Paint, PathBuilder, Pattern, Pixmap, SpreadMode, Stroke, Transform};

use crate::camera::Camera;
use crate::light::LightScene;
use crate::post::{Effect, Shake};
use crate::text::Font;
use crate::texture::{Store, Texture};
use crate::types::{Color, Rect, Space, Sprite, Vec2};

/// Texture region a sprite shows (an empty `src` means the whole texture)
pub(crate) fn sprite_src(s: &Sprite, tw: u32, th: u32) -> Rect {
    if s.src.w > 0.0 && s.src.h > 0.0 {
        s.src
    } else {
        Rect::new(0.0, 0.0, tw as f32, th as f32)
    }
}

/// Maps the sprite's rectangle (0, 0, src.w, src.h) to where it is drawn
pub(crate) fn sprite_transform(s: &Sprite, src: Rect) -> Transform {
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
    local
}

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
    /// A GPU emitter's particles
    Particles(Box<crate::particles::ParticleDraw>),
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
    /// Texture id of the render target; 0 is the screen
    pub target: u32,
    seq: u32,
}

/// A render target's settings, kept across frames
#[derive(Debug, Clone, Default)]
pub struct TargetState {
    pub size: (u32, u32),
    /// None: the main camera, fitted to the target's size
    pub camera: Option<Camera>,
    pub effects: Vec<Effect>,
}

/// The draw calls of one target, in drawing order
pub struct Pass {
    pub target: u32,
    pub size: (u32, u32),
    /// None keeps what the target showed before
    pub clear: Option<Color>,
    pub commands: Range<usize>,
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
    /// Where draw calls go now (0: the screen)
    pub target: u32,
    pub targets: BTreeMap<u32, TargetState>,
    /// The screen's post effects
    pub effects: Vec<Effect>,
    pub shake: Shake,
    /// Lights and shadow casters each lit target sees this frame (filled
    /// by the engine before drawing)
    pub light_scenes: BTreeMap<u32, LightScene>,
    /// Targets opened this frame, in order, with their clear colour
    opened: Vec<(u32, Option<Color>)>,
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
            target: 0,
            targets: BTreeMap::new(),
            effects: Vec::new(),
            shake: Shake::default(),
            light_scenes: BTreeMap::new(),
            opened: Vec::new(),
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.canvas.width(), self.canvas.height())
    }

    /// A new canvas size (an editor panel that was resized)
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.size() != (width.max(1), height.max(1)) {
            self.canvas = Pixmap::new(width.max(1), height.max(1)).expect("canvas size");
        }
    }

    pub fn begin(&mut self, clear: Color) {
        self.clear_color = clear;
        self.commands.clear();
        self.seq = 0;
        self.target = 0;
        self.opened.clear();
    }

    pub fn push(&mut self, layer: i32, depth: f32, space: Space, kind: DrawKind) {
        let target = self.target;
        if target != 0 {
            self.open(target);
        }
        self.commands.push(DrawCmd {
            layer,
            depth,
            space,
            kind,
            target,
            seq: self.seq,
        });
        self.seq += 1;
    }

    /// Send draw calls to a render target (0: the screen); false when
    /// `target` is not one
    pub fn set_target(&mut self, target: u32) -> bool {
        if target != 0 && !self.targets.contains_key(&target) {
            return false;
        }
        self.target = target;
        true
    }

    fn open(&mut self, target: u32) -> usize {
        match self.opened.iter().position(|o| o.0 == target) {
            Some(i) => i,
            None => {
                self.opened.push((target, None));
                self.opened.len() - 1
            }
        }
    }

    /// Clear a target before this frame's draw calls into it
    pub fn clear_target(&mut self, target: u32, color: Color) {
        if self.targets.contains_key(&target) {
            let i = self.open(target);
            self.opened[i].1 = Some(color);
        }
    }

    /// Size of what draw calls go into now
    pub fn target_size(&self) -> (u32, u32) {
        self.target_size_of(self.target)
    }

    fn target_size_of(&self, target: u32) -> (u32, u32) {
        match self.targets.get(&target) {
            Some(t) if target != 0 => t.size,
            _ => self.size(),
        }
    }

    /// The camera World draw calls into `target` use
    pub fn camera_of(&self, target: u32) -> Camera {
        self.targets
            .get(&target)
            .and_then(|t| t.camera)
            .unwrap_or(self.camera)
    }

    /// World rectangle the current target shows (for culling)
    pub fn visible_world(&self) -> Rect {
        self.camera_of(self.target)
            .visible_world(self.target_size())
    }

    pub fn effects_of(&self, target: u32) -> &[Effect] {
        if target == 0 {
            &self.effects
        } else {
            self.targets.get(&target).map_or(&[], |t| &t.effects)
        }
    }

    pub fn pending(&self) -> usize {
        self.commands.len()
    }

    pub(crate) fn space_transform(&self, space: Space, target: u32) -> Transform {
        match space {
            Space::World => {
                let t = self
                    .camera_of(target)
                    .transform(self.target_size_of(target));
                if target != 0 {
                    return t;
                }
                // The shake moves only the screen's world, never its HUD
                let o = self.shake.offset();
                Transform::from_translate(o.x, o.y).pre_concat(t)
            }
            Space::Screen => Transform::identity(),
        }
    }

    /// This frame's draw calls in drawing order, grouped by target (see
    /// `passes`); give them back with `recycle` to keep the allocation
    pub fn take_sorted(&mut self) -> Vec<DrawCmd> {
        let mut commands = std::mem::take(&mut self.commands);
        let rank = |t: u32| match t {
            0 => usize::MAX,
            t => self.opened.iter().position(|o| o.0 == t).unwrap_or(0),
        };
        commands.sort_by(|a, b| {
            rank(a.target)
                .cmp(&rank(b.target))
                .then(a.layer.cmp(&b.layer))
                .then(a.depth.total_cmp(&b.depth))
                .then(a.seq.cmp(&b.seq))
        });
        commands
    }

    /// One pass per target opened this frame, then the screen
    pub fn passes(&self, sorted: &[DrawCmd]) -> Vec<Pass> {
        let mut passes = Vec::with_capacity(self.opened.len() + 1);
        let mut start = 0;
        let targets = self.opened.iter().copied();
        for (target, clear) in targets.chain([(0, Some(self.clear_color))]) {
            let n = sorted[start..]
                .iter()
                .take_while(|c| c.target == target)
                .count();
            if target == 0 || self.targets.contains_key(&target) {
                passes.push(Pass {
                    target,
                    size: self.target_size_of(target),
                    clear,
                    commands: start..start + n,
                });
            }
            start += n;
        }
        passes
    }

    pub fn recycle(&mut self, mut commands: Vec<DrawCmd>) {
        commands.clear();
        self.commands = commands;
    }

    /// Sort and rasterize this frame's draw calls into the canvas and the
    /// render targets, then run their post effects
    pub fn flush(&mut self, textures: &mut Store<Texture>, fonts: &mut Store<Font>) {
        let commands = self.take_sorted();
        self.drawn_sprites = 0;
        for pass in self.passes(&commands) {
            // Worked out before the canvas is taken out (the screen's size
            // is the canvas's)
            let world = self.space_transform(Space::World, pass.target);
            let mut canvas = if pass.target == 0 {
                std::mem::replace(&mut self.canvas, empty())
            } else {
                match textures.get_mut(pass.target) {
                    Some(t) if (t.width(), t.height()) == pass.size => {
                        std::mem::replace(&mut t.pixmap, empty())
                    }
                    _ => continue,
                }
            };
            if let Some(clear) = pass.clear {
                canvas.fill(clear.to_skia());
            }
            for cmd in &commands[pass.commands.clone()] {
                let base = match cmd.space {
                    Space::World => world,
                    Space::Screen => Transform::identity(),
                };
                match &cmd.kind {
                    DrawKind::Sprite { texture, sprite } => {
                        // A target cannot show itself while it is drawn
                        if *texture == pass.target {
                            continue;
                        }
                        if let Some(tex) = textures.get_mut(*texture) {
                            if self.draw_sprite(&mut canvas, tex, sprite, base) {
                                self.drawn_sprites += 1;
                            }
                        }
                    }
                    DrawKind::Shape { shape, color } => {
                        draw_shape(&mut canvas, self.antialias, shape, *color, base)
                    }
                    DrawKind::Particles(d) => {
                        let mut tex = textures.get_mut(d.texture).filter(|_| d.textured());
                        if d.textured() && d.texture == pass.target {
                            continue;
                        }
                        d.each(|pos, rotation, size, color| {
                            let texture = if tex.is_some() { d.texture } else { 0 };
                            let look = crate::particles::look(
                                texture, d.src, 0, 0.0, pos, rotation, size, color,
                            );
                            match (look, tex.as_deref_mut()) {
                                (Some(DrawKind::Sprite { sprite, .. }), Some(t)) => {
                                    if self.draw_sprite(&mut canvas, t, &sprite, base) {
                                        self.drawn_sprites += 1;
                                    }
                                }
                                (Some(DrawKind::Shape { shape, color }), _) => {
                                    draw_shape(&mut canvas, self.antialias, &shape, color, base)
                                }
                                _ => {}
                            }
                        });
                    }
                    DrawKind::Text {
                        font,
                        text,
                        pos,
                        size,
                        color,
                    } => {
                        if let Some(font) = fonts.get_mut(*font) {
                            draw_text(&mut canvas, font, text, *pos, *size, *color, base);
                        }
                    }
                }
            }
            let effects = self.effects_of(pass.target);
            let lit = effects
                .iter()
                .any(|e| e.kind == crate::post::EFFECT_LIGHTING);
            let normals = lit
                .then(|| {
                    let cmds = &commands[pass.commands.clone()];
                    normal_buffer(pass.size, cmds, pass.target, world, textures)
                })
                .flatten();
            let light = self
                .light_scenes
                .get(&pass.target)
                .map(|scene| crate::post::Lit {
                    scene,
                    normals: normals.as_ref(),
                });
            crate::post::apply(&mut canvas, effects, light);
            if pass.target == 0 {
                self.canvas = canvas;
            } else if let Some(t) = textures.get_mut(pass.target) {
                t.pixmap = canvas;
                t.changed();
            }
        }
        self.recycle(commands);
    }

    /// Returns false when the sprite was culled
    fn draw_sprite(
        &self,
        canvas: &mut Pixmap,
        tex: &mut Texture,
        s: &Sprite,
        base: Transform,
    ) -> bool {
        let src = sprite_src(s, tex.width(), tex.height());
        let transform = base.pre_concat(sprite_transform(s, src));
        let size = (canvas.width(), canvas.height());
        if !visible(size, Rect::new(0.0, 0.0, src.w, src.h), transform) {
            return false;
        }
        let quality = tex.quality();
        let pixmap = tex.tinted(s.tint);
        let pattern = Pattern::new(
            pixmap.as_ref(),
            SpreadMode::Pad,
            quality,
            s.tint.a as f32 / 255.0,
            inset_pattern(src),
        );
        let paint = Paint {
            shader: pattern,
            anti_alias: false,
            ..Paint::default()
        };
        if let Some(rect) = tiny_skia::Rect::from_xywh(0.0, 0.0, src.w, src.h) {
            canvas.fill_rect(rect, &paint, transform, None);
        }
        true
    }

    /// RGBA (straight alpha) of one canvas pixel, for tests and tools
    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        let p = self.canvas.pixel(x, y)?.demultiply();
        Some(Color::rgba(p.red(), p.green(), p.blue(), p.alpha()))
    }
}

/// How far inside its region a sprite samples, in texels. A pixel on the
/// sprite's edge can land a hair outside the region, and nearest
/// sampling then shows the next frame or tile of the sheet: a line between
/// map tiles. Both renderers pull the region in by this much.
pub(crate) const SAMPLE_INSET: f32 = 1.0 / 64.0;

/// The region `src`, pulled in by SAMPLE_INSET on every side
pub(crate) fn inset(src: Rect) -> Rect {
    let (ex, ey) = (SAMPLE_INSET.min(src.w / 4.0), SAMPLE_INSET.min(src.h / 4.0));
    Rect::new(src.x + ex, src.y + ey, src.w - 2.0 * ex, src.h - 2.0 * ey)
}

/// The pattern transform showing `src` (inset) on a 0..w x 0..h rectangle
fn inset_pattern(src: Rect) -> Transform {
    let r = inset(src);
    let (kx, ky) = (r.w / src.w.max(1e-6), r.h / src.h.max(1e-6));
    Transform::from_row(1.0 / kx, 0.0, 0.0, 1.0 / ky, -r.x / kx, -r.y / ky)
}

/// Whether a local rectangle can touch a canvas of `size`
/// Where a sprite's region lands: the point showing its top-left texel
/// and the two edges from it, as mapping (0, 0), (w, 0) and (0, h)
/// through `base.pre_concat(sprite_transform(s, src))` would give, but
/// without building that transform (the GPU does this for every sprite)
#[cfg_attr(not(feature = "gpu"), allow(dead_code))]
pub(crate) fn sprite_quad(base: Transform, s: &Sprite, src: Rect) -> [Vec2; 3] {
    let (sin, cos) = if s.rotation == 0.0 {
        (0.0, 1.0)
    } else {
        s.rotation.sin_cos()
    };
    // The sprite's own axes before the base: rotated and scaled
    let (ax, ay) = (cos * s.scale.x, sin * s.scale.x);
    let (bx, by) = (-sin * s.scale.y, cos * s.scale.y);
    let lin = |x: f32, y: f32| Vec2::new(base.sx * x + base.kx * y, base.ky * x + base.sy * y);
    let (ux, uy) = (lin(ax, ay), lin(bx, by));
    let ox = s.pos.x - (ax * s.origin.x + bx * s.origin.y);
    let oy = s.pos.y - (ay * s.origin.x + by * s.origin.y);
    let o = Vec2::new(
        base.sx * ox + base.kx * oy + base.tx,
        base.ky * ox + base.sy * oy + base.ty,
    );
    // Flips mirror the image inside its own rectangle
    let (x0, w) = if s.flip_x {
        (src.w, -src.w)
    } else {
        (0.0, src.w)
    };
    let (y0, h) = if s.flip_y {
        (src.h, -src.h)
    } else {
        (0.0, src.h)
    };
    [
        Vec2::new(o.x + ux.x * x0 + uy.x * y0, o.y + ux.y * x0 + uy.y * y0),
        Vec2::new(ux.x * w, ux.y * w),
        Vec2::new(uy.x * h, uy.y * h),
    ]
}

/// Whether a quad (corner and edges) touches a picture of `size`
#[cfg_attr(not(feature = "gpu"), allow(dead_code))]
pub(crate) fn quad_visible(size: (u32, u32), q: &[Vec2; 3]) -> bool {
    let [a, e, f] = *q;
    let xs = [a.x, a.x + e.x, a.x + f.x, a.x + e.x + f.x];
    let ys = [a.y, a.y + e.y, a.y + f.y, a.y + e.y + f.y];
    let span = |v: [f32; 4]| {
        v.iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &x| {
                (lo.min(x), hi.max(x))
            })
    };
    let ((min_x, max_x), (min_y, max_y)) = (span(xs), span(ys));
    max_x >= 0.0 && max_y >= 0.0 && min_x <= size.0 as f32 && min_y <= size.1 as f32
}

pub(crate) fn visible(size: (u32, u32), rect: Rect, transform: Transform) -> bool {
    let mut corners = [
        tiny_skia::Point::from_xy(rect.x, rect.y),
        tiny_skia::Point::from_xy(rect.right(), rect.y),
        tiny_skia::Point::from_xy(rect.x, rect.bottom()),
        tiny_skia::Point::from_xy(rect.right(), rect.bottom()),
    ];
    transform.map_points(&mut corners);
    let (w, h) = (size.0 as f32, size.1 as f32);
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

fn empty() -> Pixmap {
    Pixmap::new(1, 1).expect("1x1 pixmap")
}

fn draw_shape(
    canvas: &mut Pixmap,
    antialias: bool,
    shape: &Shape,
    color: Color,
    transform: Transform,
) {
    let mut paint = Paint {
        anti_alias: antialias,
        ..Paint::default()
    };
    paint.set_color(color.to_skia());
    let (path, filled, thickness) = match shape {
        Shape::Rect {
            rect, filled: true, ..
        } => {
            if let Some(r) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.w, rect.h) {
                canvas.fill_rect(r, &paint, transform, None);
            }
            return;
        }
        Shape::Rect {
            rect,
            filled: false,
            thickness,
        } => (
            tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.w, rect.h).map(PathBuilder::from_rect),
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
        canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
    } else {
        let stroke = Stroke {
            width: thickness.max(0.1),
            ..Stroke::default()
        };
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    canvas: &mut Pixmap,
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
        let rect = tiny_skia::Rect::from_xywh(x, y, pixmap.width() as f32, pixmap.height() as f32);
        if let Some(rect) = rect {
            canvas.fill_rect(rect, &paint, transform, None);
        }
    }
}

/// How a sprite's local directions turn on screen: the transform's 2x2
/// part with the scale taken out (flips stay)
pub(crate) fn normal_matrix(t: Transform) -> [f32; 4] {
    let c0 = (t.sx * t.sx + t.ky * t.ky).sqrt().max(1e-6);
    let c1 = (t.kx * t.kx + t.sy * t.sy).sqrt().max(1e-6);
    [t.sx / c0, t.kx / c1, t.ky / c0, t.sy / c1]
}

/// The normal map a sprite's texture has, if it is usable
pub(crate) fn normal_map_of(textures: &Store<Texture>, tex: &Texture) -> Option<u32> {
    let n = textures.get(tex.normal)?;
    ((n.width(), n.height()) == (tex.width(), tex.height())).then_some(tex.normal)
}

/// Screen-space normals of a pass's sprites, for the lighting effect;
/// None when no sprite has a normal map (everything is flat)
fn normal_buffer(
    size: (u32, u32),
    commands: &[DrawCmd],
    target: u32,
    world: Transform,
    textures: &Store<Texture>,
) -> Option<Pixmap> {
    let has_maps = commands.iter().any(|c| match &c.kind {
        DrawKind::Sprite { texture, .. } => textures
            .get(*texture)
            .is_some_and(|t| normal_map_of(textures, t).is_some()),
        _ => false,
    });
    if !has_maps {
        return None;
    }
    let mut canvas = Pixmap::new(size.0, size.1)?;
    canvas.fill(tiny_skia::Color::from_rgba8(128, 128, 255, 255));
    for cmd in commands {
        let DrawKind::Sprite { texture, sprite } = &cmd.kind else {
            continue;
        };
        let Some(tex) = textures.get(*texture).filter(|_| *texture != target) else {
            continue;
        };
        let base = match cmd.space {
            Space::World => world,
            Space::Screen => Transform::identity(),
        };
        let src = sprite_src(sprite, tex.width(), tex.height());
        let transform = base.pre_concat(sprite_transform(sprite, src));
        if !visible(size, Rect::new(0.0, 0.0, src.w, src.h), transform) {
            continue;
        }
        let (source, m) = match normal_map_of(textures, tex).and_then(|n| textures.get(n)) {
            Some(n) => (&n.pixmap, normal_matrix(transform)),
            None => (&tex.pixmap, [0.0; 4]),
        };
        let mut turned = source.clone();
        for px in turned.data_mut().as_chunks_mut::<4>().0 {
            *px = crate::post::normal_texel(*px, m);
        }
        let pattern = Pattern::new(
            turned.as_ref(),
            SpreadMode::Pad,
            tex.quality(),
            1.0,
            Transform::from_translate(-src.x, -src.y),
        );
        let paint = Paint {
            shader: pattern,
            anti_alias: false,
            ..Paint::default()
        };
        if let Some(rect) = tiny_skia::Rect::from_xywh(0.0, 0.0, src.w, src.h) {
            canvas.fill_rect(rect, &paint, transform, None);
        }
    }
    Some(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprite_quads_match_the_sprite_transform() {
        let base = Transform::from_translate(40.0, 30.0)
            .pre_concat(Transform::from_rotate(20.0))
            .pre_scale(1.5, 1.5);
        let src = Rect::new(4.0, 8.0, 16.0, 10.0);
        for (flip_x, flip_y) in [(false, false), (true, false), (false, true), (true, true)] {
            let s = Sprite {
                src,
                pos: Vec2::new(12.0, -7.0),
                origin: Vec2::new(5.0, 3.0),
                scale: Vec2::new(2.0, -0.5),
                rotation: 0.7,
                flip_x,
                flip_y,
                ..Sprite::default()
            };
            let t = base.pre_concat(sprite_transform(&s, src));
            let at = |x: f32, y: f32| {
                let mut p = tiny_skia::Point::from_xy(x, y);
                t.map_point(&mut p);
                Vec2::new(p.x, p.y)
            };
            // The texel (0, 0) is where the transform puts the region's top left
            let (a, b, c) = (at(0.0, 0.0), at(src.w, 0.0), at(0.0, src.h));
            let [qa, qe, qf] = sprite_quad(base, &s, src);
            let close = |p: Vec2, q: Vec2| (p.x - q.x).abs() < 1e-3 && (p.y - q.y).abs() < 1e-3;
            assert!(close(qa, a), "{flip_x} {flip_y}: {qa:?} {a:?}");
            assert!(close(Vec2::new(qa.x + qe.x, qa.y + qe.y), b));
            assert!(close(Vec2::new(qa.x + qf.x, qa.y + qf.y), c));
        }
    }
}

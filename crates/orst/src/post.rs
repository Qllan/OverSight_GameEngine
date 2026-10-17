//! Post effects: a chain of full-screen passes run over the finished frame
//! (or over a render target) after everything is drawn, and the camera
//! shake. Each built-in effect has the same maths here (the software
//! renderer) and in `gpu` (WGSL), so both renderers show the same frame.
//! Custom shaders are WGSL and run only on the graphics card; the
//! software renderer leaves them out.

use tiny_skia::Pixmap;

use crate::light::{LightPx, LightScene};
use crate::types::{Color, Vec2};

pub const EFFECT_GLOW: u32 = 1;
pub const EFFECT_BLUR: u32 = 2;
pub const EFFECT_GRADE: u32 = 3;
pub const EFFECT_CRT: u32 = 4;
pub const EFFECT_SHADER: u32 = 5;
pub use crate::light::EFFECT_LIGHTING;
/// Pixels the blur and glow reach at most
pub const MAX_RADIUS: f32 = 32.0;

/// One effect of a chain, as C sees it: `kind` picks the meaning of `p`
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Effect {
    pub kind: u32,
    /// The custom shader of EFFECT_SHADER
    pub shader: u32,
    /// Glow: threshold, strength, radius. Blur: radius.
    /// Grade: brightness, contrast, saturation, gamma, tint r, g, b.
    /// CRT: scanlines, curvature, vignette, aberration.
    /// Lighting: ambient r, g, b.
    /// Shader: anything, as `post.p` in WGSL.
    pub p: [f32; 8],
}

impl Effect {
    pub fn glow(threshold: f32, strength: f32, radius: f32) -> Effect {
        Effect::with(EFFECT_GLOW, &[threshold, strength, radius])
    }

    pub fn blur(radius: f32) -> Effect {
        Effect::with(EFFECT_BLUR, &[radius])
    }

    pub fn grade(
        brightness: f32,
        contrast: f32,
        saturation: f32,
        gamma: f32,
        tint: Color,
    ) -> Effect {
        let t = |c: u8| c as f32 / 255.0;
        Effect::with(
            EFFECT_GRADE,
            &[
                brightness,
                contrast,
                saturation,
                gamma,
                t(tint.r),
                t(tint.g),
                t(tint.b),
            ],
        )
    }

    pub fn crt(scanlines: f32, curvature: f32, vignette: f32, aberration: f32) -> Effect {
        Effect::with(EFFECT_CRT, &[scanlines, curvature, vignette, aberration])
    }

    /// Lights the picture with the engine's lights; `ambient` is the light
    /// everywhere, times `brightness`
    pub fn lighting(ambient: Color, brightness: f32) -> Effect {
        let c = |v: u8| v as f32 / 255.0 * brightness;
        Effect::with(EFFECT_LIGHTING, &[c(ambient.r), c(ambient.g), c(ambient.b)])
    }

    pub fn shader(shader: u32, params: [f32; 8]) -> Effect {
        Effect {
            kind: EFFECT_SHADER,
            shader,
            p: params,
        }
    }

    fn with(kind: u32, values: &[f32]) -> Effect {
        let mut p = [0.0; 8];
        p[..values.len()].copy_from_slice(values);
        Effect { kind, shader: 0, p }
    }

    /// Blur and glow radius, kept in range
    pub fn radius(&self) -> u32 {
        let r = if self.kind == EFFECT_GLOW {
            self.p[2]
        } else {
            self.p[0]
        };
        r.clamp(1.0, MAX_RADIUS).ceil() as u32
    }
}

/// Gaussian weights for -r..=r (sigma r/2), summing to one; the GPU
/// shader works out the same ones
pub fn weights(r: u32) -> Vec<f32> {
    let sigma = r as f32 / 2.0;
    let w: Vec<f32> = (-(r as i32)..=r as i32)
        .map(|k| (-((k * k) as f32) / (2.0 * sigma * sigma)).exp())
        .collect();
    let sum: f32 = w.iter().sum();
    w.into_iter().map(|v| v / sum).collect()
}

type Px = [f32; 4];

fn luma(c: Px) -> f32 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}

/// A frame as premultiplied floats, for the software passes
struct Image {
    w: i32,
    h: i32,
    px: Vec<Px>,
}

impl Image {
    fn from(pixmap: &Pixmap) -> Image {
        let px = pixmap
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p.map(|c| c as f32 / 255.0))
            .collect();
        Image {
            w: pixmap.width() as i32,
            h: pixmap.height() as i32,
            px,
        }
    }

    fn write(&self, pixmap: &mut Pixmap) {
        for (dst, src) in pixmap
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(&self.px)
        {
            let a = src[3].clamp(0.0, 1.0);
            // Premultiplied colour never exceeds its alpha
            *dst = [
                src[0].clamp(0.0, a),
                src[1].clamp(0.0, a),
                src[2].clamp(0.0, a),
                a,
            ]
            .map(|c| (c * 255.0).round() as u8);
        }
    }

    fn at(&self, x: i32, y: i32) -> Px {
        let (x, y) = (x.clamp(0, self.w - 1), y.clamp(0, self.h - 1));
        self.px[(y * self.w + x) as usize]
    }

    /// Bilinear between texel centres; `s` is in texels
    fn sample(&self, sx: f32, sy: f32) -> Px {
        let (x0, y0) = (sx.floor(), sy.floor());
        let (fx, fy) = (sx - x0, sy - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        let lerp = |a: Px, b: Px, t: f32| [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t);
        let top = lerp(self.at(x0, y0), self.at(x0 + 1, y0), fx);
        let bottom = lerp(self.at(x0, y0 + 1), self.at(x0 + 1, y0 + 1), fx);
        lerp(top, bottom, fy)
    }

    fn map(&self, f: impl Fn(i32, i32) -> Px) -> Image {
        let mut px = Vec::with_capacity(self.px.len());
        for y in 0..self.h {
            for x in 0..self.w {
                px.push(f(x, y));
            }
        }
        Image {
            w: self.w,
            h: self.h,
            px,
        }
    }

    /// One direction of a gaussian blur, each tap passed through `f`
    fn blur(&self, r: u32, horizontal: bool, f: impl Fn(Px) -> Px) -> Image {
        let w = weights(r);
        let r = r as i32;
        self.map(|x, y| {
            let mut sum = [0.0; 4];
            for k in -r..=r {
                let c = f(if horizontal {
                    self.at(x + k, y)
                } else {
                    self.at(x, y + k)
                });
                let wk = w[(k + r) as usize];
                for i in 0..4 {
                    sum[i] += c[i] * wk;
                }
            }
            sum
        })
    }
}

/// The bright part of a pixel that glows
fn bright(c: Px, threshold: f32) -> Px {
    let f = ((luma(c) - threshold) / (1.0 - threshold).max(1e-3)).clamp(0.0, 1.0);
    c.map(|v| v * f)
}

fn grade(c: Px, p: &[f32; 8]) -> Px {
    let a = c[3];
    if a <= 0.0 {
        return c;
    }
    let mut rgb = [c[0] / a, c[1] / a, c[2] / a];
    for v in &mut rgb {
        *v = (*v + p[0] - 0.5) * p[1] + 0.5;
    }
    let l = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
    let gamma = 1.0 / p[3].max(0.01);
    for (i, v) in rgb.iter_mut().enumerate() {
        let s = (l + (*v - l) * p[2]).clamp(0.0, 1.0);
        *v = (s.powf(gamma) * p[4 + i]).clamp(0.0, 1.0) * a;
    }
    [rgb[0], rgb[1], rgb[2], a]
}

fn crt(img: &Image, x: i32, y: i32, p: &[f32; 8]) -> Px {
    let (w, h) = (img.w as f32, img.h as f32);
    let (qx, qy) = (
        (x as f32 + 0.5) / w * 2.0 - 1.0,
        (y as f32 + 0.5) / h * 2.0 - 1.0,
    );
    let r2 = qx * qx + qy * qy;
    let bend = 1.0 + p[1] * r2;
    let (cx, cy) = (qx * bend, qy * bend);
    if cx.abs() > 1.0 || cy.abs() > 1.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let sx = (cx * 0.5 + 0.5) * w - 0.5;
    let sy = (cy * 0.5 + 0.5) * h - 0.5;
    let g = img.sample(sx, sy);
    let mut c = [
        img.sample(sx + p[3], sy)[0],
        g[1],
        img.sample(sx - p[3], sy)[2],
        g[3],
    ];
    let mut dim = (1.0 - p[2] * r2 * 0.5).clamp(0.0, 1.0);
    if y % 2 == 1 {
        dim *= 1.0 - p[0].clamp(0.0, 1.0);
    }
    for v in &mut c[..3] {
        *v *= dim;
    }
    c
}

/// Run a chain over a pixmap. Custom shaders are skipped here.
pub fn apply(pixmap: &mut Pixmap, effects: &[Effect], light: Option<Lit>) {
    if !effects
        .iter()
        .any(|e| e.kind != EFFECT_SHADER && e.kind != 0)
    {
        return;
    }
    let mut img = Image::from(pixmap);
    for e in effects {
        img = match e.kind {
            EFFECT_GLOW => {
                let (threshold, strength) = (e.p[0], e.p[1]);
                let across = img.blur(e.radius(), true, |c| bright(c, threshold));
                let glow = across.blur(e.radius(), false, |c| c);
                img.map(|x, y| {
                    let (s, g) = (img.at(x, y), glow.at(x, y));
                    let mut o = [0, 1, 2].map(|i| s[i] + g[i] * strength);
                    let a = s[3].max(o[0].max(o[1]).max(o[2]).min(1.0));
                    for v in &mut o {
                        *v = v.min(a);
                    }
                    [o[0], o[1], o[2], a]
                })
            }
            EFFECT_BLUR => img
                .blur(e.radius(), true, |c| c)
                .blur(e.radius(), false, |c| c),
            EFFECT_GRADE => img.map(|x, y| grade(img.at(x, y), &e.p)),
            EFFECT_CRT => img.map(|x, y| crt(&img, x, y, &e.p)),
            EFFECT_LIGHTING => lighting(&img, &e.p, light),
            _ => continue,
        };
    }
    img.write(pixmap);
}

/// What the lighting effect lights a picture with
#[derive(Clone, Copy)]
pub struct Lit<'a> {
    pub scene: &'a LightScene,
    /// Screen-space normals (see `normal_texel`); None: all flat
    pub normals: Option<&'a Pixmap>,
}

/// A normal map texel turned to the screen by the 2x2 matrix `m` (local
/// x, y to screen; all zero: a flat surface with the texel's alpha).
/// Normal maps point green up; the result is encoded like them, but y
/// down, premultiplied.
pub fn normal_texel(px: [u8; 4], m: [f32; 4]) -> [u8; 4] {
    let a = px[3] as f32 / 255.0;
    if a <= 0.0 {
        return [0; 4];
    }
    let n = if m == [0.0; 4] {
        [0.0, 0.0, 1.0]
    } else {
        let d = |i: usize| px[i] as f32 / 255.0 / a * 2.0 - 1.0;
        let (lx, ly, z) = (d(0), -d(1), d(2));
        normalize([m[0] * lx + m[1] * ly, m[2] * lx + m[3] * ly, z])
    };
    let e = |v: f32| ((v * 0.5 + 0.5) * a * 255.0).round().clamp(0.0, 255.0) as u8;
    [e(n[0]), e(n[1]), e(n[2]), px[3]]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= 1e-6 {
        return [0.0, 0.0, 1.0];
    }
    v.map(|c| c / len)
}

/// The normal stored in a normal buffer pixel
fn decode_normal(c: Px) -> [f32; 3] {
    if c[3] <= 0.0 {
        return [0.0, 0.0, 1.0];
    }
    normalize([0, 1, 2].map(|i| c[i] / c[3] * 2.0 - 1.0))
}

/// Where a light's shadows fall inside its bounds: 1 lit, 0 dark
fn shadow_mask(light: &LightPx, edges: &[crate::light::Edge], b: [i32; 4]) -> Image {
    let (w, h) = ((b[2] - b[0]) as u32, (b[3] - b[1]) as u32);
    let mut pix = Pixmap::new(w, h).expect("light bounds");
    pix.fill(tiny_skia::Color::WHITE);
    let mut paint = tiny_skia::Paint {
        anti_alias: false,
        ..Default::default()
    };
    paint.set_color(tiny_skia::Color::BLACK);
    let shift = tiny_skia::Transform::from_translate(-b[0] as f32, -b[1] as f32);
    for q in light.shadow_quads(edges) {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(q[0].x, q[0].y);
        for p in &q[1..] {
            pb.line_to(p.x, p.y);
        }
        pb.close();
        if let Some(path) = pb.finish() {
            pix.fill_path(&path, &paint, tiny_skia::FillRule::Winding, shift, None);
        }
    }
    let mask = Image::from(&pix);
    match light.blur() {
        0 => mask,
        r => mask.blur(r, true, |c| c).blur(r, false, |c| c),
    }
}

/// How much of a light reaches the pixel centre (cx, cy) on a surface
/// facing `n`, before shadows
fn light_at(light: &LightPx, cx: f32, cy: f32, n: [f32; 3]) -> f32 {
    let (dx, dy) = (
        (light.pos.x - cx) / light.radius,
        (light.pos.y - cy) / light.radius,
    );
    let d = (dx * dx + dy * dy).sqrt();
    if d >= 1.0 {
        return 0.0;
    }
    let l = normalize([dx, dy, light.height.max(0.01)]);
    // Flat ground gets the plain falloff; normal maps tilt it up or down
    let facing = ((n[0] * l[0] + n[1] * l[1] + n[2] * l[2]) / l[2]).clamp(0.0, 2.0);
    (1.0 - d) * (1.0 - d) * facing
}

/// Light gathered over the picture: the ambient light plus every light,
/// capped at twice the picture's own brightness
fn lighting(img: &Image, p: &[f32; 8], lit: Option<Lit>) -> Image {
    let (w, h) = (img.w, img.h);
    let mut sum = vec![[p[0], p[1], p[2]]; (w * h) as usize];
    if let Some(lit) = lit {
        let normals = lit.normals.map(Image::from);
        for light in &lit.scene.lights {
            let Some(b) = light.bounds((w as u32, h as u32)) else {
                continue;
            };
            let mask = shadow_mask(light, &lit.scene.edges, b);
            for y in b[1]..b[3] {
                for x in b[0]..b[2] {
                    let n = normals
                        .as_ref()
                        .map_or([0.0, 0.0, 1.0], |n| decode_normal(n.at(x, y)));
                    let k = light_at(light, x as f32 + 0.5, y as f32 + 0.5, n);
                    if k <= 0.0 {
                        continue;
                    }
                    let k = k * mask.at(x - b[0], y - b[1])[0];
                    let s = &mut sum[(y * w + x) as usize];
                    for (v, c) in s.iter_mut().zip(light.color) {
                        *v += c * k;
                    }
                }
            }
        }
    }
    img.map(|x, y| {
        let c = img.at(x, y);
        let l = sum[(y * w + x) as usize];
        let o = [0, 1, 2].map(|i| (c[i] * l[i].min(2.0)).min(c[3]));
        [o[0], o[1], o[2], c[3]]
    })
}

/// Camera shake: the world part of the screen jitters, fading out
#[derive(Debug, Clone, Copy, Default)]
pub struct Shake {
    strength: f32,
    duration: f32,
    left: f32,
    phase: f32,
}

impl Shake {
    /// Start shaking by up to `strength` pixels for `duration` seconds; a
    /// weaker shake does not cut a stronger one short
    pub fn start(&mut self, strength: f32, duration: f32) {
        if strength <= 0.0 || duration <= 0.0 {
            return;
        }
        if strength >= self.current() {
            self.strength = strength;
            self.duration = duration;
            self.left = duration;
        }
    }

    fn current(&self) -> f32 {
        if self.left <= 0.0 {
            return 0.0;
        }
        let t = self.left / self.duration;
        self.strength * t * t
    }

    pub fn update(&mut self, dt: f32) {
        if self.left > 0.0 {
            self.left = (self.left - dt).max(0.0);
            self.phase += dt;
        }
    }

    /// How far the world is moved this frame, in screen pixels
    pub fn offset(&self) -> Vec2 {
        let s = self.current();
        if s <= 0.0 {
            return Vec2::default();
        }
        // Two waves per axis at unrelated speeds: jittery, never repeating
        // soon, and the same on every machine
        let t = self.phase;
        let wave = |a: f32, b: f32, c: f32| (libm::sinf(t * a + c) + libm::sinf(t * b)) * 0.5;
        Vec2::new(s * wave(71.0, 113.0, 0.0), s * wave(89.0, 131.0, 1.7))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixmap(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Pixmap {
        let mut p = Pixmap::new(w, h).unwrap();
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                p.data_mut()[i..i + 4].copy_from_slice(&f(x, y));
            }
        }
        p
    }

    fn apply_all(p: &mut Pixmap, effects: &[Effect]) {
        apply(p, effects, None);
    }

    fn px(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * p.width() + x) * 4) as usize;
        p.data()[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn weights_sum_to_one_and_peak_in_the_middle() {
        let w = weights(4);
        assert_eq!(w.len(), 9);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(w[4] > w[3] && w[3] > w[0]);
        assert_eq!(Effect::blur(100.0).radius(), MAX_RADIUS as u32);
        assert_eq!(Effect::glow(0.5, 1.0, 0.0).radius(), 1);
    }

    #[test]
    fn glow_spreads_light_only_from_bright_pixels() {
        let base = |bright: [u8; 4]| {
            pixmap(17, 17, move |x, y| {
                if (x, y) == (8, 8) {
                    bright
                } else {
                    [10, 10, 10, 255]
                }
            })
        };
        let mut p = base([255, 255, 255, 255]);
        apply_all(&mut p, &[Effect::glow(0.5, 2.0, 4.0)]);
        assert!(px(&p, 10, 8)[0] > 18, "{:?}", px(&p, 10, 8));
        assert_eq!(px(&p, 0, 0), [10, 10, 10, 255], "too far to reach");
        let mut dim = base([100, 100, 100, 255]);
        apply_all(&mut dim, &[Effect::glow(0.5, 2.0, 4.0)]);
        assert_eq!(px(&dim, 10, 8), [10, 10, 10, 255], "below the threshold");
    }

    #[test]
    fn blur_keeps_flat_colour_and_softens_edges() {
        let mut p = pixmap(16, 4, |x, _| {
            if x < 8 {
                [0, 0, 0, 255]
            } else {
                [200, 200, 200, 255]
            }
        });
        apply_all(&mut p, &[Effect::blur(3.0)]);
        assert_eq!(px(&p, 0, 0), [0, 0, 0, 255]);
        assert_eq!(px(&p, 15, 0), [200, 200, 200, 255]);
        let edge = px(&p, 8, 0)[0];
        assert!(edge > 60 && edge < 160, "{edge}");
    }

    #[test]
    fn grade_moves_colours() {
        let start = || pixmap(1, 1, |_, _| [200, 100, 50, 255]);
        let mut p = start();
        apply_all(&mut p, &[Effect::grade(0.0, 1.0, 0.0, 1.0, Color::WHITE)]);
        let g = px(&p, 0, 0);
        assert!(g[0] == g[1] && g[1] == g[2], "no saturation is grey: {g:?}");
        let mut p = start();
        apply_all(
            &mut p,
            &[Effect::grade(
                0.0,
                1.0,
                1.0,
                1.0,
                Color::rgba(255, 0, 0, 255),
            )],
        );
        assert_eq!(px(&p, 0, 0), [200, 0, 0, 255], "tint");
        let mut p = start();
        apply_all(&mut p, &[Effect::grade(0.2, 1.0, 1.0, 1.0, Color::WHITE)]);
        assert_eq!(px(&p, 0, 0), [251, 151, 101, 255], "brighter");
        // Transparent pixels stay transparent
        let mut clear = pixmap(1, 1, |_, _| [0, 0, 0, 0]);
        apply_all(
            &mut clear,
            &[Effect::grade(0.5, 1.0, 1.0, 1.0, Color::WHITE)],
        );
        assert_eq!(px(&clear, 0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn crt_darkens_odd_rows_and_bends_corners_away() {
        let mut p = pixmap(32, 32, |_, _| [200, 200, 200, 255]);
        apply_all(&mut p, &[Effect::crt(0.5, 0.0, 0.0, 0.0)]);
        assert_eq!(px(&p, 5, 4), [200, 200, 200, 255]);
        assert_eq!(px(&p, 5, 5), [100, 100, 100, 255]);
        let mut p = pixmap(32, 32, |_, _| [200, 200, 200, 255]);
        apply_all(&mut p, &[Effect::crt(0.0, 0.3, 0.0, 0.0)]);
        assert_eq!(
            px(&p, 0, 0),
            [0, 0, 0, 255],
            "the corner bends off the tube"
        );
        assert_eq!(px(&p, 16, 16), [200, 200, 200, 255]);
    }

    #[test]
    fn shaders_are_left_out_in_software() {
        let mut p = pixmap(2, 2, |_, _| [1, 2, 3, 255]);
        apply_all(&mut p, &[Effect::shader(1, [0.0; 8])]);
        assert_eq!(px(&p, 1, 1), [1, 2, 3, 255]);
    }

    #[test]
    fn shake_fades_out_and_a_weak_one_does_not_cut_it_short() {
        let mut s = Shake::default();
        assert_eq!(s.offset(), Vec2::default());
        s.start(10.0, 0.5);
        s.update(1.0 / 60.0);
        let first = s.offset();
        assert!(first.x != 0.0 || first.y != 0.0);
        assert!(first.x.abs() <= 10.0 && first.y.abs() <= 10.0);
        s.start(1.0, 5.0);
        for _ in 0..30 {
            s.update(1.0 / 60.0);
        }
        assert_eq!(s.offset(), Vec2::default(), "done after half a second");
    }
}

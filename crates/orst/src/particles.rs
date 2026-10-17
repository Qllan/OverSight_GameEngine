//! Particles: smoke, sparks, dust, explosions. An emitter spawns
//! particles at a steady rate or in bursts; each particle flies with its
//! own speed and spin, falls with gravity, slows with drag, and changes
//! size and colour over its life. Particles are round dots, or a texture
//! (or a region of one) tinted by the colour.

use crate::render::{DrawKind, Renderer, Shape};
use crate::tween::{lerp, lerp_color};
use crate::types::{Color, Rect, Space, Sprite, Vec2};

/// How an emitter's particles look and move. Ranges pick a random value
/// between min and max for every particle.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmitterConfig {
    /// Texture id; 0 draws round dots
    pub texture: u32,
    /// Region of the texture (w or h <= 0: the whole texture)
    pub src: Rect,
    /// Particles per second while emitting
    pub rate: f32,
    /// Most particles alive at once; new ones are skipped beyond it
    pub max: u32,
    /// Seconds a particle lives
    pub life_min: f32,
    pub life_max: f32,
    /// Pixels per second
    pub speed_min: f32,
    pub speed_max: f32,
    /// Direction in radians: 0 is right, PI/2 is down (y points down)
    pub angle: f32,
    /// Width of the cone of directions in radians (2*PI: every way)
    pub spread: f32,
    /// Acceleration in pixels per second squared
    pub gravity: Vec2,
    /// Share of speed lost per second (0: none, 1: stops in about a second)
    pub drag: f32,
    /// Size in pixels (dot diameter, or texture width) at birth and death
    pub size_start: f32,
    pub size_end: f32,
    /// Rotation speed in radians per second (textures only)
    pub spin_min: f32,
    pub spin_max: f32,
    pub color_start: Color,
    pub color_end: Color,
    /// Particles appear in a box of this half-size around the emitter
    pub area: Vec2,
    pub layer: i32,
    /// Random seed; the same seed and the same calls give the same particles
    pub seed: u32,
}

impl Default for EmitterConfig {
    fn default() -> Self {
        EmitterConfig {
            texture: 0,
            src: Rect::default(),
            rate: 20.0,
            max: 500,
            life_min: 1.0,
            life_max: 1.0,
            speed_min: 20.0,
            speed_max: 40.0,
            angle: -std::f32::consts::FRAC_PI_2,
            spread: 0.5,
            gravity: Vec2::default(),
            drag: 0.0,
            size_start: 4.0,
            size_end: 0.0,
            spin_min: 0.0,
            spin_max: 0.0,
            color_start: Color::WHITE,
            color_end: Color::rgba(255, 255, 255, 0),
            area: Vec2::default(),
            layer: 0,
            seed: 1,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Particle {
    pub pos: Vec2,
    pub vel: Vec2,
    pub age: f32,
    pub life: f32,
    pub rotation: f32,
    pub spin: f32,
}

/// The random sequence of an emitter
#[derive(Debug, Clone, Copy)]
struct Dice(u64);

impl Dice {
    fn new(seed: u32) -> Dice {
        Dice((seed as u64 ^ 0x9E37_79B9_7F4A_7C15).wrapping_mul(0xBF58_476D_1CE4_E5B9) | 1)
    }

    /// Random number in 0..1 (xorshift64*)
    fn random(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let x = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        lerp(min, max, self.random())
    }

    /// A new particle at `pos`
    fn particle(&mut self, c: &EmitterConfig, pos: Vec2) -> Particle {
        let angle = c.angle + (self.random() - 0.5) * c.spread;
        let speed = self.range(c.speed_min, c.speed_max);
        let offset = Vec2::new(
            (self.random() * 2.0 - 1.0) * c.area.x,
            (self.random() * 2.0 - 1.0) * c.area.y,
        );
        let life = self.range(c.life_min, c.life_max).max(0.001);
        let spin = self.range(c.spin_min, c.spin_max);
        let rotation = if spin != 0.0 {
            self.random() * std::f32::consts::TAU
        } else {
            0.0
        };
        Particle {
            pos: Vec2::new(pos.x + offset.x, pos.y + offset.y),
            vel: Vec2::new(angle.cos() * speed, angle.sin() * speed),
            age: 0.0,
            life,
            rotation,
            spin,
        }
    }
}

pub struct Emitter {
    pub config: EmitterConfig,
    pub pos: Vec2,
    /// Spawn at `rate` per second; bursts work either way
    pub emitting: bool,
    pub particles: Vec<Particle>,
    /// Fraction of a particle owed to the next frame
    carry: f32,
    rng: Dice,
}

impl Emitter {
    pub fn new(config: EmitterConfig, pos: Vec2) -> Emitter {
        let mut e = Emitter {
            config,
            pos,
            emitting: true,
            particles: Vec::new(),
            carry: 0.0,
            rng: Dice::new(0),
        };
        e.reseed(config.seed);
        e
    }

    /// Change the config; living particles keep flying. A new seed
    /// restarts the random sequence.
    pub fn set_config(&mut self, config: EmitterConfig) {
        if config.seed != self.config.seed {
            self.reseed(config.seed);
        }
        self.config = config;
    }

    /// Restart the random sequence (seeds are mixed so 1, 2, 3 differ a lot)
    pub fn reseed(&mut self, seed: u32) {
        self.rng = Dice::new(seed);
    }

    /// Spawn `count` particles now
    pub fn burst(&mut self, count: u32) {
        for _ in 0..count {
            if self.particles.len() >= self.config.max as usize {
                return;
            }
            let p = self.rng.particle(&self.config, self.pos);
            self.particles.push(p);
        }
    }

    /// Move particles by `dt` seconds, drop dead ones, spawn new ones
    pub fn update(&mut self, dt: f32) {
        let c = self.config;
        let keep = (1.0 - c.drag * dt).max(0.0);
        self.particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.life {
                return false;
            }
            p.vel.x = (p.vel.x + c.gravity.x * dt) * keep;
            p.vel.y = (p.vel.y + c.gravity.y * dt) * keep;
            p.pos.x += p.vel.x * dt;
            p.pos.y += p.vel.y * dt;
            p.rotation += p.spin * dt;
            true
        });
        if self.emitting && c.rate > 0.0 {
            self.carry += c.rate * dt;
            let n = self.carry.floor();
            self.carry -= n;
            self.burst(n as u32);
        } else {
            self.carry = 0.0;
        }
    }

    pub fn clear(&mut self) {
        self.particles.clear();
        self.carry = 0.0;
    }

    /// Queue the particles; `src` is the resolved texture region (the
    /// engine fills in the whole texture when the config's is empty)
    pub fn draw(&self, renderer: &mut Renderer, src: Rect, depth: f32, space: Space) {
        let c = self.config;
        for p in &self.particles {
            let t = p.age / p.life;
            let size = lerp(c.size_start, c.size_end, t);
            let color = lerp_color(c.color_start, c.color_end, t);
            if let Some(kind) = look(
                c.texture, src, c.layer, depth, p.pos, p.rotation, size, color,
            ) {
                renderer.push(c.layer, depth, space, kind);
            }
        }
    }
}

/// How one particle is drawn: a texture region `size` pixels wide, or a
/// dot `size` across; None when it cannot be seen
#[allow(clippy::too_many_arguments)]
pub(crate) fn look(
    texture: u32,
    src: Rect,
    layer: i32,
    depth: f32,
    pos: Vec2,
    rotation: f32,
    size: f32,
    color: Color,
) -> Option<DrawKind> {
    if size <= 0.0 || color.a == 0 {
        return None;
    }
    Some(if texture != 0 && src.w > 0.0 && src.h > 0.0 {
        let scale = size / src.w;
        DrawKind::Sprite {
            texture,
            sprite: Sprite {
                src,
                pos,
                origin: Vec2::new(src.w / 2.0, src.h / 2.0),
                scale: Vec2::new(scale, scale),
                rotation,
                tint: color,
                layer,
                depth,
                ..Sprite::default()
            },
        }
    } else {
        DrawKind::Shape {
            shape: Shape::Circle {
                center: pos,
                radius: size / 2.0,
                filled: true,
                thickness: 0.0,
            },
            color,
        }
    })
}

// --- GPU emitters ---------------------------------------------------------------

/// Where a particle is `age` seconds after it was born at `pos` with
/// `vel`: the exact path under gravity and drag. The graphics card works
/// out the same, so nothing moves particles frame by frame.
pub fn path(pos: Vec2, vel: Vec2, gravity: Vec2, drag: f32, age: f32) -> Vec2 {
    if drag < 1e-4 {
        let h = 0.5 * age * age;
        return Vec2::new(
            pos.x + vel.x * age + gravity.x * h,
            pos.y + vel.y * age + gravity.y * h,
        );
    }
    let (gx, gy) = (gravity.x / drag, gravity.y / drag);
    let f = (1.0 - (-drag * age).exp()) / drag;
    Vec2::new(
        pos.x + gx * age + (vel.x - gx) * f,
        pos.y + gy * age + (vel.y - gy) * f,
    )
}

/// One particle of a GPU emitter as the graphics card reads it: how it
/// was born. Everything else follows from the time.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Record {
    pub pos: Vec2,
    pub vel: Vec2,
    pub birth: f32,
    /// 0: an empty slot
    pub life: f32,
    pub rotation: f32,
    pub spin: f32,
}

impl Record {
    pub fn alive(&self, time: f32) -> bool {
        let age = time - self.birth;
        age >= 0.0 && age < self.life
    }
}

/// A GPU emitter's particles: a ring of `max` slots, overwritten in turn
#[derive(Debug, Clone)]
pub struct Ring {
    /// Unique for the life of the program, so the GPU can keep a copy
    pub id: u64,
    /// Changes when every slot changes (cleared, resized, times moved)
    pub epoch: u32,
    /// Particles ever written: the next goes to `written % len`
    pub written: u64,
    pub records: Vec<Record>,
}

impl Ring {
    fn new(len: u32) -> Ring {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Ring {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            epoch: 0,
            written: 0,
            records: vec![Record::default(); len as usize],
        }
    }
}

/// Seconds after which a GPU emitter's clock and its particles' birth
/// times move back, so f32 times stay precise in long sessions
const REBASE: f32 = 1024.0;

/// An emitter for huge numbers of particles: it only records how each
/// particle was born, and the graphics card works out where it is now.
/// It spawns the same particles as an Emitter with the same config and
/// seed; drag follows the exact curve. Particles cannot be read one by
/// one, only counted.
pub struct GpuEmitter {
    pub config: EmitterConfig,
    pub pos: Vec2,
    pub emitting: bool,
    ring: std::sync::Arc<Ring>,
    time: f32,
    carry: f32,
    rng: Dice,
}

impl GpuEmitter {
    pub fn new(config: EmitterConfig, pos: Vec2) -> GpuEmitter {
        GpuEmitter {
            config,
            pos,
            emitting: true,
            ring: std::sync::Arc::new(Ring::new(config.max)),
            time: 0.0,
            carry: 0.0,
            rng: Dice::new(config.seed),
        }
    }

    /// Like Emitter::set_config; a new `max` drops the living particles
    pub fn set_config(&mut self, config: EmitterConfig) {
        if config.seed != self.config.seed {
            self.rng = Dice::new(config.seed);
        }
        if config.max != self.config.max {
            let epoch = self.ring.epoch + 1;
            let ring = std::sync::Arc::make_mut(&mut self.ring);
            ring.records = vec![Record::default(); config.max as usize];
            ring.epoch = epoch;
        }
        self.config = config;
    }

    pub fn reseed(&mut self, seed: u32) {
        self.rng = Dice::new(seed);
    }

    /// Spawn `count` particles now; skipped while the slot they would
    /// take still holds a living one
    pub fn burst(&mut self, count: u32) {
        let (time, pos, c) = (self.time, self.pos, self.config);
        let ring = std::sync::Arc::make_mut(&mut self.ring);
        let len = ring.records.len() as u64;
        if len == 0 {
            return;
        }
        for _ in 0..count {
            let slot = &mut ring.records[(ring.written % len) as usize];
            if slot.alive(time) {
                return;
            }
            let p = self.rng.particle(&c, pos);
            *slot = Record {
                pos: p.pos,
                vel: p.vel,
                birth: time,
                life: p.life,
                rotation: p.rotation,
                spin: p.spin,
            };
            ring.written += 1;
        }
    }

    /// Move the clock by `dt` seconds and spawn at the rate
    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        if self.time >= REBASE {
            self.time -= REBASE;
            let ring = std::sync::Arc::make_mut(&mut self.ring);
            for r in &mut ring.records {
                r.birth -= REBASE;
            }
            ring.epoch += 1;
        }
        if self.emitting && self.config.rate > 0.0 {
            self.carry += self.config.rate * dt;
            let n = self.carry.floor();
            self.carry -= n;
            self.burst(n as u32);
        } else {
            self.carry = 0.0;
        }
    }

    pub fn clear(&mut self) {
        let epoch = self.ring.epoch + 1;
        let ring = std::sync::Arc::make_mut(&mut self.ring);
        ring.records.fill(Record::default());
        ring.epoch = epoch;
        self.carry = 0.0;
    }

    /// Living particles (counts every slot)
    pub fn count(&self) -> u32 {
        self.ring
            .records
            .iter()
            .filter(|r| r.alive(self.time))
            .count() as u32
    }

    pub fn ring(&self) -> &Ring {
        &self.ring
    }

    /// What the renderers need to draw the particles as they are now;
    /// `src` is the resolved texture region
    pub fn draw_data(&self, src: Rect) -> ParticleDraw {
        let c = self.config;
        ParticleDraw {
            ring: self.ring.clone(),
            time: self.time,
            texture: c.texture,
            src,
            gravity: c.gravity,
            drag: c.drag,
            size: [c.size_start, c.size_end],
            colors: [c.color_start, c.color_end],
        }
    }
}

/// A GPU emitter's particles queued for drawing
#[derive(Debug, Clone)]
pub struct ParticleDraw {
    pub ring: std::sync::Arc<Ring>,
    pub time: f32,
    pub texture: u32,
    pub src: Rect,
    pub gravity: Vec2,
    pub drag: f32,
    pub size: [f32; 2],
    pub colors: [Color; 2],
}

impl ParticleDraw {
    /// Every living particle as it looks now: position, rotation, size,
    /// colour (the software renderer's way; the GPU shader matches it)
    pub fn each(&self, mut f: impl FnMut(Vec2, f32, f32, Color)) {
        for r in &self.ring.records {
            if !r.alive(self.time) {
                continue;
            }
            let age = self.time - r.birth;
            let t = age / r.life;
            let pos = path(r.pos, r.vel, self.gravity, self.drag, age);
            let size = lerp(self.size[0], self.size[1], t);
            let color = lerp_color(self.colors[0], self.colors[1], t);
            f(pos, r.rotation + r.spin * age, size, color);
        }
    }

    /// Draws a texture (not round dots)
    pub fn textured(&self) -> bool {
        self.texture != 0 && self.src.w > 0.0 && self.src.h > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> EmitterConfig {
        EmitterConfig {
            rate: 10.0,
            life_min: 1.0,
            life_max: 1.0,
            speed_min: 10.0,
            speed_max: 10.0,
            angle: 0.0,
            spread: 0.0,
            ..EmitterConfig::default()
        }
    }

    #[test]
    fn rate_life_and_motion() {
        let mut e = Emitter::new(cfg(), Vec2::new(0.0, 0.0));
        for _ in 0..5 {
            e.update(0.1);
        }
        assert_eq!(e.particles.len(), 5, "10 per second for half a second");
        // The oldest has flown for 0.4 s at 10 px/s to the right
        let first = e.particles[0];
        assert!(
            (first.pos.x - 4.0).abs() < 1e-3 && first.pos.y == 0.0,
            "{:?}",
            first
        );
        for _ in 0..10 {
            e.update(0.1);
        }
        assert_eq!(e.particles.len(), 10, "one second of life: steady state");
        e.emitting = false;
        e.update(1.0);
        assert!(e.particles.is_empty(), "all die after their life");
    }

    #[test]
    fn gravity_drag_burst_cap_and_seed() {
        let mut e = Emitter::new(
            EmitterConfig {
                rate: 0.0,
                max: 3,
                speed_min: 0.0,
                speed_max: 0.0,
                gravity: Vec2::new(0.0, 100.0),
                ..cfg()
            },
            Vec2::new(0.0, 0.0),
        );
        e.burst(10);
        assert_eq!(e.particles.len(), 3, "capped at max");
        e.update(0.5);
        assert!((e.particles[0].vel.y - 50.0).abs() < 1e-3, "falls");

        let mut d = Emitter::new(
            EmitterConfig {
                rate: 0.0,
                drag: 1.0,
                ..cfg()
            },
            Vec2::default(),
        );
        d.burst(1);
        d.update(0.5);
        assert!(
            (d.particles[0].vel.x - 5.0).abs() < 1e-3,
            "half the speed lost"
        );

        let spread = EmitterConfig {
            rate: 0.0,
            spread: std::f32::consts::TAU,
            speed_min: 5.0,
            speed_max: 50.0,
            area: Vec2::new(10.0, 10.0),
            ..cfg()
        };
        let run = |seed| {
            let mut e = Emitter::new(EmitterConfig { seed, ..spread }, Vec2::default());
            e.burst(20);
            e.particles
                .iter()
                .map(|p| (p.pos, p.vel))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(7), run(7), "same seed, same particles");
        assert_ne!(run(7), run(8));
        let ps = run(3);
        assert!(ps
            .iter()
            .all(|(p, _)| p.x.abs() <= 10.0 && p.y.abs() <= 10.0));
        assert!(ps.iter().any(|(_, v)| v.x < 0.0) && ps.iter().any(|(_, v)| v.x > 0.0));
    }

    #[test]
    fn engine_draws_dots_and_textures() {
        use crate::engine::Engine;
        let mut e = Engine::headless(32, 32).unwrap();
        e.set_space(Space::Screen);
        let dots = EmitterConfig {
            rate: 0.0,
            speed_min: 0.0,
            speed_max: 0.0,
            size_start: 6.0,
            size_end: 6.0,
            color_start: Color::rgba(255, 0, 0, 255),
            color_end: Color::rgba(255, 0, 0, 255),
            ..cfg()
        };
        let a = e.create_emitter(dots, Vec2::new(8.0, 8.0));
        let tex = e.texture_from_rgba(2, 2, &[255; 16]).unwrap();
        let sprites = EmitterConfig {
            texture: tex,
            color_start: Color::rgba(0, 255, 0, 255),
            color_end: Color::rgba(0, 255, 0, 255),
            ..dots
        };
        let b = e.create_emitter(sprites, Vec2::new(24.0, 24.0));
        e.emitter_mut(a).unwrap().burst(1);
        e.emitter_mut(b).unwrap().burst(1);
        e.update_emitters(0.1);
        e.begin_frame(Color::BLACK);
        e.draw_emitter(a);
        e.draw_emitter(b);
        e.end_frame();
        assert_eq!(e.pixel(8, 8), Some(Color::rgba(255, 0, 0, 255)));
        assert_eq!(
            e.pixel(22, 22),
            Some(Color::rgba(0, 255, 0, 255)),
            "6 px wide"
        );
        assert_eq!(e.pixel(16, 16), Some(Color::BLACK));
        e.free_emitter(a);
        assert!(e.emitter(a).is_none());
    }

    #[test]
    fn gpu_emitters_spawn_the_same_particles() {
        let c = EmitterConfig {
            rate: 0.0,
            spread: std::f32::consts::TAU,
            speed_min: 5.0,
            speed_max: 50.0,
            spin_min: -1.0,
            spin_max: 1.0,
            area: Vec2::new(10.0, 10.0),
            seed: 5,
            ..cfg()
        };
        let mut cpu = Emitter::new(c, Vec2::new(3.0, 4.0));
        let mut gpu = GpuEmitter::new(c, Vec2::new(3.0, 4.0));
        cpu.burst(20);
        gpu.burst(20);
        for (p, r) in cpu.particles.iter().zip(&gpu.ring().records) {
            assert_eq!(
                (p.pos, p.vel, p.life, p.rotation, p.spin),
                (r.pos, r.vel, r.life, r.rotation, r.spin)
            );
        }
        assert_eq!(gpu.count(), 20);
        assert_eq!(gpu.ring().written, 20);
    }

    #[test]
    fn gpu_emitters_keep_rate_life_and_cap() {
        let mut e = GpuEmitter::new(cfg(), Vec2::default());
        for _ in 0..5 {
            e.update(0.1);
        }
        assert_eq!(e.count(), 5, "10 per second for half a second");
        for _ in 0..10 {
            e.update(0.1);
        }
        assert_eq!(e.count(), 10, "one second of life: steady state");
        e.emitting = false;
        e.update(1.0);
        assert_eq!(e.count(), 0, "all die after their life");

        let mut capped = GpuEmitter::new(
            EmitterConfig {
                rate: 0.0,
                max: 3,
                ..cfg()
            },
            Vec2::default(),
        );
        capped.burst(10);
        assert_eq!(
            (capped.count(), capped.ring().written),
            (3, 3),
            "capped at max"
        );
        capped.update(2.0);
        capped.burst(2);
        assert_eq!(capped.count(), 2, "dead slots are reused");
        let epoch = capped.ring().epoch;
        capped.clear();
        assert_eq!(capped.count(), 0);
        capped.set_config(EmitterConfig {
            max: 8,
            ..capped.config
        });
        assert_eq!(capped.ring().records.len(), 8);
        assert!(
            capped.ring().epoch > epoch + 1,
            "every change is seen by the GPU"
        );
    }

    #[test]
    fn paths_follow_gravity_and_drag() {
        let (p, v, g) = (
            Vec2::new(1.0, 2.0),
            Vec2::new(30.0, -20.0),
            Vec2::new(0.0, 50.0),
        );
        let at = path(p, v, g, 0.0, 2.0);
        assert!((at.x - 61.0).abs() < 1e-3 && (at.y - 62.0).abs() < 1e-3);
        // Drag: a fine step-by-step simulation lands in the same place
        let (mut x, mut u, dt) = (p, v, 1e-4f32);
        for _ in 0..10_000 {
            u = Vec2::new(u.x + (g.x - 1.5 * u.x) * dt, u.y + (g.y - 1.5 * u.y) * dt);
            x = Vec2::new(x.x + u.x * dt, x.y + u.y * dt);
        }
        let at = path(p, v, g, 1.5, 1.0);
        assert!(
            (at.x - x.x).abs() < 0.02 && (at.y - x.y).abs() < 0.02,
            "{at:?} {x:?}"
        );
    }

    #[test]
    fn long_sessions_keep_particles_in_place() {
        let mut e = GpuEmitter::new(
            EmitterConfig {
                rate: 0.0,
                life_min: 10.0,
                life_max: 10.0,
                ..cfg()
            },
            Vec2::default(),
        );
        e.update(1020.0);
        e.burst(1);
        let epoch = e.ring().epoch;
        e.update(5.0);
        assert!(e.ring().epoch > epoch, "the clock moved back");
        let mut seen = Vec::new();
        e.draw_data(Rect::default())
            .each(|pos, _, _, _| seen.push(pos));
        assert_eq!(seen.len(), 1);
        assert!(
            (seen[0].x - 50.0).abs() < 1e-2,
            "5 s at 10 px/s: {:?}",
            seen[0]
        );
    }

    #[test]
    fn engine_draws_gpu_emitters_in_software() {
        use crate::engine::Engine;
        let mut e = Engine::headless(32, 32).unwrap();
        e.set_space(Space::Screen);
        let dots = EmitterConfig {
            rate: 0.0,
            speed_min: 0.0,
            speed_max: 0.0,
            size_start: 6.0,
            size_end: 6.0,
            color_start: Color::rgba(255, 0, 0, 255),
            color_end: Color::rgba(255, 0, 0, 255),
            ..cfg()
        };
        let a = e.create_gpu_emitter(dots, Vec2::new(8.0, 8.0));
        let tex = e.texture_from_rgba(2, 2, &[255; 16]).unwrap();
        let sprites = EmitterConfig {
            texture: tex,
            color_start: Color::rgba(0, 255, 0, 255),
            color_end: Color::rgba(0, 255, 0, 255),
            ..dots
        };
        let b = e.create_gpu_emitter(sprites, Vec2::new(24.0, 24.0));
        e.gpu_emitter_mut(a).unwrap().burst(1);
        e.gpu_emitter_mut(b).unwrap().burst(1);
        e.update_gpu_emitters(0.1);
        e.begin_frame(Color::BLACK);
        e.draw_gpu_emitter(a);
        e.draw_gpu_emitter(b);
        e.end_frame();
        assert_eq!(e.pixel(8, 8), Some(Color::rgba(255, 0, 0, 255)));
        assert_eq!(
            e.pixel(22, 22),
            Some(Color::rgba(0, 255, 0, 255)),
            "6 px wide"
        );
        assert_eq!(e.pixel(16, 16), Some(Color::BLACK));
        e.free_gpu_emitter(a);
        assert!(e.gpu_emitter(a).is_none());
    }
}

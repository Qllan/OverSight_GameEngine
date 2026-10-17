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

pub struct Emitter {
    pub config: EmitterConfig,
    pub pos: Vec2,
    /// Spawn at `rate` per second; bursts work either way
    pub emitting: bool,
    pub particles: Vec<Particle>,
    /// Fraction of a particle owed to the next frame
    carry: f32,
    rng: u64,
}

impl Emitter {
    pub fn new(config: EmitterConfig, pos: Vec2) -> Emitter {
        let mut e = Emitter {
            config,
            pos,
            emitting: true,
            particles: Vec::new(),
            carry: 0.0,
            rng: 0,
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
        self.rng = (seed as u64 ^ 0x9E37_79B9_7F4A_7C15).wrapping_mul(0xBF58_476D_1CE4_E5B9) | 1;
    }

    /// Random number in 0..1 (xorshift64*)
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let x = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        lerp(min, max, self.random())
    }

    /// Spawn `count` particles now
    pub fn burst(&mut self, count: u32) {
        let c = self.config;
        for _ in 0..count {
            if self.particles.len() >= c.max as usize {
                return;
            }
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
            self.particles.push(Particle {
                pos: Vec2::new(self.pos.x + offset.x, self.pos.y + offset.y),
                vel: Vec2::new(angle.cos() * speed, angle.sin() * speed),
                age: 0.0,
                life,
                rotation,
                spin,
            });
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
            if size <= 0.0 {
                continue;
            }
            let color = lerp_color(c.color_start, c.color_end, t);
            if color.a == 0 {
                continue;
            }
            let kind = if c.texture != 0 && src.w > 0.0 && src.h > 0.0 {
                let scale = size / src.w;
                DrawKind::Sprite {
                    texture: c.texture,
                    sprite: Sprite {
                        src,
                        pos: p.pos,
                        origin: Vec2::new(src.w / 2.0, src.h / 2.0),
                        scale: Vec2::new(scale, scale),
                        rotation: p.rotation,
                        tint: color,
                        layer: c.layer,
                        depth,
                        ..Sprite::default()
                    },
                }
            } else {
                DrawKind::Shape {
                    shape: Shape::Circle {
                        center: p.pos,
                        radius: size / 2.0,
                        filled: true,
                        thickness: 0.0,
                    },
                    color,
                }
            };
            renderer.push(c.layer, depth, space, kind);
        }
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
}

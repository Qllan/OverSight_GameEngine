//! Tweens and easing: smooth changes of a value over time (a door that
//! slides open, a menu that flies in, a pulsing marker). `ease` bends
//! progress 0..1 along a curve; `Tween` keeps the time and gives the
//! current value. The curves are the usual Penner set, as on easings.net.

use crate::types::{Color, Vec2};
use std::f32::consts::PI;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ease {
    #[default]
    Linear = 0,
    QuadIn,
    QuadOut,
    QuadInOut,
    CubicIn,
    CubicOut,
    CubicInOut,
    SineIn,
    SineOut,
    SineInOut,
    ExpoIn,
    ExpoOut,
    ExpoInOut,
    BackIn,
    BackOut,
    BackInOut,
    ElasticIn,
    ElasticOut,
    ElasticInOut,
    BounceIn,
    BounceOut,
    BounceInOut,
}

impl Ease {
    pub const ALL: [Ease; 22] = [
        Ease::Linear,
        Ease::QuadIn,
        Ease::QuadOut,
        Ease::QuadInOut,
        Ease::CubicIn,
        Ease::CubicOut,
        Ease::CubicInOut,
        Ease::SineIn,
        Ease::SineOut,
        Ease::SineInOut,
        Ease::ExpoIn,
        Ease::ExpoOut,
        Ease::ExpoInOut,
        Ease::BackIn,
        Ease::BackOut,
        Ease::BackInOut,
        Ease::ElasticIn,
        Ease::ElasticOut,
        Ease::ElasticInOut,
        Ease::BounceIn,
        Ease::BounceOut,
        Ease::BounceInOut,
    ];

    /// Unknown numbers (from C) fall back to linear
    pub fn from_i32(v: i32) -> Ease {
        usize::try_from(v)
            .ok()
            .and_then(|i| Ease::ALL.get(i).copied())
            .unwrap_or(Ease::Linear)
    }
}

/// Eased progress for `t` in 0..1 (clamped). 0 maps to 0 and 1 to 1;
/// Back and Elastic overshoot in between.
pub fn ease(e: Ease, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // In-out curves: the "in" curve on the first half, mirrored on the second
    let in_out = |f: fn(f32) -> f32| {
        if t < 0.5 {
            f(t * 2.0) / 2.0
        } else {
            1.0 - f((1.0 - t) * 2.0) / 2.0
        }
    };
    let out = |f: fn(f32) -> f32| 1.0 - f(1.0 - t);
    match e {
        Ease::Linear => t,
        Ease::QuadIn => quad(t),
        Ease::QuadOut => out(quad),
        Ease::QuadInOut => in_out(quad),
        Ease::CubicIn => cubic(t),
        Ease::CubicOut => out(cubic),
        Ease::CubicInOut => in_out(cubic),
        Ease::SineIn => sine(t),
        Ease::SineOut => out(sine),
        Ease::SineInOut => in_out(sine),
        Ease::ExpoIn => expo(t),
        Ease::ExpoOut => out(expo),
        Ease::ExpoInOut => in_out(expo),
        Ease::BackIn => back(t),
        Ease::BackOut => out(back),
        Ease::BackInOut => in_out(back),
        Ease::ElasticIn => elastic(t),
        Ease::ElasticOut => out(elastic),
        Ease::ElasticInOut => in_out(elastic),
        Ease::BounceIn => 1.0 - bounce_out(1.0 - t),
        Ease::BounceOut => bounce_out(t),
        Ease::BounceInOut => {
            if t < 0.5 {
                (1.0 - bounce_out(1.0 - t * 2.0)) / 2.0
            } else {
                (1.0 + bounce_out(t * 2.0 - 1.0)) / 2.0
            }
        }
    }
}

fn quad(t: f32) -> f32 {
    t * t
}

fn cubic(t: f32) -> f32 {
    t * t * t
}

fn sine(t: f32) -> f32 {
    1.0 - (t * PI / 2.0).cos()
}

fn expo(t: f32) -> f32 {
    if t <= 0.0 {
        0.0
    } else {
        (2.0f32).powf(10.0 * t - 10.0)
    }
}

fn back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    (C1 + 1.0) * t * t * t - C1 * t * t
}

fn elastic(t: f32) -> f32 {
    if t <= 0.0 || t >= 1.0 {
        return t;
    }
    -(2.0f32).powf(10.0 * t - 10.0) * ((t * 10.0 - 10.75) * (2.0 * PI / 3.0)).sin()
}

fn bounce_out(t: f32) -> f32 {
    const N: f32 = 7.5625;
    const D: f32 = 2.75;
    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let t = t - 1.5 / D;
        N * t * t + 0.75
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        N * t * t + 0.9375
    } else {
        let t = t - 2.625 / D;
        N * t * t + 0.984375
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn lerp_vec2(a: Vec2, b: Vec2, t: f32) -> Vec2 {
    Vec2::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t))
}

pub fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let ch = |x: u8, y: u8| lerp(x as f32, y as f32, t).round().clamp(0.0, 255.0) as u8;
    Color::rgba(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b), ch(a.a, b.a))
}

/// What a tween does after reaching the end
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    /// Stop at the end value
    #[default]
    Once = 0,
    /// Start again from the beginning
    Loop,
    /// Go back to the start, then forward again
    PingPong,
}

impl Repeat {
    pub fn from_i32(v: i32) -> Repeat {
        match v {
            1 => Repeat::Loop,
            2 => Repeat::PingPong,
            _ => Repeat::Once,
        }
    }
}

/// Eased progress 0..1 of a tween `time` seconds after it started
/// (negative time is the delay before the start)
pub fn progress(time: f32, duration: f32, ease_fn: Ease, repeat: Repeat) -> f32 {
    if time <= 0.0 {
        return ease(ease_fn, 0.0);
    }
    if duration <= 0.0 {
        return ease(ease_fn, 1.0);
    }
    let t = match repeat {
        Repeat::Once => (time / duration).min(1.0),
        Repeat::Loop => (time / duration).fract(),
        Repeat::PingPong => {
            let p = (time / duration) % 2.0;
            if p > 1.0 {
                2.0 - p
            } else {
                p
            }
        }
    };
    ease(ease_fn, t)
}

/// A number going from `from` to `to` over `duration` seconds.
/// Call `update(dt)` every frame and read `value()`.
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub duration: f32,
    pub ease: Ease,
    pub repeat: Repeat,
    /// Seconds since the start; starts negative when there is a delay
    pub time: f32,
    pub paused: bool,
}

impl Tween {
    pub fn new(from: f32, to: f32, duration: f32, ease: Ease) -> Tween {
        Tween {
            from,
            to,
            duration,
            ease,
            repeat: Repeat::Once,
            time: 0.0,
            paused: false,
        }
    }

    pub fn repeat(mut self, repeat: Repeat) -> Tween {
        self.repeat = repeat;
        self
    }

    pub fn delay(mut self, seconds: f32) -> Tween {
        self.time = -seconds;
        self
    }

    /// Advance by `dt` seconds and return the new value
    pub fn update(&mut self, dt: f32) -> f32 {
        if !self.paused {
            self.time += dt;
        }
        self.value()
    }

    pub fn progress(&self) -> f32 {
        progress(self.time, self.duration, self.ease, self.repeat)
    }

    pub fn value(&self) -> f32 {
        lerp(self.from, self.to, self.progress())
    }

    /// A tween that repeats is never done
    pub fn done(&self) -> bool {
        self.repeat == Repeat::Once && self.time >= self.duration
    }

    /// Back to the start, keeping the delay out
    pub fn restart(&mut self) {
        self.time = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_curve_starts_at_0_and_ends_at_1() {
        for e in Ease::ALL {
            assert!(ease(e, 0.0).abs() < 1e-4, "{:?} at 0: {}", e, ease(e, 0.0));
            assert!(
                (ease(e, 1.0) - 1.0).abs() < 1e-4,
                "{:?} at 1: {}",
                e,
                ease(e, 1.0)
            );
            assert_eq!(ease(e, -1.0), ease(e, 0.0), "{:?} clamps below", e);
            assert_eq!(ease(e, 2.0), ease(e, 1.0), "{:?} clamps above", e);
        }
        for (i, e) in Ease::ALL.iter().enumerate() {
            assert_eq!(Ease::from_i32(i as i32), *e);
        }
        assert_eq!(Ease::from_i32(99), Ease::Linear);
    }

    #[test]
    fn curve_shapes() {
        assert_eq!(ease(Ease::QuadIn, 0.5), 0.25);
        assert_eq!(ease(Ease::QuadOut, 0.5), 0.75);
        for e in [
            Ease::QuadInOut,
            Ease::CubicInOut,
            Ease::SineInOut,
            Ease::BounceInOut,
        ] {
            assert!((ease(e, 0.5) - 0.5).abs() < 1e-4, "{:?} symmetric", e);
        }
        // Back goes below 0 first, Elastic out overshoots past 1
        assert!(ease(Ease::BackIn, 0.2) < 0.0);
        assert!((0..100).any(|i| ease(Ease::ElasticOut, i as f32 / 100.0) > 1.0));
        // Bounce touches the ground (1.0) between bounces
        assert!((ease(Ease::BounceOut, 1.0 / 2.75) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn tween_delay_repeat_and_done() {
        let mut t = Tween::new(10.0, 20.0, 1.0, Ease::Linear).delay(0.5);
        assert_eq!(t.update(0.25), 10.0, "still waiting");
        assert_eq!(t.update(0.75), 15.0);
        assert!(!t.done());
        assert_eq!(t.update(1.0), 20.0);
        assert!(t.done());
        assert_eq!(t.update(5.0), 20.0, "stays at the end");

        let mut l = Tween::new(0.0, 1.0, 1.0, Ease::Linear).repeat(Repeat::Loop);
        assert!((l.update(1.25) - 0.25).abs() < 1e-5);
        assert!(!l.done());

        let mut p = Tween::new(0.0, 1.0, 1.0, Ease::Linear).repeat(Repeat::PingPong);
        assert!((p.update(1.25) - 0.75).abs() < 1e-5, "on the way back");
        assert!((p.update(1.0) - 0.25).abs() < 1e-5, "forward again");

        p.paused = true;
        let v = p.value();
        assert_eq!(p.update(0.3), v);

        assert_eq!(progress(1.0, 0.0, Ease::Linear, Repeat::Once), 1.0);
        assert_eq!(
            lerp_color(Color::BLACK, Color::rgba(255, 255, 255, 255), 0.5),
            Color::rgba(128, 128, 128, 255)
        );
        assert_eq!(
            lerp_vec2(Vec2::new(0.0, 0.0), Vec2::new(4.0, -2.0), 0.5),
            Vec2::new(2.0, -1.0)
        );
    }
}

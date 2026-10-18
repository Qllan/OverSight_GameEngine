//! Frame timing: delta time, total time, frame count and an FPS estimate,
//! plus fixed logic ticks: game logic runs a whole number of equal steps
//! per frame, so it behaves the same at any frame rate and can be replayed
//! or synced over the network.

use std::time::{Duration, Instant};

/// Longest step reported as dt (after a hitch or a breakpoint the game
/// does not jump forward by seconds)
pub const MAX_DT: f32 = 0.25;

/// Most logic ticks run in one frame: a slow machine falls behind instead
/// of spending ever longer frames catching up
pub const MAX_TICKS: u32 = 8;

/// Logic ticks per second unless the game sets another rate
pub const DEFAULT_TICK_RATE: u32 = 60;

#[derive(Debug, Clone)]
pub struct Time {
    /// Seconds since the previous frame
    pub dt: f32,
    /// Seconds since the start
    pub total: f64,
    /// Frames started so far
    pub frame: u64,
    /// Frames per second, smoothed
    pub fps: f32,
    /// Fixed step instead of real time (headless engines, replays)
    pub fixed_dt: Option<f32>,
    /// Logic ticks per second
    pub tick_rate: u32,
    /// Logic ticks to run this frame
    pub ticks: u32,
    /// Logic ticks run so far, this frame's included
    pub tick_count: u64,
    /// Time not yet turned into ticks, in seconds
    acc: f64,
    last: Option<Instant>,
    frame_start: Option<Instant>,
}

impl Time {
    pub fn new(fixed_dt: Option<f32>) -> Time {
        Time {
            dt: 0.0,
            total: 0.0,
            frame: 0,
            fps: 0.0,
            fixed_dt,
            tick_rate: DEFAULT_TICK_RATE,
            ticks: 0,
            tick_count: 0,
            acc: 0.0,
            last: None,
            frame_start: None,
        }
    }

    /// Start a new frame
    pub fn tick(&mut self) {
        let now = Instant::now();
        self.dt = match (self.fixed_dt, self.last) {
            (Some(step), _) => step,
            (None, Some(last)) => (now - last).as_secs_f32().min(MAX_DT),
            (None, None) => 0.0,
        };
        self.last = Some(now);
        self.frame_start = Some(now);
        self.total += self.dt as f64;
        self.frame += 1;
        self.advance_ticks(self.dt as f64);
        if self.dt > 0.0 {
            let current = 1.0 / self.dt;
            self.fps = if self.fps == 0.0 {
                current
            } else {
                self.fps * 0.9 + current * 0.1
            };
        }
    }

    /// Seconds of one logic tick
    pub fn tick_dt(&self) -> f32 {
        1.0 / self.tick_rate as f32
    }

    /// How far the time is between the last tick and the next one, 0..1:
    /// draw moving things that much past their last tick for smooth motion
    pub fn tick_alpha(&self) -> f32 {
        (self.acc * self.tick_rate as f64).clamp(0.0, 1.0) as f32
    }

    /// Change the tick rate (at least 1 per second); time already
    /// gathered toward the next tick is kept
    pub fn set_tick_rate(&mut self, rate: u32) {
        self.tick_rate = rate.max(1);
    }

    fn advance_ticks(&mut self, dt: f64) {
        let step = 1.0 / self.tick_rate as f64;
        self.acc += dt;
        // A frame of exactly one tick in f32 seconds is a hair short or long
        // (1/50 s as f32 is 0.019999999, 1/60 s is 0.016666668): count it
        // as a whole tick and drop the hair, so it never adds up to a
        // skipped or doubled tick
        let tolerance = 1e-4;
        let due = (self.acc / step + tolerance).floor().max(0.0);
        self.ticks = (due as u32).min(MAX_TICKS);
        self.acc -= self.ticks as f64 * step;
        if self.acc.abs() < tolerance * step {
            self.acc = 0.0;
        }
        if self.ticks == MAX_TICKS {
            // Behind: drop the backlog rather than carry it into next frames
            self.acc = self.acc.min(step);
        }
        self.tick_count += self.ticks as u64;
    }

    /// Sleep until the frame has lasted 1/fps seconds
    pub fn limit(&self, fps: u32) {
        let (Some(start), true) = (self.frame_start, fps > 0) else {
            return;
        };
        let target = Duration::from_secs_f64(1.0 / fps as f64);
        let elapsed = start.elapsed();
        if elapsed < target {
            std::thread::sleep(target - elapsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_step() {
        let mut t = Time::new(Some(0.5));
        t.tick();
        t.tick();
        assert_eq!((t.dt, t.total, t.frame), (0.5, 1.0, 2));
        assert_eq!(t.fps, 2.0);
    }

    #[test]
    fn one_tick_per_frame_at_the_tick_rate() {
        // 1/60 and 1/50 as f32 are just above and just below the real step
        for rate in [60, 50, 30, 144] {
            let mut t = Time::new(Some(1.0 / rate as f32));
            t.set_tick_rate(rate);
            for _ in 0..10_000 {
                t.tick();
                assert_eq!(t.ticks, 1, "rate {rate}, frame {}", t.frame);
            }
            assert_eq!(t.tick_count, 10_000);
        }
    }

    #[test]
    fn ticks_follow_the_frame_rate() {
        // 30 FPS with 60 ticks: two ticks each frame
        let mut t = Time::new(Some(1.0 / 30.0));
        t.tick();
        assert_eq!(t.ticks, 2);
        // 120 FPS with 60 ticks: a tick every other frame, alpha in between
        let mut t = Time::new(Some(1.0 / 120.0));
        let ticks: Vec<u32> = (0..6)
            .map(|_| {
                t.tick();
                t.ticks
            })
            .collect();
        assert_eq!(ticks, [0, 1, 0, 1, 0, 1]);
        t.tick();
        assert!((t.tick_alpha() - 0.5).abs() < 1e-3, "{}", t.tick_alpha());
        assert_eq!(t.tick_dt(), 1.0 / 60.0);
    }

    #[test]
    fn a_long_frame_runs_at_most_max_ticks() {
        let mut t = Time::new(Some(MAX_DT));
        t.set_tick_rate(1000);
        t.tick();
        assert_eq!(t.ticks, MAX_TICKS);
        t.fixed_dt = Some(0.0);
        t.tick();
        assert!(t.ticks <= 1, "the backlog is dropped: {}", t.ticks);
    }

    #[test]
    fn real_time_starts_at_zero_and_is_clamped() {
        let mut t = Time::new(None);
        t.tick();
        assert_eq!(t.dt, 0.0);
        t.tick();
        assert!(t.dt >= 0.0 && t.dt <= MAX_DT);
    }
}

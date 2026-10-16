//! Frame timing: delta time, total time, frame count and an FPS estimate.

use std::time::{Duration, Instant};

/// Longest step reported as dt (after a hitch or a breakpoint the game
/// does not jump forward by seconds)
pub const MAX_DT: f32 = 0.25;

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
        if self.dt > 0.0 {
            let current = 1.0 / self.dt;
            self.fps = if self.fps == 0.0 {
                current
            } else {
                self.fps * 0.9 + current * 0.1
            };
        }
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
    fn real_time_starts_at_zero_and_is_clamped() {
        let mut t = Time::new(None);
        t.tick();
        assert_eq!(t.dt, 0.0);
        t.tick();
        assert!(t.dt >= 0.0 && t.dt <= MAX_DT);
    }
}

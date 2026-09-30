//! FixedTickLoop - Deterministic fixed-timestep game loop
//!
//! The Python main loop calls `tick()` once per frame. `tick()` sleeps until
//! the next frame boundary so the loop runs at a fixed rate (60 FPS by
//! default). Game logic should advance by `fixed_delta()` per tick, never by
//! wall-clock time, to stay deterministic.

use pyo3::prelude::*;
use std::time::{Duration, Instant};

const DEFAULT_FPS: u32 = 60;
const MAX_FPS: u32 = 240;

/// Game loop controller
#[pyclass]
pub struct FixedTickLoop {
    running: bool,
    fps: u32,
    frame_count: u64,
    /// Scheduled start time of the next frame
    next_frame_time: Option<Instant>,
}

#[pymethods]
impl FixedTickLoop {
    #[new]
    pub fn new() -> Self {
        FixedTickLoop {
            running: false,
            fps: DEFAULT_FPS,
            frame_count: 0,
            next_frame_time: None,
        }
    }

    /// Start the game loop
    pub fn start(&mut self) {
        self.running = true;
        self.next_frame_time = Some(Instant::now());
        log::info!("Game loop started at {} FPS", self.fps);
    }

    /// Stop the game loop
    pub fn stop(&mut self) {
        self.running = false;
        log::info!("Game loop stopped. Total frames: {}", self.frame_count);
    }

    /// Check if the loop is running
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Execute one tick (called by the Python main loop).
    /// With `wait=True` it sleeps until the frame boundary, releasing the GIL.
    /// Returns true if the loop should continue, false if it was stopped.
    #[pyo3(signature = (wait = true))]
    pub fn tick(&mut self, py: Python<'_>, wait: bool) -> bool {
        if !self.running {
            return false;
        }

        let step = self.timestep();
        let now = Instant::now();
        let target = self.next_frame_time.unwrap_or(now);

        if wait && target > now {
            py.detach(|| std::thread::sleep(target - now));
        }

        // Schedule the next frame from the previous target to avoid drift.
        // If we fell more than a frame behind, resync instead of bursting.
        let next = target + step;
        self.next_frame_time = Some(if next < Instant::now() {
            Instant::now() + step
        } else {
            next
        });
        self.frame_count += 1;

        true
    }

    /// Get current FPS setting
    pub fn get_fps(&self) -> u32 {
        self.fps
    }

    /// Set FPS (1..=240)
    pub fn set_fps(&mut self, fps: u32) -> PyResult<()> {
        if fps == 0 || fps > MAX_FPS {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "FPS must be between 1 and {}",
                MAX_FPS
            )));
        }
        self.fps = fps;
        Ok(())
    }

    /// Fixed simulation step in seconds (1 / fps)
    pub fn fixed_delta(&self) -> f64 {
        1.0 / self.fps as f64
    }

    /// Get total frame count
    pub fn get_frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Reset frame counter
    pub fn reset_counter(&mut self) {
        self.frame_count = 0;
        self.next_frame_time = Some(Instant::now());
    }
}

impl FixedTickLoop {
    fn timestep(&self) -> Duration {
        Duration::from_nanos(1_000_000_000 / self.fps as u64)
    }
}

impl Default for FixedTickLoop {
    fn default() -> Self {
        Self::new()
    }
}

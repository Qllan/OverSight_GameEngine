//! FixedTickLoop - Deterministic 60 FPS game loop
//!
//! Provides a fixed timestep game loop for deterministic simulation.
//! In Phase 1, this is a stub that will be expanded in Phase 2 with
//! actual rendering and input handling.

use pyo3::prelude::*;
use std::time::{Duration, Instant};

/// Fixed timestep for 60 FPS (16.67ms per frame)
const FIXED_TIMESTEP: Duration = Duration::from_millis(16);

/// Game loop controller
#[pyclass]
pub struct FixedTickLoop {
    running: bool,
    fps: u32,
    frame_count: u64,
    last_frame_time: Option<Instant>,
}

#[pymethods]
impl FixedTickLoop {
    #[new]
    fn new() -> Self {
        FixedTickLoop {
            running: false,
            fps: 60,
            frame_count: 0,
            last_frame_time: None,
        }
    }

    /// Start the game loop
    fn start(&mut self) {
        self.running = true;
        self.last_frame_time = Some(Instant::now());
        log::info!("Game loop started at {} FPS", self.fps);
    }

    /// Stop the game loop
    fn stop(&mut self) {
        self.running = false;
        log::info!("Game loop stopped. Total frames: {}", self.frame_count);
    }

    /// Check if the loop is running
    fn is_running(&self) -> bool {
        self.running
    }

    /// Execute one tick iteration (called by Python main loop)
    /// Returns true if should continue, false if should stop
    fn tick(&mut self) -> bool {
        if !self.running {
            return false;
        }

        let now = Instant::now();
        
        // Calculate delta time
        if let Some(last) = self.last_frame_time {
            let delta = now.duration_since(last);
            
            // Frame rate limiting (stub - actual implementation in Phase 2)
            if delta < FIXED_TIMESTEP {
                // In Phase 2, we'll sleep here to maintain fixed timestep
            }
        }
        
        self.last_frame_time = Some(now);
        self.frame_count += 1;
        
        true
    }

    /// Get current FPS setting
    fn get_fps(&self) -> u32 {
        self.fps
    }

    /// Set FPS (only 60 supported in Phase 1)
    fn set_fps(&mut self, fps: u32) -> PyResult<()> {
        if fps != 60 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Phase 1 only supports 60 FPS"
            ));
        }
        self.fps = fps;
        Ok(())
    }

    /// Get total frame count
    fn get_frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Reset frame counter
    fn reset_counter(&mut self) {
        self.frame_count = 0;
        self.last_frame_time = Some(Instant::now());
    }
}

impl Default for FixedTickLoop {
    fn default() -> Self {
        Self::new()
    }
}

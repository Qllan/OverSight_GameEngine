//! Sprite sheets and frame animations.
//!
//! A sheet cuts a texture into a grid of equal frames; an animation is a
//! list of frame indices played at some speed; a player keeps the time of
//! one running animation (one per character).

use crate::types::Rect;

/// A texture cut into a grid of `frame_w` x `frame_h` frames, numbered
/// row by row from the top-left. `margin` is the border around the grid,
/// `spacing` the gap between frames (as in Tiled tilesets).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpriteSheet {
    pub frame_w: u32,
    pub frame_h: u32,
    pub columns: u32,
    pub count: u32,
    pub margin: u32,
    pub spacing: u32,
}

impl SpriteSheet {
    /// Fit as many whole frames as the texture holds
    pub fn grid(
        texture_w: u32,
        texture_h: u32,
        frame_w: u32,
        frame_h: u32,
        margin: u32,
        spacing: u32,
    ) -> SpriteSheet {
        let fit = |size: u32, frame: u32| {
            if frame == 0 || size < 2 * margin + frame {
                0
            } else {
                (size - 2 * margin + spacing) / (frame + spacing)
            }
        };
        let columns = fit(texture_w, frame_w);
        let rows = fit(texture_h, frame_h);
        SpriteSheet {
            frame_w,
            frame_h,
            columns,
            count: columns * rows,
            margin,
            spacing,
        }
    }

    /// Texture region of a frame (an empty rect when out of range)
    pub fn frame(&self, index: u32) -> Rect {
        if index >= self.count || self.columns == 0 {
            return Rect::default();
        }
        let (col, row) = (index % self.columns, index / self.columns);
        Rect::new(
            (self.margin + col * (self.frame_w + self.spacing)) as f32,
            (self.margin + row * (self.frame_h + self.spacing)) as f32,
            self.frame_w as f32,
            self.frame_h as f32,
        )
    }
}

/// What happens after the last frame
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnimMode {
    /// Start over
    #[default]
    Loop = 0,
    /// Stop on the last frame
    Once = 1,
    /// Play backwards, then forwards again
    PingPong = 2,
}

/// Frame indices into a sheet, shown `fps` times per second
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub frames: Vec<u32>,
    pub fps: f32,
    pub mode: AnimMode,
}

impl Animation {
    pub fn new(frames: Vec<u32>, fps: f32, mode: AnimMode) -> Animation {
        Animation { frames, fps, mode }
    }

    /// Consecutive frames `first..first + count`
    pub fn range(first: u32, count: u32, fps: f32, mode: AnimMode) -> Animation {
        Animation::new((first..first + count).collect(), fps, mode)
    }

    /// Length of one pass in seconds
    pub fn duration(&self) -> f32 {
        if self.fps <= 0.0 {
            0.0
        } else {
            self.frames.len() as f32 / self.fps
        }
    }

    /// Position in `frames` at `time` seconds since the start
    pub fn index_at(&self, time: f32) -> usize {
        let n = self.frames.len();
        if n <= 1 || self.fps <= 0.0 {
            return 0;
        }
        let step = (time.max(0.0) * self.fps) as usize;
        match self.mode {
            AnimMode::Loop => step % n,
            AnimMode::Once => step.min(n - 1),
            AnimMode::PingPong => {
                // 0 1 2 3 2 1 0 1 ...: the ends are not shown twice
                let period = 2 * (n - 1);
                let s = step % period;
                if s < n {
                    s
                } else {
                    period - s
                }
            }
        }
    }

    /// Frame number at `time`
    pub fn frame_at(&self, time: f32) -> u32 {
        self.frames.get(self.index_at(time)).copied().unwrap_or(0)
    }
}

/// Playback state of one animation
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimPlayer {
    /// Seconds since the animation started
    pub time: f32,
    /// Playback speed multiplier (1 = normal)
    pub speed: f32,
    pub paused: bool,
}

impl Default for AnimPlayer {
    fn default() -> Self {
        AnimPlayer {
            time: 0.0,
            speed: 1.0,
            paused: false,
        }
    }
}

impl AnimPlayer {
    pub fn update(&mut self, dt: f32) {
        if !self.paused {
            self.time += dt * self.speed;
        }
    }

    pub fn restart(&mut self) {
        self.time = 0.0;
    }

    pub fn frame(&self, anim: &Animation) -> u32 {
        anim.frame_at(self.time)
    }

    /// A `Once` animation reached its last frame (loops never finish)
    pub fn finished(&self, anim: &Animation) -> bool {
        anim.mode == AnimMode::Once && self.time >= anim.duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_with_margin_and_spacing() {
        // 2px margin, 1px gaps, 16x16 frames: 3 columns fit in 54px
        let sheet = SpriteSheet::grid(54, 37, 16, 16, 2, 1);
        assert_eq!((sheet.columns, sheet.count), (3, 6));
        assert_eq!(sheet.frame(0), Rect::new(2.0, 2.0, 16.0, 16.0));
        assert_eq!(sheet.frame(4), Rect::new(19.0, 19.0, 16.0, 16.0));
        assert_eq!(sheet.frame(6), Rect::default());
        assert_eq!(SpriteSheet::grid(8, 8, 16, 16, 0, 0).count, 0);
    }

    #[test]
    fn modes_and_player() {
        let walk = Animation::range(4, 4, 10.0, AnimMode::Loop);
        assert_eq!(walk.frame_at(0.0), 4);
        assert_eq!(walk.frame_at(0.35), 7);
        assert_eq!(walk.frame_at(0.45), 4);

        let once = Animation::new(vec![1, 2, 3], 10.0, AnimMode::Once);
        assert_eq!(once.frame_at(5.0), 3);

        let ping = Animation::range(0, 4, 1.0, AnimMode::PingPong);
        let seq: Vec<u32> = (0..8).map(|t| ping.frame_at(t as f32 + 0.5)).collect();
        assert_eq!(seq, vec![0, 1, 2, 3, 2, 1, 0, 1]);

        let mut player = AnimPlayer::default();
        player.update(0.25);
        assert!(!player.finished(&once));
        player.speed = 2.0;
        player.update(0.05);
        assert_eq!(player.frame(&once), 3);
        assert!(player.finished(&once));
        player.paused = true;
        player.update(1.0);
        assert!((player.time - 0.35).abs() < 1e-6);
        assert!(!player.finished(&walk));
    }
}

//! Scenes: which part of the game is on (menu, level, pause) and the
//! transition between them. A scene is a game-defined number; the engine
//! keeps a stack of them (a pause menu is pushed over the level and popped
//! off again) and covers the switch with a fade or a wipe. The change
//! happens in the middle of the transition, when the screen is covered,
//! and `entered` tells the game for that one frame so it can load.

use crate::render::{DrawKind, Renderer, Shape};
use crate::types::{Color, Rect, Space};

/// Above the game, below the debug overlay
pub const TRANSITION_LAYER: i32 = crate::debug::DEBUG_LAYER - 1;

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transition {
    /// Switch at the next frame
    #[default]
    Cut = 0,
    /// Fade to the colour and back
    Fade = 1,
    /// The colour sweeps in from the left, then off to the right
    Wipe = 2,
}

impl Transition {
    pub fn from_i32(v: i32) -> Option<Transition> {
        match v {
            0 => Some(Transition::Cut),
            1 => Some(Transition::Fade),
            2 => Some(Transition::Wipe),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Go(u32),
    Push(u32),
    Pop,
}

#[derive(Debug, Clone, Copy)]
struct Running {
    change: Change,
    transition: Transition,
    /// Whole transition, both halves
    duration: f32,
    color: Color,
    t: f32,
    applied: bool,
}

#[derive(Debug, Default)]
pub struct Scenes {
    stack: Vec<u32>,
    running: Option<Running>,
    entered: u32,
    left: u32,
}

impl Scenes {
    /// The scene on top; 0 before the first one
    pub fn current(&self) -> u32 {
        self.stack.last().copied().unwrap_or(0)
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// The scene that became current this frame (0: none). After a pop it
    /// is the scene under the popped one, resumed.
    pub fn entered(&self) -> u32 {
        self.entered
    }

    /// The scene that stopped being current this frame (0: none)
    pub fn left(&self) -> u32 {
        self.left
    }

    pub fn transitioning(&self) -> bool {
        self.running.is_some()
    }

    /// Replace the current scene
    pub fn go(&mut self, scene: u32, transition: Transition, duration: f32, color: Color) -> bool {
        self.start(Change::Go(scene), transition, duration, color)
    }

    /// Put a scene over the current one
    pub fn push(
        &mut self,
        scene: u32,
        transition: Transition,
        duration: f32,
        color: Color,
    ) -> bool {
        self.start(Change::Push(scene), transition, duration, color)
    }

    /// Back to the scene under the current one; false when there is none
    pub fn pop(&mut self, transition: Transition, duration: f32, color: Color) -> bool {
        self.stack.len() > 1 && self.start(Change::Pop, transition, duration, color)
    }

    /// A change while another runs is refused: a double click on "Play"
    /// starts the level once
    fn start(
        &mut self,
        change: Change,
        transition: Transition,
        duration: f32,
        color: Color,
    ) -> bool {
        if self.running.is_some() || matches!(change, Change::Go(0) | Change::Push(0)) {
            return false;
        }
        let duration = if transition == Transition::Cut {
            0.0
        } else {
            duration.max(0.0)
        };
        self.running = Some(Running {
            change,
            transition,
            duration,
            color,
            t: 0.0,
            applied: false,
        });
        true
    }

    /// Advance by a frame's dt (the engine does it in poll)
    pub fn update(&mut self, dt: f32) {
        self.entered = 0;
        self.left = 0;
        let Some(mut r) = self.running else {
            return;
        };
        r.t += dt;
        if !r.applied && r.t >= r.duration * 0.5 {
            r.applied = true;
            self.left = self.current();
            match r.change {
                Change::Go(s) => {
                    self.stack.pop();
                    self.stack.push(s);
                }
                Change::Push(s) => self.stack.push(s),
                Change::Pop => {
                    self.stack.pop();
                }
            }
            self.entered = self.current();
        }
        self.running = if r.applied && r.t >= r.duration {
            None
        } else {
            Some(r)
        };
    }

    /// How much of the screen is covered, 0..1
    pub fn cover(&self) -> f32 {
        match self.running {
            Some(r) if r.duration > 0.0 => {
                let half = r.duration * 0.5;
                if r.applied {
                    (1.0 - (r.t - half) / half).clamp(0.0, 1.0)
                } else {
                    (r.t / half).clamp(0.0, 1.0)
                }
            }
            _ => 0.0,
        }
    }

    /// Draw the transition over the frame
    pub fn draw(&self, renderer: &mut Renderer) {
        let Some(r) = self.running else {
            return;
        };
        let k = self.cover();
        if k <= 0.0 {
            return;
        }
        let (w, h) = renderer.size();
        let (w, h) = (w as f32, h as f32);
        let (rect, color) = match r.transition {
            Transition::Cut => return,
            Transition::Fade => {
                let mut c = r.color;
                c.a = (c.a as f32 * k).round() as u8;
                (Rect::new(0.0, 0.0, w, h), c)
            }
            // Covers from the left, then uncovers towards the right
            Transition::Wipe if r.applied => (Rect::new(w * (1.0 - k), 0.0, w * k, h), r.color),
            Transition::Wipe => (Rect::new(0.0, 0.0, w * k, h), r.color),
        };
        let shape = Shape::Rect {
            rect,
            filled: true,
            thickness: 0.0,
        };
        renderer.push(
            TRANSITION_LAYER,
            0.0,
            Space::Screen,
            DrawKind::Shape { shape, color },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.125; // exact in binary

    #[test]
    fn a_cut_switches_at_the_next_update() {
        let mut s = Scenes::default();
        assert_eq!(s.current(), 0);
        assert!(s.go(1, Transition::Cut, 5.0, Color::BLACK));
        assert_eq!(s.current(), 0, "not before the update");
        s.update(DT);
        assert_eq!((s.current(), s.entered(), s.left()), (1, 1, 0));
        assert!(!s.transitioning());
        s.update(DT);
        assert_eq!(s.entered(), 0, "entered lasts one frame");
    }

    #[test]
    fn a_fade_switches_when_the_screen_is_covered() {
        let mut s = Scenes::default();
        s.go(1, Transition::Cut, 0.0, Color::BLACK);
        s.update(DT);
        assert!(s.go(2, Transition::Fade, 1.0, Color::BLACK));
        assert!(
            !s.go(3, Transition::Fade, 1.0, Color::BLACK),
            "one change at a time"
        );
        let mut switched_at = None;
        for i in 1..=10 {
            s.update(DT);
            if s.entered() == 2 {
                assert_eq!(s.left(), 1);
                assert!((s.cover() - 1.0).abs() < 1e-4, "covered at the switch");
                switched_at = Some(i);
            }
        }
        assert_eq!(switched_at, Some(4));
        assert_eq!(s.current(), 2);
        assert!(!s.transitioning() && s.cover() == 0.0);
    }

    #[test]
    fn push_and_pop_keep_the_scene_below() {
        let mut s = Scenes::default();
        s.go(1, Transition::Cut, 0.0, Color::BLACK);
        s.update(DT);
        assert!(
            !s.pop(Transition::Cut, 0.0, Color::BLACK),
            "nothing to go back to"
        );
        s.push(7, Transition::Cut, 0.0, Color::BLACK);
        s.update(DT);
        assert_eq!((s.current(), s.depth()), (7, 2));
        assert!(s.pop(Transition::Cut, 0.0, Color::BLACK));
        s.update(DT);
        assert_eq!((s.current(), s.entered(), s.left()), (1, 1, 7));
        assert!(
            !s.go(0, Transition::Cut, 0.0, Color::BLACK),
            "0 is no scene"
        );
    }
}

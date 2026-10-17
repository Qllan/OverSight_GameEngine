//! Gamepads: up to four controllers in fixed slots 0..3, with buttons
//! named by position (South is A on Xbox, Cross on PlayStation) and
//! sticks in screen directions (up is -1, like W in `axis(W, S)`).
//! The state here is filled by the platform backend (gilrs) or by
//! injected events in tests and replays.

pub const MAX_GAMEPADS: usize = 4;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadButton {
    /// A on Xbox, Cross on PlayStation, B on Nintendo
    South = 0,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    /// Back / View / Select / Share
    Back,
    /// Start / Menu / Options
    Start,
    Guide,
    LeftStick,
    RightStick,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

pub const BUTTON_COUNT: usize = 15;

impl GamepadButton {
    pub const ALL: [GamepadButton; BUTTON_COUNT] = [
        GamepadButton::South,
        GamepadButton::East,
        GamepadButton::West,
        GamepadButton::North,
        GamepadButton::LeftBumper,
        GamepadButton::RightBumper,
        GamepadButton::Back,
        GamepadButton::Start,
        GamepadButton::Guide,
        GamepadButton::LeftStick,
        GamepadButton::RightStick,
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
    ];

    pub fn from_i32(v: i32) -> Option<GamepadButton> {
        usize::try_from(v)
            .ok()
            .and_then(|i| GamepadButton::ALL.get(i).copied())
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadAxis {
    /// -1 left .. 1 right
    LeftX = 0,
    /// -1 up .. 1 down
    LeftY,
    RightX,
    RightY,
    /// 0 released .. 1 fully pressed
    LeftTrigger,
    RightTrigger,
}

pub const AXIS_COUNT: usize = 6;

impl GamepadAxis {
    pub const ALL: [GamepadAxis; AXIS_COUNT] = [
        GamepadAxis::LeftX,
        GamepadAxis::LeftY,
        GamepadAxis::RightX,
        GamepadAxis::RightY,
        GamepadAxis::LeftTrigger,
        GamepadAxis::RightTrigger,
    ];

    pub fn from_i32(v: i32) -> Option<GamepadAxis> {
        usize::try_from(v)
            .ok()
            .and_then(|i| GamepadAxis::ALL.get(i).copied())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Gamepad {
    pub connected: bool,
    pub name: String,
    down: [bool; BUTTON_COUNT],
    pressed: [bool; BUTTON_COUNT],
    released: [bool; BUTTON_COUNT],
    /// Raw axis values, before the dead zone
    raw: [f32; AXIS_COUNT],
}

#[derive(Debug, Clone)]
pub struct Gamepads {
    pub pads: [Gamepad; MAX_GAMEPADS],
    /// Stick and trigger values below this read as 0 (worn sticks drift)
    pub deadzone: f32,
}

impl Default for Gamepads {
    fn default() -> Self {
        Gamepads {
            pads: Default::default(),
            deadzone: 0.15,
        }
    }
}

impl Gamepads {
    /// Forget this frame's presses and releases
    pub fn next_frame(&mut self) {
        for p in &mut self.pads {
            p.pressed = [false; BUTTON_COUNT];
            p.released = [false; BUTTON_COUNT];
        }
    }

    pub fn connect(&mut self, pad: usize, name: &str) {
        if let Some(p) = self.pads.get_mut(pad) {
            p.connected = true;
            p.name = name.to_string();
        }
    }

    /// Unplugged: everything held is released, sticks go to the middle
    pub fn disconnect(&mut self, pad: usize) {
        if pad >= MAX_GAMEPADS {
            return;
        }
        for b in GamepadButton::ALL {
            self.set_button(pad, b, false);
        }
        let p = &mut self.pads[pad];
        p.raw = [0.0; AXIS_COUNT];
        p.connected = false;
    }

    /// First slot that is free, for a newly plugged controller
    pub fn free_slot(&self) -> Option<usize> {
        self.pads.iter().position(|p| !p.connected)
    }

    pub fn set_button(&mut self, pad: usize, button: GamepadButton, down: bool) {
        let Some(p) = self.pads.get_mut(pad) else {
            return;
        };
        let i = button as usize;
        if down && !p.down[i] {
            p.pressed[i] = true;
        }
        if !down && p.down[i] {
            p.released[i] = true;
        }
        p.down[i] = down;
    }

    pub fn set_axis(&mut self, pad: usize, axis: GamepadAxis, value: f32) {
        if let Some(p) = self.pads.get_mut(pad) {
            p.raw[axis as usize] = if value.is_finite() {
                value.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
    }

    pub fn connected(&self, pad: usize) -> bool {
        self.pads.get(pad).is_some_and(|p| p.connected)
    }

    pub fn down(&self, pad: usize, button: GamepadButton) -> bool {
        self.pads.get(pad).is_some_and(|p| p.down[button as usize])
    }

    pub fn pressed(&self, pad: usize, button: GamepadButton) -> bool {
        self.pads
            .get(pad)
            .is_some_and(|p| p.pressed[button as usize])
    }

    pub fn released(&self, pad: usize, button: GamepadButton) -> bool {
        self.pads
            .get(pad)
            .is_some_and(|p| p.released[button as usize])
    }

    /// Axis value after the dead zone. A stick's dead zone is round (it
    /// looks at both of its axes together), and the rest of the range is
    /// stretched so values still reach 1.
    pub fn axis(&self, pad: usize, axis: GamepadAxis) -> f32 {
        let Some(p) = self.pads.get(pad) else {
            return 0.0;
        };
        let dz = self.deadzone.clamp(0.0, 0.99);
        let rescale = |m: f32| ((m - dz) / (1.0 - dz)).clamp(0.0, 1.0);
        let stick = |x: usize, y: usize, own: usize| {
            let (vx, vy) = (p.raw[x], p.raw[y]);
            let len = (vx * vx + vy * vy).sqrt();
            if len <= dz {
                return 0.0;
            }
            p.raw[own] / len * rescale(len.min(1.0))
        };
        match axis {
            GamepadAxis::LeftX => stick(0, 1, 0),
            GamepadAxis::LeftY => stick(0, 1, 1),
            GamepadAxis::RightX => stick(2, 3, 2),
            GamepadAxis::RightY => stick(2, 3, 3),
            GamepadAxis::LeftTrigger | GamepadAxis::RightTrigger => {
                let v = p.raw[axis as usize].max(0.0);
                if v <= dz {
                    0.0
                } else {
                    rescale(v)
                }
            }
        }
    }
}

/// Reads real controllers through gilrs (XInput and Windows.Gaming.Input
/// on Windows, evdev on Linux) and keeps each one in its slot until it is
/// unplugged.
#[cfg(feature = "gamepad")]
pub struct Backend {
    gilrs: gilrs::Gilrs,
    slots: [Option<gilrs::GamepadId>; MAX_GAMEPADS],
}

#[cfg(feature = "gamepad")]
impl Backend {
    pub fn new(pads: &mut Gamepads) -> Result<Backend, String> {
        let gilrs = gilrs::Gilrs::new().map_err(|e| e.to_string())?;
        let mut b = Backend {
            gilrs,
            slots: [None; MAX_GAMEPADS],
        };
        let ids: Vec<_> = b.gilrs.gamepads().map(|(id, _)| id).collect();
        for id in ids {
            b.attach(id, pads);
        }
        Ok(b)
    }

    fn attach(&mut self, id: gilrs::GamepadId, pads: &mut Gamepads) -> Option<usize> {
        if let Some(slot) = self.slot(id) {
            return Some(slot);
        }
        let slot = pads.free_slot()?;
        self.slots[slot] = Some(id);
        pads.connect(slot, self.gilrs.gamepad(id).name());
        Some(slot)
    }

    fn slot(&self, id: gilrs::GamepadId) -> Option<usize> {
        self.slots.iter().position(|s| *s == Some(id))
    }

    pub fn pump(&mut self, pads: &mut Gamepads) {
        use gilrs::{Axis, Button, EventType};
        while let Some(ev) = self.gilrs.next_event() {
            if let EventType::Disconnected = ev.event {
                if let Some(slot) = self.slot(ev.id) {
                    self.slots[slot] = None;
                    pads.disconnect(slot);
                }
                continue;
            }
            let Some(slot) = self.attach(ev.id, pads) else {
                continue; // more than four controllers
            };
            let button = |b: Button| match b {
                Button::South => Some(GamepadButton::South),
                Button::East => Some(GamepadButton::East),
                Button::West => Some(GamepadButton::West),
                Button::North => Some(GamepadButton::North),
                Button::LeftTrigger => Some(GamepadButton::LeftBumper),
                Button::RightTrigger => Some(GamepadButton::RightBumper),
                Button::Select => Some(GamepadButton::Back),
                Button::Start => Some(GamepadButton::Start),
                Button::Mode => Some(GamepadButton::Guide),
                Button::LeftThumb => Some(GamepadButton::LeftStick),
                Button::RightThumb => Some(GamepadButton::RightStick),
                Button::DPadUp => Some(GamepadButton::DPadUp),
                Button::DPadDown => Some(GamepadButton::DPadDown),
                Button::DPadLeft => Some(GamepadButton::DPadLeft),
                Button::DPadRight => Some(GamepadButton::DPadRight),
                _ => None,
            };
            match ev.event {
                // Analog triggers come as buttons with a value
                EventType::ButtonChanged(Button::LeftTrigger2, v, _) => {
                    pads.set_axis(slot, GamepadAxis::LeftTrigger, v)
                }
                EventType::ButtonChanged(Button::RightTrigger2, v, _) => {
                    pads.set_axis(slot, GamepadAxis::RightTrigger, v)
                }
                EventType::ButtonPressed(b, _) => {
                    if let Some(b) = button(b) {
                        pads.set_button(slot, b, true);
                    }
                }
                EventType::ButtonReleased(b, _) => {
                    if let Some(b) = button(b) {
                        pads.set_button(slot, b, false);
                    }
                }
                // gilrs sticks point up for +Y; the engine's y points down
                EventType::AxisChanged(axis, v, _) => match axis {
                    Axis::LeftStickX => pads.set_axis(slot, GamepadAxis::LeftX, v),
                    Axis::LeftStickY => pads.set_axis(slot, GamepadAxis::LeftY, -v),
                    Axis::RightStickX => pads.set_axis(slot, GamepadAxis::RightX, v),
                    Axis::RightStickY => pads.set_axis(slot, GamepadAxis::RightY, -v),
                    Axis::LeftZ => pads.set_axis(slot, GamepadAxis::LeftTrigger, v),
                    Axis::RightZ => pads.set_axis(slot, GamepadAxis::RightTrigger, v),
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_press_hold_release() {
        let mut g = Gamepads::default();
        g.connect(1, "Pad");
        g.set_button(1, GamepadButton::South, true);
        assert!(g.pressed(1, GamepadButton::South) && g.down(1, GamepadButton::South));
        assert!(!g.down(0, GamepadButton::South), "other slots are separate");
        g.next_frame();
        assert!(!g.pressed(1, GamepadButton::South) && g.down(1, GamepadButton::South));
        g.disconnect(1);
        assert!(g.released(1, GamepadButton::South) && !g.connected(1));
        assert_eq!(g.free_slot(), Some(0));
        assert!(!g.down(9, GamepadButton::South), "bad slot is just off");
        assert_eq!(GamepadButton::from_i32(14), Some(GamepadButton::DPadRight));
        assert_eq!(GamepadButton::from_i32(15), None);
    }

    #[test]
    fn round_dead_zone_and_triggers() {
        let mut g = Gamepads::default();
        g.set_axis(0, GamepadAxis::LeftX, 0.1);
        g.set_axis(0, GamepadAxis::LeftY, 0.1);
        assert_eq!(g.axis(0, GamepadAxis::LeftX), 0.0, "drift is ignored");
        g.set_axis(0, GamepadAxis::LeftX, 1.0);
        g.set_axis(0, GamepadAxis::LeftY, 0.0);
        assert_eq!(g.axis(0, GamepadAxis::LeftX), 1.0, "full tilt reaches 1");
        // Diagonal: both axes count, the length stays at most 1
        g.set_axis(0, GamepadAxis::LeftX, -0.8);
        g.set_axis(0, GamepadAxis::LeftY, 0.8);
        let (x, y) = (g.axis(0, GamepadAxis::LeftX), g.axis(0, GamepadAxis::LeftY));
        assert!(x < 0.0 && y > 0.0 && (x * x + y * y).sqrt() <= 1.0 + 1e-5);
        g.set_axis(0, GamepadAxis::RightTrigger, 0.575);
        assert!((g.axis(0, GamepadAxis::RightTrigger) - 0.5).abs() < 1e-5);
        g.set_axis(0, GamepadAxis::RightTrigger, f32::NAN);
        assert_eq!(g.axis(0, GamepadAxis::RightTrigger), 0.0);
    }
}

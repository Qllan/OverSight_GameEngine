//! Input: keyboard and mouse state for the current frame.
//!
//! Keys are physical positions (WASD works on any keyboard layout).
//! "Pressed" and "released" are true only in the frame the change
//! happened; "down" is true while held.

use crate::types::Vec2;

/// Physical keys. Values are stable: they are part of the C API.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Unknown = 0,
    A = 1,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0 = 30,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Space = 40,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Left = 60,
    Right,
    Up,
    Down,
    LeftShift = 70,
    RightShift,
    LeftCtrl,
    RightCtrl,
    LeftAlt,
    RightAlt,
    F1 = 80,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Minus = 100,
    Equals,
    LeftBracket,
    RightBracket,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Backslash,
    Grave,
}

/// Number of key slots (largest value + 1)
pub const KEY_COUNT: usize = 111;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left = 0,
    Right = 1,
    Middle = 2,
}

#[derive(Debug, Clone)]
pub struct Input {
    down: [bool; KEY_COUNT],
    pressed: [bool; KEY_COUNT],
    released: [bool; KEY_COUNT],
    mouse_down: [bool; 3],
    mouse_pressed: [bool; 3],
    mouse_released: [bool; 3],
    /// Mouse position in canvas (screen) pixels
    pub mouse: Vec2,
    /// Wheel movement this frame (positive = away from the user)
    pub wheel: f32,
    /// Text typed this frame (for name entry etc.)
    pub text: String,
}

impl Default for Input {
    fn default() -> Self {
        Input {
            down: [false; KEY_COUNT],
            pressed: [false; KEY_COUNT],
            released: [false; KEY_COUNT],
            mouse_down: [false; 3],
            mouse_pressed: [false; 3],
            mouse_released: [false; 3],
            mouse: Vec2::default(),
            wheel: 0.0,
            text: String::new(),
        }
    }
}

impl Input {
    /// Forget this frame's presses and releases (call before new events)
    pub fn next_frame(&mut self) {
        self.pressed = [false; KEY_COUNT];
        self.released = [false; KEY_COUNT];
        self.mouse_pressed = [false; 3];
        self.mouse_released = [false; 3];
        self.wheel = 0.0;
        self.text.clear();
    }

    pub fn set_key(&mut self, key: Key, down: bool) {
        let i = key as usize;
        if key == Key::Unknown || i >= KEY_COUNT {
            return;
        }
        if down && !self.down[i] {
            self.pressed[i] = true;
        }
        if !down && self.down[i] {
            self.released[i] = true;
        }
        self.down[i] = down;
    }

    pub fn set_mouse_button(&mut self, button: MouseButton, down: bool) {
        let i = button as usize;
        if down && !self.mouse_down[i] {
            self.mouse_pressed[i] = true;
        }
        if !down && self.mouse_down[i] {
            self.mouse_released[i] = true;
        }
        self.mouse_down[i] = down;
    }

    /// Release everything (e.g. when the window loses focus)
    pub fn release_all(&mut self) {
        for i in 0..KEY_COUNT {
            if self.down[i] {
                self.released[i] = true;
                self.down[i] = false;
            }
        }
        for b in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            self.set_mouse_button(b, false);
        }
    }

    pub fn key_down(&self, key: Key) -> bool {
        self.down.get(key as usize).copied().unwrap_or(false)
    }

    pub fn key_pressed(&self, key: Key) -> bool {
        self.pressed.get(key as usize).copied().unwrap_or(false)
    }

    pub fn key_released(&self, key: Key) -> bool {
        self.released.get(key as usize).copied().unwrap_or(false)
    }

    pub fn mouse_down(&self, button: MouseButton) -> bool {
        self.mouse_down[button as usize]
    }

    pub fn mouse_pressed(&self, button: MouseButton) -> bool {
        self.mouse_pressed[button as usize]
    }

    pub fn mouse_released(&self, button: MouseButton) -> bool {
        self.mouse_released[button as usize]
    }

    /// -1, 0 or 1 from two opposite keys (e.g. A/D or Left/Right)
    pub fn axis(&self, negative: Key, positive: Key) -> f32 {
        (self.key_down(positive) as i32 - self.key_down(negative) as i32) as f32
    }
}

/// Map a key number from the C API back to a Key
pub fn key_from_u32(value: u32) -> Key {
    use Key::*;
    const ALL: &[Key] = &[
        A,
        B,
        C,
        D,
        E,
        F,
        G,
        H,
        I,
        J,
        K,
        L,
        M,
        N,
        O,
        P,
        Q,
        R,
        S,
        T,
        U,
        V,
        W,
        X,
        Y,
        Z,
        Num0,
        Num1,
        Num2,
        Num3,
        Num4,
        Num5,
        Num6,
        Num7,
        Num8,
        Num9,
        Space,
        Enter,
        Escape,
        Tab,
        Backspace,
        Delete,
        Insert,
        Home,
        End,
        PageUp,
        PageDown,
        Left,
        Right,
        Up,
        Down,
        LeftShift,
        RightShift,
        LeftCtrl,
        RightCtrl,
        LeftAlt,
        RightAlt,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        Minus,
        Equals,
        LeftBracket,
        RightBracket,
        Semicolon,
        Quote,
        Comma,
        Period,
        Slash,
        Backslash,
        Grave,
    ];
    ALL.iter()
        .copied()
        .find(|k| *k as u32 == value && *k != Unknown)
        .unwrap_or(Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_and_released_last_one_frame() {
        let mut input = Input::default();
        input.set_key(Key::Space, true);
        assert!(input.key_down(Key::Space) && input.key_pressed(Key::Space));
        input.next_frame();
        input.set_key(Key::Space, true); // key repeat is not a new press
        assert!(input.key_down(Key::Space) && !input.key_pressed(Key::Space));
        input.next_frame();
        input.set_key(Key::Space, false);
        assert!(!input.key_down(Key::Space) && input.key_released(Key::Space));
        input.next_frame();
        assert!(!input.key_released(Key::Space));
    }

    #[test]
    fn axis_and_key_numbers() {
        let mut input = Input::default();
        input.set_key(Key::D, true);
        assert_eq!(input.axis(Key::A, Key::D), 1.0);
        input.set_key(Key::A, true);
        assert_eq!(input.axis(Key::A, Key::D), 0.0);
        assert_eq!(key_from_u32(Key::F12 as u32), Key::F12);
        assert_eq!(key_from_u32(Key::Grave as u32), Key::Grave);
        assert_eq!(key_from_u32(29), Key::Unknown);
        assert_eq!(Key::Grave as usize + 1, KEY_COUNT);
    }
}

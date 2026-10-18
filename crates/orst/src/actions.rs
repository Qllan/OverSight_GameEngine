//! Input actions: the game asks for "jump" instead of Space, and the
//! player can bind any keys, mouse buttons, gamepad buttons or stick
//! directions to it. Pressed and released are worked out for the action
//! as a whole, so two keys bound to "jump" give one press. Bindings save
//! to a short text that players can read and edit:
//!
//! ```text
//! jump = key:Space pad:South
//! left = key:A key:Left pad:LeftX-
//! fire = mouse:Left pad1:RightTrigger+
//! ```
//!
//! `pad:` is any gamepad, `pad0:`..`pad3:` one slot (local multiplayer).

use crate::gamepad::{GamepadAxis, GamepadButton, MAX_GAMEPADS};
use crate::input::{key_from_u32, Input, Key, MouseButton, KEY_COUNT};

/// A stick or trigger past this counts as held
pub const AXIS_THRESHOLD: f32 = 0.5;

/// Any gamepad
pub const ANY_PAD: i32 = -1;

/// One input bound to an action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Key(Key),
    Mouse(MouseButton),
    /// Pad slot or ANY_PAD
    PadButton(i32, GamepadButton),
    /// Pad slot or ANY_PAD, and the direction: 1 or -1
    PadAxis(i32, GamepadAxis, i32),
}

const MOUSE: [MouseButton; 3] = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];

fn pads(pad: i32) -> std::ops::Range<usize> {
    match usize::try_from(pad) {
        Ok(p) if p < MAX_GAMEPADS => p..p + 1,
        Ok(_) => 0..0,
        Err(_) => 0..MAX_GAMEPADS,
    }
}

impl Binding {
    /// 0..1: 1 for a held button, how far for a stick
    pub fn value(&self, input: &Input) -> f32 {
        let g = &input.gamepads;
        match *self {
            Binding::Key(k) => input.key_down(k) as i32 as f32,
            Binding::Mouse(b) => input.mouse_down(b) as i32 as f32,
            Binding::PadButton(pad, b) => pads(pad).any(|p| g.down(p, b)) as i32 as f32,
            Binding::PadAxis(pad, a, dir) => pads(pad)
                .map(|p| (g.axis(p, a) * dir.signum() as f32).max(0.0))
                .fold(0.0, f32::max),
        }
    }

    /// As in a config file: `key:Space`, `pad:South`, `pad1:LeftX-`
    pub fn to_text(&self) -> String {
        let pad = |p: i32| {
            if p < 0 {
                "pad".to_string()
            } else {
                format!("pad{p}")
            }
        };
        match *self {
            Binding::Key(k) => format!("key:{k:?}"),
            Binding::Mouse(b) => format!("mouse:{b:?}"),
            Binding::PadButton(p, b) => format!("{}:{b:?}", pad(p)),
            Binding::PadAxis(p, a, d) => {
                format!("{}:{a:?}{}", pad(p), if d < 0 { '-' } else { '+' })
            }
        }
    }

    pub fn parse(text: &str) -> Option<Binding> {
        let (device, name) = text.split_once(':')?;
        match device {
            "key" => (1..KEY_COUNT as u32)
                .map(key_from_u32)
                .find(|k| *k != Key::Unknown && format!("{k:?}") == name)
                .map(Binding::Key),
            "mouse" => MOUSE
                .into_iter()
                .find(|b| format!("{b:?}") == name)
                .map(Binding::Mouse),
            _ => {
                let slot = device.strip_prefix("pad")?;
                let pad = match slot {
                    "" => ANY_PAD,
                    s => s
                        .parse::<i32>()
                        .ok()
                        .filter(|p| (0..MAX_GAMEPADS as i32).contains(p))?,
                };
                if let Some(b) = GamepadButton::ALL
                    .into_iter()
                    .find(|b| format!("{b:?}") == name)
                {
                    return Some(Binding::PadButton(pad, b));
                }
                let (axis, dir) = match name.as_bytes().last()? {
                    b'+' => (&name[..name.len() - 1], 1),
                    b'-' => (&name[..name.len() - 1], -1),
                    _ => return None,
                };
                GamepadAxis::ALL
                    .into_iter()
                    .find(|a| format!("{a:?}") == axis)
                    .map(|a| Binding::PadAxis(pad, a, dir))
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Action {
    name: String,
    bindings: Vec<Binding>,
    value: f32,
    down: bool,
    pressed: bool,
    released: bool,
}

#[derive(Debug, Default)]
pub struct Actions {
    actions: Vec<Action>,
    /// Stick values of the last frame, to catch a stick being pushed while
    /// waiting for a new binding
    last_axes: [[f32; GamepadAxis::ALL.len()]; MAX_GAMEPADS],
    captured: Option<Binding>,
}

impl Actions {
    /// How many actions there are; their ids are 1 to this
    pub fn count(&self) -> u32 {
        self.actions.len() as u32
    }

    /// The id of an action (from 1), made on first use
    pub fn action(&mut self, name: &str) -> u32 {
        if let Some(id) = self.find(name) {
            return id;
        }
        self.actions.push(Action {
            name: name.to_string(),
            bindings: Vec::new(),
            value: 0.0,
            down: false,
            pressed: false,
            released: false,
        });
        self.actions.len() as u32
    }

    pub fn find(&self, name: &str) -> Option<u32> {
        self.actions
            .iter()
            .position(|a| a.name == name)
            .map(|i| i as u32 + 1)
    }

    fn get(&self, id: u32) -> Option<&Action> {
        self.actions.get((id as usize).checked_sub(1)?)
    }

    fn get_mut(&mut self, id: u32) -> Option<&mut Action> {
        self.actions.get_mut((id as usize).checked_sub(1)?)
    }

    pub fn name(&self, id: u32) -> Option<&str> {
        self.get(id).map(|a| a.name.as_str())
    }

    /// Add a binding; one already there is not added twice
    pub fn bind(&mut self, id: u32, binding: Binding) -> bool {
        let Some(a) = self.get_mut(id) else {
            return false;
        };
        if !a.bindings.contains(&binding) {
            a.bindings.push(binding);
        }
        true
    }

    /// Replace binding `index`, or add it when index is the count
    pub fn set_binding(&mut self, id: u32, index: usize, binding: Binding) -> bool {
        match self.get_mut(id) {
            Some(a) if index < a.bindings.len() => {
                a.bindings[index] = binding;
                true
            }
            Some(a) if index == a.bindings.len() => {
                a.bindings.push(binding);
                true
            }
            _ => false,
        }
    }

    pub fn remove_binding(&mut self, id: u32, index: usize) -> bool {
        match self.get_mut(id) {
            Some(a) if index < a.bindings.len() => {
                a.bindings.remove(index);
                true
            }
            _ => false,
        }
    }

    pub fn clear_bindings(&mut self, id: u32) {
        if let Some(a) = self.get_mut(id) {
            a.bindings.clear();
        }
    }

    pub fn bindings(&self, id: u32) -> &[Binding] {
        self.get(id).map_or(&[], |a| &a.bindings)
    }

    pub fn down(&self, id: u32) -> bool {
        self.get(id).is_some_and(|a| a.down)
    }

    pub fn pressed(&self, id: u32) -> bool {
        self.get(id).is_some_and(|a| a.pressed)
    }

    pub fn released(&self, id: u32) -> bool {
        self.get(id).is_some_and(|a| a.released)
    }

    /// 0..1, the strongest of its bindings
    pub fn value(&self, id: u32) -> f32 {
        self.get(id).map_or(0.0, |a| a.value)
    }

    /// The first input pressed this frame, for "press a key" in a
    /// rebinding menu: keys, mouse buttons, pad buttons and sticks pushed
    /// past the threshold. The pad slot is the one used.
    pub fn captured(&self) -> Option<Binding> {
        self.captured
    }

    /// Work out every action from this frame's input (the engine does it
    /// in poll)
    pub fn update(&mut self, input: &Input) {
        for a in &mut self.actions {
            let value = a
                .bindings
                .iter()
                .map(|b| b.value(input))
                .fold(0.0, f32::max);
            let down = a.bindings.iter().any(|b| match b {
                Binding::PadAxis(..) => b.value(input) >= AXIS_THRESHOLD,
                _ => b.value(input) > 0.0,
            });
            a.pressed = down && !a.down;
            a.released = !down && a.down;
            a.down = down;
            a.value = value;
        }
        self.captured = Self::capture(input, &self.last_axes);
        for (p, axes) in self.last_axes.iter_mut().enumerate() {
            for (i, v) in axes.iter_mut().enumerate() {
                *v = input.gamepads.axis(p, GamepadAxis::ALL[i]);
            }
        }
    }

    fn capture(
        input: &Input,
        last: &[[f32; GamepadAxis::ALL.len()]; MAX_GAMEPADS],
    ) -> Option<Binding> {
        let key = (1..KEY_COUNT as u32)
            .map(key_from_u32)
            .find(|k| input.key_pressed(*k));
        if let Some(k) = key {
            return Some(Binding::Key(k));
        }
        if let Some(b) = MOUSE.into_iter().find(|b| input.mouse_pressed(*b)) {
            return Some(Binding::Mouse(b));
        }
        let g = &input.gamepads;
        for (p, last) in last.iter().enumerate() {
            if let Some(b) = GamepadButton::ALL.into_iter().find(|b| g.pressed(p, *b)) {
                return Some(Binding::PadButton(p as i32, b));
            }
            for (i, a) in GamepadAxis::ALL.into_iter().enumerate() {
                let (now, before) = (g.axis(p, a), last[i]);
                if now.abs() >= AXIS_THRESHOLD && before.abs() < AXIS_THRESHOLD {
                    return Some(Binding::PadAxis(
                        p as i32,
                        a,
                        if now < 0.0 { -1 } else { 1 },
                    ));
                }
            }
        }
        None
    }

    /// Every action with its bindings, one per line
    pub fn save(&self) -> String {
        let mut out = String::new();
        for a in &self.actions {
            let b: Vec<String> = a.bindings.iter().map(Binding::to_text).collect();
            out.push_str(&format!("{} = {}\n", a.name, b.join(" ")).replace(" \n", "\n"));
        }
        out
    }

    /// Bindings from `save` text. Actions in the text get exactly its
    /// bindings (new ones are made); the others keep theirs, so actions a
    /// game adds later keep their defaults. On an error nothing changes.
    pub fn load(&mut self, text: &str) -> Result<(), String> {
        let mut parsed = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (name, rest) = line
                .split_once('=')
                .ok_or_else(|| format!("line {}: expected `action = bindings`", n + 1))?;
            let name = name.trim();
            if name.is_empty() {
                return Err(format!("line {}: no action name", n + 1));
            }
            let mut bindings = Vec::new();
            for word in rest.split_whitespace() {
                let b = Binding::parse(word)
                    .ok_or_else(|| format!("line {}: unknown input `{word}`", n + 1))?;
                if !bindings.contains(&b) {
                    bindings.push(b);
                }
            }
            parsed.push((name.to_string(), bindings));
        }
        for (name, bindings) in parsed {
            let id = self.action(&name);
            if let Some(a) = self.get_mut(id) {
                a.bindings = bindings;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(actions: &mut Actions, input: &mut Input, change: impl FnOnce(&mut Input)) {
        input.next_frame();
        change(input);
        actions.update(input);
    }

    #[test]
    fn two_keys_on_one_action_press_once() {
        let (mut a, mut input) = (Actions::default(), Input::default());
        let jump = a.action("jump");
        assert_eq!(a.action("jump"), jump, "same name, same id");
        a.bind(jump, Binding::Key(Key::Space));
        a.bind(jump, Binding::Key(Key::W));
        frame(&mut a, &mut input, |i| i.set_key(Key::Space, true));
        assert!(a.pressed(jump) && a.down(jump) && a.value(jump) == 1.0);
        frame(&mut a, &mut input, |i| i.set_key(Key::W, true));
        assert!(!a.pressed(jump) && a.down(jump), "already held");
        frame(&mut a, &mut input, |i| i.set_key(Key::Space, false));
        assert!(a.down(jump) && !a.released(jump));
        frame(&mut a, &mut input, |i| i.set_key(Key::W, false));
        assert!(a.released(jump) && !a.down(jump));
    }

    #[test]
    fn sticks_count_past_the_threshold_and_give_a_value() {
        let (mut a, mut input) = (Actions::default(), Input::default());
        let left = a.action("left");
        a.bind(left, Binding::PadAxis(ANY_PAD, GamepadAxis::LeftX, -1));
        input.gamepads.connect(2, "pad");
        frame(&mut a, &mut input, |i| {
            i.gamepads.set_axis(2, GamepadAxis::LeftX, -0.3)
        });
        assert!(!a.down(left) && a.value(left) > 0.0 && a.value(left) < 0.5);
        frame(&mut a, &mut input, |i| {
            i.gamepads.set_axis(2, GamepadAxis::LeftX, -1.0)
        });
        assert!(a.pressed(left) && a.value(left) == 1.0);
        frame(&mut a, &mut input, |i| {
            i.gamepads.set_axis(2, GamepadAxis::LeftX, 1.0)
        });
        assert!(a.released(left) && a.value(left) == 0.0, "the other way");
        // Pad slot 0 only does not see pad 2
        a.set_binding(left, 0, Binding::PadAxis(0, GamepadAxis::LeftX, 1));
        frame(&mut a, &mut input, |_| {});
        assert!(!a.down(left));
    }

    #[test]
    fn capture_takes_the_first_new_input() {
        let (mut a, mut input) = (Actions::default(), Input::default());
        input.gamepads.connect(1, "pad");
        frame(&mut a, &mut input, |i| {
            i.gamepads.set_axis(1, GamepadAxis::RightTrigger, 1.0)
        });
        assert_eq!(
            a.captured(),
            Some(Binding::PadAxis(1, GamepadAxis::RightTrigger, 1))
        );
        frame(&mut a, &mut input, |_| {});
        assert_eq!(a.captured(), None, "a held trigger is not new");
        frame(&mut a, &mut input, |i| {
            i.set_mouse_button(MouseButton::Right, true)
        });
        assert_eq!(a.captured(), Some(Binding::Mouse(MouseButton::Right)));
    }

    #[test]
    fn bindings_round_trip_through_text() {
        let mut a = Actions::default();
        let jump = a.action("jump");
        a.bind(jump, Binding::Key(Key::Space));
        a.bind(jump, Binding::PadButton(ANY_PAD, GamepadButton::South));
        let fire = a.action("fire");
        a.bind(fire, Binding::Mouse(MouseButton::Left));
        a.bind(fire, Binding::PadAxis(1, GamepadAxis::RightTrigger, 1));
        a.action("unbound");
        let text = a.save();
        assert_eq!(
            text,
            "jump = key:Space pad:South\nfire = mouse:Left pad1:RightTrigger+\nunbound =\n"
        );

        let mut b = Actions::default();
        let dash = b.action("dash");
        b.bind(dash, Binding::Key(Key::LeftShift));
        b.load(&text).unwrap();
        assert_eq!(b.save().lines().count(), 4, "dash keeps its default");
        assert_eq!(b.bindings(b.find("fire").unwrap()), a.bindings(fire));

        let before = b.save();
        let err = b.load("jump = key:Space\nfire = key:Nope\n").unwrap_err();
        assert!(err.contains("line 2") && err.contains("key:Nope"), "{err}");
        assert_eq!(b.save(), before, "an error changes nothing");
        b.load("# comment\n\njump = pad0:LeftY-  # up\n").unwrap();
        assert_eq!(
            b.bindings(b.find("jump").unwrap()),
            &[Binding::PadAxis(0, GamepadAxis::LeftY, -1)]
        );
    }
}

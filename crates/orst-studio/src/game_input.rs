//! Keyboard and mouse from the Game window to the game while it plays:
//! egui's events become the engine's Input, as a window would make them

use crate::egui;
use orst::input::{Input, Key, MouseButton};
use orst::Vec2;

/// Feed this frame's events to `input`. Keys go while no text field has
/// the keyboard; the mouse while it is over `rect`, the Game view, in
/// canvas pixels. A press stays until a tick has seen it: Play clears
/// presses after each tick.
pub fn feed(ui: &egui::Ui, rect: egui::Rect, input: &mut Input) {
    let keyboard = !ui.ctx().egui_wants_keyboard_input();
    let ppp = ui.ctx().pixels_per_point();
    ui.input(|i| {
        let over = |p: egui::Pos2| rect.contains(p);
        if let Some(p) = i.pointer.latest_pos().filter(|&p| over(p)) {
            input.mouse = Vec2::new((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp);
        }
        for event in &i.events {
            match event {
                egui::Event::Key {
                    key,
                    pressed,
                    repeat: false,
                    ..
                } if keyboard || !pressed => {
                    if let Some(k) = key_of(*key) {
                        input.set_key(k, *pressed);
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } if over(*pos) || !pressed => {
                    let b = match button {
                        egui::PointerButton::Primary => MouseButton::Left,
                        egui::PointerButton::Secondary => MouseButton::Right,
                        egui::PointerButton::Middle => MouseButton::Middle,
                        _ => continue,
                    };
                    input.set_mouse_button(b, *pressed);
                }
                egui::Event::MouseWheel { unit, delta, .. }
                    if i.pointer.latest_pos().is_some_and(over) =>
                {
                    input.wheel += match unit {
                        egui::MouseWheelUnit::Point => delta.y / 50.0,
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * 10.0,
                    };
                }
                egui::Event::Text(text) if keyboard => input.text.push_str(text),
                _ => {}
            }
        }
        // egui reports modifiers as a state, not as keys
        let m = i.modifiers;
        for (key, down) in [
            (Key::LeftShift, m.shift),
            (Key::LeftCtrl, m.ctrl),
            (Key::LeftAlt, m.alt),
        ] {
            if input.key_down(key) != (down && keyboard) {
                input.set_key(key, down && keyboard);
            }
        }
    });
}

/// The engine's key for an egui key
pub fn key_of(key: egui::Key) -> Option<Key> {
    use egui::Key as E;
    let letters = [
        E::A,
        E::B,
        E::C,
        E::D,
        E::E,
        E::F,
        E::G,
        E::H,
        E::I,
        E::J,
        E::K,
        E::L,
        E::M,
        E::N,
        E::O,
        E::P,
        E::Q,
        E::R,
        E::S,
        E::T,
        E::U,
        E::V,
        E::W,
        E::X,
        E::Y,
        E::Z,
    ];
    if let Some(i) = letters.iter().position(|&k| k == key) {
        return Some(orst::input::key_from_u32(Key::A as u32 + i as u32));
    }
    let digits = [
        E::Num0,
        E::Num1,
        E::Num2,
        E::Num3,
        E::Num4,
        E::Num5,
        E::Num6,
        E::Num7,
        E::Num8,
        E::Num9,
    ];
    if let Some(i) = digits.iter().position(|&k| k == key) {
        return Some(orst::input::key_from_u32(Key::Num0 as u32 + i as u32));
    }
    let f = [
        E::F1,
        E::F2,
        E::F3,
        E::F4,
        E::F5,
        E::F6,
        E::F7,
        E::F8,
        E::F9,
        E::F10,
        E::F11,
        E::F12,
    ];
    if let Some(i) = f.iter().position(|&k| k == key) {
        return Some(orst::input::key_from_u32(Key::F1 as u32 + i as u32));
    }
    Some(match key {
        E::Space => Key::Space,
        E::Enter => Key::Enter,
        E::Escape => Key::Escape,
        E::Tab => Key::Tab,
        E::Backspace => Key::Backspace,
        E::Delete => Key::Delete,
        E::Insert => Key::Insert,
        E::Home => Key::Home,
        E::End => Key::End,
        E::PageUp => Key::PageUp,
        E::PageDown => Key::PageDown,
        E::ArrowLeft => Key::Left,
        E::ArrowRight => Key::Right,
        E::ArrowUp => Key::Up,
        E::ArrowDown => Key::Down,
        E::Minus => Key::Minus,
        E::Equals => Key::Equals,
        E::OpenBracket => Key::LeftBracket,
        E::CloseBracket => Key::RightBracket,
        E::Semicolon => Key::Semicolon,
        E::Quote => Key::Quote,
        E::Comma => Key::Comma,
        E::Period => Key::Period,
        E::Slash => Key::Slash,
        E::Backslash => Key::Backslash,
        E::Backtick => Key::Grave,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_the_mouse_reach_the_game() {
        assert_eq!(key_of(egui::Key::D), Some(Key::D));
        assert_eq!(key_of(egui::Key::Num7), Some(Key::Num7));
        assert_eq!(key_of(egui::Key::F12), Some(Key::F12));
        assert_eq!(key_of(egui::Key::ArrowUp), Some(Key::Up));

        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(200.0, 100.0));
        let mut input = Input::default();
        let key = |key, pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let run = |events: Vec<egui::Event>, input: &mut Input| {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| feed(ui, rect, input));
            out.textures_delta.clear();
        };
        run(
            vec![
                key(egui::Key::Space, true),
                egui::Event::PointerMoved(egui::pos2(110.0, 60.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(110.0, 60.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            &mut input,
        );
        assert!(input.key_pressed(Key::Space) && input.key_down(Key::Space));
        assert!(input.mouse_pressed(MouseButton::Left));
        assert_eq!(input.mouse, Vec2::new(10.0, 10.0));

        input.next_frame();
        run(
            vec![
                key(egui::Key::Space, false),
                egui::Event::PointerButton {
                    pos: egui::pos2(500.0, 400.0),
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            &mut input,
        );
        assert!(input.key_released(Key::Space) && !input.key_down(Key::Space));
        assert!(
            !input.mouse_down(MouseButton::Right),
            "a click outside the view"
        );
        assert!(input.mouse_down(MouseButton::Left), "held");
    }
}

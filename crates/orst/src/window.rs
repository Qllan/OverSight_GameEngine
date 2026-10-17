//! Window: a winit window whose events are pumped by the game's own loop
//! (`while engine.poll() { ... }`). The GPU renderer draws into it itself;
//! without one, a softbuffer surface shows the software canvas scaled to
//! the window with nearest-neighbour filtering and black bars.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use tiny_skia::Pixmap;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{Fullscreen, Window as WinitWindow, WindowId};

use crate::input::{Input, Key, MouseButton};
use crate::types::Vec2;

/// How to open the window
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    /// Canvas (logical resolution) the game draws into
    pub width: u32,
    pub height: u32,
    /// Initial window size as a multiple of the canvas (pixel-art games
    /// often use a small canvas and a scale of 3 or 4)
    pub scale: f32,
    pub resizable: bool,
    pub fullscreen: bool,
    /// Scale only by whole numbers (crisp pixels, wider bars)
    pub integer_scale: bool,
    /// Frame limit; 0 = unlimited
    pub target_fps: u32,
    /// Draw with the CPU even when a graphics card is there
    pub software: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        WindowConfig {
            title: "ORST".to_string(),
            width: 960,
            height: 540,
            scale: 1.0,
            resizable: true,
            fullscreen: false,
            integer_scale: false,
            target_fps: 60,
            software: false,
        }
    }
}

/// Where the canvas sits inside the window, in physical pixels
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
}

impl Viewport {
    pub fn fit(window: (u32, u32), canvas: (u32, u32), integer: bool) -> Viewport {
        let (ww, wh) = (window.0 as f32, window.1 as f32);
        let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
        let mut scale = (ww / cw).min(wh / ch);
        if integer && scale >= 1.0 {
            scale = scale.floor();
        }
        let scale = scale.max(1e-3);
        Viewport {
            x: ((ww - cw * scale) / 2.0).floor(),
            y: ((wh - ch * scale) / 2.0).floor(),
            scale,
        }
    }

    pub fn to_canvas(&self, p: Vec2) -> Vec2 {
        Vec2::new((p.x - self.x) / self.scale, (p.y - self.y) / self.scale)
    }
}

type SoftSurface = softbuffer::Surface<Arc<WinitWindow>, Arc<WinitWindow>>;

struct Graphics {
    window: Arc<WinitWindow>,
    /// Made on the first software present, so a GPU renderer has the
    /// window to itself
    surface: Option<SoftSurface>,
}

pub struct Window {
    event_loop: Option<EventLoop<()>>,
    config: WindowConfig,
    graphics: Option<Graphics>,
    error: Option<String>,
    close_requested: bool,
    cursor: PhysicalPosition<f64>,
    /// Column lookup table for the current window size
    columns: Vec<u32>,
}

/// Borrowed state the event handler writes into
struct Pump<'a> {
    window: &'a mut Window,
    input: &'a mut Input,
    canvas: (u32, u32),
}

impl Window {
    pub fn open(config: WindowConfig) -> Result<Window, String> {
        let event_loop = EventLoop::new().map_err(|e| format!("cannot start windowing: {}", e))?;
        let mut window = Window {
            event_loop: Some(event_loop),
            config,
            graphics: None,
            error: None,
            close_requested: false,
            cursor: PhysicalPosition::new(0.0, 0.0),
            columns: Vec::new(),
        };
        // The window is created when the platform says the app has resumed,
        // which happens during the first pumps
        let mut input = Input::default();
        let canvas = (window.config.width, window.config.height);
        for _ in 0..100 {
            window.pump(&mut input, canvas);
            if let Some(error) = window.error.take() {
                return Err(error);
            }
            if window.graphics.is_some() {
                return Ok(window);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err("the window did not open".to_string())
    }

    /// Handle pending events; false once the window was closed
    pub fn pump(&mut self, input: &mut Input, canvas: (u32, u32)) -> bool {
        // The loop is moved out while the handler borrows the rest
        let Some(mut loop_) = self.event_loop.take() else {
            return false;
        };
        let status = {
            let mut handler = Pump {
                window: self,
                input,
                canvas,
            };
            loop_.pump_app_events(Some(Duration::ZERO), &mut handler)
        };
        self.event_loop = Some(loop_);
        !self.close_requested && matches!(status, PumpStatus::Continue)
    }

    pub fn set_title(&mut self, title: &str) {
        self.config.title = title.to_string();
        if let Some(g) = &self.graphics {
            g.window.set_title(title);
        }
    }

    pub fn set_fullscreen(&mut self, on: bool) {
        self.config.fullscreen = on;
        if let Some(g) = &self.graphics {
            g.window
                .set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
        }
    }

    pub fn fullscreen(&self) -> bool {
        self.config.fullscreen
    }

    pub fn target_fps(&self) -> u32 {
        self.config.target_fps
    }

    pub fn set_target_fps(&mut self, fps: u32) {
        self.config.target_fps = fps;
    }

    /// The winit window, for the GPU renderer's surface
    pub fn handle(&self) -> Option<Arc<WinitWindow>> {
        self.graphics.as_ref().map(|g| g.window.clone())
    }

    /// Ask for the next frame to be shown (after a GPU present)
    pub fn pre_present(&self) {
        if let Some(g) = &self.graphics {
            g.window.pre_present_notify();
        }
    }

    /// Window size in physical pixels
    pub fn size(&self) -> (u32, u32) {
        self.graphics.as_ref().map_or((0, 0), |g| {
            let s = g.window.inner_size();
            (s.width, s.height)
        })
    }

    pub fn viewport(&self, canvas: (u32, u32)) -> Viewport {
        Viewport::fit(self.size(), canvas, self.config.integer_scale)
    }

    /// Show the canvas in the window
    pub fn present(&mut self, canvas: &Pixmap) -> Result<(), String> {
        let (ww, wh) = self.size();
        let (Some(nw), Some(nh)) = (NonZeroU32::new(ww), NonZeroU32::new(wh)) else {
            return Ok(()); // minimized
        };
        let (cw, ch) = (canvas.width(), canvas.height());
        let view = Viewport::fit((ww, wh), (cw, ch), self.config.integer_scale);
        // Source column for every window column (u32::MAX = black bar)
        if self.columns.len() != ww as usize {
            self.columns.resize(ww as usize, 0);
        }
        for (x, col) in self.columns.iter_mut().enumerate() {
            let cx = ((x as f32 + 0.5 - view.x) / view.scale).floor();
            *col = if cx >= 0.0 && (cx as u32) < cw {
                cx as u32
            } else {
                u32::MAX
            };
        }
        let Some(g) = self.graphics.as_mut() else {
            return Ok(());
        };
        if g.surface.is_none() {
            let context = softbuffer::Context::new(g.window.clone()).map_err(|e| e.to_string())?;
            let surface =
                softbuffer::Surface::new(&context, g.window.clone()).map_err(|e| e.to_string())?;
            g.surface = Some(surface);
        }
        let surface = g.surface.as_mut().expect("made above");
        surface.resize(nw, nh).map_err(|e| e.to_string())?;
        let mut buffer = surface.buffer_mut().map_err(|e| e.to_string())?;
        let src = canvas.data();
        for (y, row) in buffer.chunks_exact_mut(ww as usize).enumerate() {
            let cy = ((y as f32 + 0.5 - view.y) / view.scale).floor();
            if cy < 0.0 || cy as u32 >= ch {
                row.fill(0);
                continue;
            }
            let line = cy as usize * cw as usize * 4;
            for (out, &cx) in row.iter_mut().zip(&self.columns) {
                *out = if cx == u32::MAX {
                    0
                } else {
                    // Premultiplied RGBA over black is just its RGB
                    let i = line + cx as usize * 4;
                    (src[i] as u32) << 16 | (src[i + 1] as u32) << 8 | src[i + 2] as u32
                };
            }
        }
        g.window.pre_present_notify();
        buffer.present().map_err(|e| e.to_string())
    }
}

impl ApplicationHandler for Pump<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.graphics.is_some() {
            return;
        }
        let c = &self.window.config;
        let mut attributes = WinitWindow::default_attributes()
            .with_title(c.title.clone())
            .with_inner_size(LogicalSize::new(
                (c.width as f32 * c.scale).max(1.0) as f64,
                (c.height as f32 * c.scale).max(1.0) as f64,
            ))
            .with_resizable(c.resizable);
        if c.fullscreen {
            attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        // Drag and drop needs OleInitialize, which panics on a thread that
        // is already in the multithreaded COM apartment (the main thread of
        // a .NET program). The engine has no use for dropped files.
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attributes = attributes.with_drag_and_drop(false);
        }
        let result = (|| -> Result<Graphics, String> {
            let window = Arc::new(
                event_loop
                    .create_window(attributes)
                    .map_err(|e| format!("cannot open a window: {}", e))?,
            );
            Ok(Graphics {
                window,
                surface: None,
            })
        })();
        match result {
            Ok(g) => self.window.graphics = Some(g),
            Err(e) => self.window.error = Some(e),
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.window.close_requested = true,
            WindowEvent::Focused(false) => self.input.release_all(),
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.input.set_key(map_key(code), down);
                }
                if down {
                    if let Some(text) = &event.text {
                        self.input
                            .text
                            .extend(text.chars().filter(|c| !c.is_control()));
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.window.cursor = position;
                self.update_mouse();
            }
            WindowEvent::Resized(_) => self.update_mouse(),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };
                self.input
                    .set_mouse_button(button, state == ElementState::Pressed);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.input.wheel += match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
            }
            _ => {}
        }
    }
}

impl Pump<'_> {
    fn update_mouse(&mut self) {
        let view = self.window.viewport(self.canvas);
        let p = self.window.cursor;
        self.input.mouse = view.to_canvas(Vec2::new(p.x as f32, p.y as f32));
    }
}

fn map_key(code: KeyCode) -> Key {
    use KeyCode as C;
    match code {
        C::KeyA => Key::A,
        C::KeyB => Key::B,
        C::KeyC => Key::C,
        C::KeyD => Key::D,
        C::KeyE => Key::E,
        C::KeyF => Key::F,
        C::KeyG => Key::G,
        C::KeyH => Key::H,
        C::KeyI => Key::I,
        C::KeyJ => Key::J,
        C::KeyK => Key::K,
        C::KeyL => Key::L,
        C::KeyM => Key::M,
        C::KeyN => Key::N,
        C::KeyO => Key::O,
        C::KeyP => Key::P,
        C::KeyQ => Key::Q,
        C::KeyR => Key::R,
        C::KeyS => Key::S,
        C::KeyT => Key::T,
        C::KeyU => Key::U,
        C::KeyV => Key::V,
        C::KeyW => Key::W,
        C::KeyX => Key::X,
        C::KeyY => Key::Y,
        C::KeyZ => Key::Z,
        C::Digit0 | C::Numpad0 => Key::Num0,
        C::Digit1 | C::Numpad1 => Key::Num1,
        C::Digit2 | C::Numpad2 => Key::Num2,
        C::Digit3 | C::Numpad3 => Key::Num3,
        C::Digit4 | C::Numpad4 => Key::Num4,
        C::Digit5 | C::Numpad5 => Key::Num5,
        C::Digit6 | C::Numpad6 => Key::Num6,
        C::Digit7 | C::Numpad7 => Key::Num7,
        C::Digit8 | C::Numpad8 => Key::Num8,
        C::Digit9 | C::Numpad9 => Key::Num9,
        C::Space => Key::Space,
        C::Enter | C::NumpadEnter => Key::Enter,
        C::Escape => Key::Escape,
        C::Tab => Key::Tab,
        C::Backspace => Key::Backspace,
        C::Delete => Key::Delete,
        C::Insert => Key::Insert,
        C::Home => Key::Home,
        C::End => Key::End,
        C::PageUp => Key::PageUp,
        C::PageDown => Key::PageDown,
        C::ArrowLeft => Key::Left,
        C::ArrowRight => Key::Right,
        C::ArrowUp => Key::Up,
        C::ArrowDown => Key::Down,
        C::ShiftLeft => Key::LeftShift,
        C::ShiftRight => Key::RightShift,
        C::ControlLeft => Key::LeftCtrl,
        C::ControlRight => Key::RightCtrl,
        C::AltLeft => Key::LeftAlt,
        C::AltRight => Key::RightAlt,
        C::F1 => Key::F1,
        C::F2 => Key::F2,
        C::F3 => Key::F3,
        C::F4 => Key::F4,
        C::F5 => Key::F5,
        C::F6 => Key::F6,
        C::F7 => Key::F7,
        C::F8 => Key::F8,
        C::F9 => Key::F9,
        C::F10 => Key::F10,
        C::F11 => Key::F11,
        C::F12 => Key::F12,
        C::Minus | C::NumpadSubtract => Key::Minus,
        C::Equal | C::NumpadAdd => Key::Equals,
        C::BracketLeft => Key::LeftBracket,
        C::BracketRight => Key::RightBracket,
        C::Semicolon => Key::Semicolon,
        C::Quote => Key::Quote,
        C::Comma => Key::Comma,
        C::Period => Key::Period,
        C::Slash => Key::Slash,
        C::Backslash => Key::Backslash,
        C::Backquote => Key::Grave,
        _ => Key::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_letterboxes_and_maps_the_mouse() {
        // 320x180 canvas in a 1000x600 window: scale 3.125 (height 562.5)
        let v = Viewport::fit((1000, 600), (320, 180), false);
        assert!((v.scale - 3.125).abs() < 1e-4);
        assert_eq!(v.x, 0.0);
        let v = Viewport::fit((1000, 600), (320, 180), true);
        assert_eq!(v.scale, 3.0);
        assert_eq!((v.x, v.y), (20.0, 30.0));
        let p = v.to_canvas(Vec2::new(20.0 + 30.0, 30.0 + 60.0));
        assert_eq!(p, Vec2::new(10.0, 20.0));
    }
}

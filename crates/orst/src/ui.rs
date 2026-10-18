//! The interface on the screen: boxes (UiRect) that hold text, images and
//! buttons, laid out over the screen or inside one another. Engine::step
//! works out the buttons from the mouse, draw_entities draws it all over
//! the world. Positions are screen pixels of the canvas.

use crate::components::{
    read, write, UiButton, UiImage, UiRect, UiText, ALIGN_CENTER, ALIGN_END, UI_BUTTON, UI_IMAGE,
    UI_RECT, UI_TEXT,
};
use crate::ecs::Entity;
use crate::input::MouseButton;
use crate::render::{DrawKind, Shape};
use crate::types::{Color, Rect, Space, Sprite, Vec2};
use crate::Engine;

/// The draw layer of the interface: over every layer a game uses, under
/// scene transitions and the debug overlay
pub const UI_LAYER: i32 = 1 << 30;

/// Boxes inside boxes deeper than this count as on the screen (a parent
/// loop cannot hang the layout)
const MAX_DEPTH: usize = 32;

/// One box, laid out
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub entity: Entity,
    pub rect: Rect,
}

impl Engine {
    /// Where the UiRect of `e` is on the screen; None when it has none or
    /// it or a box it is in is hidden
    pub fn ui_rect(&self, e: Entity) -> Option<Rect> {
        self.ui_chain(e).map(|(rect, _)| rect)
    }

    /// The box and its order key: (layer, entity) from the screen down
    fn ui_chain(&self, e: Entity) -> Option<(Rect, Vec<(i32, Entity)>)> {
        let mut chain = Vec::new();
        let mut at = e;
        while let Some(r) = read::<UiRect>(&self.world, at, UI_RECT) {
            if r.hidden != 0 {
                return None;
            }
            chain.push((at, r));
            let parent = r.parent;
            if parent == 0 || chain.len() >= MAX_DEPTH || chain.iter().any(|c| c.0 == parent) {
                break;
            }
            if !self.world.has(parent, UI_RECT) {
                break;
            }
            at = parent;
        }
        if chain.is_empty() {
            return None;
        }
        let (w, h) = self.size();
        let mut rect = Rect::new(0.0, 0.0, w as f32, h as f32);
        let mut key = Vec::with_capacity(chain.len());
        for (ent, r) in chain.iter().rev() {
            rect = place(rect, r);
            key.push((r.layer, *ent));
        }
        Some((rect, key))
    }

    /// Every visible box in drawing order, lowest first
    pub fn ui_layout(&self) -> Vec<Placed> {
        let mut boxes: Vec<(Vec<(i32, Entity)>, Placed)> = self
            .world
            .query(&[UI_RECT])
            .into_iter()
            .filter_map(|entity| {
                let (rect, key) = self.ui_chain(entity)?;
                Some((key, Placed { entity, rect }))
            })
            .collect();
        boxes.sort_by(|a, b| a.0.cmp(&b.0));
        boxes.into_iter().map(|(_, p)| p).collect()
    }

    /// The topmost box at `pos` that has an image, a text or a button
    /// (the editor picks with it; games can ask whether the mouse is over
    /// the interface)
    pub fn ui_at(&self, pos: Vec2) -> Option<Entity> {
        self.ui_layout()
            .into_iter()
            .rev()
            .find(|p| {
                p.rect.contains(pos)
                    && [UI_IMAGE, UI_TEXT, UI_BUTTON]
                        .iter()
                        .any(|&c| self.world.has(p.entity, c))
            })
            .map(|p| p.entity)
    }

    /// Hover, hold and click of every UiButton from the mouse (in step).
    /// Only the topmost button under the mouse takes it.
    pub fn update_ui(&mut self) {
        let layout = self.ui_layout();
        let mouse = self.input.mouse;
        let top = layout
            .iter()
            .rev()
            .find(|p| p.rect.contains(mouse) && self.world.has(p.entity, UI_BUTTON))
            .map(|p| p.entity);
        let pressed = self.input.mouse_pressed(MouseButton::Left);
        let released = self.input.mouse_released(MouseButton::Left);
        for e in self.world.query(&[UI_BUTTON]) {
            let Some(mut b) = read::<UiButton>(&self.world, e, UI_BUTTON) else {
                continue;
            };
            let live = b.interactable != 0 && top == Some(e);
            let before = b;
            b.hovered = live as u8;
            b.clicked = (live && released && b.held != 0) as u8;
            if released || !(live || b.held != 0) || b.interactable == 0 {
                b.held = 0;
            }
            if live && pressed {
                b.held = 1;
            }
            // A hidden button lets go too
            if !layout.iter().any(|p| p.entity == e) {
                b.hovered = 0;
                b.held = 0;
                b.clicked = 0;
            }
            if b != before {
                write(&mut self.world, e, UI_BUTTON, &b);
            }
        }
    }

    /// Draw the interface over the world (in draw_entities)
    pub fn draw_ui(&mut self) {
        for (i, p) in self.ui_layout().into_iter().enumerate() {
            let depth = i as f32;
            let tint = read::<UiButton>(&self.world, p.entity, UI_BUTTON)
                .map_or(Color::WHITE, |b| b.tint());
            if let Some(image) = read::<UiImage>(&self.world, p.entity, UI_IMAGE) {
                self.draw_ui_image(&image, p.rect, tint, depth);
            }
            if let Some(text) = read::<UiText>(&self.world, p.entity, UI_TEXT) {
                self.draw_ui_text(&text, p.rect, depth);
            }
        }
    }

    fn draw_ui_image(&mut self, image: &UiImage, rect: Rect, tint: Color, depth: f32) {
        let color = multiply(image.color, tint);
        let size = self
            .textures
            .get(image.texture)
            .map(|t| (t.width(), t.height()));
        let Some((tw, th)) = size else {
            let shape = Shape::Rect {
                rect,
                filled: true,
                thickness: 0.0,
            };
            let kind = DrawKind::Shape { shape, color };
            self.renderer.push(UI_LAYER, depth, Space::Screen, kind);
            return;
        };
        let src = match image.src {
            s if s.w > 0.0 && s.h > 0.0 => s,
            _ => Rect::new(0.0, 0.0, tw as f32, th as f32),
        };
        for (from, to) in slices(src, rect, image.border) {
            let sprite = Sprite {
                src: from,
                pos: Vec2::new(to.x, to.y),
                scale: Vec2::new(to.w / from.w, to.h / from.h),
                tint: color,
                layer: UI_LAYER,
                depth,
                ..Sprite::default()
            };
            let kind = DrawKind::Sprite {
                texture: image.texture,
                sprite,
            };
            self.renderer.push(UI_LAYER, depth, Space::Screen, kind);
        }
    }

    fn draw_ui_text(&mut self, t: &UiText, rect: Rect, depth: f32) {
        let text = self.strings.get(t.text).to_string();
        if text.is_empty() {
            return;
        }
        let font = match t.font {
            0 => 1,
            f => f,
        };
        let Some(size) = self.fonts.get_mut(font).map(|f| f.measure(&text, t.size)) else {
            return;
        };
        let along = |align: u32, start: f32, room: f32, used: f32| match align {
            ALIGN_CENTER => start + (room - used) / 2.0,
            ALIGN_END => start + room - used,
            _ => start,
        };
        let pos = Vec2::new(
            along(t.align, rect.x, rect.w, size.x).round(),
            along(t.valign, rect.y, rect.h, size.y).round(),
        );
        let kind = DrawKind::Text {
            font,
            text,
            pos,
            size: t.size,
            color: t.color,
        };
        self.renderer.push(UI_LAYER, depth, Space::Screen, kind);
    }
}

/// The box `r` makes in `parent`
pub fn place(parent: Rect, r: &UiRect) -> Rect {
    let at = |t: Vec2| Vec2::new(parent.x + parent.w * t.x, parent.y + parent.h * t.y);
    let (a, b) = (at(r.anchor_min), at(r.anchor_max));
    let w = b.x - a.x + r.size.x;
    let h = b.y - a.y + r.size.y;
    let x = a.x + (b.x - a.x) * r.pivot.x + r.pos.x - w * r.pivot.x;
    let y = a.y + (b.y - a.y) * r.pivot.y + r.pos.y - h * r.pivot.y;
    Rect::new(x, y, w.max(0.0), h.max(0.0))
}

/// Pieces of `src` and where they go in `dest`: the whole of it, or nine
/// with `border` texels kept at the edges (smaller when the box is)
fn slices(src: Rect, dest: Rect, border: f32) -> Vec<(Rect, Rect)> {
    let b = border.min(src.w / 2.0).min(src.h / 2.0);
    if b <= 0.0 {
        return vec![(src, dest)];
    }
    let d = b.min(dest.w / 2.0).min(dest.h / 2.0);
    let cuts =
        |start: f32, len: f32, edge: f32| [start, start + edge, start + len - edge, start + len];
    let (sx, sy) = (cuts(src.x, src.w, b), cuts(src.y, src.h, b));
    let (dx, dy) = (cuts(dest.x, dest.w, d), cuts(dest.y, dest.h, d));
    let mut out = Vec::with_capacity(9);
    for j in 0..3 {
        for i in 0..3 {
            let from = Rect::new(sx[i], sy[j], sx[i + 1] - sx[i], sy[j + 1] - sy[j]);
            let to = Rect::new(dx[i], dy[j], dx[i + 1] - dx[i], dy[j + 1] - dy[j]);
            if from.w > 0.0 && from.h > 0.0 && to.w > 0.0 && to.h > 0.0 {
                out.push((from, to));
            }
        }
    }
    out
}

fn multiply(a: Color, b: Color) -> Color {
    let m = |x: u8, y: u8| ((x as u16 * y as u16 + 127) / 255) as u8;
    Color::rgba(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), m(a.a, b.a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::InputEvent;

    fn engine() -> Engine {
        Engine::headless(200, 100).unwrap()
    }

    fn spawn(e: &mut Engine, r: UiRect) -> Entity {
        let ent = e.world.spawn();
        write(&mut e.world, ent, UI_RECT, &r);
        ent
    }

    #[test]
    fn boxes_follow_their_anchors() {
        let mut e = engine();
        // A bar along the bottom, 20 high, with a 10 pixel margin
        let bar = spawn(
            &mut e,
            UiRect {
                anchor_min: Vec2::new(0.0, 1.0),
                anchor_max: Vec2::new(1.0, 1.0),
                pivot: Vec2::new(0.5, 1.0),
                pos: Vec2::new(0.0, -10.0),
                size: Vec2::new(-20.0, 20.0),
                ..Default::default()
            },
        );
        assert_eq!(e.ui_rect(bar), Some(Rect::new(10.0, 70.0, 180.0, 20.0)));
        // A 40x10 box in its top-right corner
        let corner = spawn(
            &mut e,
            UiRect {
                parent: bar,
                anchor_min: Vec2::new(1.0, 0.0),
                anchor_max: Vec2::new(1.0, 0.0),
                pivot: Vec2::new(1.0, 0.0),
                size: Vec2::new(40.0, 10.0),
                ..Default::default()
            },
        );
        assert_eq!(e.ui_rect(corner), Some(Rect::new(150.0, 70.0, 40.0, 10.0)));
        let layout: Vec<Entity> = e.ui_layout().iter().map(|p| p.entity).collect();
        assert_eq!(layout, [bar, corner], "children over parents");
        let mut r: UiRect = read(&e.world, bar, UI_RECT).unwrap();
        r.hidden = 1;
        write(&mut e.world, bar, UI_RECT, &r);
        assert_eq!(e.ui_rect(corner), None, "hidden with its parent");
        // A loop of parents does not hang
        r = UiRect {
            parent: corner,
            ..Default::default()
        };
        write(&mut e.world, bar, UI_RECT, &r);
        assert!(e.ui_rect(corner).is_some());
    }

    #[test]
    fn a_press_and_release_on_a_button_clicks_it_once() {
        let mut e = engine();
        let b = spawn(&mut e, UiRect::default()); // 160x40 in the middle
        write(&mut e.world, b, UI_BUTTON, &UiButton::default());
        let state = |e: &Engine| {
            let b: UiButton = read(&e.world, b, UI_BUTTON).unwrap();
            (b.hovered, b.held, b.clicked)
        };
        let frame = |e: &mut Engine, events: &[InputEvent]| {
            for &ev in events {
                e.inject(ev);
            }
            e.poll();
            e.step(1.0 / 60.0);
            state(e)
        };
        use InputEvent::{MouseButton as Click, MouseMove};
        let left = MouseButton::Left;
        assert_eq!(
            frame(&mut e, &[MouseMove(Vec2::new(100.0, 50.0))]),
            (1, 0, 0)
        );
        assert_eq!(frame(&mut e, &[Click(left, true)]), (1, 1, 0));
        assert_eq!(frame(&mut e, &[Click(left, false)]), (1, 0, 1));
        assert_eq!(frame(&mut e, &[]), (1, 0, 0), "one tick only");
        // Pressed on it, released off it: no click
        frame(&mut e, &[Click(left, true)]);
        frame(&mut e, &[MouseMove(Vec2::new(5.0, 5.0))]);
        assert_eq!(frame(&mut e, &[Click(left, false)]), (0, 0, 0));
        // A box drawn over it takes the mouse only when it is a button
        let over = spawn(&mut e, UiRect::default());
        write(&mut e.world, over, UI_IMAGE, &UiImage::default());
        assert_eq!(
            frame(&mut e, &[MouseMove(Vec2::new(100.0, 50.0))]),
            (1, 0, 0)
        );
        write(&mut e.world, over, UI_BUTTON, &UiButton::default());
        assert_eq!(frame(&mut e, &[]), (0, 0, 0));
        assert_eq!(e.ui_at(Vec2::new(100.0, 50.0)), Some(over));
    }

    #[test]
    fn the_interface_draws_over_the_world() {
        let mut e = engine();
        let panel = spawn(&mut e, UiRect::default());
        let red = Color::rgba(255, 0, 0, 255);
        let image = UiImage {
            color: red,
            ..Default::default()
        };
        write(&mut e.world, panel, UI_IMAGE, &image);
        let id = e.strings.intern("OK");
        let text = UiText {
            text: id,
            size: 24.0,
            ..Default::default()
        };
        write(&mut e.world, panel, UI_TEXT, &text);
        e.begin_frame(Color::BLACK);
        e.draw_entities();
        e.end_frame();
        assert_eq!(e.pixel(25, 35), Some(red), "the panel's corner");
        assert_eq!(e.pixel(5, 5), Some(Color::BLACK));
        let white = (85..115)
            .flat_map(|x| (35..65).map(move |y| (x, y)))
            .filter(|&(x, y)| e.pixel(x, y).is_some_and(|c| c.g > 200))
            .count();
        assert!(white > 20, "the text in the middle: {white}");
    }

    #[test]
    fn nine_slices_keep_their_corners() {
        let pieces = slices(
            Rect::new(0.0, 0.0, 12.0, 12.0),
            Rect::new(0.0, 0.0, 100.0, 30.0),
            4.0,
        );
        assert_eq!(pieces.len(), 9);
        assert_eq!(pieces[0].1, Rect::new(0.0, 0.0, 4.0, 4.0));
        assert_eq!(
            pieces[4].1,
            Rect::new(4.0, 4.0, 92.0, 22.0),
            "the middle stretches"
        );
        assert_eq!(
            slices(Rect::new(0.0, 0.0, 8.0, 8.0), Rect::default(), 0.0).len(),
            1
        );
    }
}

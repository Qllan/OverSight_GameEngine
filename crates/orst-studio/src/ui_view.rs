//! The Scene window's interface view: the game's screen as a frame the
//! camera pans and zooms over, with the interface laid out on it as the
//! game shows it. A click selects a box, a drag moves it, a drag on a
//! corner of the selected box resizes it. Positions snap to pixels.

use crate::egui;
use crate::player::{HEIGHT, WIDTH};
use crate::scene_view::{pan, zoom_at, SceneEvent, SceneView};
use orst::components::{read, UiRect, UI_RECT};
use orst::ecs::Entity;
use orst::ui::{fit, UiView};
use orst::{Camera, Color, Engine, Rect, Vec2};

const BACKGROUND: Color = Color::rgba(30, 31, 36, 255);
const SCREEN: Color = Color::rgba(48, 51, 58, 255);
const FRAME: Color = Color::rgba(110, 115, 128, 255);
const OUTLINE: Color = Color::rgba(150, 155, 170, 120);
const SELECTED: Color = Color::rgba(255, 170, 40, 255);
/// Screen pixels around a corner that still grab it
const HANDLE: f32 = 7.0;

/// A left drag on a box
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiDrag {
    pub entity: Entity,
    /// None: the whole box moves; Some(i): corner i (clockwise from the
    /// top left) follows the pointer and the opposite one stays
    pub corner: Option<usize>,
    pub start: Vec2,
    pub from: Rect,
}

/// The view's camera at first: the whole screen in sight
pub fn fit_camera(size: (u32, u32)) -> Camera {
    let zoom = (size.0 as f32 / WIDTH as f32).min(size.1 as f32 / HEIGHT as f32) * 0.9;
    Camera {
        center: Vec2::new(WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0),
        zoom: zoom.max(0.05),
        ..Camera::default()
    }
}

fn corners(r: Rect) -> [Vec2; 4] {
    [
        Vec2::new(r.x, r.y),
        Vec2::new(r.x + r.w, r.y),
        Vec2::new(r.x + r.w, r.y + r.h),
        Vec2::new(r.x, r.y + r.h),
    ]
}

/// The box a drag makes when the pointer is at `at`, in whole pixels
pub fn dragged(d: &UiDrag, at: Vec2) -> Rect {
    let (dx, dy) = ((at.x - d.start.x).round(), (at.y - d.start.y).round());
    let r = d.from;
    let Some(i) = d.corner else {
        return Rect::new(r.x + dx, r.y + dy, r.w, r.h);
    };
    let c = corners(r);
    let (moved, fixed) = (c[i], c[(i + 2) % 4]);
    let p = Vec2::new(moved.x + dx, moved.y + dy);
    Rect::new(
        p.x.min(fixed.x),
        p.y.min(fixed.y),
        (p.x - fixed.x).abs(),
        (p.y - fixed.y).abs(),
    )
}

/// The box under `at`: one that shows something first, then any
pub fn pick(engine: &Engine, at: Vec2) -> Option<Entity> {
    engine.ui_at(at).or_else(|| {
        let layout = engine.ui_layout();
        layout
            .iter()
            .rev()
            .find(|p| p.rect.contains(at))
            .map(|p| p.entity)
    })
}

/// The UiRect of `e` after it was dragged to make `want`
pub fn moved(engine: &Engine, e: Entity, want: Rect) -> Option<UiRect> {
    let r: UiRect = read(&engine.world, e, UI_RECT)?;
    Some(fit(engine.ui_parent_rect(e), &r, want))
}

impl SceneView {
    /// The view of the interface filling `ui`
    pub fn interface_ui(&mut self, ui: &mut egui::Ui, selected: Option<Entity>) -> Vec<SceneEvent> {
        let (rect, response, size) = self.canvas(ui, egui::Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        let to_canvas =
            |p: egui::Pos2| Vec2::new((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp);
        let mut events = Vec::new();
        let engine = &mut self.engine;
        engine.ui_view = UiView::World(WIDTH, HEIGHT);

        let mut camera = *self.ui_camera.get_or_insert_with(|| fit_camera(size));
        if response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
            let d = response.drag_delta() * ppp;
            camera = pan(camera, Vec2::new(d.x, d.y));
        }
        if let Some(pointer) = response.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                camera = zoom_at(camera, to_canvas(pointer), size, (scroll * 0.002).exp());
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.is_none())
            && !ui.ctx().egui_wants_keyboard_input()
            && response.hovered()
        {
            camera = fit_camera(size);
        }
        self.ui_camera = Some(camera);
        engine.set_camera(camera);
        let reach = HANDLE / camera.zoom;

        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let at = engine.screen_to_world(to_canvas(p));
                events.push(SceneEvent::Pick(pick(engine, at)));
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                let start = engine.screen_to_world(to_canvas(origin));
                let on_selected = selected.and_then(|e| Some((e, engine.ui_rect(e)?)));
                let corner = on_selected.and_then(|(e, r)| {
                    let near = |c: &Vec2| (c.x - start.x).abs().max((c.y - start.y).abs()) <= reach;
                    corners(r).iter().position(near).map(|i| (e, r, i))
                });
                self.ui_drag = match corner {
                    Some((entity, from, i)) => Some(UiDrag {
                        entity,
                        corner: Some(i),
                        start,
                        from,
                    }),
                    None => {
                        let inside = on_selected.filter(|(_, r)| r.contains(start));
                        inside
                            .map(|(e, _)| e)
                            .or_else(|| pick(engine, start))
                            .and_then(|entity| {
                                if Some(entity) != selected {
                                    events.push(SceneEvent::Pick(Some(entity)));
                                }
                                Some(UiDrag {
                                    entity,
                                    corner: None,
                                    start,
                                    from: engine.ui_rect(entity)?,
                                })
                            })
                    }
                };
            }
        }
        if let (Some(d), Some(p)) = (self.ui_drag, response.interact_pointer_pos()) {
            if response.dragged_by(egui::PointerButton::Primary) {
                let at = engine.screen_to_world(to_canvas(p));
                if let Some(r) = moved(engine, d.entity, dragged(&d, at)) {
                    events.push(SceneEvent::Ui(d.entity, r));
                }
            }
        }
        if !response.dragged() {
            self.ui_drag = None;
        }

        draw(engine, selected, camera);
        self.present(ui, rect);
        events
    }
}

/// The screen, the interface on it and the selected box with its corners
pub fn draw(engine: &mut Engine, selected: Option<Entity>, camera: Camera) {
    let px = 1.0 / camera.zoom;
    let screen = engine.ui_screen();
    engine.begin_frame(BACKGROUND);
    engine.set_layer(i32::MIN);
    engine.fill_rect(screen, SCREEN);
    engine.set_layer(0);
    engine.draw_ui();
    engine.set_layer(i32::MAX);
    engine.draw_rect(screen, px, FRAME);
    for p in engine.ui_layout() {
        if Some(p.entity) != selected {
            engine.draw_rect(p.rect, px, OUTLINE);
        }
    }
    if let Some(r) = selected.and_then(|e| engine.ui_rect(e)) {
        engine.draw_rect(r, 2.0 * px, SELECTED);
        let h = HANDLE * px * 0.6;
        for c in corners(r) {
            engine.fill_rect(Rect::new(c.x - h, c.y - h, 2.0 * h, 2.0 * h), SELECTED);
        }
    }
    engine.set_layer(0);
    engine.end_frame();
}

#[cfg(test)]
mod tests {
    use super::*;
    use orst::components::write;

    #[test]
    fn drags_move_and_resize_in_whole_pixels() {
        let d = UiDrag {
            entity: 1,
            corner: None,
            start: Vec2::new(0.0, 0.0),
            from: Rect::new(10.0, 10.0, 40.0, 20.0),
        };
        assert_eq!(
            dragged(&d, Vec2::new(5.4, -2.6)),
            Rect::new(15.0, 7.0, 40.0, 20.0)
        );
        let corner = UiDrag {
            corner: Some(2),
            ..d
        };
        assert_eq!(
            dragged(&corner, Vec2::new(10.0, 5.0)),
            Rect::new(10.0, 10.0, 50.0, 25.0),
            "the bottom right follows"
        );
        assert_eq!(
            dragged(&corner, Vec2::new(-60.0, 0.0)),
            Rect::new(-10.0, 10.0, 20.0, 20.0),
            "past the other corner it turns over"
        );
    }

    #[test]
    fn a_dragged_box_keeps_its_anchors() {
        let mut engine = Engine::headless(64, 64).unwrap();
        engine.ui_view = UiView::World(WIDTH, HEIGHT);
        let e = engine.world.spawn();
        let r = UiRect {
            anchor_min: Vec2::new(1.0, 1.0),
            anchor_max: Vec2::new(1.0, 1.0),
            pivot: Vec2::new(1.0, 1.0),
            ..Default::default()
        };
        write(&mut engine.world, e, UI_RECT, &r);
        let want = Rect::new(1000.0, 600.0, 100.0, 50.0);
        let after = moved(&engine, e, want).unwrap();
        assert_eq!(after.anchor_min, r.anchor_min);
        assert_eq!(after.pos, Vec2::new(-180.0, -70.0));
        write(&mut engine.world, e, UI_RECT, &after);
        assert_eq!(engine.ui_rect(e), Some(want));
        assert_eq!(
            pick(&engine, Vec2::new(1050.0, 620.0)),
            Some(e),
            "a bare box"
        );
    }
}

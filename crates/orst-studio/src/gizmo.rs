//! Handles on the selected entity for moving, turning and scaling it with
//! the mouse. They are drawn and hit in canvas pixels, so they keep their
//! size at any zoom; the drag itself is worked out in world units.

use crate::scene_view::grid_step;
use orst::components::Transform;
use orst::{Color, Engine, Space, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Move,
    Rotate,
    Scale,
}

/// The part of the gizmo under the cursor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    /// Move or scale along the world's x (move) or the entity's own x (scale)
    X,
    Y,
    /// Move freely or scale evenly
    Center,
    Ring,
}

/// Arrow length in canvas pixels
const ARM: f32 = 64.0;
const RING: f32 = 56.0;
/// How near the cursor must be to a handle
const REACH: f32 = 7.0;

const RED: Color = Color::rgba(230, 70, 70, 255);
const GREEN: Color = Color::rgba(90, 210, 90, 255);
const BLUE: Color = Color::rgba(80, 150, 255, 255);
const YELLOW: Color = Color::rgba(255, 220, 60, 255);

/// A drag on the gizmo: what was grabbed, where, and the transform then
#[derive(Debug, Clone, Copy)]
pub struct Drag {
    pub tool: Tool,
    pub handle: Handle,
    pub start: Vec2,
    pub from: Transform,
}

fn sub(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(a.x - b.x, a.y - b.y)
}

fn dot(a: Vec2, b: Vec2) -> f32 {
    a.x * b.x + a.y * b.y
}

fn len(a: Vec2) -> f32 {
    a.x.hypot(a.y)
}

fn unit(a: Vec2) -> Vec2 {
    let l = len(a).max(1e-6);
    Vec2::new(a.x / l, a.y / l)
}

/// The directions of the handles on the canvas: the world's axes for
/// moving, the entity's own for scaling
fn axes(engine: &Engine, tool: Tool, t: &Transform) -> (Vec2, Vec2, Vec2) {
    let center = engine.world_to_screen(t.pos);
    let (x, y) = match tool {
        Tool::Scale => {
            let (s, c) = t.rotation.sin_cos();
            (Vec2::new(c, s), Vec2::new(-s, c))
        }
        _ => (Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)),
    };
    let to = |d: Vec2| {
        let p = engine.world_to_screen(Vec2::new(t.pos.x + d.x, t.pos.y + d.y));
        unit(sub(p, center))
    };
    (center, to(x), to(y))
}

/// Distance from `p` to the segment a..b
fn to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = sub(b, a);
    let k = (dot(sub(p, a), ab) / dot(ab, ab).max(1e-6)).clamp(0.0, 1.0);
    len(sub(p, Vec2::new(a.x + ab.x * k, a.y + ab.y * k)))
}

/// The handle under the canvas point `p`, if any
pub fn hit(engine: &Engine, tool: Tool, t: &Transform, p: Vec2) -> Option<Handle> {
    let (c, x, y) = axes(engine, tool, t);
    if (p.x - c.x).abs() <= REACH && (p.y - c.y).abs() <= REACH && tool != Tool::Rotate {
        return Some(Handle::Center);
    }
    match tool {
        Tool::Rotate => ((len(sub(p, c)) - RING).abs() <= REACH).then_some(Handle::Ring),
        _ => {
            let end = |d: Vec2| Vec2::new(c.x + d.x * ARM, c.y + d.y * ARM);
            if to_segment(p, c, end(x)) <= REACH {
                Some(Handle::X)
            } else if to_segment(p, c, end(y)) <= REACH {
                Some(Handle::Y)
            } else {
                None
            }
        }
    }
}

/// The transform while the cursor is at `at` (world units). `snap`: the
/// grid step to snap to (Ctrl held); turns snap to 15 degrees and scales
/// to tenths.
pub fn apply(drag: &Drag, at: Vec2, snap: Option<f32>) -> Transform {
    let mut t = drag.from;
    let d = sub(at, drag.start);
    let round = |v: f32, step: f32| (v / step).round() * step;
    match drag.tool {
        Tool::Move => {
            let (mx, my) = match drag.handle {
                Handle::X => (d.x, 0.0),
                Handle::Y => (0.0, d.y),
                _ => (d.x, d.y),
            };
            t.pos = Vec2::new(t.pos.x + mx, t.pos.y + my);
            if let Some(step) = snap {
                if mx != 0.0 {
                    t.pos.x = round(t.pos.x, step);
                }
                if my != 0.0 {
                    t.pos.y = round(t.pos.y, step);
                }
            }
        }
        Tool::Rotate => {
            let a = sub(drag.start, t.pos);
            let b = sub(at, t.pos);
            let turn = b.y.atan2(b.x) - a.y.atan2(a.x);
            t.rotation = drag.from.rotation + turn;
            if snap.is_some() {
                t.rotation = round(t.rotation, 15f32.to_radians());
            }
        }
        Tool::Scale => {
            let a = sub(drag.start, t.pos);
            let b = sub(at, t.pos);
            let (s, c) = t.rotation.sin_cos();
            let along = |v: Vec2, axis: Vec2| dot(v, axis);
            let ratio = |num: f32, den: f32| if den.abs() < 1e-6 { 1.0 } else { num / den };
            let (fx, fy) = match drag.handle {
                Handle::X => {
                    let axis = Vec2::new(c, s);
                    (ratio(along(b, axis), along(a, axis)), 1.0)
                }
                Handle::Y => {
                    let axis = Vec2::new(-s, c);
                    (1.0, ratio(along(b, axis), along(a, axis)))
                }
                _ => {
                    let f = ratio(len(b), len(a));
                    (f, f)
                }
            };
            t.scale = Vec2::new(drag.from.scale.x * fx, drag.from.scale.y * fy);
            if snap.is_some() {
                t.scale = Vec2::new(round(t.scale.x, 0.1), round(t.scale.y, 0.1));
            }
        }
    }
    t
}

/// The snapping step for moving at this zoom: a tenth of the grid
pub fn move_snap(zoom: f32) -> f32 {
    grid_step(zoom) / 10.0
}

/// Draw the gizmo for `t`; `active`: the handle being dragged
pub fn draw(engine: &mut Engine, tool: Tool, t: &Transform, active: Option<Handle>) {
    let (c, x, y) = axes(engine, tool, t);
    engine.set_space(Space::Screen);
    let color = |h: Handle, base: Color| if active == Some(h) { YELLOW } else { base };
    match tool {
        Tool::Rotate => {
            engine.draw_circle(c, RING, 2.0, color(Handle::Ring, BLUE));
            let (s, co) = (t.rotation).sin_cos();
            let p = engine.world_to_screen(Vec2::new(t.pos.x + co, t.pos.y + s));
            let d = unit(sub(p, c));
            engine.draw_line(
                c,
                Vec2::new(c.x + d.x * RING, c.y + d.y * RING),
                2.0,
                color(Handle::Ring, BLUE),
            );
        }
        _ => {
            for (h, d, base) in [(Handle::X, x, RED), (Handle::Y, y, GREEN)] {
                let col = color(h, base);
                let end = Vec2::new(c.x + d.x * ARM, c.y + d.y * ARM);
                engine.draw_line(c, end, 2.0, col);
                if tool == Tool::Scale {
                    engine.fill_rect(orst::Rect::new(end.x - 5.0, end.y - 5.0, 10.0, 10.0), col);
                } else {
                    // An arrow head: two short lines back from the tip
                    let back = Vec2::new(end.x - d.x * 10.0, end.y - d.y * 10.0);
                    let side = Vec2::new(-d.y * 5.0, d.x * 5.0);
                    engine.draw_line(end, Vec2::new(back.x + side.x, back.y + side.y), 2.0, col);
                    engine.draw_line(end, Vec2::new(back.x - side.x, back.y - side.y), 2.0, col);
                }
            }
            let col = color(Handle::Center, Color::rgba(230, 230, 230, 255));
            engine.draw_rect(orst::Rect::new(c.x - 5.0, c.y - 5.0, 10.0, 10.0), 2.0, col);
        }
    }
    engine.set_space(Space::World);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        let mut e = Engine::headless(400, 300).unwrap();
        let mut cam = e.camera();
        cam.zoom = 2.0;
        e.set_camera(cam);
        e
    }

    #[test]
    fn handles_are_found_in_canvas_pixels() {
        let e = engine();
        let t = Transform::default();
        // The origin is in the middle of the canvas
        assert_eq!(
            hit(&e, Tool::Move, &t, Vec2::new(200.0, 150.0)),
            Some(Handle::Center)
        );
        assert_eq!(
            hit(&e, Tool::Move, &t, Vec2::new(250.0, 152.0)),
            Some(Handle::X)
        );
        assert_eq!(
            hit(&e, Tool::Move, &t, Vec2::new(198.0, 200.0)),
            Some(Handle::Y)
        );
        assert_eq!(hit(&e, Tool::Move, &t, Vec2::new(240.0, 200.0)), None);
        assert_eq!(
            hit(&e, Tool::Rotate, &t, Vec2::new(200.0, 150.0 - RING)),
            Some(Handle::Ring)
        );
        assert_eq!(hit(&e, Tool::Rotate, &t, Vec2::new(200.0, 150.0)), None);
        // Scale handles turn with the entity
        let turned = Transform {
            rotation: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        };
        assert_eq!(
            hit(&e, Tool::Scale, &turned, Vec2::new(200.0, 200.0)),
            Some(Handle::X)
        );
    }

    #[test]
    fn drags_move_turn_and_scale() {
        let from = Transform {
            pos: Vec2::new(10.0, 10.0),
            ..Default::default()
        };
        let drag = |tool, handle, start: (f32, f32)| Drag {
            tool,
            handle,
            start: Vec2::new(start.0, start.1),
            from,
        };
        let at = Vec2::new(23.0, 4.0);

        let t = apply(&drag(Tool::Move, Handle::Center, (10.0, 10.0)), at, None);
        assert_eq!(t.pos, Vec2::new(23.0, 4.0));
        let t = apply(&drag(Tool::Move, Handle::X, (10.0, 10.0)), at, None);
        assert_eq!(t.pos, Vec2::new(23.0, 10.0), "x only");
        let t = apply(
            &drag(Tool::Move, Handle::Center, (10.0, 10.0)),
            at,
            Some(5.0),
        );
        assert_eq!(t.pos, Vec2::new(25.0, 5.0), "snapped");

        // From the right of the entity to below it: a quarter turn clockwise
        let d = drag(Tool::Rotate, Handle::Ring, (20.0, 10.0));
        let t = apply(&d, Vec2::new(10.0, 30.0), None);
        assert!((t.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        let t = apply(&d, Vec2::new(10.0 + 10.0, 10.0 + 2.0), Some(1.0));
        assert_eq!(t.rotation.to_degrees().round(), 15.0);

        let d = drag(Tool::Scale, Handle::X, (20.0, 10.0));
        let t = apply(&d, Vec2::new(40.0, 99.0), None);
        assert_eq!(t.scale, Vec2::new(3.0, 1.0));
        let d = drag(Tool::Scale, Handle::Center, (20.0, 10.0));
        let t = apply(&d, Vec2::new(10.0, 15.0), None);
        assert_eq!(t.scale, Vec2::new(0.5, 0.5));
    }
}

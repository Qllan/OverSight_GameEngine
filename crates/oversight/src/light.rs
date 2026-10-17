//! 2D lights for the lighting effect: what a target sees of the lights
//! and shadow casters, worked out once per frame in the target's pixels,
//! so both renderers only do per-pixel maths.
//!
//! A light's shadows are the areas behind the back faces of casters (the
//! edges facing away from it), so a wall's own face stays lit and the
//! shadow starts behind it. The hard shadow is then blurred by the light's
//! softness.

use tiny_skia::{Point, Transform};

use crate::shapes::Shape;
use crate::tilemap::Tilemap;
use crate::types::{Rect, Vec2};

pub const EFFECT_LIGHTING: u32 = 6;

/// One light in a target's pixels
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightPx {
    pub pos: Vec2,
    pub radius: f32,
    /// Colour times intensity
    pub color: [f32; 3],
    pub height: f32,
    pub softness: f32,
    pub shadows: bool,
}

/// An edge of a shadow caster, with the normal pointing out of it
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    pub a: Vec2,
    pub b: Vec2,
    pub normal: Vec2,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LightScene {
    pub lights: Vec<LightPx>,
    pub edges: Vec<Edge>,
}

impl LightPx {
    /// Pixels it can change, clipped to the picture: x0, y0, x1, y1
    /// (end exclusive); None when it is off the picture
    pub fn bounds(&self, size: (u32, u32)) -> Option<[i32; 4]> {
        let reach = self.radius + self.blur() as f32 + 1.0;
        let x0 = ((self.pos.x - reach).floor() as i32).max(0);
        let y0 = ((self.pos.y - reach).floor() as i32).max(0);
        let x1 = ((self.pos.x + reach).ceil() as i32).min(size.0 as i32);
        let y1 = ((self.pos.y + reach).ceil() as i32).min(size.1 as i32);
        (x0 < x1 && y0 < y1 && self.radius > 0.0).then_some([x0, y0, x1, y1])
    }

    /// Shadow blur radius in whole pixels (0: hard)
    pub fn blur(&self) -> u32 {
        if self.softness > 0.0 && self.shadows {
            self.softness.clamp(1.0, crate::post::MAX_RADIUS).ceil() as u32
        } else {
            0
        }
    }

    /// Quads of shadow behind the edges facing away from the light
    pub fn shadow_quads(&self, edges: &[Edge]) -> Vec<[Vec2; 4]> {
        if !self.shadows {
            return Vec::new();
        }
        let far = (self.radius + self.blur() as f32 + 2.0) * 4.0;
        let away = |p: Vec2| {
            let (dx, dy) = (p.x - self.pos.x, p.y - self.pos.y);
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            Vec2::new(p.x + dx / len * far, p.y + dy / len * far)
        };
        edges
            .iter()
            .filter(|e| {
                let mid = Vec2::new((e.a.x + e.b.x) / 2.0, (e.a.y + e.b.y) / 2.0);
                let facing = (mid.x - self.pos.x) * e.normal.x + (mid.y - self.pos.y) * e.normal.y;
                facing > 0.0 && distance_to_segment(self.pos, e.a, e.b) < self.radius
            })
            .map(|e| [e.a, e.b, away(e.b), away(e.a)])
            .collect()
    }
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (a.x + dx * t - p.x, a.y + dy * t - p.y);
    (x * x + y * y).sqrt()
}

fn apply(t: Transform, p: Vec2) -> Vec2 {
    let mut q = Point::from_xy(p.x, p.y);
    t.map_point(&mut q);
    Vec2::new(q.x, q.y)
}

/// Edges of a closed outline (corners in order), normals pointing away
/// from its middle, mapped by `t`
fn outline(points: &[Vec2], t: Transform, out: &mut Vec<Edge>) {
    let pts: Vec<Vec2> = points.iter().map(|p| apply(t, *p)).collect();
    let n = pts.len() as f32;
    let mid = Vec2::new(
        pts.iter().map(|p| p.x).sum::<f32>() / n,
        pts.iter().map(|p| p.y).sum::<f32>() / n,
    );
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let mut normal = Vec2::new(b.y - a.y, a.x - b.x);
        let (cx, cy) = ((a.x + b.x) / 2.0 - mid.x, (a.y + b.y) / 2.0 - mid.y);
        if normal.x * cx + normal.y * cy < 0.0 {
            normal = Vec2::new(-normal.x, -normal.y);
        }
        out.push(Edge { a, b, normal });
    }
}

/// Edges of a collider shape (circles as 16 sides)
pub fn shape_edges(shape: &Shape, t: Transform, out: &mut Vec<Edge>) {
    match shape {
        Shape::Polygon(points) => outline(points, t, out),
        Shape::Circle { center, radius } => {
            let points: Vec<Vec2> = (0..16)
                .map(|i| {
                    let a = i as f32 / 16.0 * std::f32::consts::TAU;
                    Vec2::new(
                        center.x + libm::cosf(a) * radius,
                        center.y + libm::sinf(a) * radius,
                    )
                })
                .collect();
            outline(&points, t, out);
        }
    }
}

/// Edges between solid cells and open ones inside a world rectangle
pub fn tilemap_edges(map: &Tilemap, view: Rect, t: Transform, out: &mut Vec<Edge>) {
    let corners = [
        Vec2::new(view.x, view.y),
        Vec2::new(view.right(), view.y),
        Vec2::new(view.x, view.bottom()),
        Vec2::new(view.right(), view.bottom()),
    ]
    .map(|p| map.world_to_tile(p));
    let lo = |f: fn(&Vec2) -> f32| corners.iter().map(f).fold(f32::INFINITY, f32::min);
    let hi = |f: fn(&Vec2) -> f32| corners.iter().map(f).fold(f32::NEG_INFINITY, f32::max);
    let x0 = (lo(|p| p.x).floor() as i32).max(0);
    let y0 = (lo(|p| p.y).floor() as i32).max(0);
    let x1 = (hi(|p| p.x).ceil() as i32).min(map.width as i32);
    let y1 = (hi(|p| p.y).ceil() as i32).min(map.height as i32);
    let corner = |x: i32, y: i32| map.tile_to_world(x as f32, y as f32);
    for y in y0..y1 {
        for x in x0..x1 {
            if !map.is_solid(x, y) {
                continue;
            }
            let mid = map.cell_center(x, y);
            // Neighbour, then the shared side's corners
            let sides = [
                ((x, y - 1), (x, y), (x + 1, y)),
                ((x + 1, y), (x + 1, y), (x + 1, y + 1)),
                ((x, y + 1), (x + 1, y + 1), (x, y + 1)),
                ((x - 1, y), (x, y + 1), (x, y)),
            ];
            for (n, a, b) in sides {
                if map.is_solid(n.0, n.1) {
                    continue;
                }
                let (a, b) = (corner(a.0, a.1), corner(b.0, b.1));
                // Out of the cell, through the side's middle; the camera
                // only turns and scales, so it stays pointing out
                let out_dir = Vec2::new((a.x + b.x) / 2.0 - mid.x, (a.y + b.y) / 2.0 - mid.y);
                let normal = Vec2::new(
                    t.sx * out_dir.x + t.kx * out_dir.y,
                    t.ky * out_dir.x + t.sy * out_dir.y,
                );
                out.push(Edge {
                    a: apply(t, a),
                    b: apply(t, b),
                    normal,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(x: f32, y: f32) -> LightPx {
        LightPx {
            pos: Vec2::new(x, y),
            radius: 50.0,
            color: [1.0; 3],
            height: 0.5,
            softness: 0.0,
            shadows: true,
        }
    }

    #[test]
    fn only_back_faces_in_reach_cast() {
        let mut edges = Vec::new();
        let square = [
            Vec2::new(10.0, -5.0),
            Vec2::new(20.0, -5.0),
            Vec2::new(20.0, 5.0),
            Vec2::new(10.0, 5.0),
        ];
        outline(&square, Transform::identity(), &mut edges);
        assert_eq!(edges.len(), 4);
        let quads = light(0.0, 0.0).shadow_quads(&edges);
        // Light on the left: all but the left side face away from it
        assert_eq!(quads.len(), 3);
        assert!(quads.iter().all(|q| q[0].x > 10.0 || q[1].x > 10.0));
        assert!(quads.iter().any(|q| q[0] == Vec2::new(20.0, -5.0)));
        assert!(quads[0][2].x > 100.0, "pushed far away: {:?}", quads[0]);
        assert!(
            light(-100.0, 0.0).shadow_quads(&edges).is_empty(),
            "out of reach"
        );
        let off = LightPx {
            shadows: false,
            ..light(0.0, 0.0)
        };
        assert!(off.shadow_quads(&edges).is_empty());
    }

    #[test]
    fn bounds_clip_to_the_picture() {
        let l = light(10.0, 10.0);
        assert_eq!(l.bounds((100, 100)), Some([0, 0, 61, 61]));
        assert_eq!(light(-200.0, 0.0).bounds((100, 100)), None);
    }
}

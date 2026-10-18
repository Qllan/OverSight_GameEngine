//! Collision shapes in the world: circles and convex polygons (a box is a
//! polygon with four corners). Overlaps use the separating axis test and
//! say how to push the shapes apart; points and rays can be tested too.

use crate::types::{Rect, Vec2};

fn add(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(a.x + b.x, a.y + b.y)
}

fn sub(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(a.x - b.x, a.y - b.y)
}

fn mul(a: Vec2, s: f32) -> Vec2 {
    Vec2::new(a.x * s, a.y * s)
}

fn dot(a: Vec2, b: Vec2) -> f32 {
    a.x * b.x + a.y * b.y
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

fn len(a: Vec2) -> f32 {
    dot(a, a).sqrt()
}

fn unit(a: Vec2) -> Vec2 {
    let l = len(a);
    if l > 0.0 {
        mul(a, 1.0 / l)
    } else {
        Vec2::new(1.0, 0.0)
    }
}

/// Rotate clockwise on screen (y points down), like sprites. libm instead
/// of the system's sin and cos, which differ between Windows and Linux in
/// the last bits and would split networked games apart.
pub fn rotate(p: Vec2, radians: f32) -> Vec2 {
    if radians == 0.0 {
        return p;
    }
    let (s, c) = (libm::sinf(radians), libm::cosf(radians));
    Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Circle {
        center: Vec2,
        radius: f32,
    },
    /// Convex, corners in order (either way round)
    Polygon(Vec<Vec2>),
}

/// How two shapes overlap
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Overlap {
    /// From the first shape towards the second, length 1
    pub normal: Vec2,
    /// Move the second shape by normal * depth (or the first by minus
    /// that) and they only touch
    pub depth: f32,
}

/// Where a ray hit a shape
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeHit {
    pub point: Vec2,
    /// Out of the shape, length 1
    pub normal: Vec2,
    /// Distance along the ray from its start
    pub distance: f32,
}

impl Shape {
    /// A box of size `size` centred on `center`, turned by `rotation`
    pub fn rect(center: Vec2, size: Vec2, rotation: f32) -> Shape {
        let (hw, hh) = (size.x * 0.5, size.y * 0.5);
        let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
        Shape::Polygon(
            corners
                .iter()
                .map(|&(x, y)| add(center, rotate(Vec2::new(x, y), rotation)))
                .collect(),
        )
    }

    /// Local points scaled, turned and moved to `pos`
    pub fn polygon(points: &[Vec2], pos: Vec2, rotation: f32, scale: Vec2) -> Shape {
        Shape::Polygon(
            points
                .iter()
                .map(|p| {
                    add(
                        pos,
                        rotate(Vec2::new(p.x * scale.x, p.y * scale.y), rotation),
                    )
                })
                .collect(),
        )
    }

    pub fn center(&self) -> Vec2 {
        match self {
            Shape::Circle { center, .. } => *center,
            Shape::Polygon(p) if !p.is_empty() => {
                let sum = p.iter().fold(Vec2::default(), |s, &v| add(s, v));
                mul(sum, 1.0 / p.len() as f32)
            }
            Shape::Polygon(_) => Vec2::default(),
        }
    }

    /// The axis-aligned box around the shape
    pub fn bounds(&self) -> Rect {
        match self {
            Shape::Circle { center, radius } => Rect::new(
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            ),
            Shape::Polygon(p) => {
                let (mut lo, mut hi) =
                    (Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN));
                for v in p {
                    lo = Vec2::new(lo.x.min(v.x), lo.y.min(v.y));
                    hi = Vec2::new(hi.x.max(v.x), hi.y.max(v.y));
                }
                if p.is_empty() {
                    return Rect::default();
                }
                Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y)
            }
        }
    }

    /// The lowest and highest point of the shape along `axis` (length 1)
    fn project(&self, axis: Vec2) -> (f32, f32) {
        match self {
            Shape::Circle { center, radius } => {
                let c = dot(*center, axis);
                (c - radius, c + radius)
            }
            Shape::Polygon(p) => p.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
                let d = dot(*v, axis);
                (lo.min(d), hi.max(d))
            }),
        }
    }

    fn edge_axes(&self, out: &mut Vec<Vec2>) {
        if let Shape::Polygon(p) = self {
            for i in 0..p.len() {
                let e = sub(p[(i + 1) % p.len()], p[i]);
                if e.x != 0.0 || e.y != 0.0 {
                    out.push(unit(Vec2::new(-e.y, e.x)));
                }
            }
        }
    }

    /// Whether the shapes overlap (touching does not count), and how
    pub fn overlap(&self, other: &Shape) -> Option<Overlap> {
        if let (
            Shape::Circle {
                center: a,
                radius: ra,
            },
            Shape::Circle {
                center: b,
                radius: rb,
            },
        ) = (self, other)
        {
            let d = sub(*b, *a);
            let dist = len(d);
            let depth = ra + rb - dist;
            return (depth > 0.0).then(|| Overlap {
                normal: unit(d),
                depth,
            });
        }
        let mut axes = Vec::with_capacity(8);
        self.edge_axes(&mut axes);
        other.edge_axes(&mut axes);
        // A circle against a polygon also needs the axis through the
        // polygon corner nearest to the circle
        for (circle, poly) in [(self, other), (other, self)] {
            if let (Shape::Circle { center, .. }, Shape::Polygon(p)) = (circle, poly) {
                let nearest = p
                    .iter()
                    .min_by(|a, b| len(sub(**a, *center)).total_cmp(&len(sub(**b, *center))));
                if let Some(v) = nearest {
                    axes.push(unit(sub(*center, *v)));
                }
            }
        }
        let mut best = Overlap {
            normal: Vec2::default(),
            depth: f32::MAX,
        };
        for axis in axes {
            let (a0, a1) = self.project(axis);
            let (b0, b1) = other.project(axis);
            let depth = a1.min(b1) - a0.max(b0);
            if depth <= 0.0 {
                return None;
            }
            if depth < best.depth {
                best = Overlap {
                    normal: axis,
                    depth,
                };
            }
        }
        if best.depth == f32::MAX {
            return None;
        }
        if dot(sub(other.center(), self.center()), best.normal) < 0.0 {
            best.normal = mul(best.normal, -1.0);
        }
        Some(best)
    }

    pub fn contains(&self, point: Vec2) -> bool {
        match self {
            Shape::Circle { center, radius } => len(sub(point, *center)) < *radius,
            Shape::Polygon(p) if p.len() >= 3 => {
                let mut sign = 0.0f32;
                for i in 0..p.len() {
                    let c = cross(sub(p[(i + 1) % p.len()], p[i]), sub(point, p[i]));
                    if c == 0.0 {
                        return false;
                    }
                    if sign != 0.0 && c.signum() != sign {
                        return false;
                    }
                    sign = c.signum();
                }
                true
            }
            Shape::Polygon(_) => false,
        }
    }

    /// Where the segment from `from` to `to` first enters the shape; None
    /// when it misses or starts inside
    pub fn raycast(&self, from: Vec2, to: Vec2) -> Option<ShapeHit> {
        let d = sub(to, from);
        let length = len(d);
        if length == 0.0 || self.contains(from) {
            return None;
        }
        let dir = mul(d, 1.0 / length);
        match self {
            Shape::Circle { center, radius } => {
                let m = sub(from, *center);
                let b = dot(m, dir);
                let c = dot(m, m) - radius * radius;
                let disc = b * b - c;
                if disc < 0.0 {
                    return None;
                }
                let t = -b - disc.sqrt();
                if !(0.0..=length).contains(&t) {
                    return None;
                }
                let point = add(from, mul(dir, t));
                Some(ShapeHit {
                    point,
                    normal: unit(sub(point, *center)),
                    distance: t,
                })
            }
            Shape::Polygon(p) => {
                let mut best: Option<ShapeHit> = None;
                let center = self.center();
                for i in 0..p.len() {
                    let (a, b) = (p[i], p[(i + 1) % p.len()]);
                    let e = sub(b, a);
                    let denom = cross(dir, e);
                    if denom == 0.0 {
                        continue;
                    }
                    let t = cross(sub(a, from), e) / denom;
                    let u = cross(sub(a, from), dir) / denom;
                    if !(0.0..=length).contains(&t) || !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    if best.is_some_and(|h| h.distance <= t) {
                        continue;
                    }
                    let mut normal = unit(Vec2::new(-e.y, e.x));
                    if dot(normal, sub(a, center)) < 0.0 {
                        normal = mul(normal, -1.0);
                    }
                    best = Some(ShapeHit {
                        point: add(from, mul(dir, t)),
                        normal,
                        distance: t,
                    });
                }
                best
            }
        }
    }
}

/// Whether the points make a convex polygon with some area
pub fn is_convex(points: &[Vec2]) -> bool {
    if points.len() < 3 {
        return false;
    }
    let n = points.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let c = cross(
            sub(points[(i + 1) % n], points[i]),
            sub(points[(i + 2) % n], points[(i + 1) % n]),
        );
        if c == 0.0 {
            continue;
        }
        if sign != 0.0 && c.signum() != sign {
            return false;
        }
        sign = c.signum();
    }
    sign != 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
    }

    #[test]
    fn circles_push_apart_along_the_centres() {
        let a = Shape::Circle {
            center: v(0.0, 0.0),
            radius: 2.0,
        };
        let b = Shape::Circle {
            center: v(3.0, 0.0),
            radius: 2.0,
        };
        let o = a.overlap(&b).unwrap();
        assert!(close(o.normal, v(1.0, 0.0)) && (o.depth - 1.0).abs() < 1e-5);
        let far = Shape::Circle {
            center: v(4.0, 0.0),
            radius: 2.0,
        };
        assert_eq!(a.overlap(&far), None, "touching is not overlapping");
    }

    #[test]
    fn boxes_push_out_the_short_way() {
        let a = Shape::rect(v(0.0, 0.0), v(10.0, 10.0), 0.0);
        let b = Shape::rect(v(9.0, 2.0), v(10.0, 10.0), 0.0);
        let o = a.overlap(&b).unwrap();
        assert!(
            close(o.normal, v(1.0, 0.0)) && (o.depth - 1.0).abs() < 1e-4,
            "{o:?}"
        );
        let o = b.overlap(&a).unwrap();
        assert!(
            close(o.normal, v(-1.0, 0.0)),
            "the normal points from the first shape"
        );
        // A box turned 45 degrees reaches further along the axes
        let turned = Shape::rect(v(12.0, 0.0), v(10.0, 10.0), std::f32::consts::FRAC_PI_4);
        assert!(a.overlap(&turned).is_some());
        assert!(Shape::rect(v(12.0, 0.0), v(10.0, 10.0), 0.0)
            .overlap(&a)
            .is_none());
    }

    #[test]
    fn circle_against_a_box_corner() {
        let b = Shape::rect(v(0.0, 0.0), v(2.0, 2.0), 0.0);
        // Near the corner (1, 1) but outside along the diagonal
        let c = Shape::Circle {
            center: v(1.6, 1.6),
            radius: 0.8,
        };
        assert_eq!(b.overlap(&c), None, "the corner axis separates them");
        let c = Shape::Circle {
            center: v(1.4, 1.4),
            radius: 0.8,
        };
        let o = b.overlap(&c).unwrap();
        assert!(close(o.normal, unit(v(1.0, 1.0))), "{o:?}");
    }

    #[test]
    fn points_and_rays() {
        let tri = Shape::polygon(
            &[v(0.0, 0.0), v(4.0, 0.0), v(0.0, 4.0)],
            v(10.0, 10.0),
            0.0,
            v(1.0, 1.0),
        );
        assert!(tri.contains(v(11.0, 11.0)) && !tri.contains(v(13.5, 13.5)));
        let hit = tri.raycast(v(5.0, 11.0), v(20.0, 11.0)).unwrap();
        assert!(
            close(hit.point, v(10.0, 11.0)) && close(hit.normal, v(-1.0, 0.0)),
            "{hit:?}"
        );
        assert!((hit.distance - 5.0).abs() < 1e-4);
        let c = Shape::Circle {
            center: v(0.0, 0.0),
            radius: 1.0,
        };
        let hit = c.raycast(v(-5.0, 0.0), v(5.0, 0.0)).unwrap();
        assert!(close(hit.point, v(-1.0, 0.0)) && close(hit.normal, v(-1.0, 0.0)));
        assert!(c.raycast(v(-5.0, 2.0), v(5.0, 2.0)).is_none());
        assert!(c.raycast(v(-5.0, 0.0), v(-3.0, 0.0)).is_none(), "too short");
    }

    #[test]
    fn convex_check() {
        assert!(is_convex(&[
            v(0.0, 0.0),
            v(1.0, 0.0),
            v(1.0, 1.0),
            v(0.0, 1.0)
        ]));
        assert!(!is_convex(&[
            v(0.0, 0.0),
            v(2.0, 0.0),
            v(1.0, 0.5),
            v(2.0, 2.0),
            v(0.0, 2.0)
        ]));
        assert!(
            !is_convex(&[v(0.0, 0.0), v(1.0, 1.0), v(2.0, 2.0)]),
            "no area"
        );
    }
}

//! The collision pass between colliders. A spatial grid finds the pairs
//! that are near each other, so thousands of bullets and enemies do not
//! test every pair. Each pass lists the overlapping pairs with how they
//! overlap and whether the overlap began this pass, plus the pairs that
//! stopped overlapping. Solid colliders (not triggers) are pushed apart
//! when they move: an entity with a Velocity moves, one without stays
//! put like a wall.

use std::collections::HashMap;

use crate::components::{
    read, write, Collider, Transform, Velocity, COLLIDER, TRANSFORM, VELOCITY,
};
use crate::ecs::World;
use crate::shapes::{Overlap, Shape};
use crate::texture::Store;
use crate::types::{Rect, Vec2};

/// What happened to a pair in the last pass
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactState {
    /// Started overlapping in this pass
    Begin = 0,
    /// Still overlapping
    Stay = 1,
    /// Stopped overlapping (or one of them is gone); no normal or depth
    End = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// The lower entity id first
    pub a: u64,
    pub b: u64,
    /// From a towards b
    pub normal: Vec2,
    pub depth: f32,
    pub state: i32,
    /// Either collider is a trigger
    pub trigger: u8,
    pub _pad: [u8; 3],
}

#[derive(Debug)]
pub struct Collisions {
    /// Grid cell size in world units; about the size of a typical
    /// collider works best
    pub cell: f32,
    contacts: Vec<Contact>,
    /// Pairs overlapping after the last pass, sorted
    touching: Vec<(u64, u64)>,
}

impl Default for Collisions {
    fn default() -> Self {
        Collisions {
            cell: 64.0,
            contacts: Vec::new(),
            touching: Vec::new(),
        }
    }
}

struct Item {
    entity: u64,
    collider: Collider,
    shape: Shape,
    bounds: Rect,
    moves: bool,
}

/// Every collider in the world with its shape, in query order
fn items(world: &World, polygons: &Store<Vec<Vec2>>) -> Vec<Item> {
    let mut out = Vec::new();
    for entity in world.query(&[TRANSFORM, COLLIDER]) {
        let (Some(t), Some(c)) = (
            read::<Transform>(world, entity, TRANSFORM),
            read::<Collider>(world, entity, COLLIDER),
        ) else {
            continue;
        };
        if let Some(shape) = c.shape(&t, |id| polygons.get(id).cloned()) {
            let bounds = shape.bounds();
            out.push(Item {
                entity,
                collider: c,
                shape,
                bounds,
                // Bodies with mass are moved by the physics step; here
                // they hold still like walls
                moves: world.has(entity, VELOCITY)
                    && !world.has(entity, crate::components::RIGIDBODY),
            });
        }
    }
    out
}

fn cells(r: Rect, cell: f32) -> (i32, i32, i32, i32) {
    let c = cell.max(1.0);
    (
        (r.x / c).floor() as i32,
        (r.y / c).floor() as i32,
        (r.right() / c).floor() as i32,
        (r.bottom() / c).floor() as i32,
    )
}

fn bounds_touch(a: Rect, b: Rect) -> bool {
    a.x <= b.right() && b.x <= a.right() && a.y <= b.bottom() && b.y <= a.bottom()
}

impl Collisions {
    /// The contacts of the last pass: begins and stays sorted by pair, then
    /// the ends
    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    /// Pairs overlapping after the last pass (for snapshots)
    pub fn touching(&self) -> &[(u64, u64)] {
        &self.touching
    }

    pub fn set_touching(&mut self, mut pairs: Vec<(u64, u64)>) {
        pairs.sort_unstable();
        pairs.dedup();
        self.touching = pairs;
        self.contacts.clear();
    }

    /// Find the overlaps, push solid movers apart and work out the events
    pub fn update(&mut self, world: &mut World, polygons: &Store<Vec<Vec2>>) {
        let items = items(world, polygons);

        // Candidate pairs from the grid, deduplicated and in a fixed order
        // (the map's order changes from run to run)
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, it) in items.iter().enumerate() {
            let (x0, y0, x1, y1) = cells(it.bounds, self.cell);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    grid.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
        let mut pairs = Vec::new();
        for list in grid.values() {
            for (k, &i) in list.iter().enumerate() {
                for &j in &list[k + 1..] {
                    pairs.push((i.min(j), i.max(j)));
                }
            }
        }
        pairs.sort_unstable();
        pairs.dedup();

        let mut found: Vec<(u64, u64, Overlap, bool)> = Vec::new();
        for (i, j) in pairs {
            let (a, b) = (&items[i as usize], &items[j as usize]);
            if !a.collider.meets(&b.collider) || !bounds_touch(a.bounds, b.bounds) {
                continue;
            }
            let Some(mut o) = a.shape.overlap(&b.shape) else {
                continue;
            };
            let trigger = a.collider.trigger != 0 || b.collider.trigger != 0;
            if !trigger {
                push_apart(world, a, b, o);
            }
            // Keep the lower id first, with the normal pointing from it
            let (mut ea, mut eb) = (a.entity, b.entity);
            if ea > eb {
                std::mem::swap(&mut ea, &mut eb);
                o.normal = Vec2::new(-o.normal.x, -o.normal.y);
            }
            found.push((ea, eb, o, trigger));
        }
        found.sort_unstable_by_key(|f| (f.0, f.1));

        self.contacts.clear();
        let mut now = Vec::with_capacity(found.len());
        for (a, b, o, trigger) in found {
            let begun = self.touching.binary_search(&(a, b)).is_err();
            self.contacts.push(Contact {
                a,
                b,
                normal: o.normal,
                depth: o.depth,
                state: if begun {
                    ContactState::Begin
                } else {
                    ContactState::Stay
                } as i32,
                trigger: trigger as u8,
                _pad: [0; 3],
            });
            now.push((a, b));
        }
        for &(a, b) in &self.touching {
            if now.binary_search(&(a, b)).is_err() {
                self.contacts.push(Contact {
                    a,
                    b,
                    normal: Vec2::default(),
                    depth: 0.0,
                    state: ContactState::End as i32,
                    trigger: 0,
                    _pad: [0; 3],
                });
            }
        }
        self.touching = now;
    }
}

/// Move the movers out of each other and stop the speed into the other
fn push_apart(world: &mut World, a: &Item, b: &Item, o: Overlap) {
    let (share_a, share_b) = match (a.moves, b.moves) {
        (true, true) => (0.5, 0.5),
        (true, false) => (1.0, 0.0),
        (false, true) => (0.0, 1.0),
        (false, false) => return,
    };
    // a goes against the normal, b along it
    for (entity, share, sign) in [(a.entity, share_a, -1.0f32), (b.entity, share_b, 1.0)] {
        if share == 0.0 {
            continue;
        }
        let push = o.depth * share * sign;
        if let Some(mut t) = read::<Transform>(world, entity, TRANSFORM) {
            t.pos = Vec2::new(t.pos.x + o.normal.x * push, t.pos.y + o.normal.y * push);
            write(world, entity, TRANSFORM, &t);
        }
        if let Some(mut v) = read::<Velocity>(world, entity, VELOCITY) {
            // Speed towards the other one, along the normal
            let into = -(v.linear.x * o.normal.x + v.linear.y * o.normal.y) * sign;
            if into > 0.0 {
                v.linear = Vec2::new(
                    v.linear.x + o.normal.x * into * sign,
                    v.linear.y + o.normal.y * into * sign,
                );
                write(world, entity, VELOCITY, &v);
            }
        }
    }
}

/// Entities whose collider overlaps `shape` and has a layer bit in `mask`,
/// in query order
pub fn overlapping(
    world: &World,
    polygons: &Store<Vec<Vec2>>,
    shape: &Shape,
    mask: u32,
) -> Vec<u64> {
    let bounds = shape.bounds();
    items(world, polygons)
        .into_iter()
        .filter(|it| it.collider.layer & mask != 0 && bounds_touch(it.bounds, bounds))
        .filter(|it| match shape {
            // A query point is a tiny circle; test it exactly instead
            Shape::Circle { center, radius } if *radius == 0.0 => it.shape.contains(*center),
            _ => it.shape.overlap(shape).is_some(),
        })
        .map(|it| it.entity)
        .collect()
}

/// The first collider with a layer bit in `mask` that the segment enters
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EntityHit {
    pub entity: u64,
    pub point: Vec2,
    pub normal: Vec2,
    pub distance: f32,
}

pub fn raycast(
    world: &World,
    polygons: &Store<Vec<Vec2>>,
    from: Vec2,
    to: Vec2,
    mask: u32,
) -> Option<EntityHit> {
    let ray = Rect::new(
        from.x.min(to.x),
        from.y.min(to.y),
        (to.x - from.x).abs(),
        (to.y - from.y).abs(),
    );
    let mut best: Option<EntityHit> = None;
    for it in items(world, polygons) {
        if it.collider.layer & mask == 0 || !bounds_touch(it.bounds, ray) {
            continue;
        }
        if let Some(h) = it.shape.raycast(from, to) {
            if best.is_none_or(|b| h.distance < b.distance) {
                best = Some(EntityHit {
                    entity: it.entity,
                    point: h.point,
                    normal: h.normal,
                    distance: h.distance,
                });
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{register, COLLIDER_CIRCLE};

    fn world() -> World {
        let mut w = World::new();
        register(&mut w);
        w
    }

    fn put(w: &mut World, pos: Vec2, c: Collider, vel: Option<Vec2>) -> u64 {
        let e = w.spawn();
        write(
            w,
            e,
            TRANSFORM,
            &Transform {
                pos,
                ..Transform::default()
            },
        );
        write(w, e, COLLIDER, &c);
        if let Some(v) = vel {
            write(
                w,
                e,
                VELOCITY,
                &Velocity {
                    linear: v,
                    angular: 0.0,
                },
            );
        }
        e
    }

    fn circle(r: f32) -> Collider {
        Collider {
            kind: COLLIDER_CIRCLE,
            radius: r,
            ..Collider::default()
        }
    }

    fn pos(w: &World, e: u64) -> Vec2 {
        read::<Transform>(w, e, TRANSFORM).unwrap().pos
    }

    #[test]
    fn begin_stay_end() {
        let (mut w, polys) = (world(), Store::default());
        let mut c = Collisions::default();
        let a = put(
            &mut w,
            Vec2::new(0.0, 0.0),
            Collider {
                trigger: 1,
                ..circle(5.0)
            },
            None,
        );
        let b = put(&mut w, Vec2::new(8.0, 0.0), circle(5.0), None);
        c.update(&mut w, &polys);
        let k = c.contacts()[0];
        assert_eq!(
            (k.a, k.b, k.state, k.trigger),
            (a, b, ContactState::Begin as i32, 1)
        );
        assert!((k.normal.x - 1.0).abs() < 1e-5 && (k.depth - 2.0).abs() < 1e-5);
        c.update(&mut w, &polys);
        assert_eq!(c.contacts()[0].state, ContactState::Stay as i32);
        w.despawn(b);
        c.update(&mut w, &polys);
        let end = c.contacts()[0];
        assert_eq!(
            (c.contacts().len(), end.a, end.b, end.state),
            (1, a, b, ContactState::End as i32)
        );
        c.update(&mut w, &polys);
        assert!(c.contacts().is_empty());
    }

    #[test]
    fn layers_and_masks_choose_who_meets() {
        let (mut w, polys) = (world(), Store::default());
        let mut c = Collisions::default();
        // Bullets (layer 2) hit enemies (layer 4) but not each other
        let bullet = Collider {
            layer: 2,
            mask: 4,
            ..circle(2.0)
        };
        put(&mut w, Vec2::new(0.0, 0.0), bullet, None);
        put(&mut w, Vec2::new(1.0, 0.0), bullet, None);
        put(
            &mut w,
            Vec2::new(2.0, 0.0),
            Collider {
                layer: 4,
                mask: u32::MAX,
                ..circle(2.0)
            },
            None,
        );
        c.update(&mut w, &polys);
        assert_eq!(
            c.contacts().len(),
            2,
            "two bullets on the enemy, none between them"
        );
    }

    #[test]
    fn movers_are_pushed_out_of_walls_and_each_other() {
        let (mut w, polys) = (world(), Store::default());
        let mut c = Collisions::default();
        let wall = put(
            &mut w,
            Vec2::new(0.0, 0.0),
            Collider {
                size: Vec2::new(10.0, 10.0),
                ..Collider::default()
            },
            None,
        );
        let ball = put(
            &mut w,
            Vec2::new(7.0, 0.0),
            circle(3.0),
            Some(Vec2::new(-5.0, 2.0)),
        );
        c.update(&mut w, &polys);
        assert_eq!(pos(&w, wall), Vec2::new(0.0, 0.0), "a wall does not move");
        assert!((pos(&w, ball).x - 8.0).abs() < 1e-4);
        let v = read::<Velocity>(&w, ball, VELOCITY).unwrap().linear;
        assert!(v.x.abs() < 1e-5 && v.y == 2.0, "slides along: {v:?}");
        // Two movers share the push
        let p = put(
            &mut w,
            Vec2::new(100.0, 0.0),
            circle(5.0),
            Some(Vec2::default()),
        );
        let q = put(
            &mut w,
            Vec2::new(108.0, 0.0),
            circle(5.0),
            Some(Vec2::default()),
        );
        c.update(&mut w, &polys);
        assert!((pos(&w, p).x - 99.0).abs() < 1e-4 && (pos(&w, q).x - 109.0).abs() < 1e-4);
    }

    #[test]
    fn the_grid_finds_pairs_across_cells_in_a_fixed_order() {
        let (mut w, polys) = (world(), Store::default());
        let mut c = Collisions {
            cell: 4.0,
            ..Collisions::default()
        };
        // A long box over many cells and a row of small circles on it
        put(
            &mut w,
            Vec2::new(50.0, 0.0),
            Collider {
                size: Vec2::new(100.0, 2.0),
                ..Collider::default()
            },
            None,
        );
        for i in 0..20 {
            put(
                &mut w,
                Vec2::new(i as f32 * 5.0 + 2.0, 1.5),
                circle(1.0),
                None,
            );
        }
        c.update(&mut w, &polys);
        assert_eq!(c.contacts().len(), 20);
        let order: Vec<(u64, u64)> = c.contacts().iter().map(|k| (k.a, k.b)).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
    }

    #[test]
    fn queries_find_entities_by_shape_and_ray() {
        let (mut w, polys) = (world(), Store::default());
        let near = put(&mut w, Vec2::new(10.0, 0.0), circle(2.0), None);
        let far = put(
            &mut w,
            Vec2::new(30.0, 0.0),
            Collider {
                layer: 2,
                ..circle(2.0)
            },
            None,
        );
        let point = Shape::Circle {
            center: Vec2::new(10.5, 0.5),
            radius: 0.0,
        };
        assert_eq!(overlapping(&w, &polys, &point, u32::MAX), vec![near]);
        let area = Shape::rect(Vec2::new(20.0, 0.0), Vec2::new(40.0, 4.0), 0.0);
        assert_eq!(
            overlapping(&w, &polys, &area, 2),
            vec![far],
            "mask picks layers"
        );
        let hit = raycast(
            &w,
            &polys,
            Vec2::new(0.0, 0.0),
            Vec2::new(50.0, 0.0),
            u32::MAX,
        )
        .unwrap();
        assert_eq!(hit.entity, near);
        assert!((hit.distance - 8.0).abs() < 1e-4);
        assert_eq!(
            raycast(&w, &polys, Vec2::new(0.0, 0.0), Vec2::new(50.0, 0.0), 2)
                .unwrap()
                .entity,
            far
        );
    }
}

//! Collisions against a tile map's solid cells (`Tilemap::is_solid`) and
//! one-way platforms (`Tilemap::is_platform`).
//!
//! `move_box` moves a rectangle by a step, sliding along walls, and says
//! what it touched; a platformer is gravity plus `move_box` plus a jump
//! when `ground` is set. Boxes move one axis at a time (x, then y) and
//! every cell along the way is checked, so fast boxes do not pass
//! through thin walls. Boxes and cells are axis-aligned, so `move_box`
//! and `rect_solid` are meant for orthogonal maps; `raycast` and
//! `point_solid` work on isometric maps too.

use crate::tilemap::Tilemap;
use crate::types::{Rect, Vec2};

/// Edges closer than this count as touching, not overlapping
const EPS: f32 = 1e-3;

/// What a moving box ran into
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Contacts {
    /// Stopped while moving down (standing on something)
    pub ground: bool,
    /// Stopped while moving up
    pub ceiling: bool,
    pub wall_left: bool,
    pub wall_right: bool,
}

impl Contacts {
    pub fn any(&self) -> bool {
        self.ground || self.ceiling || self.wall_left || self.wall_right
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MoveResult {
    /// The box after the move
    pub rect: Rect,
    pub contacts: Contacts,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RayHit {
    /// Where the ray entered the solid cell (world units)
    pub point: Vec2,
    pub cell_x: i32,
    pub cell_y: i32,
    /// Side of the cell that was hit, in cell axes: (-1, 0) is the left
    /// side; (0, 0) when the ray starts inside a solid cell
    pub normal: Vec2,
    /// Distance from the start (world units)
    pub distance: f32,
}

fn cells(from: f32, to: f32, size: f32) -> (i32, i32) {
    (
        (from / size).floor() as i32,
        ((to - EPS) / size).floor() as i32,
    )
}

/// Move `rect` by `delta`, stopping at solid cells and sliding along them
pub fn move_box(map: &Tilemap, rect: Rect, delta: Vec2) -> MoveResult {
    let (tw, th) = (map.tile_w.max(1) as f32, map.tile_h.max(1) as f32);
    let mut r = rect;
    let mut contacts = Contacts::default();

    if delta.x != 0.0 {
        let (row0, row1) = cells(r.y, r.bottom(), th);
        let blocked = |c: i32| (row0..=row1).any(|row| map.is_solid(c, row));
        if delta.x > 0.0 {
            // Columns past the one the right edge is in now
            let first = ((r.right() - EPS) / tw).floor() as i32 + 1;
            let last = ((r.right() + delta.x - EPS) / tw).floor() as i32;
            match (first..=last).find(|&c| blocked(c)) {
                Some(c) => {
                    r.x = c as f32 * tw - r.w;
                    contacts.wall_right = true;
                }
                None => r.x += delta.x,
            }
        } else {
            let first = ((r.x + EPS) / tw).floor() as i32 - 1;
            let last = ((r.x + delta.x) / tw).floor() as i32;
            match (last..=first).rev().find(|&c| blocked(c)) {
                Some(c) => {
                    r.x = (c + 1) as f32 * tw;
                    contacts.wall_left = true;
                }
                None => r.x += delta.x,
            }
        }
    }

    if delta.y != 0.0 {
        let (col0, col1) = cells(r.x, r.right(), tw);
        if delta.y > 0.0 {
            // Rows below the box's bottom edge; a platform row is only
            // reached from above, so it blocks here like a solid one
            let first = ((r.bottom() - EPS) / th).floor() as i32 + 1;
            let last = ((r.bottom() + delta.y - EPS) / th).floor() as i32;
            let blocked =
                |row: i32| (col0..=col1).any(|c| map.is_solid(c, row) || map.is_platform(c, row));
            match (first..=last).find(|&row| blocked(row)) {
                Some(row) => {
                    r.y = row as f32 * th - r.h;
                    contacts.ground = true;
                }
                None => r.y += delta.y,
            }
        } else {
            let first = ((r.y + EPS) / th).floor() as i32 - 1;
            let last = ((r.y + delta.y) / th).floor() as i32;
            let blocked = |row: i32| (col0..=col1).any(|c| map.is_solid(c, row));
            match (last..=first).rev().find(|&row| blocked(row)) {
                Some(row) => {
                    r.y = (row + 1) as f32 * th;
                    contacts.ceiling = true;
                }
                None => r.y += delta.y,
            }
        }
    }

    MoveResult { rect: r, contacts }
}

/// What the box would touch if it moved a hair in each direction (for
/// standing checks without gravity, wall jumps, coyote time)
pub fn touching(map: &Tilemap, rect: Rect) -> Contacts {
    const PROBE: f32 = 0.01;
    let probe = |d: Vec2| move_box(map, rect, d).contacts;
    Contacts {
        ground: probe(Vec2::new(0.0, PROBE)).ground,
        ceiling: probe(Vec2::new(0.0, -PROBE)).ceiling,
        wall_left: probe(Vec2::new(-PROBE, 0.0)).wall_left,
        wall_right: probe(Vec2::new(PROBE, 0.0)).wall_right,
    }
}

/// Whether a world point is inside a solid cell
pub fn point_solid(map: &Tilemap, p: Vec2) -> bool {
    let (x, y) = map.tile_at(p);
    map.is_solid(x, y)
}

/// Whether a rectangle overlaps any solid cell (touching does not count)
pub fn rect_solid(map: &Tilemap, rect: Rect) -> bool {
    let (tw, th) = (map.tile_w.max(1) as f32, map.tile_h.max(1) as f32);
    let (x0, x1) = cells(rect.x + EPS, rect.right(), tw);
    let (y0, y1) = cells(rect.y + EPS, rect.bottom(), th);
    (y0..=y1).any(|y| (x0..=x1).any(|x| map.is_solid(x, y)))
}

/// The first solid cell on the segment from `from` to `to`
pub fn raycast(map: &Tilemap, from: Vec2, to: Vec2) -> Option<RayHit> {
    // Walk the grid cell by cell in tile space (Amanatides & Woo). The
    // world <-> tile mapping is linear, so the ray's parameter t is the
    // same in both spaces.
    let a = map.world_to_tile(from);
    let b = map.world_to_tile(to);
    let dir = Vec2::new(b.x - a.x, b.y - a.y);
    let length = ((to.x - from.x).powi(2) + (to.y - from.y).powi(2)).sqrt();
    let (mut cx, mut cy) = (a.x.floor() as i32, a.y.floor() as i32);
    let hit = |cx: i32, cy: i32, t: f32, normal: Vec2| RayHit {
        point: Vec2::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t),
        cell_x: cx,
        cell_y: cy,
        normal,
        distance: length * t,
    };
    if map.is_solid(cx, cy) {
        return Some(hit(cx, cy, 0.0, Vec2::default()));
    }
    let axis = |pos: f32, cell: i32, d: f32| -> (i32, f32, f32) {
        if d > 0.0 {
            (1, (cell as f32 + 1.0 - pos) / d, 1.0 / d)
        } else if d < 0.0 {
            (-1, (pos - cell as f32) / -d, -1.0 / d)
        } else {
            (0, f32::INFINITY, f32::INFINITY)
        }
    };
    let (sx, mut tx, dx) = axis(a.x, cx, dir.x);
    let (sy, mut ty, dy) = axis(a.y, cy, dir.y);
    let steps = (dir.x.abs() + dir.y.abs()).ceil() as usize + 2;
    for _ in 0..steps {
        let (t, normal) = if tx < ty {
            cx += sx;
            let t = tx;
            tx += dx;
            (t, Vec2::new(-sx as f32, 0.0))
        } else {
            cy += sy;
            let t = ty;
            ty += dy;
            (t, Vec2::new(0.0, -sy as f32))
        };
        if t > 1.0 {
            return None;
        }
        if map.is_solid(cx, cy) {
            return Some(hit(cx, cy, t, normal));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::SpriteSheet;
    use crate::tilemap::{Orientation, Properties, Property, Tileset};

    /// 10x10 map of 16px cells: floor on row 5, a wall in column 7,
    /// a one-way platform at (2, 3)
    fn level(orientation: Orientation) -> Tilemap {
        let mut map = Tilemap::new(orientation, 10, 10, 16, 16);
        let mut set = Tileset::new(1, 0, SpriteSheet::grid(32, 16, 16, 16, 0, 0));
        let flag = |name: &str| Properties::from([(name.to_string(), Property::Bool(true))]);
        set.tile_properties.insert(0, flag("solid"));
        set.tile_properties.insert(1, flag("platform"));
        map.add_tileset(set);
        let l = map.add_layer("walls");
        for x in 0..10 {
            map.layers[l].set(x, 5, 1);
        }
        for y in 0..5 {
            map.layers[l].set(7, y, 1);
        }
        map.layers[l].set(2, 3, 2);
        map
    }

    #[test]
    fn falls_lands_and_slides_into_walls() {
        let map = level(Orientation::Orthogonal);
        let fall = move_box(&map, Rect::new(52.0, 20.0, 8.0, 8.0), Vec2::new(3.0, 100.0));
        assert_eq!(fall.rect, Rect::new(55.0, 72.0, 8.0, 8.0));
        assert!(fall.contacts.ground && !fall.contacts.wall_right);
        // Resting exactly on the floor, walking does not snag on it
        let walk = move_box(&map, fall.rect, Vec2::new(200.0, 0.0));
        assert_eq!(walk.rect.right(), 112.0);
        assert!(walk.contacts.wall_right && !walk.contacts.ground);
        let back = move_box(&map, walk.rect, Vec2::new(-50.0, 0.0));
        assert_eq!(back.rect.x, 54.0);
        assert_eq!(back.contacts, Contacts::default());
        // Diagonal into the corner: both flags, both axes clamped
        let corner = move_box(&map, Rect::new(90.0, 60.0, 8.0, 8.0), Vec2::new(30.0, 30.0));
        assert_eq!((corner.rect.right(), corner.rect.bottom()), (112.0, 80.0));
        assert!(corner.contacts.wall_right && corner.contacts.ground);
        assert_eq!(
            touching(&map, corner.rect),
            Contacts {
                ground: true,
                wall_right: true,
                ..Contacts::default()
            }
        );
    }

    #[test]
    fn fast_boxes_do_not_tunnel_and_ceilings_stop_jumps() {
        let map = level(Orientation::Orthogonal);
        let fast = move_box(&map, Rect::new(4.0, 0.0, 4.0, 4.0), Vec2::new(0.0, 5000.0));
        assert_eq!(fast.rect.bottom(), 80.0);
        let wall = move_box(&map, Rect::new(0.0, 40.0, 4.0, 4.0), Vec2::new(9000.0, 0.0));
        assert_eq!(wall.rect.right(), 112.0);
        // Under the floor, jumping up hits its underside
        let jump = move_box(&map, Rect::new(4.0, 100.0, 8.0, 8.0), Vec2::new(0.0, -50.0));
        assert_eq!(jump.rect.y, 96.0);
        assert!(jump.contacts.ceiling);
    }

    #[test]
    fn one_way_platforms() {
        let map = level(Orientation::Orthogonal);
        // Platform cell (2, 3) spans x 32..48, y 48..64
        let land = move_box(&map, Rect::new(36.0, 30.0, 8.0, 8.0), Vec2::new(0.0, 20.0));
        assert_eq!(land.rect.bottom(), 48.0);
        assert!(land.contacts.ground);
        let up = move_box(&map, Rect::new(36.0, 66.0, 8.0, 8.0), Vec2::new(0.0, -30.0));
        assert_eq!(up.rect.y, 36.0, "jumps through from below");
        let side = move_box(&map, Rect::new(20.0, 52.0, 8.0, 8.0), Vec2::new(30.0, 0.0));
        assert_eq!(side.rect.x, 50.0, "walks through sideways");
        // Already halfway inside (mid-jump), falling continues through
        let inside = move_box(&map, Rect::new(36.0, 50.0, 8.0, 8.0), Vec2::new(0.0, 4.0));
        assert_eq!(inside.rect.y, 54.0);
    }

    #[test]
    fn points_rects_and_rays() {
        let map = level(Orientation::Orthogonal);
        assert!(point_solid(&map, Vec2::new(5.0, 85.0)));
        assert!(!point_solid(&map, Vec2::new(5.0, 79.9)));
        assert!(
            !rect_solid(&map, Rect::new(0.0, 72.0, 8.0, 8.0)),
            "touching"
        );
        assert!(rect_solid(&map, Rect::new(0.0, 73.0, 8.0, 8.0)));

        let hit = raycast(&map, Vec2::new(10.0, 40.0), Vec2::new(150.0, 40.0)).unwrap();
        assert_eq!((hit.cell_x, hit.cell_y), (7, 2));
        assert!((hit.point.x - 112.0).abs() < 1e-3 && (hit.distance - 102.0).abs() < 1e-3);
        assert_eq!(hit.normal, Vec2::new(-1.0, 0.0));
        let down = raycast(&map, Vec2::new(10.0, 10.0), Vec2::new(10.0, 300.0)).unwrap();
        assert!((down.point.y - 80.0).abs() < 1e-3);
        assert_eq!(down.normal, Vec2::new(0.0, -1.0));
        assert_eq!(
            raycast(&map, Vec2::new(10.0, 40.0), Vec2::new(100.0, 40.0)),
            None
        );
        let inside = raycast(&map, Vec2::new(10.0, 85.0), Vec2::new(0.0, 0.0)).unwrap();
        assert_eq!(inside.distance, 0.0);
    }

    #[test]
    fn rays_on_isometric_maps() {
        let map = level(Orientation::Isometric);
        // Along the cell x axis on row 2: the wall is at x = 7
        let from = map.cell_center(0, 2);
        let to = map.cell_center(9, 2);
        let hit = raycast(&map, from, to).unwrap();
        assert_eq!((hit.cell_x, hit.cell_y), (7, 2));
        let t = map.world_to_tile(hit.point);
        assert!((t.x - 7.0).abs() < 1e-3, "{:?}", t);
        assert!(point_solid(&map, map.cell_center(7, 2)));
    }
}

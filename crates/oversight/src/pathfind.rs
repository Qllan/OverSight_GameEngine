//! Paths over a tile map with A*. Solid cells block; a tile (or its layer)
//! with a number property `cost` is that much slower to cross (a swamp of
//! cost 3 is worth a detour of up to three cells), and cells outside the
//! map block. Diagonal steps never cut a wall's corner. Ties are broken
//! the same way every time, so every machine finds the same path.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::tilemap::Tilemap;
use crate::types::Vec2;

const DIAGONAL: f32 = std::f32::consts::SQRT_2;

/// The cost to enter each cell; None for blocked ones. Same rules as
/// `Tilemap::is_solid`, worked out once per tile id instead of per cell.
fn costs(map: &Tilemap) -> Vec<Option<f32>> {
    use crate::tilemap::{Property, GID_MASK};
    let number = |p: Option<&Property>| p.and_then(|p| p.as_f64()).map(|v| v as f32);
    let flag = |p: Option<&Property>| p.is_some_and(Property::truthy);
    // Per tile id: (solid, cost)
    let mut memo: Vec<Option<(bool, Option<f32>)>> = Vec::new();
    let mut tile = |gid: u32| {
        let id = (gid & GID_MASK) as usize;
        if memo.len() <= id {
            memo.resize(id + 1, None);
        }
        *memo[id].get_or_insert_with(|| {
            let p = map.tile_properties(gid);
            (
                flag(p.and_then(|p| p.get("solid"))),
                number(p.and_then(|p| p.get("cost"))),
            )
        })
    };
    let layers: Vec<(bool, Option<f32>)> = map
        .layers
        .iter()
        .map(|l| {
            (
                flag(l.properties.get("solid")),
                number(l.properties.get("cost")),
            )
        })
        .collect();
    let (w, h) = (map.width as i32, map.height as i32);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut cost = Some(1.0f32);
            for (layer, &(layer_solid, layer_cost)) in map.layers.iter().zip(&layers) {
                let gid = layer.get(x, y);
                if gid == 0 {
                    continue;
                }
                let (solid, own_cost) = tile(gid);
                if solid || layer_solid {
                    cost = None;
                    break;
                }
                if let (Some(c), Some(v)) = (cost.as_mut(), own_cost.or(layer_cost)) {
                    *c = c.max(v);
                }
            }
            out.push(cost);
        }
    }
    out
}

/// Cells from `from` to `to` (both included), or None when there is no
/// way. `max_cost` stops the search early (0: no limit).
pub fn find_path(
    map: &Tilemap,
    from: (i32, i32),
    to: (i32, i32),
    diagonal: bool,
    max_cost: f32,
) -> Option<Vec<(i32, i32)>> {
    let (w, h) = (map.width as i32, map.height as i32);
    let inside = |(x, y): (i32, i32)| x >= 0 && y >= 0 && x < w && y < h;
    if !inside(from) || !inside(to) {
        return None;
    }
    let costs = costs(map);
    let index = |(x, y): (i32, i32)| (y * w + x) as usize;
    let cost_of = |c: (i32, i32)| if inside(c) { costs[index(c)] } else { None };
    cost_of(to)?;
    // Octile distance, the cheapest any way can be
    let guess = |(x, y): (i32, i32)| {
        let (dx, dy) = ((x - to.0).abs() as f32, (y - to.1).abs() as f32);
        if diagonal {
            dx.max(dy) + (DIAGONAL - 1.0) * dx.min(dy)
        } else {
            dx + dy
        }
    };
    let n = (w * h) as usize;
    let mut best = vec![f32::INFINITY; n];
    let mut came = vec![u32::MAX; n];
    let mut done = vec![false; n];
    let mut open = BinaryHeap::new();
    best[index(from)] = 0.0;
    // Lowest estimate first; then the one nearer the goal; then the cell
    // index, so equal paths always come out the same
    let key =
        |f: f32, g: f32, i: usize| Reverse(((f * 1024.0) as u64, ((f - g) * 1024.0) as u64, i));
    open.push(key(guess(from), 0.0, index(from)));
    const STEPS: [(i32, i32); 8] = [
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (-1, 1),
        (1, -1),
        (-1, -1),
    ];
    while let Some(Reverse((_, _, i))) = open.pop() {
        if std::mem::replace(&mut done[i], true) {
            continue; // an older, longer way to a cell already finished
        }
        let cell = ((i as i32) % w, (i as i32) / w);
        if cell == to {
            let mut path = vec![to];
            let mut at = i;
            while came[at] != u32::MAX {
                at = came[at] as usize;
                path.push(((at as i32) % w, (at as i32) / w));
            }
            path.reverse();
            return Some(path);
        }
        let g = best[i];
        for &(dx, dy) in &STEPS[..if diagonal { 8 } else { 4 }] {
            let next = (cell.0 + dx, cell.1 + dy);
            let Some(enter) = cost_of(next) else {
                continue;
            };
            let diag = dx != 0 && dy != 0;
            // No squeezing between two walls' corners
            if diag
                && (cost_of((cell.0 + dx, cell.1)).is_none()
                    || cost_of((cell.0, cell.1 + dy)).is_none())
            {
                continue;
            }
            let ng = g + enter * if diag { DIAGONAL } else { 1.0 };
            if max_cost > 0.0 && ng > max_cost {
                continue;
            }
            let j = index(next);
            if ng < best[j] {
                best[j] = ng;
                came[j] = i as u32;
                open.push(key(ng + guess(next), ng, j));
            }
        }
    }
    None
}

/// The same in world units: from the cell centre of `from` to that of `to`
pub fn find_world_path(
    map: &Tilemap,
    from: Vec2,
    to: Vec2,
    diagonal: bool,
    max_cost: f32,
) -> Option<Vec<Vec2>> {
    let path = find_path(map, map.tile_at(from), map.tile_at(to), diagonal, max_cost)?;
    Some(
        path.into_iter()
            .map(|(x, y)| map.cell_center(x, y))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::SpriteSheet;
    use crate::tilemap::{Orientation, Property, Tileset};

    /// '#' wall, '~' swamp (cost 5), '.' floor
    fn map(rows: &[&str]) -> Tilemap {
        let (w, h) = (rows[0].len() as u32, rows.len() as u32);
        let mut m = Tilemap::new(Orientation::Orthogonal, w, h, 8, 8);
        let mut ts = Tileset::new(1, 0, SpriteSheet::grid(16, 8, 8, 8, 0, 0));
        ts.tile_properties
            .entry(0)
            .or_default()
            .insert("solid".into(), Property::Bool(true));
        ts.tile_properties
            .entry(1)
            .or_default()
            .insert("cost".into(), Property::Float(5.0));
        m.add_tileset(ts);
        let l = m.add_layer("ground");
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                let gid = match ch {
                    '#' => 1,
                    '~' => 2,
                    _ => 0,
                };
                m.layers[l].set(x as i32, y as i32, gid);
            }
        }
        m
    }

    #[test]
    fn goes_around_walls() {
        let m = map(&["....", ".##.", "...."]);
        let p = find_path(&m, (0, 1), (3, 1), false, 0.0).unwrap();
        assert_eq!(p.first(), Some(&(0, 1)));
        assert_eq!(p.last(), Some(&(3, 1)));
        assert_eq!(p.len(), 6, "{p:?}");
        assert!(p
            .windows(2)
            .all(|s| (s[0].0 - s[1].0).abs() + (s[0].1 - s[1].1).abs() == 1));
        assert_eq!(
            find_path(&m, (0, 1), (3, 1), false, 0.0),
            Some(p),
            "the same path every time"
        );
    }

    #[test]
    fn diagonals_do_not_cut_corners() {
        let m = map(&["..", "#."]);
        // (0,0) -> (1,1): the diagonal passes the wall's corner at (0,1)
        let p = find_path(&m, (0, 0), (1, 1), true, 0.0).unwrap();
        assert_eq!(p, vec![(0, 0), (1, 0), (1, 1)]);
        let open = map(&["...", "...", "..."]);
        assert_eq!(
            find_path(&open, (0, 0), (2, 2), true, 0.0).unwrap().len(),
            3
        );
    }

    #[test]
    fn slow_ground_is_avoided_when_a_detour_is_cheaper() {
        let m = map(&[".~.", "...", "..."]);
        let p = find_path(&m, (0, 0), (2, 0), false, 0.0).unwrap();
        assert!(!p.contains(&(1, 0)), "around the swamp: {p:?}");
        let narrow = map(&["#.#", ".~.", "#.#"]);
        assert!(
            find_path(&narrow, (0, 1), (2, 1), false, 0.0)
                .unwrap()
                .contains(&(1, 1)),
            "no other way"
        );
    }

    #[test]
    fn no_way_and_limits() {
        let m = map(&[".#.", ".#.", ".#."]);
        assert_eq!(find_path(&m, (0, 0), (2, 0), true, 0.0), None);
        assert_eq!(
            find_path(&m, (0, 0), (1, 0), true, 0.0),
            None,
            "the goal is a wall"
        );
        assert_eq!(find_path(&m, (0, 0), (9, 0), true, 0.0), None, "outside");
        let open = map(&["......"]);
        assert!(
            find_path(&open, (0, 0), (5, 0), false, 3.0).is_none(),
            "costs more than 3"
        );
        let w =
            find_world_path(&open, Vec2::new(1.0, 1.0), Vec2::new(20.0, 2.0), false, 0.0).unwrap();
        assert_eq!(
            w,
            vec![
                Vec2::new(4.0, 4.0),
                Vec2::new(12.0, 4.0),
                Vec2::new(20.0, 4.0)
            ]
        );
    }
}

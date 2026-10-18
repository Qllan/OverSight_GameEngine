//! Undo and redo by snapshots of the whole world and of the tiles of every
//! map. Edited scenes are small, and a snapshot can't miss a change the
//! way per-edit inverses can.

use orst::Engine;

const STEPS: usize = 200;

#[derive(Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// A gesture (a drag, typing in a field) is going on: its changes
    /// become one step
    open: bool,
}

impl History {
    /// Call before changing the world. Inside an open gesture it does
    /// nothing, so the whole gesture undoes at once.
    pub fn record(&mut self, engine: &Engine) {
        if self.open {
            return;
        }
        self.undo.push(save(engine));
        if self.undo.len() > STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.open = true;
    }

    /// The gesture ended (the button was let go, the field lost focus)
    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, engine: &mut Engine) -> Result<bool, String> {
        self.open = false;
        let Some(before) = self.undo.pop() else {
            return Ok(false);
        };
        self.redo.push(save(engine));
        load(engine, &before)?;
        Ok(true)
    }

    pub fn redo(&mut self, engine: &mut Engine) -> Result<bool, String> {
        self.open = false;
        let Some(after) = self.redo.pop() else {
            return Ok(false);
        };
        self.undo.push(save(engine));
        load(engine, &after)?;
        Ok(true)
    }
}

struct Snapshot {
    world: Vec<u8>,
    /// The tiles of each layer of each map, by map id
    tiles: Vec<(u32, Vec<Vec<u32>>)>,
}

pub fn map_tiles(engine: &Engine, map: u32) -> Option<Vec<Vec<u32>>> {
    let m = engine.tilemap(map)?;
    Some(m.layers.iter().map(|l| l.tiles.clone()).collect())
}

fn save(engine: &Engine) -> Snapshot {
    let mut world = Vec::new();
    engine.world.save(&mut world);
    let tiles = engine
        .tilemaps
        .ids()
        .filter_map(|id| Some((id, map_tiles(engine, id)?)))
        .collect();
    Snapshot { world, tiles }
}

fn load(engine: &mut Engine, snapshot: &Snapshot) -> Result<(), String> {
    engine.world.load(&snapshot.world)?;
    for (id, tiles) in &snapshot.tiles {
        let Some(map) = engine.tilemap_mut(*id) else {
            continue;
        };
        for (layer, tiles) in map.layers.iter_mut().zip(tiles) {
            if layer.tiles.len() == tiles.len() {
                layer.tiles.clone_from(tiles);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use orst::components::{read, write, Transform, TRANSFORM};
    use orst::ecs::World;

    fn x(world: &World, e: u64) -> f32 {
        read::<Transform>(world, e, TRANSFORM).unwrap().pos.x
    }

    fn set_x(world: &mut World, e: u64, v: f32) {
        let mut t: Transform = read(world, e, TRANSFORM).unwrap();
        t.pos.x = v;
        write(world, e, TRANSFORM, &t);
    }

    #[test]
    fn a_gesture_undoes_in_one_step() {
        let mut engine = Engine::headless(8, 8).unwrap();
        let mut h = History::default();
        h.record(&engine);
        let e = engine.world.spawn();
        write(&mut engine.world, e, TRANSFORM, &Transform::default());
        h.close();

        // A drag: many changes, one step
        for v in 1..=5 {
            h.record(&engine);
            set_x(&mut engine.world, e, v as f32);
        }
        h.close();
        h.record(&engine);
        set_x(&mut engine.world, e, 9.0);
        h.close();

        assert!(h.undo(&mut engine).unwrap());
        assert_eq!(x(&engine.world, e), 5.0);
        assert!(h.undo(&mut engine).unwrap());
        assert_eq!(x(&engine.world, e), 0.0, "the drag went at once");
        assert!(h.redo(&mut engine).unwrap());
        assert_eq!(x(&engine.world, e), 5.0);
        assert!(h.undo(&mut engine).unwrap() && h.undo(&mut engine).unwrap());
        assert!(!engine.world.alive(e), "back before the entity was made");
        assert!(!h.undo(&mut engine).unwrap());

        // A new change after undoing drops what could be redone
        h.record(&engine);
        engine.world.spawn();
        h.close();
        assert!(!h.can_redo());
    }

    #[test]
    fn painted_tiles_undo() {
        use orst::tilemap::{Orientation, Tilemap};
        let mut engine = Engine::headless(8, 8).unwrap();
        let mut map = Tilemap::new(Orientation::Orthogonal, 4, 4, 8, 8);
        map.add_layer("Ground");
        let id = engine.add_tilemap(map);
        let mut h = History::default();
        h.record(&engine);
        engine.tilemap_mut(id).unwrap().layers[0].set(1, 2, 5);
        h.close();
        assert!(h.undo(&mut engine).unwrap());
        assert_eq!(engine.tilemap(id).unwrap().layers[0].get(1, 2), 0);
        assert!(h.redo(&mut engine).unwrap());
        assert_eq!(engine.tilemap(id).unwrap().layers[0].get(1, 2), 5);
    }
}

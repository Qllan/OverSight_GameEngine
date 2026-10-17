//! Undo and redo by snapshots of the whole world. Edited scenes are small,
//! and a snapshot can't miss a change the way per-edit inverses can.

use orst::ecs::World;

const STEPS: usize = 200;

#[derive(Default)]
pub struct History {
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
    /// A gesture (a drag, typing in a field) is going on: its changes
    /// become one step
    open: bool,
}

impl History {
    /// Call before changing the world. Inside an open gesture it does
    /// nothing, so the whole gesture undoes at once.
    pub fn record(&mut self, world: &World) {
        if self.open {
            return;
        }
        self.undo.push(save(world));
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

    pub fn undo(&mut self, world: &mut World) -> Result<bool, String> {
        self.open = false;
        let Some(before) = self.undo.pop() else {
            return Ok(false);
        };
        self.redo.push(save(world));
        world.load(&before)?;
        Ok(true)
    }

    pub fn redo(&mut self, world: &mut World) -> Result<bool, String> {
        self.open = false;
        let Some(after) = self.redo.pop() else {
            return Ok(false);
        };
        self.undo.push(save(world));
        world.load(&after)?;
        Ok(true)
    }
}

fn save(world: &World) -> Vec<u8> {
    let mut out = Vec::new();
    world.save(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use orst::components::{read, write, Transform, TRANSFORM};
    use orst::Engine;

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
        let world = &mut engine.world;
        let mut h = History::default();
        h.record(world);
        let e = world.spawn();
        write(world, e, TRANSFORM, &Transform::default());
        h.close();

        // A drag: many changes, one step
        for v in 1..=5 {
            h.record(world);
            set_x(world, e, v as f32);
        }
        h.close();
        h.record(world);
        set_x(world, e, 9.0);
        h.close();

        assert!(h.undo(world).unwrap());
        assert_eq!(x(world, e), 5.0);
        assert!(h.undo(world).unwrap());
        assert_eq!(x(world, e), 0.0, "the drag went at once");
        assert!(h.redo(world).unwrap());
        assert_eq!(x(world, e), 5.0);
        assert!(h.undo(world).unwrap() && h.undo(world).unwrap());
        assert!(!world.alive(e), "back before the entity was made");
        assert!(!h.undo(world).unwrap());

        // A new change after undoing drops what could be redone
        h.record(world);
        world.spawn();
        h.close();
        assert!(!h.can_redo());
    }
}

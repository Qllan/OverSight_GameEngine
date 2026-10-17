//! Play inside the editor: the edited world runs as the game would, in
//! fixed ticks of Engine::step, and Stop puts it back exactly as it was
//! (a snapshot of the whole engine state, physics included). What
//! changes while playing is thrown away, as in Unity.

use crate::history::History;
use crate::scripts::Scripts;
use orst::ecs::Entity;
use orst::{Camera, Engine};

/// Seconds per tick; games run their logic at the same rate
pub const TICK: f32 = 1.0 / 60.0;
/// Ticks one frame may run at most, so a slow frame does not snowball
const MAX_TICKS: u32 = 8;

pub struct Play {
    /// The engine state when Play was pressed
    before: Vec<u8>,
    /// The editor's undo history, scene state and selection, put back on Stop
    history: History,
    dirty: bool,
    selected: Option<Entity>,
    pub paused: bool,
    /// Steps asked for while paused
    steps: u32,
    /// Real time not yet run as ticks
    owed: f32,
    /// The game's camera; the editor's own stays in the scene view
    pub camera: Camera,
    pub ticks: u64,
}

impl Play {
    pub fn start(
        engine: &mut Engine,
        history: History,
        dirty: bool,
        selected: Option<Entity>,
    ) -> Play {
        let before = engine.snapshot();
        engine.entity_emitters.clear();
        engine.input.release_all();
        engine.input.next_frame();
        Play {
            before,
            history,
            dirty,
            selected,
            paused: false,
            steps: 0,
            owed: 0.0,
            camera: game_camera(engine),
            ticks: 0,
        }
    }

    /// Run the ticks that `real_dt` seconds of real time are worth (or the
    /// steps asked for while paused); returns how many ran. Scripts update
    /// before the engine moves things, each tick.
    pub fn advance(
        &mut self,
        engine: &mut Engine,
        mut scripts: Option<&mut Scripts>,
        real_dt: f32,
    ) -> u32 {
        let mut n = 0;
        if self.paused {
            n = std::mem::take(&mut self.steps).min(MAX_TICKS);
        } else {
            self.owed += real_dt.clamp(0.0, 0.25);
            while self.owed >= TICK && n < MAX_TICKS {
                self.owed -= TICK;
                n += 1;
            }
            if n == MAX_TICKS {
                self.owed = 0.0;
            }
        }
        for _ in 0..n {
            engine.actions.update(&engine.input);
            engine.set_camera(self.camera);
            if let Some(s) = scripts.as_deref_mut() {
                s.tick(TICK);
            }
            engine.step(TICK);
            engine.time.tick_count += 1;
            // Presses and releases count for one tick
            engine.input.next_frame();
            self.camera = engine.camera();
            self.ticks += 1;
        }
        n
    }

    /// Pause and run exactly one tick
    pub fn step_once(&mut self) {
        self.paused = true;
        self.steps += 1;
    }

    /// Put the world back as it was at Play; gives back the editor's
    /// history, unsaved flag and selection
    pub fn stop(self, engine: &mut Engine) -> (Result<(), String>, History, bool, Option<Entity>) {
        let restored = engine.restore(&self.before);
        engine.entity_emitters.clear();
        engine.input.release_all();
        engine.input.next_frame();
        let selected = self.selected.filter(|&e| engine.world.alive(e));
        (restored, self.history, self.dirty, selected)
    }
}

/// The camera the game starts with: the current Camera2D, or the origin
/// at zoom 1 without one
pub fn game_camera(engine: &mut Engine) -> Camera {
    let editor = engine.camera();
    engine.set_camera(Camera::default());
    engine.follow_camera();
    let game = engine.camera();
    engine.set_camera(editor);
    game
}

#[cfg(test)]
mod tests {
    use super::*;
    use orst::components::{
        read, write, Camera2D, Transform, Velocity, CAMERA_2D, TRANSFORM, VELOCITY,
    };
    use orst::Vec2;

    #[test]
    fn play_runs_ticks_and_stop_puts_the_world_back() {
        let mut engine = Engine::headless(16, 16).unwrap();
        let e = engine.world.spawn();
        write(&mut engine.world, e, TRANSFORM, &Transform::default());
        let v = Velocity {
            linear: Vec2::new(60.0, 0.0),
            ..Default::default()
        };
        write(&mut engine.world, e, VELOCITY, &v);
        write(&mut engine.world, e, CAMERA_2D, &Camera2D::default());
        let x = |engine: &Engine| {
            read::<Transform>(&engine.world, e, TRANSFORM)
                .unwrap()
                .pos
                .x
        };

        let mut play = Play::start(&mut engine, History::default(), true, Some(e));
        assert_eq!(
            play.advance(&mut engine, None, 0.5),
            8,
            "at most 8 ticks a frame"
        );
        assert!((x(&engine) - 8.0).abs() < 1e-3);
        assert_eq!(play.camera.center.x, x(&engine), "the camera follows");
        assert_eq!(
            play.advance(&mut engine, None, 0.01),
            0,
            "the rest of a slow frame is dropped"
        );
        assert_eq!(play.advance(&mut engine, None, 0.01), 1);

        play.step_once();
        assert_eq!(
            play.advance(&mut engine, None, 1.0),
            1,
            "paused: only the step"
        );
        assert_eq!(play.advance(&mut engine, None, 1.0), 0);
        let spawned = engine.world.spawn();

        let (restored, _, dirty, selected) = play.stop(&mut engine);
        restored.unwrap();
        assert_eq!(x(&engine), 0.0);
        assert!(!engine.world.alive(spawned) && dirty && selected == Some(e));
    }
}

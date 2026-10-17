//! Physics when the engine is built without the `physics` feature: the
//! same calls, and every one says there is no physics.

use crate::ecs::World;
use crate::texture::Store;
use crate::types::Vec2;

const OFF: &str = "this engine is built without physics";

#[derive(Default)]
pub struct Physics;

impl Physics {
    pub fn set_gravity(&mut self, _g: Vec2) {}
    pub fn gravity(&self) -> Vec2 {
        Vec2::default()
    }
    pub fn set_scale(&mut self, _units_per_metre: f32) {}
    pub fn body_count(&self) -> usize {
        0
    }
    pub fn default_like(_other: &Physics) -> Physics {
        Physics
    }
    pub fn sync(&mut self, _ecs: &World, _polygons: &Store<Vec<Vec2>>, _dt: f32) {}
    pub fn step(&mut self, _ecs: &mut World, _polygons: &Store<Vec<Vec2>>, _dt: f32) {}
    pub fn apply_impulse(&mut self, _ent: u64, _impulse: Vec2) -> bool {
        false
    }
    pub fn apply_force(&mut self, _ent: u64, _force: Vec2) -> bool {
        false
    }
    pub fn apply_torque_impulse(&mut self, _ent: u64, _torque: f32) -> bool {
        false
    }
    pub fn mass(&self, _ent: u64) -> f32 {
        0.0
    }
    pub fn pin(&mut self, _a: u64, _b: u64, _at: Vec2) -> Option<u32> {
        None
    }
    pub fn weld(&mut self, _a: u64, _b: u64) -> Option<u32> {
        None
    }
    pub fn rope(&mut self, _a: u64, _b: u64, _length: f32) -> Option<u32> {
        None
    }
    pub fn spring(
        &mut self,
        _a: u64,
        _b: u64,
        _rest: f32,
        _stiffness: f32,
        _damping: f32,
    ) -> Option<u32> {
        None
    }
    pub fn remove_joint(&mut self, _id: u32) -> bool {
        false
    }
    pub fn save(&self) -> Vec<u8> {
        Vec::new()
    }
    pub fn load(_bytes: &[u8]) -> Result<Physics, String> {
        Err(format!("snapshot: it has physics, and {OFF}"))
    }
    /// Why the calls above do nothing
    pub fn missing() -> Option<&'static str> {
        Some(OFF)
    }
}

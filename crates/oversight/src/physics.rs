//! Bodies with mass, friction and joints, simulated by Rapier in its
//! deterministic mode. An entity with a Transform, a Collider and a
//! RigidBody gets a body; the components stay the way the game talks to
//! it: moving the Transform or setting the Velocity moves the body, and
//! after each step the body's place and speed are written back. Units are
//! the game's own (pixels); `scale` tells Rapier how many make a metre so
//! its tolerances fit.

use std::collections::BTreeMap;

use rapier2d::prelude::{
    ColliderBuilder, FixedJointBuilder, Group, ImpulseJointHandle, InteractionGroups,
    InteractionTestMode, PhysicsWorld, Pose, RevoluteJointBuilder, RigidBodyBuilder,
    RigidBodyHandle, RigidBodyType, RopeJointBuilder, SharedShape, SpringJointBuilder, Vector,
};
use serde::{Deserialize, Serialize};

use crate::components::{
    read, write, Collider, RigidBody, Transform, Velocity, BODY_FIXED, BODY_KINEMATIC, COLLIDER,
    COLLIDER_CIRCLE, COLLIDER_POLYGON, RIGIDBODY, TRANSFORM, VELOCITY,
};
use crate::ecs::World;
use crate::texture::Store;
use crate::types::Vec2;

fn v(p: Vec2) -> Vector {
    Vector::new(p.x, p.y)
}

fn bytes<T: Copy>(value: &T) -> Vec<u8> {
    // Safety: plain repr(C) components
    unsafe { std::slice::from_raw_parts(value as *const T as *const u8, std::mem::size_of::<T>()) }
        .to_vec()
}

/// What the engine last saw of an entity's body
#[derive(Serialize, Deserialize)]
struct Tracked {
    body: (u32, u32),
    collider: (u32, u32),
    /// The RigidBody, Collider and scale the body was built from
    desc: Vec<u8>,
    /// Place and speed last written to (or read from) the components;
    /// anything else means the game moved the entity
    pose: (f32, f32, f32),
    vel: (f32, f32, f32),
}

#[derive(Serialize, Deserialize)]
pub struct Physics {
    world: PhysicsWorld,
    entities: BTreeMap<u64, Tracked>,
    joints: BTreeMap<u32, (u32, u32)>,
    next_joint: u32,
}

impl Default for Physics {
    fn default() -> Self {
        let mut world = PhysicsWorld::new();
        world.gravity = Vector::new(0.0, 0.0);
        world.integration_parameters.length_unit = 32.0;
        Physics {
            world,
            entities: BTreeMap::new(),
            joints: BTreeMap::new(),
            next_joint: 1,
        }
    }
}

fn handle(raw: (u32, u32)) -> RigidBodyHandle {
    RigidBodyHandle::from_raw_parts(raw.0, raw.1)
}

/// The shape for Rapier, scaled; None for a missing or broken polygon
fn shape(c: &Collider, t: &Transform, polygons: &Store<Vec<Vec2>>) -> Option<SharedShape> {
    let (sx, sy) = (t.scale.x.abs(), t.scale.y.abs());
    match c.kind {
        COLLIDER_CIRCLE => Some(SharedShape::ball((c.radius * sx.max(sy)).max(1e-4))),
        COLLIDER_POLYGON => {
            let pts: Vec<Vector> = polygons
                .get(c.polygon)?
                .iter()
                .map(|p| Vector::new(p.x * t.scale.x, p.y * t.scale.y))
                .collect();
            SharedShape::convex_hull(&pts)
        }
        _ => Some(SharedShape::cuboid(
            (c.size.x * sx * 0.5).max(1e-4),
            (c.size.y * sy * 0.5).max(1e-4),
        )),
    }
}

fn body_type(kind: u32) -> RigidBodyType {
    match kind {
        BODY_FIXED => RigidBodyType::Fixed,
        BODY_KINEMATIC => RigidBodyType::KinematicPositionBased,
        _ => RigidBodyType::Dynamic,
    }
}

fn groups(c: &Collider) -> InteractionGroups {
    InteractionGroups::new(
        Group::from_bits_retain(c.layer),
        Group::from_bits_retain(c.mask),
        InteractionTestMode::And,
    )
}

/// Collider offset in the body's frame, scaled like the shape
fn offset(c: &Collider, t: &Transform) -> Pose {
    Pose::from_translation(Vector::new(c.offset.x * t.scale.x, c.offset.y * t.scale.y))
}

impl Physics {
    /// Gravity in world units per second squared (y down); default none,
    /// for top-down games
    pub fn set_gravity(&mut self, g: Vec2) {
        self.world.gravity = v(g);
    }

    pub fn gravity(&self) -> Vec2 {
        Vec2::new(self.world.gravity.x, self.world.gravity.y)
    }

    /// World units in a metre (default 32), so Rapier's tolerances fit
    pub fn set_scale(&mut self, units_per_metre: f32) {
        self.world.integration_parameters.length_unit = units_per_metre.max(1e-3);
    }

    /// No bodies, with the same gravity and scale (restoring a snapshot
    /// taken before any body existed)
    pub fn default_like(other: &Physics) -> Physics {
        let mut p = Physics::default();
        p.world.gravity = other.world.gravity;
        p.world.integration_parameters.length_unit = other.world.integration_parameters.length_unit;
        p
    }

    pub fn body_count(&self) -> usize {
        self.entities.len()
    }

    /// Make, update and remove bodies to match the components. `dt` moves
    /// kinematic bodies by their Velocity.
    pub fn sync(&mut self, ecs: &World, polygons: &Store<Vec<Vec2>>, dt: f32) {
        let live = ecs.query(&[TRANSFORM, COLLIDER, RIGIDBODY]);
        let mut sorted = live.clone();
        sorted.sort_unstable();
        let gone: Vec<u64> = self
            .entities
            .keys()
            .filter(|e| sorted.binary_search(e).is_err())
            .copied()
            .collect();
        for e in gone {
            if let Some(t) = self.entities.remove(&e) {
                self.world.remove_body(handle(t.body));
            }
        }
        for ent in live {
            let (Some(t), Some(c), Some(rb)) = (
                read::<Transform>(ecs, ent, TRANSFORM),
                read::<Collider>(ecs, ent, COLLIDER),
                read::<RigidBody>(ecs, ent, RIGIDBODY),
            ) else {
                continue;
            };
            let vel = read::<Velocity>(ecs, ent, VELOCITY);
            let Some(shape) = shape(&c, &t, polygons) else {
                continue;
            };
            let mut desc = bytes(&rb);
            desc.extend(bytes(&c));
            desc.extend(bytes(&t.scale));
            let pose = (t.pos.x, t.pos.y, t.rotation);
            let speed = vel.map_or((0.0, 0.0, 0.0), |v| (v.linear.x, v.linear.y, v.angular));

            if !self.entities.contains_key(&ent) {
                let body = RigidBodyBuilder::new(body_type(rb.kind))
                    .translation(v(t.pos))
                    .rotation(t.rotation)
                    .linvel(Vector::new(speed.0, speed.1))
                    .angvel(speed.2)
                    .linear_damping(rb.linear_damping)
                    .angular_damping(rb.angular_damping)
                    .gravity_scale(rb.gravity_scale)
                    .ccd_enabled(rb.bullet != 0)
                    .user_data(ent as u128);
                let body = if rb.fixed_rotation != 0 {
                    body.lock_rotations()
                } else {
                    body
                };
                let collider = ColliderBuilder::new(shape)
                    .density(rb.density)
                    .friction(rb.friction)
                    .restitution(rb.restitution)
                    .sensor(c.trigger != 0)
                    .collision_groups(groups(&c))
                    .position(offset(&c, &t))
                    .user_data(ent as u128);
                let (b, col) = self.world.insert(body, collider);
                self.entities.insert(
                    ent,
                    Tracked {
                        body: b.into_raw_parts(),
                        collider: col.0.into_raw_parts(),
                        desc,
                        pose,
                        vel: speed,
                    },
                );
                continue;
            }

            let tracked = self.entities.get_mut(&ent).expect("tracked");
            let h = handle(tracked.body);
            if tracked.desc != desc {
                if let Some(b) = self.world.bodies.get_mut(h) {
                    b.set_body_type(body_type(rb.kind), true);
                    b.set_linear_damping(rb.linear_damping);
                    b.set_angular_damping(rb.angular_damping);
                    b.set_gravity_scale(rb.gravity_scale, true);
                    b.lock_rotations(rb.fixed_rotation != 0, true);
                    b.enable_ccd(rb.bullet != 0);
                }
                let ch = rapier2d::prelude::ColliderHandle::from_raw_parts(
                    tracked.collider.0,
                    tracked.collider.1,
                );
                if let Some(col) = self.world.colliders.get_mut(ch) {
                    col.set_shape(shape);
                    col.set_density(rb.density);
                    col.set_friction(rb.friction);
                    col.set_restitution(rb.restitution);
                    col.set_sensor(c.trigger != 0);
                    col.set_collision_groups(groups(&c));
                    col.set_position_wrt_parent(offset(&c, &t));
                }
                tracked.desc = desc;
            }
            let Some(b) = self.world.bodies.get_mut(h) else {
                continue;
            };
            let kinematic = rb.kind == BODY_KINEMATIC;
            if kinematic {
                // Kinematic bodies go where the game puts them, plus their
                // Velocity
                let next = Pose::new(
                    Vector::new(pose.0 + speed.0 * dt, pose.1 + speed.1 * dt),
                    pose.2 + speed.2 * dt,
                );
                b.set_next_kinematic_position(next);
            } else if tracked.pose != pose {
                b.set_position(Pose::new(Vector::new(pose.0, pose.1), pose.2), true);
            }
            if !kinematic && vel.is_some() && tracked.vel != speed {
                b.set_linvel(Vector::new(speed.0, speed.1), true);
                b.set_angvel(speed.2, true);
            }
            tracked.pose = pose;
            tracked.vel = speed;
        }
    }

    /// Advance by dt and write places and speeds back to the components
    pub fn step(&mut self, ecs: &mut World, polygons: &Store<Vec<Vec2>>, dt: f32) {
        self.sync(ecs, polygons, dt);
        self.world.integration_parameters.dt = dt.max(0.0);
        self.world.step();
        for (&ent, tracked) in self.entities.iter_mut() {
            let Some(b) = self.world.bodies.get_mut(handle(tracked.body)) else {
                continue;
            };
            // Forces last one step
            b.reset_forces(false);
            b.reset_torques(false);
            if b.is_fixed() {
                continue;
            }
            let p = b.translation();
            let pose = (p.x, p.y, b.rotation().angle());
            let lv = b.linvel();
            let speed = (lv.x, lv.y, b.angvel());
            if let Some(mut t) = read::<Transform>(ecs, ent, TRANSFORM) {
                t.pos = Vec2::new(pose.0, pose.1);
                t.rotation = pose.2;
                write(ecs, ent, TRANSFORM, &t);
            }
            if let Some(mut v) = read::<Velocity>(ecs, ent, VELOCITY) {
                if !b.is_kinematic() {
                    v.linear = Vec2::new(speed.0, speed.1);
                    v.angular = speed.2;
                    write(ecs, ent, VELOCITY, &v);
                }
            }
            tracked.pose = pose;
            if !b.is_kinematic() {
                tracked.vel = speed;
            }
        }
    }

    fn body_mut(&mut self, ent: u64) -> Option<&mut rapier2d::prelude::RigidBody> {
        let h = handle(self.entities.get(&ent)?.body);
        self.world.bodies.get_mut(h)
    }

    pub fn apply_impulse(&mut self, ent: u64, impulse: Vec2) -> bool {
        self.body_mut(ent)
            .map(|b| b.apply_impulse(v(impulse), true))
            .is_some()
    }

    /// Pushes during the next step only
    pub fn apply_force(&mut self, ent: u64, force: Vec2) -> bool {
        self.body_mut(ent)
            .map(|b| b.add_force(v(force), true))
            .is_some()
    }

    pub fn apply_torque_impulse(&mut self, ent: u64, torque: f32) -> bool {
        self.body_mut(ent)
            .map(|b| b.apply_torque_impulse(torque, true))
            .is_some()
    }

    pub fn mass(&self, ent: u64) -> f32 {
        self.entities
            .get(&ent)
            .and_then(|t| self.world.bodies.get(handle(t.body)))
            .map_or(0.0, |b| b.mass())
    }

    fn pose(&self, ent: u64) -> Option<Pose> {
        let t = self.entities.get(&ent)?;
        Some(*self.world.bodies.get(handle(t.body))?.position())
    }

    fn add_joint(
        &mut self,
        a: u64,
        b: u64,
        joint: impl Into<rapier2d::prelude::GenericJoint>,
    ) -> Option<u32> {
        let (ha, hb) = (
            handle(self.entities.get(&a)?.body),
            handle(self.entities.get(&b)?.body),
        );
        if a == b {
            return None;
        }
        let h = self.world.insert_impulse_joint(ha, hb, joint);
        let id = self.next_joint;
        self.next_joint += 1;
        self.joints.insert(id, h.into_raw_parts());
        Some(id)
    }

    /// A hinge at a world point: the two turn freely around it
    pub fn pin(&mut self, a: u64, b: u64, at: Vec2) -> Option<u32> {
        let (pa, pb) = (self.pose(a)?, self.pose(b)?);
        let joint = RevoluteJointBuilder::new()
            .local_anchor1(pa.inverse_transform_point(v(at)))
            .local_anchor2(pb.inverse_transform_point(v(at)));
        self.add_joint(a, b, joint)
    }

    /// Glue the two together as they are now
    pub fn weld(&mut self, a: u64, b: u64) -> Option<u32> {
        let (pa, pb) = (self.pose(a)?, self.pose(b)?);
        let joint = FixedJointBuilder::new()
            .local_frame1(pa.inv_mul(&pb))
            .local_frame2(Pose::IDENTITY);
        self.add_joint(a, b, joint)
    }

    /// The centres stay at most `length` apart
    pub fn rope(&mut self, a: u64, b: u64, length: f32) -> Option<u32> {
        self.add_joint(a, b, RopeJointBuilder::new(length.max(0.0)))
    }

    /// The centres are pulled towards `rest` apart
    pub fn spring(
        &mut self,
        a: u64,
        b: u64,
        rest: f32,
        stiffness: f32,
        damping: f32,
    ) -> Option<u32> {
        self.add_joint(
            a,
            b,
            SpringJointBuilder::new(rest.max(0.0), stiffness, damping),
        )
    }

    pub fn remove_joint(&mut self, id: u32) -> bool {
        match self.joints.remove(&id) {
            Some(raw) => self
                .world
                .remove_impulse_joint(ImpulseJointHandle::from_raw_parts(raw.0, raw.1))
                .is_some(),
            None => false,
        }
    }

    /// Why physics calls would do nothing (never, here)
    pub fn missing() -> Option<&'static str> {
        None
    }

    pub fn save(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    pub fn load(bytes: &[u8]) -> Result<Physics, String> {
        bincode::deserialize(bytes).map_err(|e| format!("snapshot: physics: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{register, COLLIDER_BOX};

    fn setup() -> (World, Store<Vec<Vec2>>, Physics) {
        let mut w = World::new();
        register(&mut w);
        let mut p = Physics::default();
        p.set_gravity(Vec2::new(0.0, 500.0));
        (w, Store::default(), p)
    }

    fn body(w: &mut World, pos: Vec2, size: Vec2, rb: RigidBody) -> u64 {
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
        write(
            w,
            e,
            COLLIDER,
            &Collider {
                kind: COLLIDER_BOX,
                size,
                ..Collider::default()
            },
        );
        write(w, e, RIGIDBODY, &rb);
        e
    }

    fn pos(w: &World, e: u64) -> Vec2 {
        read::<Transform>(w, e, TRANSFORM).unwrap().pos
    }

    #[test]
    fn a_box_falls_and_rests_on_the_ground() {
        let (mut w, polys, mut p) = setup();
        body(
            &mut w,
            Vec2::new(0.0, 100.0),
            Vec2::new(400.0, 20.0),
            RigidBody {
                kind: BODY_FIXED,
                ..RigidBody::default()
            },
        );
        let crate_ = body(
            &mut w,
            Vec2::new(0.0, 0.0),
            Vec2::new(20.0, 20.0),
            RigidBody::default(),
        );
        write(&mut w, crate_, VELOCITY, &Velocity::default());
        for _ in 0..180 {
            p.step(&mut w, &polys, 1.0 / 60.0);
        }
        let y = pos(&w, crate_).y;
        assert!((y - 80.0).abs() < 1.0, "rests on top of the ground: {y}");
        let vy = read::<Velocity>(&w, crate_, VELOCITY).unwrap().linear.y;
        assert!(vy.abs() < 1.0, "and stops: {vy}");
        assert!(p.mass(crate_) > 0.0);
    }

    #[test]
    fn the_game_moves_bodies_through_components() {
        let (mut w, polys, mut p) = setup();
        p.set_gravity(Vec2::new(0.0, 0.0));
        let b = body(
            &mut w,
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 10.0),
            RigidBody::default(),
        );
        write(
            &mut w,
            b,
            VELOCITY,
            &Velocity {
                linear: Vec2::new(60.0, 0.0),
                angular: 0.0,
            },
        );
        for _ in 0..60 {
            p.step(&mut w, &polys, 1.0 / 60.0);
        }
        assert!((pos(&w, b).x - 60.0).abs() < 0.5, "{:?}", pos(&w, b));
        // Teleport
        write(
            &mut w,
            b,
            TRANSFORM,
            &Transform {
                pos: Vec2::new(500.0, 0.0),
                ..Transform::default()
            },
        );
        p.step(&mut w, &polys, 1.0 / 60.0);
        assert!((pos(&w, b).x - 501.0).abs() < 0.5);
        // A kinematic platform follows its Velocity and is not pushed
        let plat = body(
            &mut w,
            Vec2::new(0.0, 50.0),
            Vec2::new(40.0, 4.0),
            RigidBody {
                kind: BODY_KINEMATIC,
                ..RigidBody::default()
            },
        );
        write(
            &mut w,
            plat,
            VELOCITY,
            &Velocity {
                linear: Vec2::new(0.0, -30.0),
                angular: 0.0,
            },
        );
        for _ in 0..31 {
            p.step(&mut w, &polys, 1.0 / 60.0);
        }
        assert!((pos(&w, plat).y - 35.0).abs() < 0.5, "{:?}", pos(&w, plat));
        // Removing the RigidBody removes the body
        w.remove(b, RIGIDBODY);
        p.step(&mut w, &polys, 1.0 / 60.0);
        assert_eq!(p.body_count(), 1);
    }

    #[test]
    fn impulses_and_joints() {
        let (mut w, polys, mut p) = setup();
        p.set_gravity(Vec2::new(0.0, 0.0));
        let a = body(
            &mut w,
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 10.0),
            RigidBody {
                kind: BODY_FIXED,
                ..RigidBody::default()
            },
        );
        let b = body(
            &mut w,
            Vec2::new(50.0, 0.0),
            Vec2::new(10.0, 10.0),
            RigidBody::default(),
        );
        p.sync(&w, &polys, 0.0);
        let rope = p.rope(a, b, 60.0).unwrap();
        assert!(p.apply_impulse(b, Vec2::new(p.mass(b) * 200.0, 0.0)));
        for _ in 0..60 {
            p.step(&mut w, &polys, 1.0 / 60.0);
        }
        let d = pos(&w, b).x;
        assert!(d < 62.0, "the rope holds it: {d}");
        assert!(p.remove_joint(rope) && !p.remove_joint(rope));
        assert!(p.pin(a, b, Vec2::new(25.0, 0.0)).is_some());
        assert!(p.weld(a, b).is_some() && p.spring(a, b, 10.0, 5.0, 1.0).is_some());
        assert_eq!(p.rope(a, a, 1.0), None, "not to itself");
        assert!(!p.apply_force(12345, Vec2::new(1.0, 0.0)), "no such body");
    }

    #[test]
    fn a_saved_simulation_continues_the_same() {
        let (mut w, polys, mut p) = setup();
        body(
            &mut w,
            Vec2::new(0.0, 100.0),
            Vec2::new(400.0, 20.0),
            RigidBody {
                kind: BODY_FIXED,
                ..RigidBody::default()
            },
        );
        for i in 0..8 {
            body(
                &mut w,
                Vec2::new(i as f32 * 7.0, -(i as f32) * 22.0),
                Vec2::new(20.0, 20.0),
                RigidBody::default(),
            );
        }
        for _ in 0..30 {
            p.step(&mut w, &polys, 1.0 / 60.0);
        }
        let bytes = p.save();
        let mut q = Physics::load(&bytes).unwrap();
        let mut saved = Vec::new();
        w.save(&mut saved);
        let mut w2 = World::new();
        w2.load(&saved).unwrap();
        for _ in 0..60 {
            p.step(&mut w, &polys, 1.0 / 60.0);
            q.step(&mut w2, &polys, 1.0 / 60.0);
        }
        assert_eq!(p.save(), q.save(), "byte for byte");
    }
}

namespace ORST;

/// <summary>
/// Physics (Rapier, deterministic): bodies with mass for entities with a
/// Transform, a Collider and a RigidBody. Call PhysicsStep once per tick.
/// Moving the Transform or setting the Velocity moves a body; after the
/// step its place and speed are written back. UpdateMotion skips these
/// entities, and UpdateCollisions reports them without pushing them.
/// </summary>
public sealed partial class Engine
{
    public void PhysicsStep(float dt) => Native.orst_physics_step(Handle, dt);
    /// <summary>Units per second squared, y down; default none (top-down games)</summary>
    public Vec2 Gravity { set => Native.orst_set_gravity(Handle, value); }
    /// <summary>World units in a metre, default 32</summary>
    public float PhysicsScale { set => Native.orst_set_physics_scale(Handle, value); }

    public bool ApplyImpulse(Entity e, Vec2 impulse) => Native.orst_apply_impulse(Handle, e.Id, impulse);
    /// <summary>Pushes during the next step only</summary>
    public bool ApplyForce(Entity e, Vec2 force) => Native.orst_apply_force(Handle, e.Id, force);
    public bool ApplyTorqueImpulse(Entity e, float torque) => Native.orst_apply_torque_impulse(Handle, e.Id, torque);
    public float BodyMass(Entity e) => Native.orst_body_mass(Handle, e.Id);

    /// <summary>A hinge at a world point; 0 on error</summary>
    public uint JointPin(Entity a, Entity b, Vec2 at) => Native.orst_joint_pin(Handle, a.Id, b.Id, at);
    /// <summary>Glued as they are now</summary>
    public uint JointWeld(Entity a, Entity b) => Native.orst_joint_weld(Handle, a.Id, b.Id);
    /// <summary>At most this far apart</summary>
    public uint JointRope(Entity a, Entity b, float length) => Native.orst_joint_rope(Handle, a.Id, b.Id, length);
    public uint JointSpring(Entity a, Entity b, float rest, float stiffness, float damping) =>
        Native.orst_joint_spring(Handle, a.Id, b.Id, rest, stiffness, damping);
    public bool RemoveJoint(uint joint) => Native.orst_remove_joint(Handle, joint);
}

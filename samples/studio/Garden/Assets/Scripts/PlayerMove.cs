using ORST;

/// Moves the entity with the move actions (arrows, WASD or a gamepad;
/// Assets/Input.actions). With a Velocity the engine moves it and walls
/// stop it; without one it slides anywhere. It stands still while paused.
public class PlayerMove : Behaviour
{
    [Range(0, 500)] public float Speed = 120;

    public override void Update(float dt)
    {
        var x = Engine.ActionAxis(Engine.Action("left"), Engine.Action("right"));
        var y = Engine.ActionAxis(Engine.Action("up"), Engine.Action("down"));
        var move = PauseMenu.Paused ? new Vec2(0, 0) : new Vec2(x, y) * Speed;
        if (Engine.Has<Velocity>(Entity))
            Engine.Get<Velocity>(Entity).Linear = move;
        else
            Transform.Pos += move * dt;
    }
}

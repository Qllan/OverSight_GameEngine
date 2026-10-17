using ORST;

/// Moves the entity with the arrow keys or WASD. With a Velocity the
/// engine moves it and walls stop it; without one it slides anywhere.
public class PlayerMove : Behaviour
{
    [Range(0, 500)] public float Speed = 120;

    public override void Update(float dt)
    {
        var x = Engine.Axis(Key.Left, Key.Right) + Engine.Axis(Key.A, Key.D);
        var y = Engine.Axis(Key.Up, Key.Down) + Engine.Axis(Key.W, Key.S);
        var move = new Vec2(Math.Clamp(x, -1, 1), Math.Clamp(y, -1, 1)) * Speed;
        if (Engine.Has<Velocity>(Entity))
            Engine.Get<Velocity>(Entity).Linear = move;
        else
            Transform.Pos += move * dt;
    }
}

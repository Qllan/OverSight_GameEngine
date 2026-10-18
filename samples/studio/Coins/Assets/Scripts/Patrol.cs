using ORST;

/// Goes back and forth along Path from where it starts, and sends the
/// player back to the start on touch
public class Patrol : Behaviour
{
    public Vec2 Path = new(80, 0);
    [Range(0, 5)] public float Speed = 1;

    private Vec2 from;
    private float time;

    public override void Start() => from = Transform.Pos;

    public override void Update(float dt)
    {
        time += dt * Speed;
        Transform.Pos = from + Path * ((1 - MathF.Cos(time)) / 2);
        Transform.Rotation += dt * 4;
    }

    public override void OnContactBegin(Entity other, Contact contact) =>
        GetBehaviour<Respawn>(other)?.Hit();
}

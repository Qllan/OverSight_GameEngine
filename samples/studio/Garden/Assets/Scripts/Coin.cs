using ORST;

/// A coin the player picks up: it counts toward the Score and goes
public class Coin : Behaviour
{
    public int Value = 1;
    [Range(0, 0.5f)] public float Pulse = 0.12f;

    private float time;

    public override void Update(float dt)
    {
        time += dt;
        var s = 1 + Pulse * MathF.Sin(time * 6);
        Transform.Scale = new Vec2(s, s);
    }

    public override void OnContactBegin(Entity other, Contact contact)
    {
        if (GetBehaviour<PlayerMove>(other) is null) return;
        FindBehaviour<Score>()?.Add(Value);
        Destroy(Entity);
    }
}

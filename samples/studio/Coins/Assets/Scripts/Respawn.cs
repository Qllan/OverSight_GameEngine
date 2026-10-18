using ORST;

/// Remembers where the entity started; Hit puts it back there
public class Respawn : Behaviour
{
    private Vec2 start;

    public override void Start() => start = Transform.Pos;

    public void Hit()
    {
        Transform.Pos = start;
        Log("Ouch! Back to the start");
    }
}

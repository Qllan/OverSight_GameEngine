using ORST;

/// Lights the game with the Light entities; Ambient is the light everywhere
public class Lighting : Behaviour
{
    [Range(0, 1)] public float Ambient = 0.35f;

    public override void Start() => Engine.SetEffects(Effect.Lighting(Color.White, Ambient));
}

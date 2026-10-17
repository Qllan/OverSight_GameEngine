// __NAME__: collect the coins. WASD, arrows or a gamepad stick to move.
// F3 shows the debug panel, Esc quits.
using OverSight;

using var engine = new Engine(new Config
{
    Title = "__NAME__",
    Width = 320, Height = 180, // the game draws into 320x180...
    Scale = 4,                 // ...shown 4 times bigger
    IntegerScale = true,
});

var rng = new Random(1);
Vec2 RandomSpot() => new(16 + rng.NextSingle() * 288, 24 + rng.NextSingle() * 140);

var player = new Vec2(160, 90);
const float speed = 90;
var coins = Enumerable.Range(0, 5).Select(_ => RandomSpot()).ToArray();
int score = 0;

while (engine.Poll())
{
    if (engine.KeyPressed(Key.Escape)) break;
    float dt = engine.Dt;

    var dir = new Vec2(
        engine.Axis(Key.A, Key.D) + engine.Axis(Key.Left, Key.Right) + engine.PadAxis(0, PadAxis.LeftX),
        engine.Axis(Key.W, Key.S) + engine.Axis(Key.Up, Key.Down) + engine.PadAxis(0, PadAxis.LeftY));
    float len = MathF.Sqrt(dir.X * dir.X + dir.Y * dir.Y);
    if (len > 1) dir = dir * (1 / len);
    player = player + dir * (speed * dt);
    player = new Vec2(Math.Clamp(player.X, 4, 316), Math.Clamp(player.Y, 4, 176));

    for (int i = 0; i < coins.Length; i++)
    {
        float dx = coins[i].X - player.X, dy = coins[i].Y - player.Y;
        if (dx * dx + dy * dy < 10 * 10)
        {
            score++;
            coins[i] = RandomSpot();
        }
    }

    engine.BeginFrame(new Color(30, 34, 48));
    foreach (var c in coins) engine.FillCircle(c, 4, new Color(255, 210, 80));
    engine.FillRect(new Rect(player.X - 4, player.Y - 4, 8, 8), new Color(120, 200, 255));
    engine.DrawText($"Монеты: {score}", new Vec2(4, 2), 10, Color.White);
    engine.EndFrame();
}

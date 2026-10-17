// Platformer: run, jump, land on one-way planks, reach the flag.
// A/D or arrows to run, Space/W/Up to jump. Falling into a pit respawns.
using OverSight;

var opt = Sample.Parse(args);
using var engine = new Engine(Sample.Window("OverSight: платформер", opt));
using var map = engine.LoadTilemap(Path.Combine(opt.Assets, "platformer.tmj"));
using var hero = engine.LoadTexture(Path.Combine(opt.Assets, "hero.png"));
var frames = hero.Sheet(16, 16);
var info = map.Info;
float mapW = info.Width * info.TileW, mapH = info.Height * info.TileH;

var spawn = map.FindObject("player") is { } p ? new Vec2(p.Rect.X, p.Rect.Y) : new Vec2(40, 190);
var goal = map.FindObject("goal")?.Rect ?? default;

const float gravity = 900, runSpeed = 110, jumpSpeed = 300;
var box = new Rect(spawn.X - 5, spawn.Y - 14, 10, 14);
var vel = new Vec2();
float sinceGround = 1, jumpBuffer = 1, runTime = 0;
bool left = false, won = false;
var run = Animation.Range(8, 4, 10f);

// Dust from the feet: a trail while running, a puff on landing
using var dust = engine.CreateEmitter(new EmitterConfig
{
    Rate = 14, Max = 80, LifeMin = 0.25f, LifeMax = 0.5f, SpeedMin = 8, SpeedMax = 30, Spread = 3f,
    Gravity = new Vec2(0, -20), Drag = 3, SizeStart = 4, SizeEnd = 1,
    ColorStart = new Color(235, 225, 205, 200), ColorEnd = new Color(235, 225, 205, 0),
    Area = new Vec2(3, 0), Layer = 4,
});
bool JumpKey(Func<Key, bool> state) => state(Key.Space) || state(Key.W) || state(Key.Up);

// Run right, hop over the crate, then over the pit
var script = new Sample.Script((5, Key.D, true), (66, Key.Space, true), (82, Key.Space, false),
    (100, Key.Space, true), (116, Key.Space, false));
while (script.Next(engine, opt))
{
    float dt = engine.Dt;
    float dir = Math.Clamp(engine.Axis(Key.A, Key.D) + engine.Axis(Key.Left, Key.Right), -1, 1);
    vel.X = dir * runSpeed;
    if (dir != 0) left = dir < 0;
    runTime = dir != 0 ? runTime + dt : 0;

    // Forgiving jumps: a little after leaving a ledge, a little before landing
    sinceGround += dt;
    jumpBuffer += dt;
    if (JumpKey(engine.KeyPressed)) jumpBuffer = 0;
    if (jumpBuffer < 0.12f && sinceGround < 0.1f)
    {
        vel.Y = -jumpSpeed;
        jumpBuffer = sinceGround = 1;
    }
    // Short hop when the button is let go early
    if (!JumpKey(engine.KeyDown) && vel.Y < -jumpSpeed / 3) vel.Y = -jumpSpeed / 3;

    vel.Y = MathF.Min(vel.Y + gravity * dt, 500);
    var r = map.MoveBox(box, vel * dt);
    box = r.Rect;
    if (r.Contacts.Ground)
    {
        if (sinceGround > 0.2f) dust.Burst(8);
        vel.Y = 0;
        sinceGround = 0;
    }
    if (r.Contacts.Ceiling) vel.Y = 0;
    if (box.Y > mapH) { box = new Rect(spawn.X - 5, spawn.Y - 14, 10, 14); vel = new Vec2(); }
    if (box.Overlaps(goal)) won = true;

    var feet = new Vec2(box.X + box.W / 2, box.Bottom);
    dust.Pos = feet;
    dust.Emitting = sinceGround == 0 && dir != 0;
    engine.UpdateEmitters(dt);
    engine.LookAt(Sample.Follow(feet, mapW, mapH));
    engine.BeginFrame(new Color(90, 150, 210));
    map.Draw();
    dust.Draw();
    bool airborne = sinceGround > 0.05f;
    uint f = airborne ? 9 : (dir != 0 ? run.FrameAt(runTime) : 8);
    engine.DrawFrame(hero, frames, f, new Sprite { Pos = feet, Origin = new Vec2(8, 16), FlipX = left, Layer = 5 });

    engine.Space = Space.Screen;
    engine.Layer = 10;
    engine.DrawText("A/D: бег, Space: прыжок", new Vec2(4, 2), 10, Color.White);
    if (won) engine.DrawText("Победа!", new Vec2(120, 70), 24, new Color(255, 230, 90));
    engine.Space = Space.World;
    engine.Layer = 0;
    engine.EndFrame();
}
return Sample.Finish(engine, opt, "platformer");

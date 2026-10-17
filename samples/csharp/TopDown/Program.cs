// Top-down: walk around a garden, bump into walls and crates, read a sign.
// WASD or arrows to walk. Tiles and the hero are y-sorted together.
using OverSight;

var opt = Sample.Parse(args);
using var engine = new Engine(Sample.Window("OverSight: вид сверху", opt));
using var map = engine.LoadTilemap(Path.Combine(opt.Assets, "topdown.tmj"));
using var hero = engine.LoadTexture(Path.Combine(opt.Assets, "hero.png"));
var frames = hero.Sheet(16, 16);
var info = map.Info;
float mapW = info.Width * info.TileW, mapH = info.Height * info.TileH;
uint ground = (uint)map.LayerIndex("ground"), walls = (uint)map.LayerIndex("walls");

// The hero's feet: a small box that collides, the sprite drawn above it
var feet = new Vec2(64, 168);
if (map.FindObject("player") is { } spawn) feet = new Vec2(spawn.Rect.X, spawn.Rect.Y);
const float boxW = 10, boxH = 6;
var walk = Animation.Range(0, 4, 8f);
float walkTime = 0;
int row = 0; // 0 down, 1 up, 2 side
bool left = false;
var sign = map.FindObject("sign");
string signText = sign != null ? map.String(sign, "text") ?? "" : "";

var script = new Sample.Script((10, Key.D, true), (70, Key.D, false), (70, Key.W, true),
    (100, Key.W, false), (100, Key.S, true), (180, Key.S, false));
while (script.Next(engine, opt))
{
    float dt = engine.Dt;
    var dir = new Vec2(
        Math.Clamp(engine.Axis(Key.A, Key.D) + engine.Axis(Key.Left, Key.Right), -1, 1),
        Math.Clamp(engine.Axis(Key.W, Key.S) + engine.Axis(Key.Up, Key.Down), -1, 1));
    float len = MathF.Sqrt(dir.X * dir.X + dir.Y * dir.Y);
    if (len > 0)
    {
        const float speed = 70f;
        var box = new Rect(feet.X - boxW / 2, feet.Y - boxH, boxW, boxH);
        box = map.MoveBox(box, dir * (speed * dt / len)).Rect;
        feet = new Vec2(box.X + boxW / 2, box.Bottom);
        walkTime += dt;
        if (MathF.Abs(dir.X) > MathF.Abs(dir.Y)) { row = 2; left = dir.X < 0; }
        else row = dir.Y < 0 ? 1 : 0;
    }
    else walkTime = 0;

    engine.LookAt(Sample.Follow(feet, mapW, mapH));
    engine.BeginFrame(new Color(20, 24, 30));
    map.DrawLayer(ground, 0);
    map.DrawLayer(walls, 1, ySort: true);
    var s = new Sprite { Pos = feet, Origin = new Vec2(8, 15), FlipX = left, Layer = 1, Depth = feet.Y };
    engine.DrawFrame(hero, frames, (uint)(row * 4) + (len > 0 ? walk.FrameAt(walkTime) : 0), s);

    engine.Space = Space.Screen;
    engine.Layer = 10;
    engine.DrawText("WASD: ходить", new Vec2(4, 2), 10, Color.White);
    if (sign != null && sign.Rect.Overlaps(new Rect(feet.X - 6, feet.Y - 12, 12, 12)))
    {
        engine.FillRect(new Rect(20, 140, 280, 30), new Color(0, 0, 0, 200));
        engine.DrawText(signText, new Vec2(28, 148), 12, new Color(255, 240, 200));
    }
    engine.Space = Space.World;
    engine.Layer = 0;
    engine.EndFrame();
}
return Sample.Finish(engine, opt, "topdown");

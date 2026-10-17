// Isometric: step from cell to cell around stone blocks; the mouse shows
// the cell under it. WASD or arrows move one cell (W/Up is up-right on
// screen, A/Left up-left). Blocks and the hero are y-sorted together.
using OverSight;

var opt = Sample.Parse(args);
using var engine = new Engine(Sample.Window("OverSight: изометрия", opt));
using var map = engine.LoadTilemap(Path.Combine(opt.Assets, "iso.tmj"));
using var hero = engine.LoadTexture(Path.Combine(opt.Assets, "hero.png"));
var frames = hero.Sheet(16, 16);
var info = map.Info;
uint floorLayer = (uint)map.LayerIndex("floor"), blocks = (uint)map.LayerIndex("blocks");

int cx = 2, cy = 2;
if (map.FindObject("player") is { } p)
{
    var t = map.WorldToTile(new Vec2(p.Rect.X, p.Rect.Y));
    cx = (int)MathF.Floor(t.X);
    cy = (int)MathF.Floor(t.Y);
}
Vec2 from = map.CellCenter(cx, cy), pos = from;
float stepTime = 1; // 0..1 while moving between cells
const float stepDuration = 0.18f;
int row = 0;
bool left = false;

var script = new Sample.Script((5, Key.D, true), (60, Key.D, false), (60, Key.S, true), (130, Key.S, false));
while (script.Next(engine, opt))
{
    float dt = engine.Dt;
    if (stepTime >= 1)
    {
        int dx = 0, dy = 0;
        if (engine.KeyDown(Key.D) || engine.KeyDown(Key.Right)) dx = 1;
        else if (engine.KeyDown(Key.A) || engine.KeyDown(Key.Left)) dx = -1;
        else if (engine.KeyDown(Key.S) || engine.KeyDown(Key.Down)) dy = 1;
        else if (engine.KeyDown(Key.W) || engine.KeyDown(Key.Up)) dy = -1;
        if ((dx != 0 || dy != 0) && !map.IsSolid(cx + dx, cy + dy))
        {
            cx += dx;
            cy += dy;
            from = pos;
            stepTime = 0;
            // Screen direction of the step picks the sprite row
            row = dy > 0 || dx > 0 ? 0 : 1;
            left = dx < 0 || dy > 0;
        }
    }
    if (stepTime < 1)
    {
        stepTime = MathF.Min(stepTime + dt / stepDuration, 1);
        pos = from + (map.CellCenter(cx, cy) - from) * stepTime;
    }

    engine.LookAt(pos);
    engine.BeginFrame(new Color(25, 30, 40));
    map.DrawLayer(floorLayer, 0);
    map.DrawLayer(blocks, 1, ySort: true);
    uint f = (uint)(row * 4) + (stepTime < 1 ? 1 + (uint)(stepTime * 3) % 3 : 0);
    engine.DrawFrame(hero, frames, f, new Sprite
    {
        Pos = new Vec2(pos.X, pos.Y + 2),
        Origin = new Vec2(8, 15),
        FlipX = left,
        Layer = 1,
        Depth = pos.Y + info.TileH / 2f, // its cell's bottom corner
    });

    // Hovered cell outline
    var h = map.WorldToTile(engine.MouseWorld);
    int hx = (int)MathF.Floor(h.X), hy = (int)MathF.Floor(h.Y);
    if (hx >= 0 && hy >= 0 && hx < info.Width && hy < info.Height)
    {
        Vec2[] c =
        [
            map.TileToWorld(new Vec2(hx, hy)), map.TileToWorld(new Vec2(hx + 1, hy)),
            map.TileToWorld(new Vec2(hx + 1, hy + 1)), map.TileToWorld(new Vec2(hx, hy + 1)),
        ];
        engine.Layer = 2;
        var edge = map.IsSolid(hx, hy) ? new Color(255, 90, 90) : new Color(255, 255, 255, 180);
        for (int i = 0; i < 4; i++) engine.DrawLine(c[i], c[(i + 1) % 4], 1, edge);
    }

    engine.Space = Space.Screen;
    engine.Layer = 10;
    engine.DrawText("WASD: шаг по клеткам", new Vec2(4, 2), 10, Color.White);
    engine.Space = Space.World;
    engine.Layer = 0;
    engine.EndFrame();
}
return Sample.Finish(engine, opt, "isometric");

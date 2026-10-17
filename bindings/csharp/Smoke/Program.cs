using System.Runtime.InteropServices;
using OverSight;

static void Check(bool ok, string what)
{
    if (!ok) { Console.Error.WriteLine("failed: " + what); Environment.Exit(1); }
}

using var engine = new Engine(new Config("Smoke", 64, 64) { Headless = true, TargetFps = 0 });

try { engine.LoadTexture("missing.png"); Check(false, "missing texture throws"); }
catch (OverSightException e) { Check(e.Message.Contains("missing.png"), "error names the file"); }

var white = new byte[8 * 8 * 4];
Array.Fill(white, (byte)255);
using var tiles = engine.TextureFromRgba(8, 8, white);
Check(tiles.Width == 8, "texture size");

using var map = engine.CreateTilemap(Orientation.Orthogonal, 8, 8, 8, 8);
Check(map.AddTileset(1, tiles, tiles.Sheet(8, 8)), "tileset");
Check(map.SetTileFlag(1, "solid"), "solid flag");
for (int x = 0; x < 8; x++) map.SetTile(0, x, 7, 1);

var player = new Rect(20, 0, 6, 6);
bool grounded = false;
for (int i = 0; i < 120 && !grounded; i++)
{
    var r = map.MoveBox(player, new Vec2(0, 4));
    player = r.Rect;
    grounded = r.Contacts.Ground;
}
Check(grounded && player.Bottom == 56f, $"landed at {player}");
var hit = map.Raycast(new Vec2(0, 30), new Vec2(0, 100));
Check(hit is { CellY: 7 }, "raycast");

Check(Easing.Ease(Ease.QuadIn, 0.5f) == 0.25f, "ease");
var slide = new Vec2Tween(new Vec2(0, 0), new Vec2(10, 20), 1f, delay: 0.5f);
Check(slide.Update(0.5f).X == 0f, "tween delay");
var mid = slide.Update(0.5f);
Check(mid.X == 5f && mid.Y == 10f && !slide.Done, $"tween middle {mid}");
var fade = new ColorTween(Color.Black, Color.White, 2f, repeat: Repeat.PingPong);
Check(fade.Update(3f) == new Color(128, 128, 128), "color ping-pong");

Check(Marshal.SizeOf<EmitterConfig>() == 104, "EmitterConfig layout");
var sparks = engine.CreateEmitter(new EmitterConfig { Rate = 10, Texture = tiles.Id }, new Vec2(8, 8));
sparks.Update(0.55f);
Check(sparks.Count == 5 && sparks.Emitting, $"emitter rate {sparks.Count}");
sparks.Emitting = false;
sparks.Burst(3);
Check(sparks.Count == 8 && sparks.Config.Rate == 10f, "burst and config");

Check(Animation.Range(4, 4, 8f).FrameAt(0.3f) == 6, "animation");

engine.LookAt(new Vec2(32, 32));
Check(engine.Poll(), "poll");
engine.BeginFrame(Color.Black);
map.Draw();
engine.Space = Space.Screen;
engine.FillRect(new Rect(0, 0, 4, 4), new Color(255, 0, 0));
var sprite = new Sprite { Pos = new Vec2(40, 0), Scale = new Vec2(1, 1), FlipX = true, Tint = new Color(0, 0, 255) };
engine.Draw(tiles, sprite);
engine.DrawText("Привет", new Vec2(10, 10), 12, Color.White);
engine.EndFrame();
Check(engine.Pixel(1, 1) == new Color(255, 0, 0), "red rect");
Check(engine.Pixel(30, 60) == Color.White, "floor tile");
Check(engine.Pixel(44, 4) == new Color(0, 0, 255), $"tinted sprite {engine.Pixel(44, 4)}");
Check(engine.MeasureText("Привет", 12).X > 20, "text size");

engine.InjectKey(Key.Space, true);
engine.Poll();
Check(engine.KeyPressed(Key.Space), "injected key");

engine.InjectGamepadConnected(1, true);
engine.InjectPadButton(1, Pad.South, true);
engine.InjectPadAxis(1, PadAxis.LeftY, -1f);
engine.Poll();
Check(engine.GamepadConnected(1) && engine.GamepadName(1) == "Injected", "gamepad connected");
Check(engine.PadPressed(1, Pad.South) && !engine.PadDown(0, Pad.South), "gamepad button");
Check(engine.PadStick(1).Y == -1f, "gamepad stick");

Check(engine.RendererName == "software", "renderer name");
engine.HotReload = true;
Check(engine.HotReload && engine.ReloadChanged() == 0 && engine.Reloaded == 0, "hot reload");

engine.Debug = true;
Check(engine.Debug, "debug on");
engine.BeginFrame(Color.Black);
engine.DebugRect(new Rect(0, 57, 8, 7), new Color(0, 255, 0)); // below the stats panel
engine.EndFrame();
Check(engine.Pixel(0, 60) == new Color(0, 255, 0), $"debug outline {engine.Pixel(0, 60)}");
Console.WriteLine("smoke_cs: ok");

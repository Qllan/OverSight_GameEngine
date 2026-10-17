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
var path = map.FindPath(new Vec2(4, 52), new Vec2(60, 52), diagonal: false);
Check(path.Length == 8 && path[^1].X == 60 && path[^1].Y == 52 && map.FindPath(new Vec2(4, 52), new Vec2(4, 60)).Length == 0, "FindPath");

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
using (var dust = engine.CreateGpuEmitter(new EmitterConfig { Rate = 10 }, new Vec2(8, 8)))
{
    engine.UpdateGpuEmitters(0.55f);
    dust.Burst(3);
    Check(dust.Count == 8 && dust.Config.Rate == 10f, "GPU emitter");
}

Check(Animation.Range(4, 4, 8f).FrameAt(0.3f) == 6, "animation");

engine.LookAt(new Vec2(32, 32));
Check(engine.Poll(), "poll");
Check(engine.Ticks == 1 && engine.TickDt == 1f / 60f, "one tick per headless frame");
engine.RandomSeed(1);
Check(engine.RandomU32() == 0x910A2DECu, "seeded random numbers");
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
// Entities: move everything that has a velocity
var a = engine.Spawn();
var b = engine.Spawn();
engine.Set(a, new Pos { X = 0, Y = 0 });
engine.Set(a, new Vel { X = 1, Y = 2 });
engine.Set(b, new Pos { X = 5, Y = 5 });
engine.ForEach((Entity _, ref Pos p, ref Vel v) => { p.X += v.X; p.Y += v.Y; });
Check(engine.Get<Pos>(a).X == 1 && engine.Get<Pos>(a).Y == 2, "ForEach writes in place");
engine.Get<Pos>(b).X = 7;
Check(engine.Get<Pos>(b).X == 7 && !engine.Has<Vel>(b), "Get by ref");
Check(engine.Query<Pos>().Length == 2 && engine.Query<Pos, Vel>().Single() == a, "queries");
var saved = engine.Snapshot();
ulong hash = engine.StateHash;
Check(engine.Despawn(a) && !engine.Alive(a) && !engine.TryGet<Pos>(a, out _), "despawn");
Check(engine.StateHash != hash, "the hash follows the state");
engine.Restore(saved);
Check(engine.Alive(a) && engine.Get<Pos>(a).X == 1 && engine.StateHash == hash, "restore");
try { engine.Restore(new byte[] { 1, 2, 3 }); Check(false, "bad snapshot throws"); }
catch (OverSightException) { Check(engine.Alive(a), "a failed restore changes nothing"); }
engine.Despawn(a);
var n = engine.Spawn();
engine.Set(n, new Named { D = 1.5, B = 2 }); // 16 bytes aligned to 8
Check(engine.Get<Named>(n).D == 1.5 && engine.Get<Named>(n).B == 2, "named component");

// Built-in components: the engine moves and draws them
engine.Debug = false;
Check(engine.Component<Transform>() == 1 && Marshal.SizeOf<SpriteRenderer>() == 40 && Marshal.SizeOf<Body>() == 24, "built-in ids and layouts");
using var red = engine.TextureFromRgba(1, 1, new byte[] { 255, 0, 0, 255 });
var mover = engine.Spawn();
engine.Set(mover, new Transform(new Vec2(10, 20)) { Scale = new Vec2(3, 3) });
engine.Set(mover, new Velocity(new Vec2(4, 0), 1));
engine.Set(mover, new SpriteRenderer(red) { Layer = 20 });
engine.UpdateMotion(0.5f);
Check(engine.Get<Transform>(mover).Pos.X == 12 && engine.Get<Transform>(mover).Rotation == 0.5f, "UpdateMotion");
engine.Get<Transform>(mover).Rotation = 0;
engine.BeginFrame(Color.Black);
engine.Space = Space.Screen;
engine.DrawEntities();
engine.EndFrame();
Check(engine.Pixel(13, 21) == new Color(255, 0, 0), $"DrawEntities {engine.Pixel(13, 21)}");

// Colliders: a box and a circle overlap along x
var wall = engine.Spawn();
engine.Set(wall, new Transform(new Vec2(100, 100)));
engine.Set(wall, Collider.Box(new Vec2(10, 10)));
var ball = engine.Spawn();
engine.Set(ball, new Transform(new Vec2(107, 100)));
engine.Set(ball, Collider.Circle(3));
Check(engine.EntityOverlap(wall, ball) is { Depth: > 0.99f and < 1.01f } o && o.Normal.X > 0.99f, "EntityOverlap");
Check(engine.Component<Collider>() == 5 && Marshal.SizeOf<Collider>() == 40, "Collider id and layout");
uint tri = engine.CreatePolygon(new Vec2(0, -2), new Vec2(2, 2), new Vec2(-2, 2));
Check(tri != 0, "CreatePolygon");
engine.Set(ball, new Velocity(new Vec2(-5, 0)));
engine.UpdateCollisions();
var contact = engine.Contacts().Single();
Check(contact.State == ContactState.Begin && contact.Other(wall) == ball && contact.Depth > 0.99f, "UpdateCollisions");
Check(engine.Get<Transform>(ball).Pos.X > 107.99f && engine.Get<Velocity>(ball).Linear.X == 0, "a mover is pushed out of the wall");
Check(engine.QueryPoint(new Vec2(100, 100)).Single() == wall && engine.QueryCircle(new Vec2(120, 100), 10).Single() == ball, "queries");
Check(engine.RaycastEntities(new Vec2(150, 100), new Vec2(0, 100)) is { } eh && eh.Entity == ball, "RaycastEntities");
engine.Despawn(wall);
engine.Despawn(ball);
engine.UpdateCollisions();
Check(engine.Contacts().Single().State == ContactState.End, "end after despawn");

// Physics: a crate falls onto a fixed floor
engine.Gravity = new Vec2(0, 400);
var floor = engine.Spawn();
engine.Set(floor, new Transform(new Vec2(500, 100)));
engine.Set(floor, Collider.Box(new Vec2(300, 20)));
engine.Set(floor, new RigidBody(BodyKind.Fixed));
var box = engine.Spawn();
engine.Set(box, new Transform(new Vec2(500, 0)));
engine.Set(box, Collider.Box(new Vec2(16, 16)));
engine.Set(box, new RigidBody());
engine.Set(box, new Velocity());
Check(engine.Component<RigidBody>() == 6 && Marshal.SizeOf<RigidBody>() == 32 && engine.BodyMass(box) > 0, "RigidBody");
for (int i = 0; i < 120; i++) engine.PhysicsStep(1f / 60);
Check(Math.Abs(engine.Get<Transform>(box).Pos.Y - 82) < 1, $"the crate rests on the floor {engine.Get<Transform>(box).Pos}");
Check(engine.JointRope(floor, box, 50) != 0, "JointRope");
engine.Despawn(floor);
engine.Despawn(box);
engine.UpdateCollisions();

// Scenes: a cut switches at the next frame, a push keeps the scene below
Check(engine.GoScene(1, Transition.Cut) && engine.Scene == 0, "scene changes at the next frame");
engine.Poll();
Check(engine.Scene == 1 && engine.SceneEntered == 1, "GoScene");
engine.PushScene(2, Transition.Wipe, 0.1f);
Check(!engine.GoScene(3), "one change at a time");
for (int i = 0; i < 10; i++) engine.Poll();
Check(engine.Scene == 2 && engine.SceneDepth == 2 && !engine.SceneTransitioning, "PushScene");

// Actions: two inputs on one action, rebinding by capture, saved as text
var jump = engine.Action("jump");
Check(engine.Bind(jump, Binding.Key(Key.J)) && engine.Bind(jump, Binding.Button(Pad.North)), "Bind");
engine.InjectKey(Key.J, true);
engine.Poll();
Check(engine.ActionPressed(jump) && engine.CapturedBinding == Binding.Key(Key.J), "ActionPressed");
engine.InjectKey(Key.J, false);
engine.Poll();
Check(engine.ActionReleased(jump) && engine.CapturedBinding == null, "ActionReleased");
Check(engine.SaveBindings() == "jump = key:J pad:North\n", $"SaveBindings {engine.SaveBindings()}");
engine.LoadBindings("jump = pad1:LeftY-");
Check(engine.Bindings(jump).Single() == Binding.Axis(PadAxis.LeftY, -1, 1), "LoadBindings");
Check(engine.BindingText(engine.Bindings(jump)[0]) == "pad1:LeftY-", "BindingText");
try { engine.LoadBindings("jump = nope"); Check(false, "a bad line throws"); }
catch (OverSightException) { Check(engine.Bindings(jump).Length == 1, "a bad line changes nothing"); }

// Render targets and post effects: a red target on screen, then greyed
engine.Debug = false;
using (var target = engine.CreateTarget(8, 8))
{
    engine.BeginFrame(Color.Black);
    engine.DrawTo(target);
    engine.ClearTarget(target, new Color(255, 0, 0));
    engine.DrawToScreen();
    engine.Space = Space.Screen;
    engine.Draw(target, 0, 0);
    engine.EndFrame();
    Check(engine.Pixel(4, 4) == new Color(255, 0, 0), "DrawTo");
    engine.SetTargetCamera(target, new Camera(new Vec2(0, 0)));
    engine.SetTargetCamera(target, null);
}
engine.SetEffects(Effect.Grade(saturation: 0), Effect.Glow(), Effect.Crt(0, 0, 0, 0));
engine.BeginFrame(new Color(255, 0, 0));
engine.EndFrame();
var grey = engine.Pixel(30, 30);
Check(grey.R == grey.G && grey.G == grey.B && grey.R > 0, $"SetEffects {grey.R} {grey.G} {grey.B}");
engine.SetEffects();
using (var shader = engine.CreateShader(
    "fn effect(uv: vec2<f32>, pixel: vec2<f32>) -> vec4<f32> { return load(vec2<i32>(pixel)); }"))
    Check(shader.Id != 0, "CreateShader");
try { engine.CreateShader("fn effect() {}"); Check(false, "a bad shader throws"); }
catch (OverSightException) { }
try { engine.SetEffects(new Effect { Kind = (EffectKind)42 }); Check(false, "a bad effect throws"); }
catch (OverSightException) { }
engine.Shake(4, 0.2f);

// Lighting: one light in the dark
engine.SetEffects(Effect.Lighting());
var lamp = engine.Spawn();
engine.Set(lamp, new Transform(new Vec2(32, 32)));
engine.Set(lamp, new Light(40));
engine.ShadowLayers = 1;
engine.LookAt(new Vec2(32, 32));
engine.BeginFrame(Color.White);
engine.EndFrame();
Check(engine.Pixel(32, 32).R > 200 && engine.Pixel(0, 0).R == 0, "Lighting");
Check(Marshal.SizeOf<Light>() == 32, "Light layout");
engine.SetEffects();
engine.Despawn(lamp);

// Skeletons
using (var rig = engine.SkeletonFromJson("""
    {"bones": [{"name": "root"}, {"name": "arm", "parent": "root", "x": 10}],
     "slots": [{"name": "arm", "bone": "arm", "attachment": "a", "attachments": {"a": {"region": [0, 0, 2, 2]}}}],
     "animations": {"wave": {"bones": {"arm": {"rotate": [{"time": 0, "value": 0}, {"time": 1, "value": 90}]}},
                             "events": [{"time": 0.5, "name": "half"}]}}}
    """))
using (var skel = engine.CreateSkeleton(rig))
{
    skel.Play("wave", looped: false);
    engine.UpdateSkeletons(0.6f);
    Check(skel.Fired("half") && skel.Animation == "wave" && !skel.Done, "Skeleton events");
    int arm = skel.Bone("arm");
    skel.Root = new Transform(new Vec2(100, 0));
    Check(arm == 1 && skel.BoneWorld(arm).Pos.X == 110, "Skeleton bones");
    var noDance = false;
    try { skel.Play("dance"); } catch (OverSightException) { noDance = true; }
    Check(noDance, "Skeleton errors");
    skel.SetAttachment("arm", null);
}

Console.WriteLine("smoke_cs: ok");

struct Pos { public float X, Y; }
struct Vel { public float X, Y; }
[Component("Renamed")] struct Named { public double D; public byte B; }

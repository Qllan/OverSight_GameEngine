using System.Runtime.InteropServices;

namespace OverSight;

[StructLayout(LayoutKind.Sequential)]
public struct Color : IEquatable<Color>
{
    public byte R, G, B, A;

    public Color(byte r, byte g, byte b, byte a = 255) { R = r; G = g; B = b; A = a; }

    public static readonly Color Black = new(0, 0, 0);
    public static readonly Color White = new(255, 255, 255);
    public static readonly Color Transparent = new(0, 0, 0, 0);

    public bool Equals(Color o) => R == o.R && G == o.G && B == o.B && A == o.A;
    public override bool Equals(object? obj) => obj is Color c && Equals(c);
    public override int GetHashCode() => HashCode.Combine(R, G, B, A);
    public static bool operator ==(Color a, Color b) => a.Equals(b);
    public static bool operator !=(Color a, Color b) => !a.Equals(b);
    public override string ToString() => $"({R}, {G}, {B}, {A})";
}

[StructLayout(LayoutKind.Sequential)]
public struct Vec2
{
    public float X, Y;

    public Vec2(float x, float y) { X = x; Y = y; }

    public static Vec2 operator +(Vec2 a, Vec2 b) => new(a.X + b.X, a.Y + b.Y);
    public static Vec2 operator -(Vec2 a, Vec2 b) => new(a.X - b.X, a.Y - b.Y);
    public static Vec2 operator *(Vec2 a, float s) => new(a.X * s, a.Y * s);
    public override string ToString() => $"({X}, {Y})";
}

/// <summary>Top-left corner and size</summary>
[StructLayout(LayoutKind.Sequential)]
public struct Rect
{
    public float X, Y, W, H;

    public Rect(float x, float y, float w, float h) { X = x; Y = y; W = w; H = h; }

    public readonly float Right => X + W;
    public readonly float Bottom => Y + H;
    public readonly bool Overlaps(Rect o) => X < o.Right && o.X < Right && Y < o.Bottom && o.Y < Bottom;
    public override string ToString() => $"({X}, {Y}, {W}, {H})";
}

[StructLayout(LayoutKind.Sequential)]
public struct Camera
{
    /// <summary>World point at the centre of the screen</summary>
    public Vec2 Center;
    /// <summary>Screen pixels per world unit</summary>
    public float Zoom;
    public float Rotation;

    public Camera(Vec2 center, float zoom = 1f, float rotation = 0f) { Center = center; Zoom = zoom; Rotation = rotation; }
}

/// <summary>How a sprite is drawn. Src is the texture region (empty: the
/// whole texture); Origin lands on Pos and is the pivot.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct Sprite
{
    public Rect Src;
    public Vec2 Pos;
    public Vec2 Origin;
    public Vec2 Scale;
    /// <summary>Radians, clockwise</summary>
    public float Rotation;
    private byte flipX, flipY;
    public Color Tint;
    /// <summary>Lower layers are drawn first</summary>
    public int Layer;
    /// <summary>Order within a layer: pass y for y-sorting</summary>
    public float Depth;

    public Sprite() { Scale = new Vec2(1, 1); Tint = Color.White; }

    public bool FlipX { readonly get => flipX != 0; set => flipX = value ? (byte)1 : (byte)0; }
    public bool FlipY { readonly get => flipY != 0; set => flipY = value ? (byte)1 : (byte)0; }
}

/// <summary>How an emitter's particles look and move. new EmitterConfig() starts
/// from the engine's defaults. A Min/Max pair picks a random value for every particle.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct EmitterConfig
{
    /// <summary>Texture id (Texture.Id); 0 draws round dots</summary>
    public uint Texture;
    /// <summary>Region of the texture; empty: the whole texture</summary>
    public Rect Src;
    /// <summary>Particles per second while emitting</summary>
    public float Rate;
    /// <summary>Most particles alive at once</summary>
    public uint Max;
    public float LifeMin, LifeMax;
    /// <summary>Pixels per second</summary>
    public float SpeedMin, SpeedMax;
    /// <summary>Direction in radians: 0 right, PI/2 down</summary>
    public float Angle;
    /// <summary>Width of the cone of directions (2*PI: every way)</summary>
    public float Spread;
    public Vec2 Gravity;
    /// <summary>Share of speed lost per second</summary>
    public float Drag;
    /// <summary>Pixels: dot diameter or texture width</summary>
    public float SizeStart, SizeEnd;
    public float SpinMin, SpinMax;
    public Color ColorStart, ColorEnd;
    /// <summary>Spawn box half-size around the emitter</summary>
    public Vec2 Area;
    public int Layer;
    /// <summary>Same seed and calls: same particles</summary>
    public uint Seed;

    public EmitterConfig() { this = Native.ov_default_emitter(); }
}

/// <summary>A texture cut into a grid of equal frames, numbered row by row</summary>
[StructLayout(LayoutKind.Sequential)]
public struct SpriteSheet
{
    public uint FrameW, FrameH, Columns, Count, Margin, Spacing;

    public static SpriteSheet Grid(uint textureW, uint textureH, uint frameW, uint frameH, uint margin = 0, uint spacing = 0)
        => Native.ov_sheet_grid(textureW, textureH, frameW, frameH, margin, spacing);

    public readonly Rect Frame(uint index) => Native.ov_sheet_frame(this, index);
}

/// <summary>What draws the frames</summary>
public enum Renderer { Auto = 0, Software = 1, Gpu = 2 }
public enum AnimMode { Loop = 0, Once = 1, PingPong = 2 }
public enum Ease
{
    Linear,
    QuadIn, QuadOut, QuadInOut,
    CubicIn, CubicOut, CubicInOut,
    SineIn, SineOut, SineInOut,
    ExpoIn, ExpoOut, ExpoInOut,
    BackIn, BackOut, BackInOut,
    ElasticIn, ElasticOut, ElasticInOut,
    BounceIn, BounceOut, BounceInOut,
}
public enum Repeat { Once = 0, Loop = 1, PingPong = 2 }
public enum Space { World = 0, Screen = 1 }

public enum Transition { Cut = 0, Fade = 1, Wipe = 2 }
public enum Orientation { Orthogonal = 0, Isometric = 1 }
public enum MouseButton { Left = 0, Right = 1, Middle = 2 }
/// <summary>Gamepad buttons by position: South is A on Xbox, Cross on PlayStation</summary>
public enum Pad
{
    South, East, West, North,
    LeftBumper, RightBumper,
    Back, Start, Guide,
    LeftStick, RightStick,
    DPadUp, DPadDown, DPadLeft, DPadRight,
}
/// <summary>Sticks -1..1 with y down (up is -1); triggers 0..1</summary>
public enum PadAxis { LeftX, LeftY, RightX, RightY, LeftTrigger, RightTrigger }

[StructLayout(LayoutKind.Sequential)]
public struct Contacts
{
    private byte ground, ceiling, wallLeft, wallRight;

    /// <summary>Stopped while moving down</summary>
    public readonly bool Ground => ground != 0;
    /// <summary>Stopped while moving up</summary>
    public readonly bool Ceiling => ceiling != 0;
    public readonly bool WallLeft => wallLeft != 0;
    public readonly bool WallRight => wallRight != 0;
}

[StructLayout(LayoutKind.Sequential)]
public struct MoveResult
{
    public Rect Rect;
    public Contacts Contacts;
}

[StructLayout(LayoutKind.Sequential)]
public struct RayHit
{
    public Vec2 Point;
    public int CellX, CellY;
    /// <summary>Side hit, in cell axes; (0, 0) when the ray starts inside</summary>
    public Vec2 Normal;
    public float Distance;
}

[StructLayout(LayoutKind.Sequential)]
public struct TilemapInfo
{
    public Orientation Orientation;
    public uint Width, Height, TileW, TileH, LayerCount, ObjectCount;
}

/// <summary>Tile id flags, as in Tiled</summary>
public static class TileFlags
{
    public const uint FlipH = 0x80000000u;
    public const uint FlipV = 0x40000000u;
    public const uint FlipD = 0x20000000u;
}

/// <summary>Frame indices shown Fps times per second</summary>
public sealed class Animation
{
    public uint[] Frames;
    public float Fps;
    public AnimMode Mode;

    public Animation(uint[] frames, float fps, AnimMode mode = AnimMode.Loop) { Frames = frames; Fps = fps; Mode = mode; }

    public static Animation Range(uint first, uint count, float fps, AnimMode mode = AnimMode.Loop)
    {
        var frames = new uint[count];
        for (uint i = 0; i < count; i++) frames[i] = first + i;
        return new Animation(frames, fps, mode);
    }

    public uint FrameAt(float time) => Native.ov_anim_frame_at(Frames, (uint)Frames.Length, Fps, (int)Mode, time);
    public float Duration => Fps > 0 ? Frames.Length / Fps : 0;
}

public static class Easing
{
    public static float Ease(Ease e, float t) => Native.ov_ease((int)e, t);
    public static float Lerp(float a, float b, float t) => a + (b - a) * t;
    public static Vec2 Lerp(Vec2 a, Vec2 b, float t) => new(Lerp(a.X, b.X, t), Lerp(a.Y, b.Y, t));
    public static Color Lerp(Color a, Color b, float t) => Native.ov_lerp_color(a, b, t);
}

/// <summary>Timing of a tween: eased progress 0..1 over Duration seconds.
/// Call Update(dt) every frame; FloatTween, Vec2Tween and ColorTween give the value.</summary>
public abstract class TweenBase
{
    public float Duration;
    public Ease Ease;
    public Repeat Repeat;
    /// <summary>Seconds since the start; negative while waiting out a delay</summary>
    public float Time;
    public bool Paused;

    protected TweenBase(float duration, Ease ease, Repeat repeat, float delay)
    {
        Duration = duration; Ease = ease; Repeat = repeat; Time = -delay;
    }

    public float Progress => Native.ov_tween_progress(Time, Duration, (int)Ease, (int)Repeat);
    public bool Done => Repeat == Repeat.Once && Time >= Duration;
    public void Restart() => Time = 0;
    protected void Advance(float dt) { if (!Paused) Time += dt; }
}

public sealed class FloatTween(float from, float to, float duration, Ease ease = Ease.Linear,
    Repeat repeat = Repeat.Once, float delay = 0) : TweenBase(duration, ease, repeat, delay)
{
    public float From = from, To = to;
    public float Value => Easing.Lerp(From, To, Progress);
    public float Update(float dt) { Advance(dt); return Value; }
}

public sealed class Vec2Tween(Vec2 from, Vec2 to, float duration, Ease ease = Ease.Linear,
    Repeat repeat = Repeat.Once, float delay = 0) : TweenBase(duration, ease, repeat, delay)
{
    public Vec2 From = from, To = to;
    public Vec2 Value => Easing.Lerp(From, To, Progress);
    public Vec2 Update(float dt) { Advance(dt); return Value; }
}

public sealed class ColorTween(Color from, Color to, float duration, Ease ease = Ease.Linear,
    Repeat repeat = Repeat.Once, float delay = 0) : TweenBase(duration, ease, repeat, delay)
{
    public Color From = from, To = to;
    public Color Value => Easing.Lerp(From, To, Progress);
    public Color Update(float dt) { Advance(dt); return Value; }
}

public sealed class OverSightException : Exception
{
    public OverSightException(string message) : base(message) { }
}

// Built-in components: Engine.UpdateMotion moves and Engine.DrawEntities
// draws entities that have them. Create them with new (): default() has a
// zero scale and a transparent tint.

/// <summary>Where an entity is</summary>
[StructLayout(LayoutKind.Sequential)]
public struct Transform
{
    public Vec2 Pos;
    /// <summary>Radians, clockwise</summary>
    public float Rotation;
    public Vec2 Scale;

    public Transform() { Scale = new Vec2(1, 1); }
    public Transform(Vec2 pos, float rotation = 0) : this() { Pos = pos; Rotation = rotation; }
}

/// <summary>Units and radians per second</summary>
[StructLayout(LayoutKind.Sequential)]
public struct Velocity
{
    public Vec2 Linear;
    public float Angular;

    public Velocity(Vec2 linear, float angular = 0) { Linear = linear; Angular = angular; }
}

/// <summary>A sprite drawn at the entity's transform</summary>
[StructLayout(LayoutKind.Sequential)]
public struct SpriteRenderer
{
    public uint Texture;
    /// <summary>Texture region; zero size: the whole texture</summary>
    public Rect Src;
    /// <summary>Point placed at the position, rotated around</summary>
    public Vec2 Origin;
    public Color Tint;
    public int Layer;
    private byte flipX, flipY, hidden, ySort;

    public SpriteRenderer() { Tint = Color.White; }
    public SpriteRenderer(Texture texture) : this() { Texture = texture.Id; }

    public bool FlipX { readonly get => flipX != 0; set => flipX = (byte)(value ? 1 : 0); }
    public bool FlipY { readonly get => flipY != 0; set => flipY = (byte)(value ? 1 : 0); }
    public bool Hidden { readonly get => hidden != 0; set => hidden = (byte)(value ? 1 : 0); }
    /// <summary>Order within the layer by y (top-down games)</summary>
    public bool YSort { readonly get => ySort != 0; set => ySort = (byte)(value ? 1 : 0); }
}

/// <summary>A box that collides with a tile map while the entity moves</summary>
[StructLayout(LayoutKind.Sequential)]
public struct Body
{
    /// <summary>Relative to the position</summary>
    public Rect Bounds;
    /// <summary>The map it collides with; 0: it moves freely</summary>
    public uint Tilemap;
    /// <summary>What it stopped at or rests against after the last move</summary>
    public Contacts Contacts;

    public Body(Rect bounds, Tilemap? map = null) { Bounds = bounds; Tilemap = map?.Id ?? 0; }
}

public enum BindingKind { None = 0, Key = 1, Mouse = 2, PadButton = 3, PadAxis = 4 }

/// <summary>An input bound to an action: Binding.Key(Key.Space), Binding.Button(Pad.South)</summary>
[StructLayout(LayoutKind.Sequential)]
public struct Binding : IEquatable<Binding>
{
    public BindingKind Kind;
    /// <summary>Key, mouse button, pad button or pad axis</summary>
    public int Code;
    /// <summary>Pad slot 0..3, -1: any pad</summary>
    public int Slot;
    /// <summary>Pad axis: 1 or -1 (LeftX -1 is left)</summary>
    public int Direction;

    public static Binding Key(Key key) => new() { Kind = BindingKind.Key, Code = (int)key, Slot = -1 };
    public static Binding Mouse(MouseButton button) => new() { Kind = BindingKind.Mouse, Code = (int)button, Slot = -1 };
    public static Binding Button(Pad button, int slot = -1) => new() { Kind = BindingKind.PadButton, Code = (int)button, Slot = slot };
    public static Binding Axis(PadAxis axis, int direction, int slot = -1) =>
        new() { Kind = BindingKind.PadAxis, Code = (int)axis, Slot = slot, Direction = direction };

    public bool Equals(Binding o) => Kind == o.Kind && Code == o.Code && Slot == o.Slot && Direction == o.Direction;
    public override bool Equals(object? obj) => obj is Binding b && Equals(b);
    public override int GetHashCode() => HashCode.Combine(Kind, Code, Slot, Direction);
    public static bool operator ==(Binding a, Binding b) => a.Equals(b);
    public static bool operator !=(Binding a, Binding b) => !a.Equals(b);
}

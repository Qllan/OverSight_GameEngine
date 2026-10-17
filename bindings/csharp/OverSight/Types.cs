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

/// <summary>A texture cut into a grid of equal frames, numbered row by row</summary>
[StructLayout(LayoutKind.Sequential)]
public struct SpriteSheet
{
    public uint FrameW, FrameH, Columns, Count, Margin, Spacing;

    public static SpriteSheet Grid(uint textureW, uint textureH, uint frameW, uint frameH, uint margin = 0, uint spacing = 0)
        => Native.ov_sheet_grid(textureW, textureH, frameW, frameH, margin, spacing);

    public readonly Rect Frame(uint index) => Native.ov_sheet_frame(this, index);
}

public enum AnimMode { Loop = 0, Once = 1, PingPong = 2 }
public enum Space { World = 0, Screen = 1 }
public enum Orientation { Orthogonal = 0, Isometric = 1 }
public enum MouseButton { Left = 0, Right = 1, Middle = 2 }

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

public sealed class OverSightException : Exception
{
    public OverSightException(string message) : base(message) { }
}

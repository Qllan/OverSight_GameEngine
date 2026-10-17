using System.Runtime.InteropServices;

namespace OverSight;

/// <summary>A WGSL post effect shader (see Engine.CreateShader); GPU only</summary>
public sealed class Shader : Resource
{
    internal Shader(Engine e, uint id) : base(e, id) { }
    protected override void Free(IntPtr e, uint id) => Native.ov_free_shader(e, id);
}

public enum EffectKind : uint { Glow = 1, Blur = 2, Grade = 3, Crt = 4, Shader = 5 }

/// <summary>One post effect of a chain; build it with the static methods</summary>
[StructLayout(LayoutKind.Sequential)]
public struct Effect
{
    public EffectKind Kind;
    public uint ShaderId;
    public float P0, P1, P2, P3, P4, P5, P6, P7;

    private static Effect Make(EffectKind kind, float p0 = 0, float p1 = 0, float p2 = 0, float p3 = 0,
                               float p4 = 0, float p5 = 0, float p6 = 0) =>
        new() { Kind = kind, P0 = p0, P1 = p1, P2 = p2, P3 = p3, P4 = p4, P5 = p5, P6 = p6 };

    /// <summary>Pixels brighter than threshold (0..1) spread light radius pixels</summary>
    public static Effect Glow(float threshold = 0.6f, float strength = 1f, float radius = 8f) =>
        Make(EffectKind.Glow, threshold, strength, radius);

    public static Effect Blur(float radius) => Make(EffectKind.Blur, radius);

    public static Effect Grade(float brightness = 0f, float contrast = 1f, float saturation = 1f,
                               float gamma = 1f, Color? tint = null)
    {
        var t = tint ?? Color.White;
        return Make(EffectKind.Grade, brightness, contrast, saturation, gamma, t.R / 255f, t.G / 255f, t.B / 255f);
    }

    public static Effect Crt(float scanlines = 0.3f, float curvature = 0.1f, float vignette = 0.5f,
                             float aberration = 1f) =>
        Make(EffectKind.Crt, scanlines, curvature, vignette, aberration);

    /// <summary>A custom shader; the numbers reach WGSL as post.p</summary>
    public static Effect Custom(Shader shader, params float[] p)
    {
        var e = new Effect { Kind = EffectKind.Shader, ShaderId = shader.Id };
        float At(int i) => i < p.Length ? p[i] : 0f;
        (e.P0, e.P1, e.P2, e.P3, e.P4, e.P5, e.P6, e.P7) =
            (At(0), At(1), At(2), At(3), At(4), At(5), At(6), At(7));
        return e;
    }
}

/// <summary>
/// Render targets: textures draw calls can go into. Draw into one after
/// DrawTo(target), go back with DrawToScreen(), and draw it like any
/// texture. Every frame starts on the screen; targets are drawn in the
/// order they are first used, the screen last. A target that is not
/// cleared keeps its picture and is drawn over.
/// Post effects run over a target or the screen every frame it is drawn.
/// </summary>
public sealed partial class Engine
{
    public Texture CreateTarget(uint width, uint height) =>
        new(this, Check(Native.ov_create_target(Handle, width, height), "render target"));

    public void DrawTo(Texture target)
    {
        if (!Native.ov_set_target(Handle, target.Id)) throw new OverSightException(LastError);
    }

    public void DrawToScreen() => Native.ov_set_target(Handle, 0);

    /// <summary>Clear a target this frame, before its draw calls</summary>
    public void ClearTarget(Texture target, Color color) => Native.ov_clear_target(Handle, target.Id, color);

    /// <summary>The camera World draw calls into a target use; null: the main one</summary>
    public void SetTargetCamera(Texture target, Camera? camera) =>
        Native.ov_set_target_camera(Handle, target.Id, camera is { } c ? new[] { c } : null);

    /// <summary>The screen's post effects, run in order every frame</summary>
    public void SetEffects(params Effect[] effects) => SetEffectsOf(0, effects);

    public void SetEffects(Texture target, params Effect[] effects) => SetEffectsOf(target.Id, effects);

    private void SetEffectsOf(uint target, Effect[] effects)
    {
        if (!Native.ov_set_effects(Handle, target, effects, (uint)effects.Length))
            throw new OverSightException(LastError);
    }

    /// <summary>
    /// A post effect shader in WGSL. It defines
    /// <c>fn effect(uv: vec2&lt;f32&gt;, pixel: vec2&lt;f32&gt;) -&gt; vec4&lt;f32&gt;</c>
    /// and may use <c>load(p)</c>, <c>textureSample(source, samp, uv)</c>,
    /// <c>post.size</c>, <c>post.time</c> and <c>post.p[0..2]</c>.
    /// </summary>
    public Shader CreateShader(string wgsl) => new(this, Check(Native.ov_create_shader(Handle, wgsl), "shader"));

    /// <summary>Shake the world on screen by up to strength pixels, fading out</summary>
    public void Shake(float strength, float duration) => Native.ov_shake(Handle, strength, duration);
}

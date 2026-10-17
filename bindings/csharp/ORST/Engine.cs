using System.Runtime.InteropServices;
using System.Text;

namespace ORST;

public sealed class Config
{
    public string Title = "ORST";
    /// <summary>Canvas: the logical resolution the game draws into</summary>
    public uint Width = 960, Height = 540;
    /// <summary>Initial window size = canvas * Scale</summary>
    public float Scale = 1f;
    public bool Resizable = true;
    public bool Fullscreen;
    /// <summary>Scale by whole numbers only (crisp pixel art)</summary>
    public bool IntegerScale;
    /// <summary>0 = unlimited</summary>
    public uint TargetFps = 60;
    /// <summary>No window: render offscreen (tests, CI)</summary>
    public bool Headless;
    /// <summary>Auto: the graphics card for a window (the CPU if it fails), the CPU headless</summary>
    public Renderer Renderer = Renderer.Auto;

    public Config() { }

    public Config(string title, uint width, uint height, float scale = 1f)
    {
        Title = title; Width = width; Height = height; Scale = scale;
    }
}

/// <summary>A resource id owned by an engine; Dispose frees it</summary>
public abstract class Resource : IDisposable
{
    protected readonly Engine Engine;
    public uint Id { get; private set; }

    protected Resource(Engine engine, uint id) { Engine = engine; Id = id; }

    protected abstract void Free(IntPtr e, uint id);

    public void Dispose()
    {
        if (Id != 0 && !Engine.Disposed) Free(Engine.Handle, Id);
        Id = 0;
        GC.SuppressFinalize(this);
    }
}

public sealed class Texture : Resource
{
    internal Texture(Engine e, uint id) : base(e, id) { }
    protected override void Free(IntPtr e, uint id) => Native.orst_free_texture(e, id);

    public uint Width { get { Native.orst_texture_size(Engine.Handle, Id, out var w, out _); return w; } }
    public uint Height { get { Native.orst_texture_size(Engine.Handle, Id, out _, out var h); return h; } }
    public bool Smooth { set => Native.orst_set_texture_smooth(Engine.Handle, Id, value); }

    public SpriteSheet Sheet(uint frameW, uint frameH, uint margin = 0, uint spacing = 0)
        => SpriteSheet.Grid(Width, Height, frameW, frameH, margin, spacing);
}

public sealed class Font : Resource
{
    internal Font(Engine e, uint id) : base(e, id) { }
    protected override void Free(IntPtr e, uint id) => Native.orst_free_font(e, id);
}

public sealed class Sound : Resource
{
    internal Sound(Engine e, uint id) : base(e, id) { }
    protected override void Free(IntPtr e, uint id) => Native.orst_free_sound(e, id);

    /// <summary>Returns a voice id for Engine.StopVoice (0 when nothing plays)</summary>
    public uint Play(float volume = 1f, float pitch = 1f) => Native.orst_play_sound(Engine.Handle, Id, volume, pitch);
}

/// <summary>Spawns and moves particles; draw it every frame with Draw()</summary>
public sealed class Emitter : Resource
{
    internal Emitter(Engine e, uint id) : base(e, id) { }
    protected override void Free(IntPtr e, uint id) => Native.orst_free_emitter(e, id);

    private IntPtr E => Engine.Handle;

    public EmitterConfig Config { get => Native.orst_emitter_config(E, Id); set => Native.orst_set_emitter_config(E, Id, value); }
    public Vec2 Pos { get => Native.orst_emitter_pos(E, Id); set => Native.orst_set_emitter_pos(E, Id, value); }
    public bool Emitting { get => Native.orst_emitter_emitting(E, Id); set => Native.orst_set_emitter_emitting(E, Id, value); }
    public uint Count => Native.orst_emitter_count(E, Id);
    public void Burst(uint count) => Native.orst_emitter_burst(E, Id, count);
    public void Clear() => Native.orst_emitter_clear(E, Id);
    public void Update(float dt) => Native.orst_update_emitter(E, Id, dt);
    public void Draw() => Native.orst_draw_emitter(E, Id);
}

/// <summary>The same configs as Emitter, for huge numbers of particles: the
/// graphics card works out where each one is. They can be counted, not read.</summary>
public sealed class GpuEmitter : Resource
{
    internal GpuEmitter(Engine e, uint id) : base(e, id) { }
    protected override void Free(IntPtr e, uint id) => Native.orst_free_gpu_emitter(e, id);

    private IntPtr E => Engine.Handle;

    /// <summary>A new Max drops the living particles</summary>
    public EmitterConfig Config { get => Native.orst_gpu_emitter_config(E, Id); set => Native.orst_set_gpu_emitter_config(E, Id, value); }
    public Vec2 Pos { get => Native.orst_gpu_emitter_pos(E, Id); set => Native.orst_set_gpu_emitter_pos(E, Id, value); }
    public bool Emitting { set => Native.orst_set_gpu_emitter_emitting(E, Id, value); }
    public uint Count => Native.orst_gpu_emitter_count(E, Id);
    public void Burst(uint count) => Native.orst_gpu_emitter_burst(E, Id, count);
    public void Clear() => Native.orst_gpu_emitter_clear(E, Id);
    public void Draw() => Native.orst_draw_gpu_emitter(E, Id);
}

/// <summary>A spawn point, trigger or other marker placed on a map</summary>
public sealed class MapObject
{
    public uint Id;
    public string Name = "", Class = "";
    public Rect Rect;
    public bool Point, Visible;
    public float Rotation;
    public uint Gid, Layer;
    internal uint Index;
}

public sealed class Tilemap : Resource
{
    internal Tilemap(Engine e, uint id) : base(e, id) { }
    protected override void Free(IntPtr e, uint id) => Native.orst_free_tilemap(e, id);

    private IntPtr E => Engine.Handle;

    public TilemapInfo Info { get { Native.orst_tilemap_info(E, Id, out var i); return i; } }
    public int LayerIndex(string name) => Native.orst_tilemap_layer_index(E, Id, name);
    public uint GetTile(uint layer, int x, int y) => Native.orst_tilemap_get_tile(E, Id, layer, x, y);
    public void SetTile(uint layer, int x, int y, uint gid) => Native.orst_tilemap_set_tile(E, Id, layer, x, y, gid);
    public bool AddTileset(uint firstGid, Texture texture, SpriteSheet sheet)
        => Native.orst_tilemap_add_tileset(E, Id, firstGid, texture.Id, sheet);
    public bool SetTileFlag(uint gid, string name, bool value = true) => Native.orst_tilemap_set_tile_flag(E, Id, gid, name, value);
    public bool IsSolid(int x, int y) => Native.orst_tilemap_is_solid(E, Id, x, y);
    public Vec2 TileToWorld(Vec2 tile) => Native.orst_tilemap_tile_to_world(E, Id, tile);
    public Vec2 WorldToTile(Vec2 p) => Native.orst_tilemap_world_to_tile(E, Id, p);
    public Vec2 CellCenter(int x, int y) => Native.orst_tilemap_cell_center(E, Id, x, y);

    public void Draw(int baseLayer = 0) => Native.orst_draw_tilemap(E, Id, baseLayer);
    public void DrawLayer(uint layer, int drawLayer, bool ySort = false, float depth = 0, Color? tint = null)
        => Native.orst_draw_tilemap_layer(E, Id, layer, drawLayer, depth, ySort, tint ?? Color.White);

    private delegate nuint StringGetter(IntPtr e, uint map, uint index, byte[]? buf, nuint size);

    private string Fetch(StringGetter get, uint index)
    {
        var n = get(E, Id, index, null, 0);
        var buf = new byte[(int)n + 1];
        get(E, Id, index, buf, (nuint)buf.Length);
        return Encoding.UTF8.GetString(buf, 0, (int)n);
    }

    public List<MapObject> Objects()
    {
        var list = new List<MapObject>();
        for (uint i = 0; i < Info.ObjectCount; i++)
        {
            Native.orst_tilemap_object(E, Id, i, out var o);
            list.Add(new MapObject
            {
                Id = o.Id,
                Name = Fetch(Native.orst_tilemap_object_name, i),
                Class = Fetch(Native.orst_tilemap_object_class, i),
                Rect = o.Rect,
                Point = o.Point != 0,
                Visible = o.Visible != 0,
                Rotation = o.Rotation,
                Gid = o.Gid,
                Layer = o.Layer,
                Index = i,
            });
        }
        return list;
    }

    public MapObject? FindObject(string name)
    {
        int i = Native.orst_tilemap_find_object(E, Id, name);
        return i < 0 ? null : Objects()[i];
    }

    public double Number(MapObject o, string property, double fallback = 0)
        => Native.orst_tilemap_object_number(E, Id, o.Index, property, fallback);

    public string? String(MapObject o, string property)
    {
        long n = Native.orst_tilemap_object_string(E, Id, o.Index, property, null, 0);
        if (n < 0) return null;
        var buf = new byte[n + 1];
        Native.orst_tilemap_object_string(E, Id, o.Index, property, buf, (nuint)buf.Length);
        return Encoding.UTF8.GetString(buf, 0, (int)n);
    }

    public MoveResult MoveBox(Rect rect, Vec2 delta) => Native.orst_move_box(E, Id, rect, delta);
    public Contacts Touching(Rect rect) => Native.orst_touching(E, Id, rect);
    /// <summary>Cell centres around solid cells, from the cell of from to that of to;
    /// empty when there is no way. A number property "cost" makes a tile slower.</summary>
    public Vec2[] FindPath(Vec2 from, Vec2 to, bool diagonal = true, float maxCost = 0)
    {
        var path = new Vec2[Native.orst_find_path(E, Id, from, to, diagonal, maxCost, null, 0)];
        Native.orst_find_path(E, Id, from, to, diagonal, maxCost, path, (uint)path.Length);
        return path;
    }

    public RayHit? Raycast(Vec2 from, Vec2 to) => Native.orst_raycast(E, Id, from, to, out var hit) ? hit : null;
    public bool PointSolid(Vec2 p) => Native.orst_point_solid(E, Id, p);
    public bool RectSolid(Rect r) => Native.orst_rect_solid(E, Id, r);
}

/// <summary>
/// The engine. The game owns the loop:
/// <code>
/// using var engine = new Engine(new Config("My game", 320, 180, 4f));
/// while (engine.Poll()) {
///     float dt = engine.Dt;
///     engine.BeginFrame(Color.Black);
///     // update with dt, then draw
///     engine.EndFrame();
/// }
/// </code>
/// </summary>
public sealed partial class Engine : IDisposable
{
    public const uint DefaultFont = 1;

    internal IntPtr Handle { get; private set; }
    internal bool Disposed => Handle == IntPtr.Zero;

    public static string LastError => Marshal.PtrToStringUTF8(Native.orst_last_error()) ?? "";

    public Engine(Config? config = null)
    {
        config ??= new Config();
        var title = Marshal.StringToCoTaskMemUTF8(config.Title);
        try
        {
            var native = new NativeConfig
            {
                Title = title,
                Width = config.Width,
                Height = config.Height,
                Scale = config.Scale,
                Resizable = B(config.Resizable),
                Fullscreen = B(config.Fullscreen),
                IntegerScale = B(config.IntegerScale),
                TargetFps = config.TargetFps,
                Headless = B(config.Headless),
                Renderer = (int)config.Renderer,
            };
            Handle = Native.orst_engine_create(native);
        }
        finally
        {
            Marshal.FreeCoTaskMem(title);
        }
        if (Handle == IntPtr.Zero) throw new OrstException("cannot start the engine: " + LastError);
    }

    private static byte B(bool v) => v ? (byte)1 : (byte)0;

    private static uint Check(uint id, string what)
        => id != 0 ? id : throw new OrstException(what + ": " + LastError);

    public void Dispose()
    {
        if (Handle != IntPtr.Zero) Native.orst_engine_destroy(Handle);
        Handle = IntPtr.Zero;
    }

    // frame
    public bool Poll() => Native.orst_poll(Handle);
    public void Quit() => Native.orst_quit(Handle);
    public float Dt => Native.orst_dt(Handle);
    public double Time => Native.orst_time(Handle);
    public float Fps => Native.orst_fps(Handle);
    public ulong Frame => Native.orst_frame(Handle);

    // fixed logic ticks: for (int i = 0; i < engine.Ticks; i++) Update(engine.TickDt);
    /// <summary>Logic ticks per second, 60 by default</summary>
    public uint TickRate { set => Native.orst_set_tick_rate(Handle, value); }
    /// <summary>Logic ticks to run this frame</summary>
    public uint Ticks => Native.orst_ticks(Handle);
    public float TickDt => Native.orst_tick_dt(Handle);
    /// <summary>0..1: how far the time is toward the next tick, for smooth drawing</summary>
    public float TickAlpha => Native.orst_tick_alpha(Handle);
    public ulong TickCount => Native.orst_tick_count(Handle);

    // seeded random numbers: the same seed gives the same game on any machine
    public void RandomSeed(ulong seed) => Native.orst_random_seed(Handle, seed);
    public uint RandomU32() => Native.orst_random_u32(Handle);
    /// <summary>0 &lt;= x &lt; 1</summary>
    public float RandomFloat() => Native.orst_random_float(Handle);
    /// <summary>min &lt;= x &lt; max</summary>
    public float RandomRange(float min, float max) => Native.orst_random_range(Handle, min, max);
    /// <summary>min &lt;= x &lt;= max</summary>
    public int RandomInt(int min, int max) => Native.orst_random_int(Handle, min, max);
    public void BeginFrame(Color clear) => Native.orst_begin_frame(Handle, clear);
    public void EndFrame() => Native.orst_end_frame(Handle);
    public string Title { set => Native.orst_set_title(Handle, value); }
    public bool Fullscreen { get => Native.orst_is_fullscreen(Handle); set => Native.orst_set_fullscreen(Handle, value); }
    public uint TargetFps { set => Native.orst_set_target_fps(Handle, value); }
    public bool HasWindow => Native.orst_has_window(Handle);
    public (uint W, uint H) CanvasSize { get { Native.orst_canvas_size(Handle, out var w, out var h); return (w, h); } }
    public void SaveScreenshot(string path)
    {
        if (!Native.orst_save_screenshot(Handle, path)) throw new OrstException(LastError);
    }
    public Color Pixel(uint x, uint y) => Native.orst_pixel(Handle, x, y);

    // resources
    public Texture LoadTexture(string path) => new(this, Check(Native.orst_load_texture(Handle, path), path));
    public Texture TextureFromRgba(uint width, uint height, byte[] rgba)
    {
        if (rgba.Length < width * height * 4) throw new ArgumentException("not enough pixel data", nameof(rgba));
        return new(this, Check(Native.orst_texture_from_rgba(Handle, width, height, rgba), "texture"));
    }
    public Font LoadFont(string path) => new(this, Check(Native.orst_load_font(Handle, path), path));
    public Tilemap LoadTilemap(string path) => new(this, Check(Native.orst_load_tilemap(Handle, path), path));
    public Tilemap CreateTilemap(Orientation o, uint width, uint height, uint tileW, uint tileH, uint layers = 1)
        => new(this, Check(Native.orst_create_tilemap(Handle, (int)o, width, height, tileW, tileH, layers), "tilemap"));
    public Emitter CreateEmitter(EmitterConfig config, Vec2 pos = default)
        => new(this, Check(Native.orst_create_emitter(Handle, config, pos), "emitter"));
    /// <summary>Advance every emitter by dt seconds</summary>
    public void UpdateEmitters(float dt) => Native.orst_update_emitters(Handle, dt);

    public GpuEmitter CreateGpuEmitter(EmitterConfig config, Vec2 pos = default) =>
        new(this, Check(Native.orst_create_gpu_emitter(Handle, config, pos), "GPU emitter"));

    public void UpdateGpuEmitters(float dt) => Native.orst_update_gpu_emitters(Handle, dt);
    public Sound LoadSound(string path) => new(this, Check(Native.orst_load_sound(Handle, path), path));

    // camera and draw state
    public Camera Camera { get => Native.orst_get_camera(Handle); set => Native.orst_set_camera(Handle, value); }
    public void LookAt(Vec2 center, float zoom = 1f, float rotation = 0f) => Camera = new Camera(center, zoom, rotation);
    public Vec2 WorldToScreen(Vec2 p) => Native.orst_world_to_screen(Handle, p);
    public Vec2 ScreenToWorld(Vec2 p) => Native.orst_screen_to_world(Handle, p);
    public int Layer { set => Native.orst_set_layer(Handle, value); }
    public float Depth { set => Native.orst_set_depth(Handle, value); }
    public Space Space { set => Native.orst_set_space(Handle, (int)value); }
    public bool Antialias { set => Native.orst_set_antialias(Handle, value); }

    // drawing
    public void Draw(Texture t, in Sprite s) => Native.orst_draw_sprite(Handle, t.Id, s);
    public void Draw(Texture t, float x, float y) => Native.orst_draw_texture(Handle, t.Id, x, y);
    public void DrawFrame(Texture t, in SpriteSheet sheet, uint frame, in Sprite s)
        => Native.orst_draw_frame(Handle, t.Id, sheet, frame, s);
    public void FillRect(Rect r, Color c) => Native.orst_fill_rect(Handle, r, c);
    public void DrawRect(Rect r, float thickness, Color c) => Native.orst_draw_rect(Handle, r, thickness, c);
    public void DrawLine(Vec2 a, Vec2 b, float thickness, Color c) => Native.orst_draw_line(Handle, a, b, thickness, c);
    public void FillCircle(Vec2 center, float radius, Color c) => Native.orst_fill_circle(Handle, center, radius, c);
    public void DrawCircle(Vec2 center, float radius, float thickness, Color c)
        => Native.orst_draw_circle(Handle, center, radius, thickness, c);
    public void DrawText(string text, Vec2 pos, float size, Color c, Font? font = null)
        => Native.orst_draw_text(Handle, font?.Id ?? DefaultFont, text, pos, size, c);
    public Vec2 MeasureText(string text, float size, Font? font = null)
        => Native.orst_measure_text(Handle, font?.Id ?? DefaultFont, text, size);

    // input
    public bool KeyDown(Key k) => Native.orst_key_down(Handle, (int)k);
    public bool KeyPressed(Key k) => Native.orst_key_pressed(Handle, (int)k);
    public bool KeyReleased(Key k) => Native.orst_key_released(Handle, (int)k);
    public float Axis(Key negative, Key positive) => Native.orst_axis(Handle, (int)negative, (int)positive);
    public bool MouseDown(MouseButton b) => Native.orst_mouse_down(Handle, (int)b);
    public bool MousePressed(MouseButton b) => Native.orst_mouse_pressed(Handle, (int)b);
    public bool MouseReleased(MouseButton b) => Native.orst_mouse_released(Handle, (int)b);
    public Vec2 Mouse => Native.orst_mouse_pos(Handle);
    public Vec2 MouseWorld => Native.orst_mouse_world(Handle);
    public float Wheel => Native.orst_mouse_wheel(Handle);
    public string TextInput => Marshal.PtrToStringUTF8(Native.orst_text_input(Handle)) ?? "";
    public void InjectKey(Key k, bool down) => Native.orst_inject_key(Handle, (int)k, down);
    public void InjectMouseButton(MouseButton b, bool down) => Native.orst_inject_mouse_button(Handle, (int)b, down);
    public void InjectMouseMove(Vec2 p) => Native.orst_inject_mouse_move(Handle, p);

    // gamepads 0..3; sticks have y down, triggers are 0..1
    public bool GamepadConnected(int pad) => Native.orst_gamepad_connected(Handle, pad);
    public string GamepadName(int pad)
    {
        var n = Native.orst_gamepad_name(Handle, pad, null, 0);
        var buf = new byte[(int)n + 1];
        Native.orst_gamepad_name(Handle, pad, buf, (nuint)buf.Length);
        return Encoding.UTF8.GetString(buf, 0, (int)n);
    }
    public bool PadDown(int pad, Pad b) => Native.orst_gamepad_down(Handle, pad, (int)b);
    public bool PadPressed(int pad, Pad b) => Native.orst_gamepad_pressed(Handle, pad, (int)b);
    public bool PadReleased(int pad, Pad b) => Native.orst_gamepad_released(Handle, pad, (int)b);
    public float PadAxis(int pad, PadAxis a) => Native.orst_gamepad_axis(Handle, pad, (int)a);
    /// <summary>Left stick as a vector (y down)</summary>
    public Vec2 PadStick(int pad) => new(PadAxis(pad, ORST.PadAxis.LeftX), PadAxis(pad, ORST.PadAxis.LeftY));
    public float GamepadDeadzone { set => Native.orst_set_gamepad_deadzone(Handle, value); }
    public void InjectGamepadConnected(int pad, bool on) => Native.orst_inject_gamepad_connected(Handle, pad, on);
    public void InjectPadButton(int pad, Pad b, bool down) => Native.orst_inject_gamepad_button(Handle, pad, (int)b, down);
    public void InjectPadAxis(int pad, PadAxis a, float v) => Native.orst_inject_gamepad_axis(Handle, pad, (int)a, v);

    /// <summary>"gpu: graphics card" or "software"</summary>
    public string RendererName
    {
        get
        {
            var n = Native.orst_renderer_name(Handle, null, 0);
            var buf = new byte[(int)n + 1];
            Native.orst_renderer_name(Handle, buf, (nuint)buf.Length);
            return Encoding.UTF8.GetString(buf, 0, (int)n);
        }
    }

    // hot reload: textures and maps reload in place when their files change
    public bool HotReload { get => Native.orst_hot_reload_enabled(Handle); set => Native.orst_set_hot_reload(Handle, value); }
    public uint ReloadChanged() => Native.orst_reload_changed(Handle);
    /// <summary>Resources reloaded by the last Poll (e.g. to re-read map objects)</summary>
    public uint Reloaded => Native.orst_reloaded(Handle);

    // debug overlay: drawn on top only while on; F3 toggles it
    public bool Debug { get => Native.orst_debug_enabled(Handle); set => Native.orst_set_debug(Handle, value); }
    /// <summary>Key that toggles the overlay; null for none</summary>
    public Key? DebugKey { set => Native.orst_set_debug_key(Handle, value is { } k ? (int)k : 0); }
    public void DebugRect(Rect r, Color? c = null) => Native.orst_debug_rect(Handle, r, c ?? DebugRed);
    public void DebugLine(Vec2 a, Vec2 b, Color? c = null) => Native.orst_debug_line(Handle, a, b, c ?? DebugRed);
    public void DebugCircle(Vec2 center, float radius, Color? c = null)
        => Native.orst_debug_circle(Handle, center, radius, c ?? DebugRed);
    public void DebugText(string text, Vec2 pos, Color? c = null) => Native.orst_debug_text(Handle, text, pos, c ?? Color.White);
    public void DebugTilemap(Tilemap map) => Native.orst_debug_tilemap(Handle, map.Id);
    private static readonly Color DebugRed = new(255, 60, 60);

    // sound
    public void StopVoice(uint voice) => Native.orst_stop_voice(Handle, voice);
    public bool VoicePlaying(uint voice) => Native.orst_voice_playing(Handle, voice);
    public void StopAllSounds() => Native.orst_stop_all_sounds(Handle);
    public void PlayMusic(string path, float volume = 1f, bool looped = true)
    {
        if (!Native.orst_play_music(Handle, path, volume, looped)) throw new OrstException(LastError);
    }
    public void StopMusic() => Native.orst_stop_music(Handle);
    public void PauseMusic(bool paused) => Native.orst_pause_music(Handle, paused);
    public bool MusicPlaying => Native.orst_music_playing(Handle);
    public float MusicVolume { set => Native.orst_set_music_volume(Handle, value); }
    public float MasterVolume { set => Native.orst_set_master_volume(Handle, value); }
}

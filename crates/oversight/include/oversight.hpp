// OverSight 2D: C++17 wrapper over oversight.h, with RAII resources.
//
//     ov::Engine engine({"My game", 320, 180, 4.0f});
//     while (engine.poll()) {
//         float dt = engine.dt();
//         engine.begin_frame(ov::Color::Black);
//         // update with dt, then draw
//         engine.end_frame();
//     }
//
// Failures to create the engine or load a resource throw ov::Error.
// Resources free themselves; they must not outlive their engine.
#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <stdexcept>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

#include "oversight.h"

namespace ov {

struct Error : std::runtime_error {
    using std::runtime_error::runtime_error;
};

inline std::string last_error() { return ov_last_error(); }

using Entity = OvEntity;

/// The name a component type is registered under. Give each component a
/// name with OV_COMPONENT(Type) after its definition, at global scope; the
/// C# binding uses the struct's name, so the same name works in both.
template <class T>
struct ComponentName {
    static_assert(sizeof(T) == 0, "name the component: OV_COMPONENT(Type) after its definition");
};

#define OV_COMPONENT(T)                                  \
    template <>                                          \
    struct ov::ComponentName<T> {                        \
        static constexpr const char *value = #T;         \
    }

struct Vec2 : OvVec2 {
    constexpr Vec2() : OvVec2{0, 0} {}
    constexpr Vec2(float x, float y) : OvVec2{x, y} {}
    constexpr Vec2(OvVec2 v) : OvVec2(v) {}
    constexpr Vec2 operator+(Vec2 o) const { return {x + o.x, y + o.y}; }
    constexpr Vec2 operator-(Vec2 o) const { return {x - o.x, y - o.y}; }
    constexpr Vec2 operator*(float s) const { return {x * s, y * s}; }
};

struct Rect : OvRect {
    constexpr Rect() : OvRect{0, 0, 0, 0} {}
    constexpr Rect(float x, float y, float w, float h) : OvRect{x, y, w, h} {}
    constexpr Rect(OvRect r) : OvRect(r) {}
    constexpr float right() const { return x + w; }
    constexpr float bottom() const { return y + h; }
    bool overlaps(const Rect &o) const { return ov_rects_overlap(*this, o); }
};

struct Color : OvColor {
    constexpr Color() : OvColor{0, 0, 0, 255} {}
    constexpr Color(uint8_t r, uint8_t g, uint8_t b, uint8_t a = 255) : OvColor{r, g, b, a} {}
    constexpr Color(OvColor c) : OvColor(c) {}
    static const Color Black, White, Transparent;
    constexpr bool operator==(const Color &o) const { return r == o.r && g == o.g && b == o.b && a == o.a; }
    constexpr bool operator!=(const Color &o) const { return !(*this == o); }
};
inline constexpr Color Color::Black{0, 0, 0, 255};
inline constexpr Color Color::White{255, 255, 255, 255};
inline constexpr Color Color::Transparent{0, 0, 0, 0};

struct Sprite : OvSprite {
    Sprite() : OvSprite(ov_default_sprite()) {}
};

/// Particle look and motion; starts from the engine's defaults
struct EmitterConfig : OvEmitter {
    EmitterConfig() : OvEmitter(ov_default_emitter()) {}
    EmitterConfig(const OvEmitter &c) : OvEmitter(c) {}
};

using Camera = OvCamera;
using SpriteSheet = OvSpriteSheet;
using Contacts = OvContacts;
using MoveResult = OvMoveResult;
using RayHit = OvRayHit;

enum class Space : int32_t { World = OV_SPACE_WORLD, Screen = OV_SPACE_SCREEN };
enum class Orientation : int32_t { Orthogonal = OV_ORTHOGONAL, Isometric = OV_ISOMETRIC };
enum class Mouse : int32_t { Left = OV_MOUSE_LEFT, Right = OV_MOUSE_RIGHT, Middle = OV_MOUSE_MIDDLE };
enum class AnimMode : int32_t { Loop = OV_ANIM_LOOP, Once = OV_ANIM_ONCE, PingPong = OV_ANIM_PING_PONG };

enum class Pad : int32_t {
    South = OV_PAD_SOUTH, East, West, North,
    LeftBumper, RightBumper,
    Back, Start, Guide,
    LeftStick, RightStick,
    DPadUp, DPadDown, DPadLeft, DPadRight,
};
enum class PadAxis : int32_t { LeftX = OV_PAD_LEFT_X, LeftY, RightX, RightY, LeftTrigger, RightTrigger };

enum class Key : int32_t {
    Unknown = OV_KEY_UNKNOWN,
    A = OV_KEY_A,
    B = OV_KEY_B,
    C = OV_KEY_C,
    D = OV_KEY_D,
    E = OV_KEY_E,
    F = OV_KEY_F,
    G = OV_KEY_G,
    H = OV_KEY_H,
    I = OV_KEY_I,
    J = OV_KEY_J,
    K = OV_KEY_K,
    L = OV_KEY_L,
    M = OV_KEY_M,
    N = OV_KEY_N,
    O = OV_KEY_O,
    P = OV_KEY_P,
    Q = OV_KEY_Q,
    R = OV_KEY_R,
    S = OV_KEY_S,
    T = OV_KEY_T,
    U = OV_KEY_U,
    V = OV_KEY_V,
    W = OV_KEY_W,
    X = OV_KEY_X,
    Y = OV_KEY_Y,
    Z = OV_KEY_Z,
    Num0 = OV_KEY_NUM0,
    Num1 = OV_KEY_NUM1,
    Num2 = OV_KEY_NUM2,
    Num3 = OV_KEY_NUM3,
    Num4 = OV_KEY_NUM4,
    Num5 = OV_KEY_NUM5,
    Num6 = OV_KEY_NUM6,
    Num7 = OV_KEY_NUM7,
    Num8 = OV_KEY_NUM8,
    Num9 = OV_KEY_NUM9,
    Space = OV_KEY_SPACE,
    Enter = OV_KEY_ENTER,
    Escape = OV_KEY_ESCAPE,
    Tab = OV_KEY_TAB,
    Backspace = OV_KEY_BACKSPACE,
    Delete = OV_KEY_DELETE,
    Insert = OV_KEY_INSERT,
    Home = OV_KEY_HOME,
    End = OV_KEY_END,
    PageUp = OV_KEY_PAGE_UP,
    PageDown = OV_KEY_PAGE_DOWN,
    Left = OV_KEY_LEFT,
    Right = OV_KEY_RIGHT,
    Up = OV_KEY_UP,
    Down = OV_KEY_DOWN,
    LeftShift = OV_KEY_LEFT_SHIFT,
    RightShift = OV_KEY_RIGHT_SHIFT,
    LeftCtrl = OV_KEY_LEFT_CTRL,
    RightCtrl = OV_KEY_RIGHT_CTRL,
    LeftAlt = OV_KEY_LEFT_ALT,
    RightAlt = OV_KEY_RIGHT_ALT,
    F1 = OV_KEY_F1,
    F2 = OV_KEY_F2,
    F3 = OV_KEY_F3,
    F4 = OV_KEY_F4,
    F5 = OV_KEY_F5,
    F6 = OV_KEY_F6,
    F7 = OV_KEY_F7,
    F8 = OV_KEY_F8,
    F9 = OV_KEY_F9,
    F10 = OV_KEY_F10,
    F11 = OV_KEY_F11,
    F12 = OV_KEY_F12,
    Minus = OV_KEY_MINUS,
    Equals = OV_KEY_EQUALS,
    LeftBracket = OV_KEY_LEFT_BRACKET,
    RightBracket = OV_KEY_RIGHT_BRACKET,
    Semicolon = OV_KEY_SEMICOLON,
    Quote = OV_KEY_QUOTE,
    Comma = OV_KEY_COMMA,
    Period = OV_KEY_PERIOD,
    Slash = OV_KEY_SLASH,
    Backslash = OV_KEY_BACKSLASH,
    Grave = OV_KEY_GRAVE,
};

enum class Renderer : int32_t { Auto = OV_RENDERER_AUTO, Software = OV_RENDERER_SOFTWARE, Gpu = OV_RENDERER_GPU };

struct Config {
    std::string title = "OverSight";
    uint32_t width = 960, height = 540;
    float scale = 1.0f;
    bool resizable = true;
    bool fullscreen = false;
    bool integer_scale = false;
    uint32_t target_fps = 60;
    bool headless = false;
    Renderer renderer = Renderer::Auto;
};

inline SpriteSheet sheet_grid(uint32_t texture_w, uint32_t texture_h, uint32_t frame_w, uint32_t frame_h,
                              uint32_t margin = 0, uint32_t spacing = 0) {
    return ov_sheet_grid(texture_w, texture_h, frame_w, frame_h, margin, spacing);
}

/// Frame indices shown fps times per second
struct Animation {
    std::vector<uint32_t> frames;
    float fps = 10.0f;
    AnimMode mode = AnimMode::Loop;

    static Animation range(uint32_t first, uint32_t count, float fps, AnimMode mode = AnimMode::Loop) {
        Animation a{{}, fps, mode};
        for (uint32_t i = 0; i < count; ++i) a.frames.push_back(first + i);
        return a;
    }
    uint32_t frame_at(float time) const {
        return ov_anim_frame_at(frames.data(), static_cast<uint32_t>(frames.size()), fps,
                                static_cast<int32_t>(mode), time);
    }
    float duration() const { return fps > 0 ? frames.size() / fps : 0.0f; }
};

enum class Ease : int32_t {
    Linear = OV_EASE_LINEAR,
    QuadIn, QuadOut, QuadInOut,
    CubicIn, CubicOut, CubicInOut,
    SineIn, SineOut, SineInOut,
    ExpoIn, ExpoOut, ExpoInOut,
    BackIn, BackOut, BackInOut,
    ElasticIn, ElasticOut, ElasticInOut,
    BounceIn, BounceOut, BounceInOut,
};
enum class Repeat : int32_t { Once = OV_REPEAT_ONCE, Loop = OV_REPEAT_LOOP, PingPong = OV_REPEAT_PING_PONG };

inline float ease(Ease e, float t) { return ov_ease(static_cast<int32_t>(e), t); }
inline float lerp(float a, float b, float t) { return a + (b - a) * t; }
inline Vec2 lerp(Vec2 a, Vec2 b, float t) { return {lerp(a.x, b.x, t), lerp(a.y, b.y, t)}; }
inline Color lerp(Color a, Color b, float t) { return ov_lerp_color(a, b, t); }

/// A value going from `from` to `to` over `duration` seconds: float, Vec2 or
/// Color. Call update(dt) every frame and read value().
template <class T>
struct Tween {
    T from{}, to{};
    float duration = 1.0f;
    Ease ease = Ease::Linear;
    Repeat repeat = Repeat::Once;
    float time = 0.0f;  // negative while waiting out a delay
    bool paused = false;

    Tween() = default;
    Tween(T from_, T to_, float duration_, Ease ease_ = Ease::Linear, Repeat repeat_ = Repeat::Once,
          float delay = 0.0f)
        : from(from_), to(to_), duration(duration_), ease(ease_), repeat(repeat_), time(-delay) {}

    T update(float dt) {
        if (!paused) time += dt;
        return value();
    }
    float progress() const {
        return ov_tween_progress(time, duration, static_cast<int32_t>(ease), static_cast<int32_t>(repeat));
    }
    T value() const { return ov::lerp(from, to, progress()); }
    bool done() const { return repeat == Repeat::Once && time >= duration; }
    void restart() { time = 0.0f; }
};

namespace detail {
// A resource id that frees itself; move-only
template <void (*Free)(OvEngine *, uint32_t)>
class Handle {
public:
    Handle() = default;
    Handle(OvEngine *e, uint32_t id) : e_(e), id_(id) {}
    Handle(const Handle &) = delete;
    Handle &operator=(const Handle &) = delete;
    Handle(Handle &&o) noexcept : e_(o.e_), id_(std::exchange(o.id_, 0)) {}
    Handle &operator=(Handle &&o) noexcept {
        if (this != &o) {
            reset();
            e_ = o.e_;
            id_ = std::exchange(o.id_, 0);
        }
        return *this;
    }
    ~Handle() { reset(); }
    void reset() {
        if (id_) Free(e_, id_);
        id_ = 0;
    }
    uint32_t id() const { return id_; }
    explicit operator bool() const { return id_ != 0; }

protected:
    OvEngine *e_ = nullptr;
    uint32_t id_ = 0;
};

inline uint32_t check(uint32_t id, const std::string &what) {
    if (!id) throw Error(what + ": " + ov_last_error());
    return id;
}

inline std::string fetch(size_t (*get)(OvEngine *, uint32_t, uint32_t, char *, size_t), OvEngine *e,
                         uint32_t map, uint32_t index) {
    std::string s(get(e, map, index, nullptr, 0), '\0');
    get(e, map, index, s.data(), s.size() + 1);
    return s;
}
}  // namespace detail

class Texture : public detail::Handle<ov_free_texture> {
public:
    using Handle::Handle;
    uint32_t width() const { uint32_t w = 0; ov_texture_size(e_, id_, &w, nullptr); return w; }
    uint32_t height() const { uint32_t h = 0; ov_texture_size(e_, id_, nullptr, &h); return h; }
    void set_smooth(bool smooth) { ov_set_texture_smooth(e_, id_, smooth); }
    SpriteSheet sheet(uint32_t frame_w, uint32_t frame_h, uint32_t margin = 0, uint32_t spacing = 0) const {
        return sheet_grid(width(), height(), frame_w, frame_h, margin, spacing);
    }
};

class Font : public detail::Handle<ov_free_font> {
public:
    using Handle::Handle;
};

class Sound : public detail::Handle<ov_free_sound> {
public:
    using Handle::Handle;
    /// Returns a voice id for Engine::stop_voice
    uint32_t play(float volume = 1.0f, float pitch = 1.0f) { return ov_play_sound(e_, id_, volume, pitch); }
};

class Emitter : public detail::Handle<ov_free_emitter> {
public:
    using Handle::Handle;
    EmitterConfig config() const { return ov_emitter_config(e_, id_); }
    void set_config(const EmitterConfig &c) { ov_set_emitter_config(e_, id_, &c); }
    Vec2 pos() const { return ov_emitter_pos(e_, id_); }
    void set_pos(Vec2 p) { ov_set_emitter_pos(e_, id_, p); }
    bool emitting() const { return ov_emitter_emitting(e_, id_); }
    void set_emitting(bool on) { ov_set_emitter_emitting(e_, id_, on); }
    void burst(uint32_t count) { ov_emitter_burst(e_, id_, count); }
    uint32_t count() const { return ov_emitter_count(e_, id_); }
    void clear() { ov_emitter_clear(e_, id_); }
    void update(float dt) { ov_update_emitter(e_, id_, dt); }
    void draw() { ov_draw_emitter(e_, id_); }
};

struct MapObject {
    uint32_t id = 0;
    std::string name, cls;
    Rect rect;
    bool point = false, visible = true;
    float rotation = 0;
    uint32_t gid = 0;
    uint32_t layer = 0;
    uint32_t index = 0; // for number() and string()
};

class Tilemap : public detail::Handle<ov_free_tilemap> {
public:
    using Handle::Handle;
    OvTilemapInfo info() const { OvTilemapInfo i{}; ov_tilemap_info(e_, id_, &i); return i; }
    int layer_index(const std::string &name) const { return ov_tilemap_layer_index(e_, id_, name.c_str()); }
    uint32_t tile(uint32_t layer, int x, int y) const { return ov_tilemap_get_tile(e_, id_, layer, x, y); }
    void set_tile(uint32_t layer, int x, int y, uint32_t gid) { ov_tilemap_set_tile(e_, id_, layer, x, y, gid); }
    bool add_tileset(uint32_t first_gid, const Texture &texture, SpriteSheet sheet) {
        return ov_tilemap_add_tileset(e_, id_, first_gid, texture.id(), sheet);
    }
    bool set_tile_flag(uint32_t gid, const std::string &name, bool value = true) {
        return ov_tilemap_set_tile_flag(e_, id_, gid, name.c_str(), value);
    }
    bool solid(int x, int y) const { return ov_tilemap_is_solid(e_, id_, x, y); }
    Vec2 tile_to_world(Vec2 tile) const { return ov_tilemap_tile_to_world(e_, id_, tile); }
    Vec2 world_to_tile(Vec2 p) const { return ov_tilemap_world_to_tile(e_, id_, p); }
    Vec2 cell_center(int x, int y) const { return ov_tilemap_cell_center(e_, id_, x, y); }

    void draw(int base_layer = 0) { ov_draw_tilemap(e_, id_, base_layer); }
    void draw_layer(uint32_t layer, int draw_layer, bool y_sort = false, float depth = 0,
                    Color tint = Color::White) {
        ov_draw_tilemap_layer(e_, id_, layer, draw_layer, depth, y_sort, tint);
    }

    std::vector<MapObject> objects() const {
        std::vector<MapObject> out;
        uint32_t n = info().object_count;
        for (uint32_t i = 0; i < n; ++i) {
            OvMapObject o{};
            ov_tilemap_object(e_, id_, i, &o);
            MapObject m;
            m.id = o.id;
            m.name = detail::fetch(ov_tilemap_object_name, e_, id_, i);
            m.cls = detail::fetch(ov_tilemap_object_class, e_, id_, i);
            m.rect = o.rect;
            m.point = o.point;
            m.visible = o.visible;
            m.rotation = o.rotation;
            m.gid = o.gid;
            m.layer = o.layer;
            m.index = i;
            out.push_back(std::move(m));
        }
        return out;
    }
    std::optional<MapObject> find_object(const std::string &name) const {
        int i = ov_tilemap_find_object(e_, id_, name.c_str());
        if (i < 0) return std::nullopt;
        return objects()[static_cast<size_t>(i)];
    }
    double number(const MapObject &o, const std::string &prop, double fallback = 0) const {
        return ov_tilemap_object_number(e_, id_, o.index, prop.c_str(), fallback);
    }
    std::optional<std::string> string(const MapObject &o, const std::string &prop) const {
        int64_t n = ov_tilemap_object_string(e_, id_, o.index, prop.c_str(), nullptr, 0);
        if (n < 0) return std::nullopt;
        std::string s(static_cast<size_t>(n), '\0');
        ov_tilemap_object_string(e_, id_, o.index, prop.c_str(), s.data(), s.size() + 1);
        return s;
    }

    MoveResult move_box(Rect rect, Vec2 delta) const { return ov_move_box(e_, id_, rect, delta); }
    Contacts touching(Rect rect) const { return ov_touching(e_, id_, rect); }
    std::optional<RayHit> raycast(Vec2 from, Vec2 to) const {
        RayHit hit{};
        if (!ov_raycast(e_, id_, from, to, &hit)) return std::nullopt;
        return hit;
    }
    bool point_solid(Vec2 p) const { return ov_point_solid(e_, id_, p); }
    bool rect_solid(Rect r) const { return ov_rect_solid(e_, id_, r); }
};

class Engine {
public:
    explicit Engine(const Config &c = {}) {
        OvConfig oc{c.title.c_str(), c.width, c.height, c.scale, c.resizable,
                    c.fullscreen, c.integer_scale, c.target_fps, c.headless,
                    static_cast<int32_t>(c.renderer)};
        e_ = ov_engine_create(&oc);
        if (!e_) throw Error(std::string("cannot start the engine: ") + ov_last_error());
    }
    Engine(const Engine &) = delete;
    Engine &operator=(const Engine &) = delete;
    ~Engine() { ov_engine_destroy(e_); }

    OvEngine *raw() { return e_; }

    // frame
    bool poll() { return ov_poll(e_); }
    void quit() { ov_quit(e_); }
    float dt() const { return ov_dt(e_); }
    double time() const { return ov_time(e_); }
    float fps() const { return ov_fps(e_); }
    uint64_t frame() const { return ov_frame(e_); }
    // fixed logic ticks: for (uint32_t i = 0; i < engine.ticks(); ++i) update(engine.tick_dt());
    void set_tick_rate(uint32_t ticks_per_second) { ov_set_tick_rate(e_, ticks_per_second); }
    uint32_t ticks() const { return ov_ticks(e_); }
    float tick_dt() const { return ov_tick_dt(e_); }
    float tick_alpha() const { return ov_tick_alpha(e_); }
    uint64_t tick_count() const { return ov_tick_count(e_); }
    // seeded random numbers, the same on any machine
    void random_seed(uint64_t seed) { ov_random_seed(e_, seed); }
    uint32_t random_u32() { return ov_random_u32(e_); }
    float random_float() { return ov_random_float(e_); }
    float random_range(float min, float max) { return ov_random_range(e_, min, max); }
    int32_t random_int(int32_t min, int32_t max) { return ov_random_int(e_, min, max); }
    void begin_frame(Color clear = Color::Black) { ov_begin_frame(e_, clear); }
    void end_frame() { ov_end_frame(e_); }
    void set_title(const std::string &t) { ov_set_title(e_, t.c_str()); }
    void set_fullscreen(bool on) { ov_set_fullscreen(e_, on); }
    bool fullscreen() const { return ov_is_fullscreen(e_); }
    void set_target_fps(uint32_t fps) { ov_set_target_fps(e_, fps); }
    std::pair<uint32_t, uint32_t> canvas_size() const {
        uint32_t w = 0, h = 0;
        ov_canvas_size(e_, &w, &h);
        return {w, h};
    }
    void save_screenshot(const std::string &path) {
        if (!ov_save_screenshot(e_, path.c_str())) throw Error(ov_last_error());
    }
    Color pixel(uint32_t x, uint32_t y) const { return ov_pixel(e_, x, y); }

    // resources
    Texture load_texture(const std::string &path) {
        return {e_, detail::check(ov_load_texture(e_, path.c_str()), path)};
    }
    Texture texture_from_rgba(uint32_t w, uint32_t h, const uint8_t *rgba) {
        return {e_, detail::check(ov_texture_from_rgba(e_, w, h, rgba), "texture")};
    }
    Font load_font(const std::string &path) { return {e_, detail::check(ov_load_font(e_, path.c_str()), path)}; }
    Tilemap load_tilemap(const std::string &path) {
        return {e_, detail::check(ov_load_tilemap(e_, path.c_str()), path)};
    }
    Tilemap create_tilemap(Orientation o, uint32_t w, uint32_t h, uint32_t tile_w, uint32_t tile_h,
                           uint32_t layers = 1) {
        return {e_, detail::check(ov_create_tilemap(e_, static_cast<int32_t>(o), w, h, tile_w, tile_h, layers),
                                  "tilemap")};
    }
    Emitter create_emitter(const EmitterConfig &config, Vec2 pos = {}) {
        return {e_, detail::check(ov_create_emitter(e_, &config, pos), "emitter")};
    }
    /// Advance every emitter by dt seconds
    void update_emitters(float dt) { ov_update_emitters(e_, dt); }
    Sound load_sound(const std::string &path) { return {e_, detail::check(ov_load_sound(e_, path.c_str()), path)}; }

    // camera and draw state
    Camera camera() const { return ov_get_camera(e_); }
    void set_camera(const Camera &c) { ov_set_camera(e_, c); }
    /// Centre the camera on a world point
    void look_at(Vec2 center, float zoom = 1.0f, float rotation = 0.0f) {
        ov_set_camera(e_, OvCamera{center, zoom, rotation});
    }
    Vec2 world_to_screen(Vec2 p) const { return ov_world_to_screen(e_, p); }
    Vec2 screen_to_world(Vec2 p) const { return ov_screen_to_world(e_, p); }
    void set_layer(int layer) { ov_set_layer(e_, layer); }
    void set_depth(float depth) { ov_set_depth(e_, depth); }
    void set_space(Space s) { ov_set_space(e_, static_cast<int32_t>(s)); }
    void set_antialias(bool on) { ov_set_antialias(e_, on); }

    // drawing
    void draw(const Texture &t, const Sprite &s) { ov_draw_sprite(e_, t.id(), &s); }
    void draw(const Texture &t, float x, float y) { ov_draw_texture(e_, t.id(), x, y); }
    void draw_frame(const Texture &t, const SpriteSheet &sheet, uint32_t frame, const Sprite &s) {
        ov_draw_frame(e_, t.id(), &sheet, frame, &s);
    }
    void fill_rect(Rect r, Color c) { ov_fill_rect(e_, r, c); }
    void draw_rect(Rect r, float thickness, Color c) { ov_draw_rect(e_, r, thickness, c); }
    void draw_line(Vec2 a, Vec2 b, float thickness, Color c) { ov_draw_line(e_, a, b, thickness, c); }
    void fill_circle(Vec2 center, float radius, Color c) { ov_fill_circle(e_, center, radius, c); }
    void draw_circle(Vec2 center, float radius, float thickness, Color c) {
        ov_draw_circle(e_, center, radius, thickness, c);
    }
    void draw_text(const std::string &text, Vec2 pos, float size, Color c, const Font *font = nullptr) {
        ov_draw_text(e_, font ? font->id() : OV_DEFAULT_FONT, text.c_str(), pos, size, c);
    }
    Vec2 measure_text(const std::string &text, float size, const Font *font = nullptr) {
        return ov_measure_text(e_, font ? font->id() : OV_DEFAULT_FONT, text.c_str(), size);
    }

    // input
    bool key_down(Key k) const { return ov_key_down(e_, static_cast<int32_t>(k)); }
    bool key_pressed(Key k) const { return ov_key_pressed(e_, static_cast<int32_t>(k)); }
    bool key_released(Key k) const { return ov_key_released(e_, static_cast<int32_t>(k)); }
    float axis(Key negative, Key positive) const {
        return ov_axis(e_, static_cast<int32_t>(negative), static_cast<int32_t>(positive));
    }
    bool mouse_down(Mouse b) const { return ov_mouse_down(e_, static_cast<int32_t>(b)); }
    bool mouse_pressed(Mouse b) const { return ov_mouse_pressed(e_, static_cast<int32_t>(b)); }
    bool mouse_released(Mouse b) const { return ov_mouse_released(e_, static_cast<int32_t>(b)); }
    Vec2 mouse() const { return ov_mouse_pos(e_); }
    Vec2 mouse_world() const { return ov_mouse_world(e_); }
    float wheel() const { return ov_mouse_wheel(e_); }
    std::string text_input() { return ov_text_input(e_); }
    void inject_key(Key k, bool down) { ov_inject_key(e_, static_cast<int32_t>(k), down); }
    void inject_mouse_button(Mouse b, bool down) { ov_inject_mouse_button(e_, static_cast<int32_t>(b), down); }
    void inject_mouse_move(Vec2 p) { ov_inject_mouse_move(e_, p); }

    // gamepads 0..3; sticks have y down, triggers are 0..1
    bool gamepad_connected(int pad) const { return ov_gamepad_connected(e_, pad); }
    std::string gamepad_name(int pad) const {
        std::string s(ov_gamepad_name(e_, pad, nullptr, 0), '\0');
        ov_gamepad_name(e_, pad, s.data(), s.size() + 1);
        return s;
    }
    bool pad_down(int pad, Pad b) const { return ov_gamepad_down(e_, pad, static_cast<int32_t>(b)); }
    bool pad_pressed(int pad, Pad b) const { return ov_gamepad_pressed(e_, pad, static_cast<int32_t>(b)); }
    bool pad_released(int pad, Pad b) const { return ov_gamepad_released(e_, pad, static_cast<int32_t>(b)); }
    float pad_axis(int pad, PadAxis a) const { return ov_gamepad_axis(e_, pad, static_cast<int32_t>(a)); }
    /// Left stick as a vector (y down)
    Vec2 pad_stick(int pad) const { return {pad_axis(pad, PadAxis::LeftX), pad_axis(pad, PadAxis::LeftY)}; }
    void set_gamepad_deadzone(float dz) { ov_set_gamepad_deadzone(e_, dz); }
    void inject_gamepad_connected(int pad, bool on) { ov_inject_gamepad_connected(e_, pad, on); }
    void inject_pad_button(int pad, Pad b, bool down) {
        ov_inject_gamepad_button(e_, pad, static_cast<int32_t>(b), down);
    }
    void inject_pad_axis(int pad, PadAxis a, float v) { ov_inject_gamepad_axis(e_, pad, static_cast<int32_t>(a), v); }

    /// "gpu: <graphics card>" or "software"
    std::string renderer_name() const {
        std::string s(ov_renderer_name(e_, nullptr, 0), '\0');
        ov_renderer_name(e_, s.data(), s.size() + 1);
        return s;
    }

    // hot reload: textures and maps reload in place when their files change
    void set_hot_reload(bool on) { ov_set_hot_reload(e_, on); }
    bool hot_reload() const { return ov_hot_reload_enabled(e_); }
    uint32_t reload_changed() { return ov_reload_changed(e_); }
    /// Resources reloaded by the last poll (e.g. to re-read map objects)
    uint32_t reloaded() const { return ov_reloaded(e_); }

    // debug overlay: drawn on top only while on; F3 toggles it
    void set_debug(bool on) { ov_set_debug(e_, on); }
    bool debug() const { return ov_debug_enabled(e_); }
    void set_debug_key(std::optional<Key> k) { ov_set_debug_key(e_, k ? static_cast<int32_t>(*k) : 0); }
    void debug_rect(Rect r, Color c = {255, 60, 60}) { ov_debug_rect(e_, r, c); }
    void debug_line(Vec2 a, Vec2 b, Color c = {255, 60, 60}) { ov_debug_line(e_, a, b, c); }
    void debug_circle(Vec2 center, float radius, Color c = {255, 60, 60}) { ov_debug_circle(e_, center, radius, c); }
    void debug_text(const std::string &text, Vec2 pos, Color c = Color::White) {
        ov_debug_text(e_, text.c_str(), pos, c);
    }
    void debug_tilemap(const Tilemap &map) { ov_debug_tilemap(e_, map.id()); }

    // entities and components: plain structs named with OV_COMPONENT(Type).
    // Pointers from get() stay valid until the next spawn, despawn, set or remove.
    template <class T>
    uint32_t component() {
        static_assert(std::is_trivially_copyable_v<T>, "components are plain data");
        uint32_t id = ov_component(e_, ComponentName<T>::value, sizeof(T), alignof(T));
        if (id == 0) throw Error(ov_last_error());
        return id;
    }
    Entity spawn() { return ov_spawn(e_); }
    bool despawn(Entity en) { return ov_despawn(e_, en); }
    bool alive(Entity en) const { return ov_alive(e_, en); }
    uint32_t entity_count() const { return ov_entity_count(e_); }
    template <class T>
    T &set(Entity en, const T &value) {
        uint32_t c = component<T>();
        if (!ov_set(e_, en, c, &value)) throw Error("set on a dead entity");
        return *static_cast<T *>(ov_get(e_, en, c));
    }
    template <class T>
    T *get(Entity en) { return static_cast<T *>(ov_get(e_, en, component<T>())); }
    template <class T>
    bool has(Entity en) { return ov_has(e_, en, component<T>()); }
    template <class T>
    bool remove(Entity en) { return ov_remove(e_, en, component<T>()); }
    /// Entities with all of Ts (none: every entity)
    template <class... Ts>
    std::vector<Entity> query() {
        std::array<uint32_t, sizeof...(Ts)> ids{component<Ts>()...};
        std::vector<Entity> out(ov_entity_count(e_));
        out.resize(ov_query(e_, ids.data(), uint32_t(ids.size()), out.data(), uint32_t(out.size())));
        return out;
    }
    /// f(entity, Ts &...) for each entity with all of Ts. Spawning or
    /// changing components inside f moves data: collect and do it after.
    template <class... Ts, class F>
    void each(F &&f) {
        std::array<uint32_t, sizeof...(Ts)> ids{component<Ts>()...};
        for (Entity en : query<Ts...>()) each_one<Ts...>(f, en, ids, std::index_sequence_for<Ts...>{});
    }
    void clear_world() { ov_world_clear(e_); }
    /// Entities, components, random generator and tick counter as bytes
    std::vector<uint8_t> snapshot() const {
        std::vector<uint8_t> bytes(ov_snapshot(e_, nullptr, 0));
        ov_snapshot(e_, bytes.data(), bytes.size());
        return bytes;
    }
    /// Continue from a snapshot; throws (and changes nothing) on bad bytes
    void restore(const std::vector<uint8_t> &bytes) {
        if (!ov_restore(e_, bytes.data(), bytes.size())) throw Error(ov_last_error());
    }
    uint64_t state_hash() const { return ov_state_hash(e_); }

    // sound
    void stop_voice(uint32_t voice) { ov_stop_voice(e_, voice); }
    bool voice_playing(uint32_t voice) const { return ov_voice_playing(e_, voice); }
    void stop_all_sounds() { ov_stop_all_sounds(e_); }
    void play_music(const std::string &path, float volume = 1.0f, bool looped = true) {
        if (!ov_play_music(e_, path.c_str(), volume, looped)) throw Error(ov_last_error());
    }
    void stop_music() { ov_stop_music(e_); }
    void pause_music(bool paused) { ov_pause_music(e_, paused); }
    bool music_playing() const { return ov_music_playing(e_); }
    void set_music_volume(float v) { ov_set_music_volume(e_, v); }
    void set_master_volume(float v) { ov_set_master_volume(e_, v); }

private:
    template <class... Ts, class F, std::size_t... I>
    void each_one(F &f, Entity en, const std::array<uint32_t, sizeof...(Ts)> &ids, std::index_sequence<I...>) {
        f(en, *static_cast<Ts *>(ov_get(e_, en, ids[I]))...);
    }

    OvEngine *e_ = nullptr;
};

}  // namespace ov

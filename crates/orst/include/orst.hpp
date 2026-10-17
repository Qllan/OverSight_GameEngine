// ORST: C++17 wrapper over orst.h, with RAII resources.
//
//     orst::Engine engine({"My game", 320, 180, 4.0f});
//     while (engine.poll()) {
//         float dt = engine.dt();
//         engine.begin_frame(orst::Color::Black);
//         // update with dt, then draw
//         engine.end_frame();
//     }
//
// Failures to create the engine or load a resource throw orst::Error.
// Resources free themselves; they must not outlive their engine.
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <initializer_list>
#include <optional>
#include <stdexcept>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

#include "orst.h"

namespace orst {

struct Error : std::runtime_error {
    using std::runtime_error::runtime_error;
};

inline std::string last_error() { return orst_last_error(); }

using Entity = OrstEntity;

/// The name a component type is registered under. Give each component a
/// name with ORST_COMPONENT(Type) after its definition, at global scope; the
/// C# binding uses the struct's name, so the same name works in both.
template <class T>
struct ComponentName {
    static_assert(sizeof(T) == 0, "name the component: ORST_COMPONENT(Type) after its definition");
};

#define ORST_COMPONENT(T)                                  \
    template <>                                          \
    struct orst::ComponentName<T> {                        \
        static constexpr const char *value = #T;         \
    }

struct Vec2 : OrstVec2 {
    constexpr Vec2() : OrstVec2{0, 0} {}
    constexpr Vec2(float x, float y) : OrstVec2{x, y} {}
    constexpr Vec2(OrstVec2 v) : OrstVec2(v) {}
    constexpr Vec2 operator+(Vec2 o) const { return {x + o.x, y + o.y}; }
    constexpr Vec2 operator-(Vec2 o) const { return {x - o.x, y - o.y}; }
    constexpr Vec2 operator*(float s) const { return {x * s, y * s}; }
};

struct Rect : OrstRect {
    constexpr Rect() : OrstRect{0, 0, 0, 0} {}
    constexpr Rect(float x, float y, float w, float h) : OrstRect{x, y, w, h} {}
    constexpr Rect(OrstRect r) : OrstRect(r) {}
    constexpr float right() const { return x + w; }
    constexpr float bottom() const { return y + h; }
    bool overlaps(const Rect &o) const { return orst_rects_overlap(*this, o); }
};

struct Color : OrstColor {
    constexpr Color() : OrstColor{0, 0, 0, 255} {}
    constexpr Color(uint8_t r, uint8_t g, uint8_t b, uint8_t a = 255) : OrstColor{r, g, b, a} {}
    constexpr Color(OrstColor c) : OrstColor(c) {}
    static const Color Black, White, Transparent;
    constexpr bool operator==(const Color &o) const { return r == o.r && g == o.g && b == o.b && a == o.a; }
    constexpr bool operator!=(const Color &o) const { return !(*this == o); }
};
inline constexpr Color Color::Black{0, 0, 0, 255};
inline constexpr Color Color::White{255, 255, 255, 255};
inline constexpr Color Color::Transparent{0, 0, 0, 0};

struct Sprite : OrstSprite {
    Sprite() : OrstSprite(orst_default_sprite()) {}
};

/// Particle look and motion; starts from the engine's defaults
struct EmitterConfig : OrstEmitter {
    EmitterConfig() : OrstEmitter(orst_default_emitter()) {}
    EmitterConfig(const OrstEmitter &c) : OrstEmitter(c) {}
};

using Camera = OrstCamera;
using SpriteSheet = OrstSpriteSheet;
using Contacts = OrstContacts;
using MoveResult = OrstMoveResult;
using RayHit = OrstRayHit;

// Built-in components: the engine moves (update_motion) and draws
// (draw_entities) entities that have them
struct Transform : OrstTransform {
    constexpr Transform() : OrstTransform{{0, 0}, 0, {1, 1}} {}
    constexpr Transform(Vec2 pos, float rotation = 0, Vec2 scale = {1, 1}) : OrstTransform{pos, rotation, scale} {}
};
struct Velocity : OrstVelocity {
    constexpr Velocity() : OrstVelocity{{0, 0}, 0} {}
    constexpr Velocity(Vec2 linear, float angular = 0) : OrstVelocity{linear, angular} {}
};
struct SpriteRenderer : OrstSpriteRenderer {
    constexpr SpriteRenderer() : OrstSpriteRenderer{0, {0, 0, 0, 0}, {0, 0}, {255, 255, 255, 255}, 0, 0, 0, 0, 0} {}
};
struct Body : OrstBody {
    constexpr Body() : OrstBody{{0, 0, 0, 0}, 0, {false, false, false, false}} {}
    constexpr Body(Rect bounds, uint32_t tilemap = 0) : OrstBody{bounds, tilemap, {false, false, false, false}} {}
};
struct Collider : OrstCollider {
    constexpr Collider() : OrstCollider{ORST_COLLIDER_BOX, 0, {0, 0}, {0, 0}, 0, 1, 0xFFFFFFFFu, 0, {0, 0, 0}} {}
    static constexpr Collider box(Vec2 size, Vec2 offset = {}) {
        Collider c;
        c.size = size;
        c.offset = offset;
        return c;
    }
    static constexpr Collider circle(float radius, Vec2 offset = {}) {
        Collider c;
        c.kind = ORST_COLLIDER_CIRCLE;
        c.radius = radius;
        c.offset = offset;
        return c;
    }
    static constexpr Collider from_polygon(uint32_t id, Vec2 offset = {}) {
        Collider c;
        c.kind = ORST_COLLIDER_POLYGON;
        c.polygon = id;
        c.offset = offset;
        return c;
    }
};
struct RigidBody : OrstRigidBody {
    constexpr RigidBody(uint32_t kind = ORST_BODY_DYNAMIC)
        : OrstRigidBody{kind, 1.0f, 0.5f, 0.0f, 0.0f, 0.0f, 1.0f, 0, 0, {0, 0}} {}
    static constexpr RigidBody dynamic() { return RigidBody(ORST_BODY_DYNAMIC); }
    static constexpr RigidBody fixed() { return RigidBody(ORST_BODY_FIXED); }
    static constexpr RigidBody kinematic() { return RigidBody(ORST_BODY_KINEMATIC); }
};
/// A light at the entity's position (see OrstLight)
struct Light : OrstLight {
    constexpr Light(float radius = 160.0f, Color color = Color::White, float intensity = 1.0f)
        : OrstLight{{0, 0}, color, intensity, radius, 0.5f, 4.0f, 1, {0, 0, 0}} {}
};
/// What an entity is called (see OrstName)
struct Name : OrstName {
    Name() : OrstName{} {}
    Name(const std::string &name) : OrstName{} {
        size_t n = name.size() < sizeof(text) ? name.size() : sizeof(text);
        while (n > 0 && n < name.size() && (static_cast<unsigned char>(name[n]) & 0xC0) == 0x80) --n;
        name.copy(text, n);
    }
    std::string str() const {
        const void *zero = std::memchr(text, 0, sizeof(text));
        return std::string(text, zero ? static_cast<const char *>(zero) - text : sizeof(text));
    }
};
using Overlap = OrstOverlap;
using EntityHit = OrstEntityHit;
enum class ContactState : int32_t { Begin = ORST_CONTACT_BEGIN, Stay = ORST_CONTACT_STAY, End = ORST_CONTACT_END };
struct Contact : OrstContact {
    bool begin() const { return state == ORST_CONTACT_BEGIN; }
    bool end() const { return state == ORST_CONTACT_END; }
    /// The other entity when one of them is `me`
    OrstEntity other(OrstEntity me) const { return a == me ? b : a; }
};
template <>
struct ComponentName<Transform> { static constexpr const char *value = "Transform"; };
template <>
struct ComponentName<Velocity> { static constexpr const char *value = "Velocity"; };
template <>
struct ComponentName<SpriteRenderer> { static constexpr const char *value = "SpriteRenderer"; };
template <>
struct ComponentName<Body> { static constexpr const char *value = "Body"; };
template <>
struct ComponentName<Collider> { static constexpr const char *value = "Collider"; };
template <>
struct ComponentName<RigidBody> { static constexpr const char *value = "RigidBody"; };
template <>
struct ComponentName<Light> { static constexpr const char *value = "Light"; };
template <>
struct ComponentName<Name> { static constexpr const char *value = "Name"; };

enum class Transition : int32_t { Cut = ORST_TRANSITION_CUT, Fade = ORST_TRANSITION_FADE, Wipe = ORST_TRANSITION_WIPE };
enum class Space : int32_t { World = ORST_SPACE_WORLD, Screen = ORST_SPACE_SCREEN };
enum class Orientation : int32_t { Orthogonal = ORST_ORTHOGONAL, Isometric = ORST_ISOMETRIC };
enum class Mouse : int32_t { Left = ORST_MOUSE_LEFT, Right = ORST_MOUSE_RIGHT, Middle = ORST_MOUSE_MIDDLE };
enum class AnimMode : int32_t { Loop = ORST_ANIM_LOOP, Once = ORST_ANIM_ONCE, PingPong = ORST_ANIM_PING_PONG };

enum class Pad : int32_t {
    South = ORST_PAD_SOUTH, East, West, North,
    LeftBumper, RightBumper,
    Back, Start, Guide,
    LeftStick, RightStick,
    DPadUp, DPadDown, DPadLeft, DPadRight,
};
enum class PadAxis : int32_t { LeftX = ORST_PAD_LEFT_X, LeftY, RightX, RightY, LeftTrigger, RightTrigger };

enum class Key : int32_t {
    Unknown = ORST_KEY_UNKNOWN,
    A = ORST_KEY_A,
    B = ORST_KEY_B,
    C = ORST_KEY_C,
    D = ORST_KEY_D,
    E = ORST_KEY_E,
    F = ORST_KEY_F,
    G = ORST_KEY_G,
    H = ORST_KEY_H,
    I = ORST_KEY_I,
    J = ORST_KEY_J,
    K = ORST_KEY_K,
    L = ORST_KEY_L,
    M = ORST_KEY_M,
    N = ORST_KEY_N,
    O = ORST_KEY_O,
    P = ORST_KEY_P,
    Q = ORST_KEY_Q,
    R = ORST_KEY_R,
    S = ORST_KEY_S,
    T = ORST_KEY_T,
    U = ORST_KEY_U,
    V = ORST_KEY_V,
    W = ORST_KEY_W,
    X = ORST_KEY_X,
    Y = ORST_KEY_Y,
    Z = ORST_KEY_Z,
    Num0 = ORST_KEY_NUM0,
    Num1 = ORST_KEY_NUM1,
    Num2 = ORST_KEY_NUM2,
    Num3 = ORST_KEY_NUM3,
    Num4 = ORST_KEY_NUM4,
    Num5 = ORST_KEY_NUM5,
    Num6 = ORST_KEY_NUM6,
    Num7 = ORST_KEY_NUM7,
    Num8 = ORST_KEY_NUM8,
    Num9 = ORST_KEY_NUM9,
    Space = ORST_KEY_SPACE,
    Enter = ORST_KEY_ENTER,
    Escape = ORST_KEY_ESCAPE,
    Tab = ORST_KEY_TAB,
    Backspace = ORST_KEY_BACKSPACE,
    Delete = ORST_KEY_DELETE,
    Insert = ORST_KEY_INSERT,
    Home = ORST_KEY_HOME,
    End = ORST_KEY_END,
    PageUp = ORST_KEY_PAGE_UP,
    PageDown = ORST_KEY_PAGE_DOWN,
    Left = ORST_KEY_LEFT,
    Right = ORST_KEY_RIGHT,
    Up = ORST_KEY_UP,
    Down = ORST_KEY_DOWN,
    LeftShift = ORST_KEY_LEFT_SHIFT,
    RightShift = ORST_KEY_RIGHT_SHIFT,
    LeftCtrl = ORST_KEY_LEFT_CTRL,
    RightCtrl = ORST_KEY_RIGHT_CTRL,
    LeftAlt = ORST_KEY_LEFT_ALT,
    RightAlt = ORST_KEY_RIGHT_ALT,
    F1 = ORST_KEY_F1,
    F2 = ORST_KEY_F2,
    F3 = ORST_KEY_F3,
    F4 = ORST_KEY_F4,
    F5 = ORST_KEY_F5,
    F6 = ORST_KEY_F6,
    F7 = ORST_KEY_F7,
    F8 = ORST_KEY_F8,
    F9 = ORST_KEY_F9,
    F10 = ORST_KEY_F10,
    F11 = ORST_KEY_F11,
    F12 = ORST_KEY_F12,
    Minus = ORST_KEY_MINUS,
    Equals = ORST_KEY_EQUALS,
    LeftBracket = ORST_KEY_LEFT_BRACKET,
    RightBracket = ORST_KEY_RIGHT_BRACKET,
    Semicolon = ORST_KEY_SEMICOLON,
    Quote = ORST_KEY_QUOTE,
    Comma = ORST_KEY_COMMA,
    Period = ORST_KEY_PERIOD,
    Slash = ORST_KEY_SLASH,
    Backslash = ORST_KEY_BACKSLASH,
    Grave = ORST_KEY_GRAVE,
};

enum class Renderer : int32_t { Auto = ORST_RENDERER_AUTO, Software = ORST_RENDERER_SOFTWARE, Gpu = ORST_RENDERER_GPU };

struct Config {
    std::string title = "ORST";
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
    return orst_sheet_grid(texture_w, texture_h, frame_w, frame_h, margin, spacing);
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
        return orst_anim_frame_at(frames.data(), static_cast<uint32_t>(frames.size()), fps,
                                static_cast<int32_t>(mode), time);
    }
    float duration() const { return fps > 0 ? frames.size() / fps : 0.0f; }
};

enum class Ease : int32_t {
    Linear = ORST_EASE_LINEAR,
    QuadIn, QuadOut, QuadInOut,
    CubicIn, CubicOut, CubicInOut,
    SineIn, SineOut, SineInOut,
    ExpoIn, ExpoOut, ExpoInOut,
    BackIn, BackOut, BackInOut,
    ElasticIn, ElasticOut, ElasticInOut,
    BounceIn, BounceOut, BounceInOut,
};
enum class Repeat : int32_t { Once = ORST_REPEAT_ONCE, Loop = ORST_REPEAT_LOOP, PingPong = ORST_REPEAT_PING_PONG };

inline float ease(Ease e, float t) { return orst_ease(static_cast<int32_t>(e), t); }
inline float lerp(float a, float b, float t) { return a + (b - a) * t; }
inline Vec2 lerp(Vec2 a, Vec2 b, float t) { return {lerp(a.x, b.x, t), lerp(a.y, b.y, t)}; }
inline Color lerp(Color a, Color b, float t) { return orst_lerp_color(a, b, t); }

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
        return orst_tween_progress(time, duration, static_cast<int32_t>(ease), static_cast<int32_t>(repeat));
    }
    T value() const { return orst::lerp(from, to, progress()); }
    bool done() const { return repeat == Repeat::Once && time >= duration; }
    void restart() { time = 0.0f; }
};

namespace detail {
// A resource id that frees itself; move-only
template <void (*Free)(OrstEngine *, uint32_t)>
class Handle {
public:
    Handle() = default;
    Handle(OrstEngine *e, uint32_t id) : e_(e), id_(id) {}
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
    OrstEngine *e_ = nullptr;
    uint32_t id_ = 0;
};

inline uint32_t check(uint32_t id, const std::string &what) {
    if (!id) throw Error(what + ": " + orst_last_error());
    return id;
}

inline std::string fetch(size_t (*get)(OrstEngine *, uint32_t, uint32_t, char *, size_t), OrstEngine *e,
                         uint32_t map, uint32_t index) {
    std::string s(get(e, map, index, nullptr, 0), '\0');
    get(e, map, index, s.data(), s.size() + 1);
    return s;
}
}  // namespace detail

class Texture : public detail::Handle<orst_free_texture> {
public:
    using Handle::Handle;
    uint32_t width() const { uint32_t w = 0; orst_texture_size(e_, id_, &w, nullptr); return w; }
    uint32_t height() const { uint32_t h = 0; orst_texture_size(e_, id_, nullptr, &h); return h; }
    void set_smooth(bool smooth) { orst_set_texture_smooth(e_, id_, smooth); }
    SpriteSheet sheet(uint32_t frame_w, uint32_t frame_h, uint32_t margin = 0, uint32_t spacing = 0) const {
        return sheet_grid(width(), height(), frame_w, frame_h, margin, spacing);
    }
};

/// A WGSL post effect shader (see orst_create_shader); GPU only
class Shader : public detail::Handle<orst_free_shader> {
public:
    using Handle::Handle;
};

/// One post effect of a chain
struct Effect : OrstEffect {
    Effect() : OrstEffect{} {}
    /// Pixels brighter than `threshold` (0..1) spread light `radius` pixels
    static Effect glow(float threshold = 0.6f, float strength = 1.0f, float radius = 8.0f) {
        return make(ORST_EFFECT_GLOW, {threshold, strength, radius});
    }
    static Effect blur(float radius) { return make(ORST_EFFECT_BLUR, {radius}); }
    static Effect grade(float brightness = 0.0f, float contrast = 1.0f, float saturation = 1.0f,
                        float gamma = 1.0f, Color tint = Color::White) {
        return make(ORST_EFFECT_GRADE, {brightness, contrast, saturation, gamma, tint.r / 255.0f,
                                      tint.g / 255.0f, tint.b / 255.0f});
    }
    static Effect crt(float scanlines = 0.3f, float curvature = 0.1f, float vignette = 0.5f,
                      float aberration = 1.0f) {
        return make(ORST_EFFECT_CRT, {scanlines, curvature, vignette, aberration});
    }
    /// Lights the picture with Light entities; `ambient` everywhere
    static Effect lighting(Color ambient = Color::Black, float brightness = 1.0f) {
        return make(ORST_EFFECT_LIGHTING, {ambient.r / 255.0f * brightness, ambient.g / 255.0f * brightness,
                                         ambient.b / 255.0f * brightness});
    }
    static Effect custom(const Shader &shader, std::array<float, 8> params = {}) {
        Effect e;
        e.kind = ORST_EFFECT_SHADER;
        e.shader = shader.id();
        for (size_t i = 0; i < 8; ++i) e.p[i] = params[i];
        return e;
    }

private:
    static Effect make(uint32_t kind, std::initializer_list<float> values) {
        Effect e;
        e.kind = kind;
        size_t i = 0;
        for (float v : values) e.p[i++] = v;
        return e;
    }
};
static_assert(sizeof(Effect) == sizeof(OrstEffect));

class Font : public detail::Handle<orst_free_font> {
public:
    using Handle::Handle;
};

class Sound : public detail::Handle<orst_free_sound> {
public:
    using Handle::Handle;
    /// Returns a voice id for Engine::stop_voice
    uint32_t play(float volume = 1.0f, float pitch = 1.0f) { return orst_play_sound(e_, id_, volume, pitch); }
};

class Emitter : public detail::Handle<orst_free_emitter> {
public:
    using Handle::Handle;
    EmitterConfig config() const { return orst_emitter_config(e_, id_); }
    void set_config(const EmitterConfig &c) { orst_set_emitter_config(e_, id_, &c); }
    Vec2 pos() const { return orst_emitter_pos(e_, id_); }
    void set_pos(Vec2 p) { orst_set_emitter_pos(e_, id_, p); }
    bool emitting() const { return orst_emitter_emitting(e_, id_); }
    void set_emitting(bool on) { orst_set_emitter_emitting(e_, id_, on); }
    void burst(uint32_t count) { orst_emitter_burst(e_, id_, count); }
    uint32_t count() const { return orst_emitter_count(e_, id_); }
    void clear() { orst_emitter_clear(e_, id_); }
    void update(float dt) { orst_update_emitter(e_, id_, dt); }
    void draw() { orst_draw_emitter(e_, id_); }
};

/// The same configs as Emitter, for huge numbers of particles: the
/// graphics card works out where each one is. They can be counted, not read.
class GpuEmitter : public detail::Handle<orst_free_gpu_emitter> {
public:
    using Handle::Handle;
    EmitterConfig config() const { return orst_gpu_emitter_config(e_, id_); }
    /// A new max drops the living particles
    void set_config(const EmitterConfig &c) { orst_set_gpu_emitter_config(e_, id_, &c); }
    Vec2 pos() const { return orst_gpu_emitter_pos(e_, id_); }
    void set_pos(Vec2 p) { orst_set_gpu_emitter_pos(e_, id_, p); }
    void set_emitting(bool on) { orst_set_gpu_emitter_emitting(e_, id_, on); }
    void burst(uint32_t count) { orst_gpu_emitter_burst(e_, id_, count); }
    uint32_t count() const { return orst_gpu_emitter_count(e_, id_); }
    void clear() { orst_gpu_emitter_clear(e_, id_); }
    void draw() { orst_draw_gpu_emitter(e_, id_); }
};

/// A loaded skeleton file; skeletons made from it keep working after it goes
class SkeletonData : public detail::Handle<orst_free_skeleton_data> {
public:
    using Handle::Handle;
};

/// An animated skeleton; angles are radians, clockwise
class Skeleton : public detail::Handle<orst_free_skeleton> {
public:
    using Handle::Handle;
    /// Fades from the current animation over `mix` seconds
    void play(const std::string &name, bool looped = true, float mix = 0.0f) {
        if (!orst_skeleton_play(e_, id_, name.c_str(), looped, mix)) throw Error(orst_last_error());
    }
    void stop() { orst_skeleton_stop(e_, id_); }
    std::string animation() const {
        std::string s(orst_skeleton_animation(e_, id_, nullptr, 0), '\0');
        orst_skeleton_animation(e_, id_, s.data(), s.size() + 1);
        return s;
    }
    /// A non-looping animation reached its end
    bool done() const { return orst_skeleton_done(e_, id_); }
    /// The event was passed in the last update
    bool fired(const std::string &event) const { return orst_skeleton_fired(e_, id_, event.c_str()); }
    void set_root(const Transform &t) { orst_set_skeleton_root(e_, id_, t); }
    void set_tint(Color c) { orst_set_skeleton_tint(e_, id_, c); }
    void set_speed(float speed) { orst_set_skeleton_speed(e_, id_, speed); }
    /// Attachments as sprites of the atlas, at the current layer, depth and space
    void draw(const Texture &atlas) { orst_draw_skeleton(e_, id_, atlas.id()); }
    /// A bone's index, -1 if none
    int bone(const std::string &name) const { return orst_skeleton_bone(e_, id_, name.c_str()); }
    Transform bone_world(int bone) const {
        OrstTransform t = orst_skeleton_bone_world(e_, id_, bone);
        return {t.pos, t.rotation, t.scale};
    }
    /// A point in a bone's own space, in the world (a muzzle's tip)
    Vec2 bone_point(int bone, Vec2 local) const { return orst_skeleton_bone_point(e_, id_, bone, local); }
    /// Turn a bone by code, blended over the animation by `weight`; 0 gives it back
    void set_override(int bone, float rotation, float weight = 1.0f) {
        orst_set_bone_override(e_, id_, bone, rotation, weight);
    }
    /// An empty name hides the slot
    void set_attachment(const std::string &slot, const std::string &attachment) {
        const char *a = attachment.empty() ? nullptr : attachment.c_str();
        if (!orst_skeleton_set_attachment(e_, id_, slot.c_str(), a)) throw Error(orst_last_error());
    }
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

class Tilemap : public detail::Handle<orst_free_tilemap> {
public:
    using Handle::Handle;
    OrstTilemapInfo info() const { OrstTilemapInfo i{}; orst_tilemap_info(e_, id_, &i); return i; }
    int layer_index(const std::string &name) const { return orst_tilemap_layer_index(e_, id_, name.c_str()); }
    uint32_t tile(uint32_t layer, int x, int y) const { return orst_tilemap_get_tile(e_, id_, layer, x, y); }
    void set_tile(uint32_t layer, int x, int y, uint32_t gid) { orst_tilemap_set_tile(e_, id_, layer, x, y, gid); }
    bool add_tileset(uint32_t first_gid, const Texture &texture, SpriteSheet sheet) {
        return orst_tilemap_add_tileset(e_, id_, first_gid, texture.id(), sheet);
    }
    bool set_tile_flag(uint32_t gid, const std::string &name, bool value = true) {
        return orst_tilemap_set_tile_flag(e_, id_, gid, name.c_str(), value);
    }
    bool solid(int x, int y) const { return orst_tilemap_is_solid(e_, id_, x, y); }
    Vec2 tile_to_world(Vec2 tile) const { return orst_tilemap_tile_to_world(e_, id_, tile); }
    Vec2 world_to_tile(Vec2 p) const { return orst_tilemap_world_to_tile(e_, id_, p); }
    Vec2 cell_center(int x, int y) const { return orst_tilemap_cell_center(e_, id_, x, y); }

    void draw(int base_layer = 0) { orst_draw_tilemap(e_, id_, base_layer); }
    void draw_layer(uint32_t layer, int draw_layer, bool y_sort = false, float depth = 0,
                    Color tint = Color::White) {
        orst_draw_tilemap_layer(e_, id_, layer, draw_layer, depth, y_sort, tint);
    }

    std::vector<MapObject> objects() const {
        std::vector<MapObject> out;
        uint32_t n = info().object_count;
        for (uint32_t i = 0; i < n; ++i) {
            OrstMapObject o{};
            orst_tilemap_object(e_, id_, i, &o);
            MapObject m;
            m.id = o.id;
            m.name = detail::fetch(orst_tilemap_object_name, e_, id_, i);
            m.cls = detail::fetch(orst_tilemap_object_class, e_, id_, i);
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
        int i = orst_tilemap_find_object(e_, id_, name.c_str());
        if (i < 0) return std::nullopt;
        return objects()[static_cast<size_t>(i)];
    }
    double number(const MapObject &o, const std::string &prop, double fallback = 0) const {
        return orst_tilemap_object_number(e_, id_, o.index, prop.c_str(), fallback);
    }
    std::optional<std::string> string(const MapObject &o, const std::string &prop) const {
        int64_t n = orst_tilemap_object_string(e_, id_, o.index, prop.c_str(), nullptr, 0);
        if (n < 0) return std::nullopt;
        std::string s(static_cast<size_t>(n), '\0');
        orst_tilemap_object_string(e_, id_, o.index, prop.c_str(), s.data(), s.size() + 1);
        return s;
    }

    MoveResult move_box(Rect rect, Vec2 delta) const { return orst_move_box(e_, id_, rect, delta); }
    Contacts touching(Rect rect) const { return orst_touching(e_, id_, rect); }
    /// Cell centres around solid cells, from the cell of `from` to that of
    /// `to`; empty when there is no way
    std::vector<Vec2> find_path(Vec2 from, Vec2 to, bool diagonal = true, float max_cost = 0) const {
        std::vector<Vec2> out(orst_find_path(e_, id_, from, to, diagonal, max_cost, nullptr, 0));
        orst_find_path(e_, id_, from, to, diagonal, max_cost, out.data(), uint32_t(out.size()));
        return out;
    }
    std::optional<RayHit> raycast(Vec2 from, Vec2 to) const {
        RayHit hit{};
        if (!orst_raycast(e_, id_, from, to, &hit)) return std::nullopt;
        return hit;
    }
    bool point_solid(Vec2 p) const { return orst_point_solid(e_, id_, p); }
    bool rect_solid(Rect r) const { return orst_rect_solid(e_, id_, r); }
};

/// An input bound to an action: Binding::key(Key::Space), Binding::pad(Pad::South)
struct Binding : OrstBinding {
    constexpr Binding() : OrstBinding{0, 0, -1, 0} {}
    constexpr Binding(OrstBinding b) : OrstBinding(b) {}
    static constexpr Binding key(Key k) { return OrstBinding{ORST_BIND_KEY, int32_t(k), -1, 0}; }
    static constexpr Binding mouse(Mouse m) { return OrstBinding{ORST_BIND_MOUSE, int32_t(m), -1, 0}; }
    /// pad: slot 0..3, -1 any
    static constexpr Binding pad(Pad b, int pad = -1) { return OrstBinding{ORST_BIND_PAD_BUTTON, int32_t(b), pad, 0}; }
    /// direction: 1 or -1 (LeftX -1 is left)
    static constexpr Binding axis(PadAxis a, int direction, int pad = -1) {
        return OrstBinding{ORST_BIND_PAD_AXIS, int32_t(a), pad, direction};
    }
    constexpr bool operator==(const Binding &o) const {
        return kind == o.kind && code == o.code && pad == o.pad && direction == o.direction;
    }
};

class Engine {
public:
    explicit Engine(const Config &c = {}) {
        OrstConfig oc{c.title.c_str(), c.width, c.height, c.scale, c.resizable,
                    c.fullscreen, c.integer_scale, c.target_fps, c.headless,
                    static_cast<int32_t>(c.renderer)};
        e_ = orst_engine_create(&oc);
        if (!e_) throw Error(std::string("cannot start the engine: ") + orst_last_error());
    }
    Engine(const Engine &) = delete;
    Engine &operator=(const Engine &) = delete;
    ~Engine() { orst_engine_destroy(e_); }

    OrstEngine *raw() { return e_; }

    // frame
    bool poll() { return orst_poll(e_); }
    void quit() { orst_quit(e_); }
    float dt() const { return orst_dt(e_); }
    double time() const { return orst_time(e_); }
    float fps() const { return orst_fps(e_); }
    uint64_t frame() const { return orst_frame(e_); }
    // fixed logic ticks: for (uint32_t i = 0; i < engine.ticks(); ++i) update(engine.tick_dt());
    void set_tick_rate(uint32_t ticks_per_second) { orst_set_tick_rate(e_, ticks_per_second); }
    uint32_t ticks() const { return orst_ticks(e_); }
    float tick_dt() const { return orst_tick_dt(e_); }
    float tick_alpha() const { return orst_tick_alpha(e_); }
    uint64_t tick_count() const { return orst_tick_count(e_); }
    // seeded random numbers, the same on any machine
    void random_seed(uint64_t seed) { orst_random_seed(e_, seed); }
    uint32_t random_u32() { return orst_random_u32(e_); }
    float random_float() { return orst_random_float(e_); }
    float random_range(float min, float max) { return orst_random_range(e_, min, max); }
    int32_t random_int(int32_t min, int32_t max) { return orst_random_int(e_, min, max); }
    void begin_frame(Color clear = Color::Black) { orst_begin_frame(e_, clear); }
    void end_frame() { orst_end_frame(e_); }
    void set_title(const std::string &t) { orst_set_title(e_, t.c_str()); }
    void set_fullscreen(bool on) { orst_set_fullscreen(e_, on); }
    bool fullscreen() const { return orst_is_fullscreen(e_); }
    void set_target_fps(uint32_t fps) { orst_set_target_fps(e_, fps); }
    std::pair<uint32_t, uint32_t> canvas_size() const {
        uint32_t w = 0, h = 0;
        orst_canvas_size(e_, &w, &h);
        return {w, h};
    }
    void save_screenshot(const std::string &path) {
        if (!orst_save_screenshot(e_, path.c_str())) throw Error(orst_last_error());
    }
    Color pixel(uint32_t x, uint32_t y) const { return orst_pixel(e_, x, y); }

    // resources
    Texture load_texture(const std::string &path) {
        return {e_, detail::check(orst_load_texture(e_, path.c_str()), path)};
    }
    Texture texture_from_rgba(uint32_t w, uint32_t h, const uint8_t *rgba) {
        return {e_, detail::check(orst_texture_from_rgba(e_, w, h, rgba), "texture")};
    }
    Font load_font(const std::string &path) { return {e_, detail::check(orst_load_font(e_, path.c_str()), path)}; }
    Tilemap load_tilemap(const std::string &path) {
        return {e_, detail::check(orst_load_tilemap(e_, path.c_str()), path)};
    }
    Tilemap create_tilemap(Orientation o, uint32_t w, uint32_t h, uint32_t tile_w, uint32_t tile_h,
                           uint32_t layers = 1) {
        return {e_, detail::check(orst_create_tilemap(e_, static_cast<int32_t>(o), w, h, tile_w, tile_h, layers),
                                  "tilemap")};
    }
    Emitter create_emitter(const EmitterConfig &config, Vec2 pos = {}) {
        return {e_, detail::check(orst_create_emitter(e_, &config, pos), "emitter")};
    }
    /// Advance every emitter by dt seconds
    void update_emitters(float dt) { orst_update_emitters(e_, dt); }
    GpuEmitter create_gpu_emitter(const EmitterConfig &config, Vec2 pos = {}) {
        return {e_, detail::check(orst_create_gpu_emitter(e_, &config, pos), "GPU emitter")};
    }
    void update_gpu_emitters(float dt) { orst_update_gpu_emitters(e_, dt); }
    SkeletonData load_skeleton(const std::string &path) {
        return {e_, detail::check(orst_load_skeleton(e_, path.c_str()), path)};
    }
    SkeletonData skeleton_from_json(const std::string &json) {
        return {e_, detail::check(orst_skeleton_from_json(e_, json.c_str()), "skeleton")};
    }
    Skeleton create_skeleton(const SkeletonData &data) {
        return {e_, detail::check(orst_create_skeleton(e_, data.id()), "skeleton")};
    }
    /// Advance every skeleton by dt seconds
    void update_skeletons(float dt) { orst_update_skeletons(e_, dt); }
    Sound load_sound(const std::string &path) { return {e_, detail::check(orst_load_sound(e_, path.c_str()), path)}; }

    // render targets, post effects and shake
    /// A transparent texture to draw into (draw_to), shown like any texture
    Texture create_target(uint32_t width, uint32_t height) {
        return {e_, detail::check(orst_create_target(e_, width, height), "render target")};
    }
    /// Draw calls go into `target` until draw_to_screen (or the next frame)
    void draw_to(const Texture &target) {
        if (!orst_set_target(e_, target.id())) throw Error(last_error());
    }
    void draw_to_screen() { orst_set_target(e_, 0); }
    /// Clear a target this frame; one that is not cleared is drawn over
    void clear_target(const Texture &target, Color c) { orst_clear_target(e_, target.id(), c); }
    void set_target_camera(const Texture &target, const Camera &c) {
        orst_set_target_camera(e_, target.id(), &c);
    }
    void reset_target_camera(const Texture &target) { orst_set_target_camera(e_, target.id(), nullptr); }
    /// The screen's post effects, run in order every frame
    void set_effects(const std::vector<Effect> &effects) { set_effects_of(0, effects); }
    void set_effects(const Texture &target, const std::vector<Effect> &effects) {
        set_effects_of(target.id(), effects);
    }
    Shader create_shader(const std::string &wgsl) {
        return {e_, detail::check(orst_create_shader(e_, wgsl.c_str()), "shader")};
    }
    /// Shake the world on screen by up to `strength` pixels, fading out
    void shake(float strength, float duration) { orst_shake(e_, strength, duration); }
    /// Lighting: a normal map of the same size (red right, green up)
    void set_normal_map(const Texture &t, const Texture &normal_map) { orst_set_normal_map(e_, t.id(), normal_map.id()); }
    /// Colliders on these layers cast shadows
    void set_shadow_layers(uint32_t layers) { orst_set_shadow_layers(e_, layers); }
    void set_shadow_tilemap(const Tilemap &map, bool on = true) { orst_set_shadow_tilemap(e_, map.id(), on); }

    // camera and draw state
    Camera camera() const { return orst_get_camera(e_); }
    void set_camera(const Camera &c) { orst_set_camera(e_, c); }
    /// Centre the camera on a world point
    void look_at(Vec2 center, float zoom = 1.0f, float rotation = 0.0f) {
        orst_set_camera(e_, OrstCamera{center, zoom, rotation});
    }
    Vec2 world_to_screen(Vec2 p) const { return orst_world_to_screen(e_, p); }
    Vec2 screen_to_world(Vec2 p) const { return orst_screen_to_world(e_, p); }
    void set_layer(int layer) { orst_set_layer(e_, layer); }
    void set_depth(float depth) { orst_set_depth(e_, depth); }
    void set_space(Space s) { orst_set_space(e_, static_cast<int32_t>(s)); }
    void set_antialias(bool on) { orst_set_antialias(e_, on); }

    // drawing
    void draw(const Texture &t, const Sprite &s) { orst_draw_sprite(e_, t.id(), &s); }
    void draw(const Texture &t, float x, float y) { orst_draw_texture(e_, t.id(), x, y); }
    void draw_frame(const Texture &t, const SpriteSheet &sheet, uint32_t frame, const Sprite &s) {
        orst_draw_frame(e_, t.id(), &sheet, frame, &s);
    }
    void fill_rect(Rect r, Color c) { orst_fill_rect(e_, r, c); }
    void draw_rect(Rect r, float thickness, Color c) { orst_draw_rect(e_, r, thickness, c); }
    void draw_line(Vec2 a, Vec2 b, float thickness, Color c) { orst_draw_line(e_, a, b, thickness, c); }
    void fill_circle(Vec2 center, float radius, Color c) { orst_fill_circle(e_, center, radius, c); }
    void draw_circle(Vec2 center, float radius, float thickness, Color c) {
        orst_draw_circle(e_, center, radius, thickness, c);
    }
    void draw_text(const std::string &text, Vec2 pos, float size, Color c, const Font *font = nullptr) {
        orst_draw_text(e_, font ? font->id() : ORST_DEFAULT_FONT, text.c_str(), pos, size, c);
    }
    Vec2 measure_text(const std::string &text, float size, const Font *font = nullptr) {
        return orst_measure_text(e_, font ? font->id() : ORST_DEFAULT_FONT, text.c_str(), size);
    }

    // input
    bool key_down(Key k) const { return orst_key_down(e_, static_cast<int32_t>(k)); }
    bool key_pressed(Key k) const { return orst_key_pressed(e_, static_cast<int32_t>(k)); }
    bool key_released(Key k) const { return orst_key_released(e_, static_cast<int32_t>(k)); }
    float axis(Key negative, Key positive) const {
        return orst_axis(e_, static_cast<int32_t>(negative), static_cast<int32_t>(positive));
    }
    bool mouse_down(Mouse b) const { return orst_mouse_down(e_, static_cast<int32_t>(b)); }
    bool mouse_pressed(Mouse b) const { return orst_mouse_pressed(e_, static_cast<int32_t>(b)); }
    bool mouse_released(Mouse b) const { return orst_mouse_released(e_, static_cast<int32_t>(b)); }
    Vec2 mouse() const { return orst_mouse_pos(e_); }
    Vec2 mouse_world() const { return orst_mouse_world(e_); }
    float wheel() const { return orst_mouse_wheel(e_); }
    std::string text_input() { return orst_text_input(e_); }
    void inject_key(Key k, bool down) { orst_inject_key(e_, static_cast<int32_t>(k), down); }
    void inject_mouse_button(Mouse b, bool down) { orst_inject_mouse_button(e_, static_cast<int32_t>(b), down); }
    void inject_mouse_move(Vec2 p) { orst_inject_mouse_move(e_, p); }

    // gamepads 0..3; sticks have y down, triggers are 0..1
    bool gamepad_connected(int pad) const { return orst_gamepad_connected(e_, pad); }
    std::string gamepad_name(int pad) const {
        std::string s(orst_gamepad_name(e_, pad, nullptr, 0), '\0');
        orst_gamepad_name(e_, pad, s.data(), s.size() + 1);
        return s;
    }
    bool pad_down(int pad, Pad b) const { return orst_gamepad_down(e_, pad, static_cast<int32_t>(b)); }
    bool pad_pressed(int pad, Pad b) const { return orst_gamepad_pressed(e_, pad, static_cast<int32_t>(b)); }
    bool pad_released(int pad, Pad b) const { return orst_gamepad_released(e_, pad, static_cast<int32_t>(b)); }
    float pad_axis(int pad, PadAxis a) const { return orst_gamepad_axis(e_, pad, static_cast<int32_t>(a)); }
    /// Left stick as a vector (y down)
    Vec2 pad_stick(int pad) const { return {pad_axis(pad, PadAxis::LeftX), pad_axis(pad, PadAxis::LeftY)}; }
    void set_gamepad_deadzone(float dz) { orst_set_gamepad_deadzone(e_, dz); }
    void inject_gamepad_connected(int pad, bool on) { orst_inject_gamepad_connected(e_, pad, on); }
    void inject_pad_button(int pad, Pad b, bool down) {
        orst_inject_gamepad_button(e_, pad, static_cast<int32_t>(b), down);
    }
    void inject_pad_axis(int pad, PadAxis a, float v) { orst_inject_gamepad_axis(e_, pad, static_cast<int32_t>(a), v); }

    /// "gpu: <graphics card>" or "software"
    std::string renderer_name() const {
        std::string s(orst_renderer_name(e_, nullptr, 0), '\0');
        orst_renderer_name(e_, s.data(), s.size() + 1);
        return s;
    }

    // hot reload: textures and maps reload in place when their files change
    void set_hot_reload(bool on) { orst_set_hot_reload(e_, on); }
    bool hot_reload() const { return orst_hot_reload_enabled(e_); }
    uint32_t reload_changed() { return orst_reload_changed(e_); }
    /// Resources reloaded by the last poll (e.g. to re-read map objects)
    uint32_t reloaded() const { return orst_reloaded(e_); }

    // debug overlay: drawn on top only while on; F3 toggles it
    void set_debug(bool on) { orst_set_debug(e_, on); }
    bool debug() const { return orst_debug_enabled(e_); }
    void set_debug_key(std::optional<Key> k) { orst_set_debug_key(e_, k ? static_cast<int32_t>(*k) : 0); }
    void debug_rect(Rect r, Color c = {255, 60, 60}) { orst_debug_rect(e_, r, c); }
    void debug_line(Vec2 a, Vec2 b, Color c = {255, 60, 60}) { orst_debug_line(e_, a, b, c); }
    void debug_circle(Vec2 center, float radius, Color c = {255, 60, 60}) { orst_debug_circle(e_, center, radius, c); }
    void debug_text(const std::string &text, Vec2 pos, Color c = Color::White) {
        orst_debug_text(e_, text.c_str(), pos, c);
    }
    void debug_tilemap(const Tilemap &map) { orst_debug_tilemap(e_, map.id()); }

    // entities and components: plain structs named with ORST_COMPONENT(Type).
    // Pointers from get() stay valid until the next spawn, despawn, set or remove.
    template <class T>
    uint32_t component() {
        static_assert(std::is_trivially_copyable_v<T>, "components are plain data");
        uint32_t id = orst_component(e_, ComponentName<T>::value, sizeof(T), alignof(T));
        if (id == 0) throw Error(orst_last_error());
        return id;
    }
    Entity spawn() { return orst_spawn(e_); }
    bool despawn(Entity en) { return orst_despawn(e_, en); }
    bool alive(Entity en) const { return orst_alive(e_, en); }
    uint32_t entity_count() const { return orst_entity_count(e_); }
    template <class T>
    T &set(Entity en, const T &value) {
        uint32_t c = component<T>();
        if (!orst_set(e_, en, c, &value)) throw Error("set on a dead entity");
        return *static_cast<T *>(orst_get(e_, en, c));
    }
    template <class T>
    T *get(Entity en) { return static_cast<T *>(orst_get(e_, en, component<T>())); }
    template <class T>
    bool has(Entity en) { return orst_has(e_, en, component<T>()); }
    template <class T>
    bool remove(Entity en) { return orst_remove(e_, en, component<T>()); }
    /// Entities with all of Ts (none: every entity)
    template <class... Ts>
    std::vector<Entity> query() {
        std::array<uint32_t, sizeof...(Ts)> ids{component<Ts>()...};
        std::vector<Entity> out(orst_entity_count(e_));
        out.resize(orst_query(e_, ids.data(), uint32_t(ids.size()), out.data(), uint32_t(out.size())));
        return out;
    }
    /// f(entity, Ts &...) for each entity with all of Ts. Spawning or
    /// changing components inside f moves data: collect and do it after.
    template <class... Ts, class F>
    void each(F &&f) {
        std::array<uint32_t, sizeof...(Ts)> ids{component<Ts>()...};
        for (Entity en : query<Ts...>()) each_one<Ts...>(f, en, ids, std::index_sequence_for<Ts...>{});
    }
    void clear_world() { orst_world_clear(e_); }
    /// The oldest entity with this Name; 0: none
    Entity find_named(const std::string &name) { return orst_find_named(e_, name.c_str()); }

    /// What is inside T, for tools (see orst_set_fields):
    ///     engine.set_fields<Enemy>({{"speed", ORST_FIELD_F32, offsetof(Enemy, speed), 0, 0, 10}});
    template <class T>
    void set_fields(std::initializer_list<OrstField> fields) {
        if (!orst_set_fields(e_, component<T>(), fields.begin(), uint32_t(fields.size())))
            throw Error(orst_last_error());
    }
    struct FieldInfo {
        std::string name;
        uint32_t type, offset, flags;
        float min, max;
        std::vector<std::string> options;
    };
    std::vector<FieldInfo> fields(uint32_t component) {
        std::vector<FieldInfo> out;
        uint32_t n = orst_field_count(e_, component);
        for (uint32_t i = 0; i < n; ++i) {
            OrstField f;
            if (!orst_get_field(e_, component, i, &f)) break;
            FieldInfo info{f.name, f.type, f.offset, f.flags, f.min, f.max, {}};
            for (std::string rest = f.options ? f.options : ""; !rest.empty();) {
                size_t bar = rest.find('|');
                info.options.push_back(rest.substr(0, bar));
                rest = bar == std::string::npos ? "" : rest.substr(bar + 1);
            }
            out.push_back(std::move(info));
        }
        return out;
    }
    /// Move entities with Transform + Velocity by dt; with a Body on a tile
    /// map they slide along walls
    void update_motion(float dt) { orst_update_motion(e_, dt); }
    /// A convex polygon for colliders, 3 to 64 corners in order
    uint32_t create_polygon(const std::vector<Vec2> &points) {
        uint32_t id = orst_create_polygon(e_, points.data(), uint32_t(points.size()));
        if (id == 0) throw Error(orst_last_error());
        return id;
    }
    void free_polygon(uint32_t id) { orst_free_polygon(e_, id); }
    /// How the colliders of two entities overlap (layers aside)
    std::optional<Overlap> overlap(Entity a, Entity b) const {
        Overlap o{};
        if (!orst_entity_overlap(e_, a, b, &o)) return std::nullopt;
        return o;
    }
    void debug_colliders() { orst_debug_colliders(e_); }
    // physics (Rapier): bodies with mass, once per tick
    void physics_step(float dt) { orst_physics_step(e_, dt); }
    void set_gravity(Vec2 g) { orst_set_gravity(e_, g); }
    void set_physics_scale(float units_per_metre) { orst_set_physics_scale(e_, units_per_metre); }
    bool apply_impulse(Entity en, Vec2 impulse) { return orst_apply_impulse(e_, en, impulse); }
    bool apply_force(Entity en, Vec2 force) { return orst_apply_force(e_, en, force); }
    bool apply_torque_impulse(Entity en, float torque) { return orst_apply_torque_impulse(e_, en, torque); }
    float body_mass(Entity en) { return orst_body_mass(e_, en); }
    uint32_t joint_pin(Entity a, Entity b, Vec2 at) { return orst_joint_pin(e_, a, b, at); }
    uint32_t joint_weld(Entity a, Entity b) { return orst_joint_weld(e_, a, b); }
    uint32_t joint_rope(Entity a, Entity b, float length) { return orst_joint_rope(e_, a, b, length); }
    uint32_t joint_spring(Entity a, Entity b, float rest, float stiffness, float damping) {
        return orst_joint_spring(e_, a, b, rest, stiffness, damping);
    }
    bool remove_joint(uint32_t joint) { return orst_remove_joint(e_, joint); }

    /// Overlaps, pushes and events between colliders; after update_motion
    void update_collisions() { orst_update_collisions(e_); }
    void set_collision_cell(float size) { orst_set_collision_cell(e_, size); }
    std::vector<Contact> contacts() const {
        std::vector<Contact> out(orst_contact_count(e_));
        for (uint32_t i = 0; i < out.size(); i++) orst_contact(e_, i, &out[i]);
        return out;
    }
    std::vector<Entity> query_point(Vec2 p, uint32_t mask = 0xFFFFFFFFu) const {
        return collect([&](Entity *o, uint32_t n) { return orst_query_point(e_, p, mask, o, n); });
    }
    std::vector<Entity> query_rect(Rect r, uint32_t mask = 0xFFFFFFFFu) const {
        return collect([&](Entity *o, uint32_t n) { return orst_query_rect(e_, r, mask, o, n); });
    }
    std::vector<Entity> query_circle(Vec2 c, float radius, uint32_t mask = 0xFFFFFFFFu) const {
        return collect([&](Entity *o, uint32_t n) { return orst_query_circle(e_, c, radius, mask, o, n); });
    }
    std::optional<EntityHit> raycast_entities(Vec2 from, Vec2 to, uint32_t mask = 0xFFFFFFFFu) const {
        EntityHit h{};
        if (!orst_raycast_entities(e_, from, to, mask, &h)) return std::nullopt;
        return h;
    }
    /// Draw every entity with Transform + SpriteRenderer
    void draw_entities() { orst_draw_entities(e_); }
    /// Entities, components, random generator and tick counter as bytes
    std::vector<uint8_t> snapshot() const {
        std::vector<uint8_t> bytes(orst_snapshot(e_, nullptr, 0));
        orst_snapshot(e_, bytes.data(), bytes.size());
        return bytes;
    }
    /// Continue from a snapshot; throws (and changes nothing) on bad bytes
    void restore(const std::vector<uint8_t> &bytes) {
        if (!orst_restore(e_, bytes.data(), bytes.size())) throw Error(orst_last_error());
    }
    uint64_t state_hash() const { return orst_state_hash(e_); }

    // input actions: the game asks for "jump", players rebind it
    uint32_t action(const std::string &name) {
        uint32_t id = orst_action(e_, name.c_str());
        if (id == 0) throw Error(orst_last_error());
        return id;
    }
    bool bind(uint32_t action, Binding b) { return orst_bind(e_, action, b); }
    bool set_binding(uint32_t action, uint32_t index, Binding b) { return orst_set_binding(e_, action, index, b); }
    bool remove_binding(uint32_t action, uint32_t index) { return orst_remove_binding(e_, action, index); }
    void clear_bindings(uint32_t action) { orst_clear_bindings(e_, action); }
    std::vector<Binding> bindings(uint32_t action) const {
        std::vector<Binding> out(orst_binding_count(e_, action));
        for (uint32_t i = 0; i < out.size(); i++) orst_get_binding(e_, action, i, &out[i]);
        return out;
    }
    /// "key:Space", "pad:South", "pad1:LeftX-"
    std::string binding_text(Binding b) const {
        std::string s(orst_binding_text(e_, b, nullptr, 0), '\0');
        orst_binding_text(e_, b, s.data(), s.size() + 1);
        return s;
    }
    bool action_down(uint32_t action) const { return orst_action_down(e_, action); }
    bool action_pressed(uint32_t action) const { return orst_action_pressed(e_, action); }
    bool action_released(uint32_t action) const { return orst_action_released(e_, action); }
    float action_value(uint32_t action) const { return orst_action_value(e_, action); }
    float action_axis(uint32_t negative, uint32_t positive) const { return orst_action_axis(e_, negative, positive); }
    /// The first input pressed this frame, for "press a key" menus
    std::optional<Binding> captured_binding() const {
        Binding b;
        if (!orst_captured_binding(e_, &b)) return std::nullopt;
        return b;
    }
    std::string save_bindings() const {
        std::string s(orst_bindings_save(e_, nullptr, 0), '\0');
        orst_bindings_save(e_, s.data(), s.size() + 1);
        return s;
    }
    /// Throws (and changes nothing) on a bad line
    void load_bindings(const std::string &text) {
        if (!orst_bindings_load(e_, text.c_str())) throw Error(orst_last_error());
    }

    // scenes: numbers the game picks (0 = none); the switch happens in the
    // middle of the transition and scene_entered() reports it for one frame
    bool go_scene(uint32_t scene, Transition t = Transition::Fade, float seconds = 0.5f, Color c = Color::Black) {
        return orst_scene_go(e_, scene, int32_t(t), seconds, c);
    }
    bool push_scene(uint32_t scene, Transition t = Transition::Fade, float seconds = 0.5f, Color c = Color::Black) {
        return orst_scene_push(e_, scene, int32_t(t), seconds, c);
    }
    bool pop_scene(Transition t = Transition::Fade, float seconds = 0.5f, Color c = Color::Black) {
        return orst_scene_pop(e_, int32_t(t), seconds, c);
    }
    uint32_t scene() const { return orst_scene(e_); }
    uint32_t scene_entered() const { return orst_scene_entered(e_); }
    uint32_t scene_left() const { return orst_scene_left(e_); }
    uint32_t scene_depth() const { return orst_scene_depth(e_); }
    bool scene_transitioning() const { return orst_scene_transitioning(e_); }

    // sound
    void stop_voice(uint32_t voice) { orst_stop_voice(e_, voice); }
    bool voice_playing(uint32_t voice) const { return orst_voice_playing(e_, voice); }
    void stop_all_sounds() { orst_stop_all_sounds(e_); }
    void play_music(const std::string &path, float volume = 1.0f, bool looped = true) {
        if (!orst_play_music(e_, path.c_str(), volume, looped)) throw Error(orst_last_error());
    }
    void stop_music() { orst_stop_music(e_); }
    void pause_music(bool paused) { orst_pause_music(e_, paused); }
    bool music_playing() const { return orst_music_playing(e_); }
    void set_music_volume(float v) { orst_set_music_volume(e_, v); }
    void set_master_volume(float v) { orst_set_master_volume(e_, v); }

private:
    void set_effects_of(uint32_t target, const std::vector<Effect> &effects) {
        if (!orst_set_effects(e_, target, effects.data(), uint32_t(effects.size()))) throw Error(last_error());
    }
    template <class F>
    static std::vector<Entity> collect(F &&query) {
        std::vector<Entity> out(query(nullptr, 0));
        out.resize(query(out.data(), uint32_t(out.size())));
        return out;
    }
    template <class... Ts, class F, std::size_t... I>
    void each_one(F &f, Entity en, const std::array<uint32_t, sizeof...(Ts)> &ids, std::index_sequence<I...>) {
        f(en, *static_cast<Ts *>(orst_get(e_, en, ids[I]))...);
    }

    OrstEngine *e_ = nullptr;
};

}  // namespace orst

// C++ scripts for ORST Studio, as in Unity: a Behaviour on an entity whose
// described fields are a component the Inspector shows and scenes save.
//
//     struct Spin : orst::Behaviour {
//         float speed = 2;
//         static void fields(orst::Fields<Spin> &f) { f("speed", &Spin::speed).range(0, 10); }
//         void update(float dt) override { transform().rotation += speed * dt; }
//     };
//     ORST_BEHAVIOUR(Spin);
//
// The studio builds every .cpp under Assets into one library and loads it
// in place of the one before; the initial values of the fields are the
// component's defaults. start() runs before the first update() of each
// entity, update(dt) every tick (60 a second) before the engine moves
// things, after the contact calls for the tick before.
#pragma once

#include "orst.hpp"

#include <algorithm>
#include <cstring>
#include <map>
#include <memory>
#include <string>
#include <typeinfo>
#include <vector>

#if defined(_WIN32)
#define ORST_SCRIPT_EXPORT extern "C" __declspec(dllexport)
#else
#define ORST_SCRIPT_EXPORT extern "C" __attribute__((visibility("default")))
#endif

namespace orst {

class Behaviour;

namespace script {

/// One described field: where it is in the object and in the component
struct Slot {
    std::string name;
    uint32_t type;
    size_t object_offset;
    uint32_t size;
    uint32_t offset = 0;
    uint32_t flags = 0;
    float min = 0, max = 0;
    std::string options;
};

template <class V>
constexpr uint32_t field_type() {
    if constexpr (std::is_enum_v<V>) {
        static_assert(sizeof(V) == 4, "enum fields are 32-bit");
        return std::is_signed_v<std::underlying_type_t<V>> ? ORST_FIELD_I32 : ORST_FIELD_U32;
    } else if constexpr (std::is_same_v<V, float>) {
        return ORST_FIELD_F32;
    } else if constexpr (std::is_same_v<V, int32_t>) {
        return ORST_FIELD_I32;
    } else if constexpr (std::is_same_v<V, uint32_t>) {
        return ORST_FIELD_U32;
    } else if constexpr (std::is_same_v<V, bool>) {
        return ORST_FIELD_BOOL;
    } else if constexpr (std::is_same_v<V, uint8_t>) {
        return ORST_FIELD_U8;
    } else if constexpr (std::is_same_v<V, Vec2>) {
        return ORST_FIELD_VEC2;
    } else if constexpr (std::is_same_v<V, Color>) {
        return ORST_FIELD_COLOR;
    } else if constexpr (std::is_same_v<V, Rect>) {
        return ORST_FIELD_RECT;
    } else if constexpr (std::is_same_v<V, Entity>) {
        return ORST_FIELD_ENTITY;
    } else if constexpr (std::is_same_v<V, Str>) {
        return ORST_FIELD_STRING;
    } else {
        static_assert(sizeof(V) == 0, "fields are numbers, bool, Vec2, Color, Rect, Entity, Str or enums");
        return 0;
    }
}

struct Type {
    std::string name;
    const std::type_info *info;
    std::unique_ptr<Behaviour> (*make)();
    std::vector<Slot> (*describe)();
};

/// The Behaviours of this library, filled by ORST_BEHAVIOUR
inline std::vector<Type> &types() {
    static std::vector<Type> t;
    return t;
}

/// What the library is doing now; lives in orst_script_main
struct State;
State &state();
Engine &engine();
void log(uint32_t level, const std::string &text);
Behaviour *find(const std::type_info &type, Entity e);
Behaviour *find_any(const std::type_info &type);

}  // namespace script

/// How a Behaviour lists its fields: f("speed", &Player::speed).range(0, 10)
template <class T>
class Fields {
public:
    class Field {
    public:
        explicit Field(script::Slot &s) : s_(s) {}
        /// A slider from min to max in the Inspector
        Field &range(float min, float max) {
            s_.min = min;
            s_.max = max;
            return *this;
        }
        /// A float in radians, shown in degrees
        Field &angle() {
            s_.flags |= ORST_FIELD_ANGLE;
            return *this;
        }
        /// A uint32_t of layer bits, shown as checkboxes
        Field &layers() {
            s_.flags |= ORST_FIELD_LAYERS;
            return *this;
        }
        /// Names of the values 0, 1, 2... of a number or enum: "Walk|Run"
        Field &options(const std::string &names) {
            s_.options = names;
            return *this;
        }

    private:
        script::Slot &s_;
    };

    template <class V>
    Field operator()(const char *name, V T::*member) {
        alignas(T) static unsigned char probe[sizeof(T)];
        auto *t = reinterpret_cast<T *>(probe);
        size_t at = reinterpret_cast<unsigned char *>(&(t->*member)) - probe;
        slots.push_back({name, script::field_type<V>(), at, uint32_t(sizeof(V))});
        return Field(slots.back());
    }

    std::vector<script::Slot> slots;
};

/// A script on an entity. Fields listed in a static fields() function are
/// a component named after the class.
class Behaviour {
public:
    virtual ~Behaviour() = default;
    virtual void start() {}
    virtual void update(float) {}
    /// The entity's collider began to touch another's, or to overlap a trigger
    virtual void on_contact_begin(Entity, const Contact &) {}
    /// The two stopped touching
    virtual void on_contact_end(Entity, const Contact &) {}

    /// The entity this script is on
    Entity entity() const { return entity_; }
    /// The game's engine
    Engine &engine() const { return script::engine(); }
    /// The entity's Transform, in place
    Transform &transform() const { return *engine().get<Transform>(entity_); }
    /// The T script on an entity (this one by default); nullptr when it
    /// has none. Changes to its fields stick.
    template <class T>
    T *get_behaviour(Entity e = 0) const {
        return static_cast<T *>(script::find(typeid(T), e ? e : entity_));
    }
    /// The T script on whichever entity has one: the score, the level
    template <class T>
    static T *find_behaviour() {
        return static_cast<T *>(script::find_any(typeid(T)));
    }
    /// Remove an entity from the game
    void destroy(Entity e) const { engine().despawn(e); }

    /// A line in the Console
    static void log(const std::string &text) { script::log(0, text); }
    static void warn(const std::string &text) { script::log(1, text); }
    static void error(const std::string &text) { script::log(2, text); }

    /// Set by the library
    Entity entity_ = 0;
    std::vector<unsigned char> synced_;
};

namespace script {

template <class T>
struct Registrar {
    explicit Registrar(const char *name) {
        static_assert(std::is_base_of_v<Behaviour, T>, "ORST_BEHAVIOUR names a Behaviour");
        types().push_back({name, &typeid(T), [] { return std::unique_ptr<Behaviour>(new T()); },
                           [] {
                               Fields<T> f;
                               if constexpr (has_fields<T>(0)) T::fields(f);
                               return f.slots;
                           }});
    }
    template <class U>
    static constexpr auto has_fields(int) -> decltype(U::fields(std::declval<Fields<U> &>()), true) {
        return true;
    }
    template <class U>
    static constexpr bool has_fields(...) {
        return false;
    }
};

}  // namespace script
}  // namespace orst

/// Make a Behaviour known to ORST Studio, after its definition
#define ORST_BEHAVIOUR(T) static ::orst::script::Registrar<T> orst_registrar_##T(#T)

#ifdef ORST_SCRIPT_MAIN
// The library's entry points, which ORST Studio calls. Compiled once, in the
// file the studio writes next to the project's build.

namespace orst::script {

struct Script {
    const Type *type;
    uint32_t id;
    uint32_t size;
    std::vector<Slot> slots;
    std::map<Entity, std::unique_ptr<Behaviour>> live;

    void read(Behaviour &b, const unsigned char *p) const {
        for (auto &s : slots) std::memcpy(reinterpret_cast<unsigned char *>(&b) + s.object_offset, p + s.offset, s.size);
    }
    void write(const Behaviour &b, unsigned char *p) const {
        for (auto &s : slots)
            std::memcpy(p + s.offset, reinterpret_cast<const unsigned char *>(&b) + s.object_offset, s.size);
    }
};

struct State {
    OrstEngine *handle = nullptr;
    std::unique_ptr<Engine> engine;
    void (*log)(uint32_t, const char *, size_t) = nullptr;
    std::vector<Script> scripts;
    bool playing = false;
    bool first_tick = false;
    std::string error;
};

State &state() {
    static State s;
    return s;
}

Engine &engine() { return *state().engine; }

void log(uint32_t level, const std::string &text) {
    if (state().log) state().log(level, text.data(), text.size());
}

Behaviour *find(const std::type_info &type, Entity e) {
    for (auto &s : state().scripts) {
        if (*s.type->info != type) continue;
        auto it = s.live.find(e);
        return it == s.live.end() ? nullptr : it->second.get();
    }
    return nullptr;
}

Behaviour *find_any(const std::type_info &type) {
    for (auto &s : state().scripts)
        if (*s.type->info == type && !s.live.empty()) return s.live.begin()->second.get();
    return nullptr;
}

static uint32_t measure(uint32_t type, uint32_t &align) {
    switch (type) {
        case ORST_FIELD_BOOL:
        case ORST_FIELD_U8: align = 1; return 1;
        case ORST_FIELD_VEC2: align = 4; return 8;
        case ORST_FIELD_COLOR: align = 1; return 4;
        case ORST_FIELD_RECT: align = 4; return 16;
        case ORST_FIELD_ENTITY: align = 8; return 8;
        default: align = 4; return 4;
    }
}

/// Fields in, call, fields out; false when the script threw
template <class F>
static bool run(Script &s, Behaviour &b, F &&call) {
    auto &st = state();
    auto *p = static_cast<unsigned char *>(orst_get(st.handle, b.entity_, s.id));
    if (!p) return true;
    // The bytes win only when something else changed them (the
    // Inspector, the C API); otherwise the object has the truth
    if (b.synced_.size() != s.size || std::memcmp(p, b.synced_.data(), s.size) != 0) s.read(b, p);
    try {
        call(b);
    } catch (const std::exception &e) {
        log(2, s.type->name + ": " + e.what());
        return false;
    } catch (...) {
        log(2, s.type->name + ": an exception");
        return false;
    }
    // The call may have added components, which moves the rows
    p = static_cast<unsigned char *>(orst_get(st.handle, b.entity_, s.id));
    if (p) {
        s.write(b, p);
        b.synced_.assign(p, p + s.size);
    }
    return true;
}

}  // namespace orst::script

/// The studio's engine and its Console
ORST_SCRIPT_EXPORT int orst_script_attach(OrstEngine *engine, void (*log)(uint32_t, const char *, size_t)) {
    auto &st = orst::script::state();
    st.handle = engine;
    st.log = log;
    st.engine = std::make_unique<orst::Engine>(engine, orst::Engine::Borrowed{});
    return 0;
}

/// Define a component for each Behaviour; how many, -1 on error
ORST_SCRIPT_EXPORT int orst_script_define() {
    using namespace orst::script;
    auto &st = state();
    st.scripts.clear();
    try {
        std::vector<const Type *> sorted;
        for (auto &t : types()) sorted.push_back(&t);
        std::sort(sorted.begin(), sorted.end(), [](auto *a, auto *b) { return a->name < b->name; });
        for (const Type *type : sorted) {
            Script s{type, 0, 0, type->describe(), {}};
            uint32_t size = 0, align = 1;
            for (auto &slot : s.slots) {
                uint32_t a;
                uint32_t bytes = measure(slot.type, a);
                size = (size + a - 1) / a * a;
                slot.offset = size;
                size += bytes;
                align = std::max(align, a);
            }
            size = (size + align - 1) / align * align;
            s.size = size;
            std::vector<unsigned char> defaults(size);
            auto made = type->make();
            s.write(*made, defaults.data());
            std::vector<OrstField> fields;
            for (auto &slot : s.slots)
                fields.push_back({slot.name.c_str(), slot.type, slot.offset, slot.flags, slot.min, slot.max,
                                  slot.options.empty() ? nullptr : slot.options.c_str()});
            s.id = orst_define_component(st.handle, type->name.c_str(), size, align, fields.data(),
                                         uint32_t(fields.size()), size ? defaults.data() : nullptr);
            if (s.id == 0) throw orst::Error(type->name + ": " + orst_last_error());
            st.scripts.push_back(std::move(s));
        }
        return int(st.scripts.size());
    } catch (const std::exception &e) {
        st.error = e.what();
        st.scripts.clear();
        return -1;
    }
}

/// Play: objects are made as entities are met
ORST_SCRIPT_EXPORT void orst_script_start() {
    auto &st = orst::script::state();
    st.playing = true;
    st.first_tick = true;
    for (auto &s : st.scripts) s.live.clear();
}

/// One tick: start() for entities new to a script, the contact calls, then
/// update() for all; how many calls threw
ORST_SCRIPT_EXPORT int orst_script_tick(float dt) {
    using namespace orst::script;
    auto &st = state();
    if (!st.playing) return 0;
    int thrown = 0;
    std::vector<std::vector<orst::Entity>> met;
    for (auto &s : st.scripts) {
        std::vector<orst::Entity> es(orst_entity_count(st.handle));
        es.resize(orst_query(st.handle, &s.id, 1, es.data(), uint32_t(es.size())));
        for (auto it = s.live.begin(); it != s.live.end();)
            it = std::find(es.begin(), es.end(), it->first) == es.end() ? s.live.erase(it) : std::next(it);
        met.push_back(std::move(es));
    }
    for (size_t i = 0; i < st.scripts.size(); i++) {
        auto &s = st.scripts[i];
        for (auto e : met[i]) {
            if (s.live.count(e)) continue;
            auto b = s.type->make();
            b->entity_ = e;
            auto &ref = *b;
            s.live[e] = std::move(b);
            if (!run(s, ref, [](orst::Behaviour &x) { x.start(); })) thrown++;
        }
    }
    // Contacts are the engine's from the tick before; none yet on the first
    if (!st.first_tick) {
        for (auto &c : st.engine->contacts()) {
            if (c.state == ORST_CONTACT_STAY) continue;
            for (auto me : {c.a, c.b})
                for (auto &s : st.scripts) {
                    auto it = s.live.find(me);
                    if (it == s.live.end()) continue;
                    auto other = c.other(me);
                    bool ok = c.begin() ? run(s, *it->second, [&](orst::Behaviour &x) { x.on_contact_begin(other, c); })
                                        : run(s, *it->second, [&](orst::Behaviour &x) { x.on_contact_end(other, c); });
                    if (!ok) thrown++;
                }
        }
    }
    st.first_tick = false;
    for (size_t i = 0; i < st.scripts.size(); i++) {
        auto &s = st.scripts[i];
        for (auto e : met[i]) {
            auto it = s.live.find(e);
            if (it != s.live.end() && !run(s, *it->second, [dt](orst::Behaviour &x) { x.update(dt); })) thrown++;
        }
    }
    return thrown;
}

/// Stop: the objects go
ORST_SCRIPT_EXPORT void orst_script_stop() {
    auto &st = orst::script::state();
    st.playing = false;
    for (auto &s : st.scripts) s.live.clear();
}

/// Everything goes before the library is unloaded
ORST_SCRIPT_EXPORT void orst_script_detach() {
    auto &st = orst::script::state();
    st.playing = false;
    st.scripts.clear();
    st.engine.reset();
}

/// The last error, valid until the next call
ORST_SCRIPT_EXPORT const char *orst_script_error() { return orst::script::state().error.c_str(); }

#endif

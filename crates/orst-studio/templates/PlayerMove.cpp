#include "orst_script.hpp"

#include <algorithm>

using namespace orst;

/// Moves the entity with the arrow keys or WASD. With a Velocity the
/// engine moves it and walls stop it; without one it slides anywhere.
struct PlayerMove : Behaviour {
    float speed = 120;

    static void fields(Fields<PlayerMove> &f) { f("Speed", &PlayerMove::speed).range(0, 500); }

    void update(float dt) override {
        auto &e = engine();
        float x = e.axis(Key::Left, Key::Right) + e.axis(Key::A, Key::D);
        float y = e.axis(Key::Up, Key::Down) + e.axis(Key::W, Key::S);
        Vec2 move = Vec2(std::clamp(x, -1.f, 1.f), std::clamp(y, -1.f, 1.f)) * speed;
        if (auto *v = e.get<Velocity>(entity()))
            v->linear = move;
        else
            transform().pos = Vec2(transform().pos) + move * dt;
    }
};
ORST_BEHAVIOUR(PlayerMove);

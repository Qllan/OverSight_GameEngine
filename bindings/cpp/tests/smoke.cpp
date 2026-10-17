// Headless smoke test of the C++ wrapper.
#include <cstdio>
#include <cstdlib>
#include <string>

#include "oversight.hpp"

struct Pos {
    float x, y;
};
OV_COMPONENT(Pos);
struct Vel {
    float x, y;
};
OV_COMPONENT(Vel);

#define CHECK(cond)                                                              \
    do {                                                                         \
        if (!(cond)) {                                                           \
            std::fprintf(stderr, "%s:%d: failed: %s\n", __FILE__, __LINE__, #cond); \
            std::exit(1);                                                        \
        }                                                                        \
    } while (0)

int main() {
    ov::Engine engine({"Smoke", 64, 64, 1.0f, false, false, false, 0, true});

    bool threw = false;
    try {
        engine.load_texture("missing.png");
    } catch (const ov::Error &e) {
        threw = std::string(e.what()).find("missing.png") != std::string::npos;
    }
    CHECK(threw);

    uint8_t white[8 * 8 * 4];
    for (auto &b : white) b = 255;
    ov::Texture tiles = engine.texture_from_rgba(8, 8, white);
    CHECK(tiles.width() == 8);

    // Side view: floor on row 7 and a one-way platform
    ov::Tilemap map = engine.create_tilemap(ov::Orientation::Orthogonal, 8, 8, 8, 8);
    CHECK(map.add_tileset(1, tiles, tiles.sheet(8, 8)));
    CHECK(map.set_tile_flag(1, "solid"));
    for (int x = 0; x < 8; ++x) map.set_tile(0, x, 7, 1);
    CHECK(map.solid(3, 7) && !map.solid(3, 6));

    // Gravity until landing
    ov::Rect player{20, 0, 6, 6};
    bool grounded = false;
    for (int i = 0; i < 120 && !grounded; ++i) {
        ov::MoveResult r = map.move_box(player, {0, 4});
        player = r.rect;
        grounded = r.contacts.ground;
    }
    CHECK(grounded && player.bottom() == 56.0f);
    auto hit = map.raycast({0, 30}, {0, 100});
    CHECK(hit && hit->cell_y == 7);

    CHECK(ov::ease(ov::Ease::QuadIn, 0.5f) == 0.25f);
    ov::Tween<ov::Vec2> slide({0, 0}, {10, 20}, 1.0f, ov::Ease::Linear, ov::Repeat::Once, 0.5f);
    CHECK(slide.update(0.5f).x == 0.0f);
    ov::Vec2 mid = slide.update(0.5f);
    CHECK(mid.x == 5.0f && mid.y == 10.0f && !slide.done());
    ov::Tween<ov::Color> fade(ov::Color::Black, ov::Color::White, 2.0f, ov::Ease::Linear, ov::Repeat::PingPong);
    CHECK(fade.update(3.0f) == ov::Color(128, 128, 128));

    ov::EmitterConfig sparks_cfg;
    sparks_cfg.rate = 10;
    sparks_cfg.texture = tiles.id();
    ov::Emitter sparks = engine.create_emitter(sparks_cfg, {8, 8});
    sparks.update(0.55f);
    CHECK(sparks.count() == 5 && sparks.emitting());
    sparks.set_emitting(false);
    sparks.burst(3);
    CHECK(sparks.count() == 8);
    CHECK(sparks.config().rate == 10.0f);

    ov::Animation walk = ov::Animation::range(4, 4, 8.0f);
    CHECK(walk.frame_at(0.3f) == 6);

    engine.look_at({32, 32});
    CHECK(engine.poll());
    CHECK(engine.ticks() == 1 && engine.tick_dt() == 1.0f / 60.0f);
    engine.random_seed(1);
    CHECK(engine.random_u32() == 0x910A2DECu);
    engine.begin_frame(ov::Color::Black);
    map.draw();
    engine.set_space(ov::Space::Screen);
    engine.fill_rect({0, 0, 4, 4}, {255, 0, 0});
    engine.draw_text("Привет", {10, 10}, 12, ov::Color::White);
    engine.end_frame();
    CHECK(engine.pixel(1, 1) == ov::Color(255, 0, 0));
    CHECK(engine.pixel(30, 60) == ov::Color::White);
    CHECK(engine.measure_text("Привет", 12).x > 20);

    engine.inject_key(ov::Key::Space, true);
    engine.poll();
    CHECK(engine.key_pressed(ov::Key::Space));

    engine.inject_gamepad_connected(1, true);
    engine.inject_pad_button(1, ov::Pad::South, true);
    engine.inject_pad_axis(1, ov::PadAxis::LeftY, -1.0f);
    engine.poll();
    CHECK(engine.gamepad_connected(1) && engine.gamepad_name(1) == "Injected");
    CHECK(engine.pad_pressed(1, ov::Pad::South) && !engine.pad_down(0, ov::Pad::South));
    CHECK(engine.pad_stick(1).y == -1.0f);

    CHECK(engine.renderer_name() == "software");
    engine.set_hot_reload(true);
    CHECK(engine.hot_reload() && engine.reload_changed() == 0 && engine.reloaded() == 0);

    // Debug overlay: drawn only while on, above the game
    engine.set_debug(true);
    CHECK(engine.debug());
    engine.begin_frame(ov::Color::Black);
    engine.debug_rect({0, 57, 8, 7}, {0, 255, 0});
    engine.end_frame();
    CHECK(engine.pixel(0, 60) == ov::Color(0, 255, 0)); // below the stats panel
    // Entities: move everything that has a velocity
    ov::Entity a = engine.spawn(), b = engine.spawn();
    engine.set(a, Pos{0, 0});
    engine.set(a, Vel{1, 2});
    engine.set(b, Pos{5, 5});
    engine.each<Pos, Vel>([](ov::Entity, Pos &p, Vel &v) {
        p.x += v.x;
        p.y += v.y;
    });
    CHECK(engine.get<Pos>(a)->x == 1 && engine.get<Pos>(a)->y == 2);
    CHECK(engine.get<Pos>(b)->x == 5 && engine.get<Vel>(b) == nullptr);
    CHECK((engine.query<Pos>().size() == 2 && engine.query<Pos, Vel>().size() == 1));
    CHECK(engine.remove<Vel>(a) && !engine.has<Vel>(a));
    auto saved = engine.snapshot();
    engine.despawn(b);
    engine.restore(saved);
    CHECK(engine.alive(b) && engine.get<Pos>(b)->x == 5);
    engine.clear_world();
    CHECK(engine.entity_count() == 0 && !engine.alive(a));

    // Built-in components: the engine moves them
    CHECK(engine.component<ov::Transform>() == OV_TRANSFORM && engine.component<ov::Body>() == OV_BODY);
    ov::Entity mover = engine.spawn();
    engine.set(mover, ov::Transform{{10, 20}});
    engine.set(mover, ov::Velocity{{4, 0}, 1});
    engine.update_motion(0.5f);
    CHECK(engine.get<ov::Transform>(mover)->pos.x == 12 && engine.get<ov::Transform>(mover)->scale.x == 1);
    CHECK(engine.get<ov::Transform>(mover)->rotation == 0.5f);

    // Colliders: a box and a circle overlap along x
    ov::Entity wall = engine.spawn(), ball = engine.spawn();
    engine.set(wall, ov::Transform{{100, 100}});
    engine.set(wall, ov::Collider::box({10, 10}));
    engine.set(ball, ov::Transform{{107, 100}});
    engine.set(ball, ov::Collider::circle(3));
    auto o = engine.overlap(wall, ball);
    CHECK(o && o->normal.x > 0.99f && o->depth > 0.99f && o->depth < 1.01f);
    CHECK(engine.component<ov::Collider>() == OV_COLLIDER);
    CHECK(engine.create_polygon({{0, -2}, {2, 2}, {-2, 2}}) != 0);
    engine.update_collisions();
    auto contacts = engine.contacts();
    CHECK(contacts.size() == 1 && contacts[0].begin() && contacts[0].other(wall) == ball);
    CHECK(engine.query_point({100, 100}).size() == 1 && engine.raycast_entities({150, 100}, {0, 100})->entity == ball);

    // Scenes: a push keeps the scene below, a pop resumes it
    CHECK(engine.go_scene(1, ov::Transition::Cut));
    engine.poll();
    CHECK(engine.push_scene(2, ov::Transition::Cut) && !engine.pop_scene());
    engine.poll();
    CHECK(engine.scene() == 2 && engine.pop_scene(ov::Transition::Cut));
    engine.poll();
    CHECK(engine.scene() == 1 && engine.scene_entered() == 1 && engine.scene_left() == 2);

    // Actions: bound keys, rebinding by capture, saved as text
    uint32_t jump = engine.action("jump");
    CHECK(engine.bind(jump, ov::Binding::key(ov::Key::J)) && engine.bind(jump, ov::Binding::pad(ov::Pad::South)));
    engine.inject_key(ov::Key::J, true);
    engine.poll();
    CHECK(engine.action_pressed(jump) && engine.captured_binding() == ov::Binding::key(ov::Key::J));
    CHECK(engine.save_bindings() == "jump = key:J pad:South\n");
    engine.load_bindings("jump = pad:LeftX+");
    CHECK(engine.bindings(jump).size() == 1 && engine.binding_text(engine.bindings(jump)[0]) == "pad:LeftX+");

    std::printf("smoke_cpp: ok\n");
    return 0;
}

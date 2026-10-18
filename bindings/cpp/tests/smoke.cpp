// Headless smoke test of the C++ wrapper.
#include <cstdio>
#include <cstdlib>
#include <string>

#include "orst.hpp"

struct Pos {
    float x, y;
};
ORST_COMPONENT(Pos);
struct Vel {
    float x, y;
};
ORST_COMPONENT(Vel);

#define CHECK(cond)                                                              \
    do {                                                                         \
        if (!(cond)) {                                                           \
            std::fprintf(stderr, "%s:%d: failed: %s\n", __FILE__, __LINE__, #cond); \
            std::exit(1);                                                        \
        }                                                                        \
    } while (0)

int main() {
    orst::Engine engine({"Smoke", 64, 64, 1.0f, false, false, false, 0, true});

    bool threw = false;
    try {
        engine.load_texture("missing.png");
    } catch (const orst::Error &e) {
        threw = std::string(e.what()).find("missing.png") != std::string::npos;
    }
    CHECK(threw);

    uint8_t white[8 * 8 * 4];
    for (auto &b : white) b = 255;
    orst::Texture tiles = engine.texture_from_rgba(8, 8, white);
    CHECK(tiles.width() == 8);

    // Side view: floor on row 7 and a one-way platform
    orst::Tilemap map = engine.create_tilemap(orst::Orientation::Orthogonal, 8, 8, 8, 8);
    CHECK(map.add_tileset(1, tiles, tiles.sheet(8, 8)));
    CHECK(map.set_tile_flag(1, "solid"));
    for (int x = 0; x < 8; ++x) map.set_tile(0, x, 7, 1);
    CHECK(map.find_path({4, 52}, {60, 52}).size() == 8 && map.find_path({4, 52}, {4, 60}).empty());
    CHECK(map.solid(3, 7) && !map.solid(3, 6));

    // Gravity until landing
    orst::Rect player{20, 0, 6, 6};
    bool grounded = false;
    for (int i = 0; i < 120 && !grounded; ++i) {
        orst::MoveResult r = map.move_box(player, {0, 4});
        player = r.rect;
        grounded = r.contacts.ground;
    }
    CHECK(grounded && player.bottom() == 56.0f);
    auto hit = map.raycast({0, 30}, {0, 100});
    CHECK(hit && hit->cell_y == 7);

    CHECK(orst::ease(orst::Ease::QuadIn, 0.5f) == 0.25f);
    orst::Tween<orst::Vec2> slide({0, 0}, {10, 20}, 1.0f, orst::Ease::Linear, orst::Repeat::Once, 0.5f);
    CHECK(slide.update(0.5f).x == 0.0f);
    orst::Vec2 mid = slide.update(0.5f);
    CHECK(mid.x == 5.0f && mid.y == 10.0f && !slide.done());
    orst::Tween<orst::Color> fade(orst::Color::Black, orst::Color::White, 2.0f, orst::Ease::Linear, orst::Repeat::PingPong);
    CHECK(fade.update(3.0f) == orst::Color(128, 128, 128));

    orst::EmitterConfig sparks_cfg;
    sparks_cfg.rate = 10;
    sparks_cfg.texture = tiles.id();
    orst::Emitter sparks = engine.create_emitter(sparks_cfg, {8, 8});
    sparks.update(0.55f);
    CHECK(sparks.count() == 5 && sparks.emitting());
    sparks.set_emitting(false);
    sparks.burst(3);
    CHECK(sparks.count() == 8);
    CHECK(sparks.config().rate == 10.0f);
    orst::GpuEmitter dust = engine.create_gpu_emitter(sparks_cfg, {8, 8});
    engine.update_gpu_emitters(0.55f);
    dust.burst(3);
    CHECK(dust.count() == 8 && dust.config().rate == 10.0f);

    orst::Animation walk = orst::Animation::range(4, 4, 8.0f);
    CHECK(walk.frame_at(0.3f) == 6);

    engine.look_at({32, 32});
    CHECK(engine.poll());
    CHECK(engine.ticks() == 1 && engine.tick_dt() == 1.0f / 60.0f);
    engine.random_seed(1);
    CHECK(engine.random_u32() == 0x910A2DECu);
    engine.begin_frame(orst::Color::Black);
    map.draw();
    engine.set_space(orst::Space::Screen);
    engine.fill_rect({0, 0, 4, 4}, {255, 0, 0});
    engine.draw_text("Привет", {10, 10}, 12, orst::Color::White);
    engine.end_frame();
    CHECK(engine.pixel(1, 1) == orst::Color(255, 0, 0));
    CHECK(engine.pixel(30, 60) == orst::Color::White);
    CHECK(engine.measure_text("Привет", 12).x > 20);

    engine.inject_key(orst::Key::Space, true);
    engine.poll();
    CHECK(engine.key_pressed(orst::Key::Space));

    engine.inject_gamepad_connected(1, true);
    engine.inject_pad_button(1, orst::Pad::South, true);
    engine.inject_pad_axis(1, orst::PadAxis::LeftY, -1.0f);
    engine.poll();
    CHECK(engine.gamepad_connected(1) && engine.gamepad_name(1) == "Injected");
    CHECK(engine.pad_pressed(1, orst::Pad::South) && !engine.pad_down(0, orst::Pad::South));
    CHECK(engine.pad_stick(1).y == -1.0f);

    CHECK(engine.renderer_name() == "software");
    engine.set_hot_reload(true);
    CHECK(engine.hot_reload() && engine.reload_changed() == 0 && engine.reloaded() == 0);

    // Debug overlay: drawn only while on, above the game
    engine.set_debug(true);
    CHECK(engine.debug());
    engine.begin_frame(orst::Color::Black);
    engine.debug_rect({0, 57, 8, 7}, {0, 255, 0});
    engine.end_frame();
    CHECK(engine.pixel(0, 60) == orst::Color(0, 255, 0)); // below the stats panel
    // Entities: move everything that has a velocity
    orst::Entity a = engine.spawn(), b = engine.spawn();
    engine.set(a, Pos{0, 0});
    engine.set(a, Vel{1, 2});
    engine.set(b, Pos{5, 5});
    engine.each<Pos, Vel>([](orst::Entity, Pos &p, Vel &v) {
        p.x += v.x;
        p.y += v.y;
    });
    CHECK(engine.get<Pos>(a)->x == 1 && engine.get<Pos>(a)->y == 2);
    CHECK(engine.get<Pos>(b)->x == 5 && engine.get<Vel>(b) == nullptr);
    CHECK((engine.query<Pos>().size() == 2 && engine.query<Pos, Vel>().size() == 1));
    CHECK(engine.remove<Vel>(a) && !engine.has<Vel>(a));
    engine.set_fields<Vel>({{"x", ORST_FIELD_F32, offsetof(Vel, x), ORST_FIELD_ANGLE, 0, 0, nullptr}});
    auto vel_fields = engine.fields(engine.component<Vel>());
    CHECK(vel_fields.size() == 1 && vel_fields[0].name == "x" && vel_fields[0].flags == ORST_FIELD_ANGLE);
    engine.set(a, orst::Name(std::string(40, 'x') + "\xd1\x8f\xd1\x8f\xd1\x8f")); // 46 bytes
    CHECK(engine.find_named(engine.get<orst::Name>(a)->str()) == a && engine.get<orst::Name>(a)->str().size() == 46);
    CHECK(orst::Name(std::string(63, 'x') + "\xd1\x8f").str().size() == 63); // a letter is never cut in half
    {
        orst::Camera before = engine.camera();
        orst::Entity cam = engine.spawn();
        engine.set(cam, orst::Transform{});
        engine.set(cam, orst::Velocity{{60, 0}});
        engine.set(cam, orst::Camera2D(2.0f));
        engine.step(0.5f);
        CHECK(engine.camera().center.x == 30.0f && engine.camera().zoom == 2.0f);
        engine.set(cam, orst::ParticleEmitter());
        engine.step(0.1f);
        CHECK(sizeof(orst::ParticleEmitter) == 108);
        engine.despawn(cam);
        engine.set_camera(before);
    }
    auto collider = engine.fields(ORST_COLLIDER);
    CHECK(collider[0].name == "kind" && collider[0].options.size() == 3 && collider[0].options[2] == "Polygon");
    auto saved = engine.snapshot();
    engine.despawn(b);
    engine.restore(saved);
    CHECK(engine.alive(b) && engine.get<Pos>(b)->x == 5);
    engine.clear_world();
    CHECK(engine.entity_count() == 0 && !engine.alive(a));

    // Built-in components: the engine moves them
    CHECK(engine.component<orst::Transform>() == ORST_TRANSFORM && engine.component<orst::Body>() == ORST_BODY);
    orst::Entity mover = engine.spawn();
    engine.set(mover, orst::Transform{{10, 20}});
    engine.set(mover, orst::Velocity{{4, 0}, 1});
    engine.update_motion(0.5f);
    CHECK(engine.get<orst::Transform>(mover)->pos.x == 12 && engine.get<orst::Transform>(mover)->scale.x == 1);
    CHECK(engine.get<orst::Transform>(mover)->rotation == 0.5f);

    // Colliders: a box and a circle overlap along x
    orst::Entity wall = engine.spawn(), ball = engine.spawn();
    engine.set(wall, orst::Transform{{100, 100}});
    engine.set(wall, orst::Collider::box({10, 10}));
    engine.set(ball, orst::Transform{{107, 100}});
    engine.set(ball, orst::Collider::circle(3));
    auto o = engine.overlap(wall, ball);
    CHECK(o && o->normal.x > 0.99f && o->depth > 0.99f && o->depth < 1.01f);
    CHECK(engine.component<orst::Collider>() == ORST_COLLIDER);
    CHECK(engine.create_polygon({{0, -2}, {2, 2}, {-2, 2}}) != 0);
    engine.update_collisions();
    auto contacts = engine.contacts();
    CHECK(contacts.size() == 1 && contacts[0].begin() && contacts[0].other(wall) == ball);
    CHECK(engine.query_point({100, 100}).size() == 1 && engine.raycast_entities({150, 100}, {0, 100})->entity == ball);

    // Physics: a crate falls onto a fixed floor
    engine.set_gravity({0, 400});
    orst::Entity floor = engine.spawn(), crate = engine.spawn();
    engine.set(floor, orst::Transform{{500, 100}});
    engine.set(floor, orst::Collider::box({300, 20}));
    engine.set(floor, orst::RigidBody::fixed());
    engine.set(crate, orst::Transform{{500, 0}});
    engine.set(crate, orst::Collider::box({16, 16}));
    engine.set(crate, orst::RigidBody::dynamic());
    for (int i = 0; i < 120; i++) engine.physics_step(1.0f / 60);
    CHECK(engine.get<orst::Transform>(crate)->pos.y > 81 && engine.get<orst::Transform>(crate)->pos.y < 83);
    CHECK(engine.component<orst::RigidBody>() == ORST_RIGIDBODY && engine.body_mass(crate) > 0);
    engine.despawn(floor);
    engine.despawn(crate);

    // Scenes: a push keeps the scene below, a pop resumes it
    CHECK(engine.go_scene(1, orst::Transition::Cut));
    engine.poll();
    CHECK(engine.push_scene(2, orst::Transition::Cut) && !engine.pop_scene());
    engine.poll();
    CHECK(engine.scene() == 2 && engine.pop_scene(orst::Transition::Cut));
    engine.poll();
    CHECK(engine.scene() == 1 && engine.scene_entered() == 1 && engine.scene_left() == 2);

    // Actions: bound keys, rebinding by capture, saved as text
    uint32_t jump = engine.action("jump");
    CHECK(engine.bind(jump, orst::Binding::key(orst::Key::J)) && engine.bind(jump, orst::Binding::pad(orst::Pad::South)));
    engine.inject_key(orst::Key::J, true);
    engine.poll();
    CHECK(engine.action_pressed(jump) && engine.captured_binding() == orst::Binding::key(orst::Key::J));
    CHECK(engine.save_bindings() == "jump = key:J pad:South\n");
    engine.load_bindings("jump = pad:LeftX+");
    CHECK(engine.bindings(jump).size() == 1 && engine.binding_text(engine.bindings(jump)[0]) == "pad:LeftX+");

    // Render targets and post effects: a red target on screen, then greyed
    engine.set_debug(false);
    orst::Texture target = engine.create_target(8, 8);
    engine.begin_frame(orst::Color::Black);
    engine.draw_to(target);
    engine.clear_target(target, {255, 0, 0});
    engine.draw_to_screen();
    engine.set_space(orst::Space::Screen);
    engine.draw(target, 0, 0);
    engine.end_frame();
    CHECK(engine.pixel(4, 4) == orst::Color(255, 0, 0) && engine.pixel(20, 20) == orst::Color::Black);
    engine.set_effects({orst::Effect::grade(0, 1, 0), orst::Effect::glow(), orst::Effect::crt(0, 0, 0, 0)});
    engine.begin_frame(orst::Color(255, 0, 0));
    engine.end_frame();
    orst::Color grey = engine.pixel(30, 30);
    CHECK(grey.r == grey.g && grey.g == grey.b && grey.r > 0);
    engine.set_effects({});
    orst::Shader keep = engine.create_shader(
        "fn effect(uv: vec2<f32>, pixel: vec2<f32>) -> vec4<f32> { return load(vec2<i32>(pixel)); }");
    bool bad_shader = false;
    try {
        engine.create_shader("fn effect() {}");
    } catch (const orst::Error &) {
        bad_shader = true;
    }
    CHECK(keep && bad_shader);
    engine.shake(4, 0.2f);

    // Lighting: one light in the dark
    engine.set_effects({orst::Effect::lighting()});
    orst::Entity lamp = engine.spawn();
    engine.set(lamp, orst::Transform{{32, 32}});
    engine.set(lamp, orst::Light(40));
    engine.set_shadow_layers(1);
    engine.set_normal_map(target, target);
    engine.begin_frame(orst::Color::White);
    engine.end_frame();
    CHECK(engine.pixel(32, 32).r > 200 && engine.pixel(0, 0).r == 0);
    CHECK(engine.component<orst::Light>() == ORST_LIGHT);
    engine.set_effects({});
    engine.despawn(lamp);

    // Skeletons
    auto rig = engine.skeleton_from_json(R"({"bones": [{"name": "root"}, {"name": "arm", "parent": "root", "x": 10}],
        "slots": [{"name": "arm", "bone": "arm", "attachment": "a", "attachments": {"a": {"region": [0, 0, 2, 2]}}}],
        "animations": {"wave": {"bones": {"arm": {"rotate": [{"time": 0, "value": 0}, {"time": 1, "value": 90}]}},
                                "events": [{"time": 0.5, "name": "half"}]}}})");
    orst::Skeleton skel = engine.create_skeleton(rig);
    skel.play("wave", false);
    engine.update_skeletons(0.6f);
    CHECK(skel.fired("half") && skel.animation() == "wave");
    int arm = skel.bone("arm");
    skel.set_root(orst::Transform{{100, 0}});
    CHECK(arm == 1 && skel.bone_world(arm).pos.x == 110);
    bool no_dance = false;
    try {
        skel.play("dance");
    } catch (const orst::Error &) {
        no_dance = true;
    }
    CHECK(no_dance);
    skel.set_attachment("arm", "");
    skel.draw(target);

    // Preferences
    const char *tmp = std::getenv("TMPDIR") ? std::getenv("TMPDIR") : std::getenv("TEMP");
    auto prefs_dir = std::string(tmp ? tmp : ".") + "/orst_smoke_cpp_prefs";
    engine.set_save_dir(prefs_dir);
    CHECK(engine.set_pref("name", "Ёж") && engine.pref("name") == "Ёж" && engine.pref("none", "x") == "x");

    // Translations
    CHECK(engine.load_locale("en", R"({"hi": "Hello"})") && engine.load_locale("ru", R"({"hi": "Привет"})"));
    CHECK(engine.languages().size() == 2 && engine.language() == "en" && engine.set_language("ru") && engine.tr("hi") == "Привет");

    // The interface components keep the engine's ids
    auto label = engine.spawn();
    engine.set(label, orst::UiRect({40, 12}));
    engine.set(label, orst::UiText(engine.intern("Hi")));
    engine.set(label, orst::UiButton());
    CHECK(engine.component<orst::UiButton>() == ORST_UI_BUTTON);
    CHECK(engine.text(orst::Str{engine.get<orst::UiText>(label)->text}) == "Hi");

    std::printf("smoke_cpp: ok\n");
    return 0;
}

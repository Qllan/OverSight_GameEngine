// Top-down: walk around a garden, bump into walls and crates, read a sign.
// WASD, arrows or a gamepad stick to walk. Tiles and the hero are y-sorted together.
#include "common.hpp"

int main(int argc, char **argv) try {
    Options opt = parse_options(argc, argv);
    orst::Engine engine(window("ORST: вид сверху", opt));
    orst::Tilemap map = engine.load_tilemap(opt.assets + "/topdown.tmj");
    orst::Texture hero = engine.load_texture(opt.assets + "/hero.png");
    orst::SpriteSheet frames = hero.sheet(16, 16);
    OrstTilemapInfo info = map.info();
    const float map_w = float(info.width * info.tile_w), map_h = float(info.height * info.tile_h);
    const uint32_t ground = uint32_t(map.layer_index("ground")), walls = uint32_t(map.layer_index("walls"));

    // The hero's feet: a small box that collides, the sprite drawn above it
    orst::Vec2 feet{64, 168};
    if (auto spawn = map.find_object("player")) feet = {spawn->rect.x, spawn->rect.y};
    const float box_w = 10, box_h = 6;
    orst::Animation walk = orst::Animation::range(0, 4, 8.0f);
    float walk_time = 0;
    int row = 0;  // 0 down, 1 up, 2 side
    bool left = false;
    auto sign = map.find_object("sign");
    std::string sign_text = sign ? map.string(*sign, "text").value_or("") : "";
    // Edit topdown.tmj in Tiled or hero.png and save: the running game picks it up
    engine.set_hot_reload(true);

    Script script{{{10, orst::Key::D, true}, {70, orst::Key::D, false}, {70, orst::Key::W, true},
                   {100, orst::Key::W, false}, {100, orst::Key::S, true}, {180, orst::Key::S, false}}};
    int frame = 0;
    while (next_frame(engine, opt, script, frame)) {
        float dt = engine.dt();
        if (engine.reloaded()) {
            sign = map.find_object("sign");
            sign_text = sign ? map.string(*sign, "text").value_or("") : "";
        }
        orst::Vec2 dir{engine.axis(orst::Key::A, orst::Key::D) + engine.axis(orst::Key::Left, orst::Key::Right),
                     engine.axis(orst::Key::W, orst::Key::S) + engine.axis(orst::Key::Up, orst::Key::Down)};
        dir = dir + engine.pad_stick(0);  // first gamepad's left stick
        dir = {clampf(dir.x, -1, 1), clampf(dir.y, -1, 1)};
        float len = std::sqrt(dir.x * dir.x + dir.y * dir.y);
        if (len > 0) {
            const float speed = 70.0f;
            orst::Vec2 step = dir * (speed * dt / len);
            orst::Rect box{feet.x - box_w / 2, feet.y - box_h, box_w, box_h};
            box = map.move_box(box, step).rect;
            feet = {box.x + box_w / 2, box.bottom()};
            walk_time += dt;
            if (std::fabs(dir.x) > std::fabs(dir.y)) {
                row = 2;
                left = dir.x < 0;
            } else {
                row = dir.y < 0 ? 1 : 0;
            }
        } else {
            walk_time = 0;
        }

        engine.look_at(follow(feet, map_w, map_h, 320, 180));
        engine.begin_frame(orst::Color(20, 24, 30));
        map.draw_layer(ground, 0);
        map.draw_layer(walls, 1, true);  // y-sorted with the hero
        orst::Sprite s;
        s.pos = feet;
        s.origin = {8, 15};
        s.flip_x = left;
        s.layer = 1;
        s.depth = feet.y;
        engine.draw_frame(hero, frames, uint32_t(row * 4) + (len > 0 ? walk.frame_at(walk_time) : 0), s);

        engine.set_space(orst::Space::Screen);
        engine.set_layer(10);
        engine.draw_text("WASD: ходить", {4, 2}, 10, orst::Color::White);
        if (sign && sign->rect.overlaps(orst::Rect{feet.x - 6, feet.y - 12, 12, 12})) {
            engine.fill_rect({20, 140, 280, 30}, orst::Color(0, 0, 0, 200));
            engine.draw_text(sign_text, {28, 148}, 12, orst::Color(255, 240, 200));
        }
        engine.set_space(orst::Space::World);
        engine.set_layer(0);
        engine.end_frame();
    }
    return finish(engine, opt, "topdown");
} catch (const orst::Error &e) {
    std::fprintf(stderr, "error: %s\n", e.what());
    return 1;
}

// Platformer: run, jump, land on one-way planks, reach the flag.
// A/D or arrows to run, Space/W/Up to jump. Falling into a pit respawns.
#include "common.hpp"

int main(int argc, char **argv) try {
    Options opt = parse_options(argc, argv);
    ov::Engine engine(window("OverSight: платформер", opt));
    ov::Tilemap map = engine.load_tilemap(opt.assets + "/platformer.tmj");
    ov::Texture hero = engine.load_texture(opt.assets + "/hero.png");
    ov::SpriteSheet frames = hero.sheet(16, 16);
    OvTilemapInfo info = map.info();
    const float map_w = float(info.width * info.tile_w), map_h = float(info.height * info.tile_h);

    ov::Vec2 spawn{40, 190};
    if (auto p = map.find_object("player")) spawn = {p->rect.x, p->rect.y};
    ov::Rect goal{};
    if (auto g = map.find_object("goal")) goal = g->rect;

    const float gravity = 900, run_speed = 110, jump_speed = 300;
    ov::Rect box{spawn.x - 5, spawn.y - 14, 10, 14};
    ov::Vec2 vel{};
    float since_ground = 1, jump_buffer = 1, run_time = 0;
    bool left = false, won = false;
    ov::Animation run = ov::Animation::range(8, 4, 10.0f);

    // Run right, hop over the crate, then over the pit
    Script script{{{5, ov::Key::D, true}, {66, ov::Key::Space, true}, {82, ov::Key::Space, false},
                   {100, ov::Key::Space, true}, {116, ov::Key::Space, false}}};
    int frame = 0;
    while (next_frame(engine, opt, script, frame)) {
        float dt = engine.dt();
        float dir = engine.axis(ov::Key::A, ov::Key::D) + engine.axis(ov::Key::Left, ov::Key::Right);
        dir = clampf(dir, -1, 1);
        vel.x = dir * run_speed;
        if (dir != 0) left = dir < 0;
        run_time = dir != 0 ? run_time + dt : 0;

        // Forgiving jumps: a little after leaving a ledge, a little before landing
        since_ground += dt;
        jump_buffer += dt;
        if (engine.key_pressed(ov::Key::Space) || engine.key_pressed(ov::Key::W) || engine.key_pressed(ov::Key::Up))
            jump_buffer = 0;
        if (jump_buffer < 0.12f && since_ground < 0.1f) {
            vel.y = -jump_speed;
            jump_buffer = since_ground = 1;
        }
        // Short hop when the button is let go early
        bool held = engine.key_down(ov::Key::Space) || engine.key_down(ov::Key::W) || engine.key_down(ov::Key::Up);
        if (!held && vel.y < -jump_speed / 3) vel.y = -jump_speed / 3;

        vel.y = std::fmin(vel.y + gravity * dt, 500.0f);
        ov::MoveResult r = map.move_box(box, vel * dt);
        box = r.rect;
        if (r.contacts.ground) {
            vel.y = 0;
            since_ground = 0;
        }
        if (r.contacts.ceiling) vel.y = 0;
        if (box.y > map_h) {
            box = {spawn.x - 5, spawn.y - 14, 10, 14};
            vel = {};
        }
        if (box.overlaps(goal)) won = true;

        ov::Vec2 feet{box.x + box.w / 2, box.bottom()};
        engine.look_at(follow(feet, map_w, map_h, 320, 180));
        engine.begin_frame(ov::Color(90, 150, 210));
        map.draw();
        ov::Sprite s;
        s.pos = feet;
        s.origin = {8, 16};
        s.flip_x = left;
        s.layer = 5;
        bool airborne = since_ground > 0.05f;
        uint32_t f = airborne ? 9 : (dir != 0 ? run.frame_at(run_time) : 8);
        engine.draw_frame(hero, frames, f, s);

        engine.set_space(ov::Space::Screen);
        engine.set_layer(10);
        engine.draw_text("A/D: бег, Space: прыжок", {4, 2}, 10, ov::Color::White);
        if (won) engine.draw_text("Победа!", {120, 70}, 24, ov::Color(255, 230, 90));
        engine.set_space(ov::Space::World);
        engine.set_layer(0);
        engine.end_frame();
    }
    return finish(engine, opt, "platformer");
} catch (const ov::Error &e) {
    std::fprintf(stderr, "error: %s\n", e.what());
    return 1;
}

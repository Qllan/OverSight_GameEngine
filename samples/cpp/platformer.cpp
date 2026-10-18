// Platformer: run, jump, land on one-way planks, reach the flag.
// A/D or arrows to run, Space/W/Up to jump; or a gamepad: stick and A. Falling into a pit respawns.
#include "common.hpp"

int main(int argc, char **argv) try {
    Options opt = parse_options(argc, argv);
    orst::Engine engine(window("ORST: платформер", opt));
    orst::Tilemap map = engine.load_tilemap(opt.assets + "/platformer.tmj");
    orst::Texture hero = engine.load_texture(opt.assets + "/hero.png");
    orst::SpriteSheet frames = hero.sheet(16, 16);
    OrstTilemapInfo info = map.info();
    const float map_w = float(info.width * info.tile_w), map_h = float(info.height * info.tile_h);

    orst::Vec2 spawn{40, 190};
    if (auto p = map.find_object("player")) spawn = {p->rect.x, p->rect.y};
    orst::Rect goal{};
    if (auto g = map.find_object("goal")) goal = g->rect;

    const float gravity = 900, run_speed = 110, jump_speed = 300;
    orst::Rect box{spawn.x - 5, spawn.y - 14, 10, 14};
    orst::Vec2 vel{};
    float since_ground = 1, jump_buffer = 1, run_time = 0;
    bool left = false, won = false;
    orst::Animation run = orst::Animation::range(8, 4, 10.0f);

    // Dust from the feet: a trail while running, a puff on landing
    orst::EmitterConfig dust_cfg;
    dust_cfg.rate = 14;
    dust_cfg.max = 80;
    dust_cfg.life_min = 0.25f;
    dust_cfg.life_max = 0.5f;
    dust_cfg.speed_min = 8;
    dust_cfg.speed_max = 30;
    dust_cfg.spread = 3.0f;
    dust_cfg.gravity = {0, -20};
    dust_cfg.drag = 3;
    dust_cfg.size_start = 4;
    dust_cfg.size_end = 1;
    dust_cfg.color_start = orst::Color(235, 225, 205, 200);
    dust_cfg.color_end = orst::Color(235, 225, 205, 0);
    dust_cfg.area = {3, 0};
    dust_cfg.layer = 4;
    orst::Emitter dust = engine.create_emitter(dust_cfg);

    // Run right, hop over the crate, then over the pit
    Script script{{{5, orst::Key::D, true}, {66, orst::Key::Space, true}, {82, orst::Key::Space, false},
                   {100, orst::Key::Space, true}, {116, orst::Key::Space, false}}};
    int frame = 0;
    while (next_frame(engine, opt, script, frame)) {
        float dt = engine.dt();
        float dir = engine.axis(orst::Key::A, orst::Key::D) + engine.axis(orst::Key::Left, orst::Key::Right) +
                    engine.pad_axis(0, orst::PadAxis::LeftX);
        dir = clampf(dir, -1, 1);
        vel.x = dir * run_speed;
        if (dir != 0) left = dir < 0;
        run_time = dir != 0 ? run_time + dt : 0;

        // Forgiving jumps: a little after leaving a ledge, a little before landing
        since_ground += dt;
        jump_buffer += dt;
        if (engine.key_pressed(orst::Key::Space) || engine.key_pressed(orst::Key::W) || engine.key_pressed(orst::Key::Up) ||
            engine.pad_pressed(0, orst::Pad::South))
            jump_buffer = 0;
        if (jump_buffer < 0.12f && since_ground < 0.1f) {
            vel.y = -jump_speed;
            jump_buffer = since_ground = 1;
        }
        // Short hop when the button is let go early
        bool held = engine.key_down(orst::Key::Space) || engine.key_down(orst::Key::W) || engine.key_down(orst::Key::Up) ||
                    engine.pad_down(0, orst::Pad::South);
        if (!held && vel.y < -jump_speed / 3) vel.y = -jump_speed / 3;

        vel.y = std::fmin(vel.y + gravity * dt, 500.0f);
        orst::MoveResult r = map.move_box(box, vel * dt);
        box = r.rect;
        if (r.contacts.ground) {
            if (since_ground > 0.2f) dust.burst(8);
            vel.y = 0;
            since_ground = 0;
        }
        if (r.contacts.ceiling) vel.y = 0;
        if (box.y > map_h) {
            box = {spawn.x - 5, spawn.y - 14, 10, 14};
            vel = {};
        }
        if (box.overlaps(goal)) won = true;

        orst::Vec2 feet{box.x + box.w / 2, box.bottom()};
        dust.set_pos(feet);
        dust.set_emitting(since_ground == 0 && dir != 0);
        engine.update_emitters(dt);
        engine.look_at(follow(feet, map_w, map_h, 320, 180));
        engine.begin_frame(orst::Color(90, 150, 210));
        map.draw();
        dust.draw();
        orst::Sprite s;
        s.pos = feet;
        s.origin = {8, 16};
        s.flip_x = left;
        s.layer = 5;
        bool airborne = since_ground > 0.05f;
        uint32_t f = airborne ? 9 : (dir != 0 ? run.frame_at(run_time) : 8);
        engine.draw_frame(hero, frames, f, s);

        engine.set_space(orst::Space::Screen);
        engine.set_layer(10);
        engine.draw_text("A/D: бег, Space: прыжок", {4, 2}, 10, orst::Color::White);
        if (won) engine.draw_text("Победа!", {120, 70}, 24, orst::Color(255, 230, 90));
        engine.set_space(orst::Space::World);
        engine.set_layer(0);
        engine.end_frame();
    }
    return finish(engine, opt, "platformer");
} catch (const orst::Error &e) {
    std::fprintf(stderr, "error: %s\n", e.what());
    return 1;
}

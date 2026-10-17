// Isometric: step from cell to cell around stone blocks; the mouse shows
// the cell under it. WASD or arrows move one cell (W/Up is up-right on
// screen, A/Left up-left). Blocks and the hero are y-sorted together.
#include "common.hpp"

int main(int argc, char **argv) try {
    Options opt = parse_options(argc, argv);
    ov::Engine engine(window("OverSight: изометрия", opt));
    ov::Tilemap map = engine.load_tilemap(opt.assets + "/iso.tmj");
    ov::Texture hero = engine.load_texture(opt.assets + "/hero.png");
    ov::SpriteSheet frames = hero.sheet(16, 16);
    OvTilemapInfo info = map.info();
    const uint32_t floor_layer = uint32_t(map.layer_index("floor")), blocks = uint32_t(map.layer_index("blocks"));

    int cx = 2, cy = 2;
    if (auto p = map.find_object("player")) {
        ov::Vec2 t = map.world_to_tile({p->rect.x, p->rect.y});
        cx = int(std::floor(t.x));
        cy = int(std::floor(t.y));
    }
    ov::Vec2 from = map.cell_center(cx, cy), pos = from;
    float step_time = 1;  // 0..1 while moving between cells
    const float step_duration = 0.18f;
    int row = 0;
    bool left = false;

    Script script{{{5, ov::Key::D, true}, {60, ov::Key::D, false}, {60, ov::Key::S, true}, {130, ov::Key::S, false}}};
    int frame = 0;
    while (next_frame(engine, opt, script, frame)) {
        float dt = engine.dt();
        if (step_time >= 1) {
            int dx = 0, dy = 0;
            if (engine.key_down(ov::Key::D) || engine.key_down(ov::Key::Right)) dx = 1;
            else if (engine.key_down(ov::Key::A) || engine.key_down(ov::Key::Left)) dx = -1;
            else if (engine.key_down(ov::Key::S) || engine.key_down(ov::Key::Down)) dy = 1;
            else if (engine.key_down(ov::Key::W) || engine.key_down(ov::Key::Up)) dy = -1;
            if ((dx || dy) && !map.solid(cx + dx, cy + dy)) {
                cx += dx;
                cy += dy;
                from = pos;
                step_time = 0;
                // Screen direction of the step picks the sprite row
                row = dy > 0 || dx > 0 ? 0 : 1;
                left = dx < 0 || dy > 0;
            }
        }
        if (step_time < 1) {
            step_time = std::fmin(step_time + dt / step_duration, 1.0f);
            ov::Vec2 to = map.cell_center(cx, cy);
            pos = from + (to - from) * step_time;
        }

        engine.look_at(pos);
        engine.begin_frame(ov::Color(25, 30, 40));
        map.draw_layer(floor_layer, 0);
        map.draw_layer(blocks, 1, true);
        ov::Sprite s;
        s.pos = {pos.x, pos.y + 2};
        s.origin = {8, 15};
        s.flip_x = left;
        s.layer = 1;
        s.depth = pos.y + float(info.tile_h) / 2;  // its cell's bottom corner
        uint32_t f = uint32_t(row * 4) + (step_time < 1 ? 1 + uint32_t(step_time * 3) % 3 : 0);
        engine.draw_frame(hero, frames, f, s);

        // Hovered cell outline
        ov::Vec2 t = map.world_to_tile(engine.mouse_world());
        int hx = int(std::floor(t.x)), hy = int(std::floor(t.y));
        if (hx >= 0 && hy >= 0 && hx < int(info.width) && hy < int(info.height)) {
            ov::Vec2 c[4] = {map.tile_to_world({float(hx), float(hy)}), map.tile_to_world({hx + 1.0f, float(hy)}),
                             map.tile_to_world({hx + 1.0f, hy + 1.0f}), map.tile_to_world({float(hx), hy + 1.0f})};
            engine.set_layer(2);
            ov::Color edge = map.solid(hx, hy) ? ov::Color(255, 90, 90) : ov::Color(255, 255, 255, 180);
            for (int i = 0; i < 4; ++i) engine.draw_line(c[i], c[(i + 1) % 4], 1, edge);
        }

        engine.set_space(ov::Space::Screen);
        engine.set_layer(10);
        engine.draw_text("WASD: шаг по клеткам", {4, 2}, 10, ov::Color::White);
        engine.set_space(ov::Space::World);
        engine.set_layer(0);
        engine.end_frame();
    }
    return finish(engine, opt, "isometric");
} catch (const ov::Error &e) {
    std::fprintf(stderr, "error: %s\n", e.what());
    return 1;
}

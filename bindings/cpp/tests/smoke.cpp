// Headless smoke test of the C++ wrapper.
#include <cstdio>
#include <cstdlib>
#include <string>

#include "oversight.hpp"

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

    ov::Animation walk = ov::Animation::range(4, 4, 8.0f);
    CHECK(walk.frame_at(0.3f) == 6);

    engine.look_at({32, 32});
    CHECK(engine.poll());
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
    std::printf("smoke_cpp: ok\n");
    return 0;
}

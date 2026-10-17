// __NAME__: collect the coins. WASD, arrows or a gamepad stick to move.
// F3 shows the debug panel, Esc quits.
#include <cmath>
#include <cstdio>
#include <random>
#include <string>
#include <vector>

#include "oversight.hpp"

int main() try {
    ov::Config config;
    config.title = "__NAME__";
    config.width = 320;   // the game draws into 320x180...
    config.height = 180;
    config.scale = 4;     // ...shown 4 times bigger
    config.integer_scale = true;
    ov::Engine engine(config);

    std::mt19937 rng(1);
    auto random_spot = [&] {
        return ov::Vec2{std::uniform_real_distribution<float>(16, 304)(rng),
                        std::uniform_real_distribution<float>(24, 164)(rng)};
    };

    ov::Vec2 player{160, 90};
    const float speed = 90;
    std::vector<ov::Vec2> coins;
    for (int i = 0; i < 5; ++i) coins.push_back(random_spot());
    int score = 0;

    while (engine.poll()) {
        if (engine.key_pressed(ov::Key::Escape)) break;
        float dt = engine.dt();

        ov::Vec2 dir{engine.axis(ov::Key::A, ov::Key::D) + engine.axis(ov::Key::Left, ov::Key::Right) +
                         engine.pad_axis(0, ov::PadAxis::LeftX),
                     engine.axis(ov::Key::W, ov::Key::S) + engine.axis(ov::Key::Up, ov::Key::Down) +
                         engine.pad_axis(0, ov::PadAxis::LeftY)};
        float len = std::sqrt(dir.x * dir.x + dir.y * dir.y);
        if (len > 1) dir = dir * (1 / len);
        player = player + dir * (speed * dt);
        player.x = std::fmin(std::fmax(player.x, 4.0f), 316.0f);
        player.y = std::fmin(std::fmax(player.y, 4.0f), 176.0f);

        for (ov::Vec2 &c : coins) {
            float dx = c.x - player.x, dy = c.y - player.y;
            if (dx * dx + dy * dy < 10 * 10) {
                ++score;
                c = random_spot();
            }
        }

        engine.begin_frame(ov::Color(30, 34, 48));
        for (ov::Vec2 c : coins) engine.fill_circle(c, 4, ov::Color(255, 210, 80));
        engine.fill_rect({player.x - 4, player.y - 4, 8, 8}, ov::Color(120, 200, 255));
        engine.draw_text("Монеты: " + std::to_string(score), {4, 2}, 10, ov::Color::White);
        engine.end_frame();
    }
    return 0;
} catch (const ov::Error &e) {
    std::fprintf(stderr, "error: %s\n", e.what());
    return 1;
}

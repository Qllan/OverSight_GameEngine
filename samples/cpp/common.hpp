// Shared by the samples: command line, scripted input for headless runs.
#pragma once

#include <cstdio>
#include <cstdlib>
#include <cmath>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

#include "oversight.hpp"

#ifndef OV_SAMPLE_ASSETS
#define OV_SAMPLE_ASSETS "assets"
#endif

struct Options {
    std::string assets = OV_SAMPLE_ASSETS;
    // --headless N: no window, run N frames with scripted input and exit
    int frames = 0;
    std::string screenshot;  // --screenshot file.png: the last frame
    // --gpu / --software: what draws (default: the graphics card for a
    // window, the CPU headless)
    ov::Renderer renderer = ov::Renderer::Auto;
};

inline Options parse_options(int argc, char **argv) {
    Options o;
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        bool more = i + 1 < argc;
        if (a == "--assets" && more) o.assets = argv[++i];
        else if (a == "--headless" && more) o.frames = std::atoi(argv[++i]);
        else if (a == "--screenshot" && more) o.screenshot = argv[++i];
        else if (a == "--gpu") o.renderer = ov::Renderer::Gpu;
        else if (a == "--software") o.renderer = ov::Renderer::Software;
    }
    return o;
}

// Key presses at given frames, for headless runs (a bot instead of a player)
struct Script {
    struct Step {
        int frame;
        ov::Key key;
        bool down;
    };
    std::vector<Step> steps;

    void feed(ov::Engine &engine, int frame) const {
        for (const Step &s : steps)
            if (s.frame == frame) engine.inject_key(s.key, s.down);
    }
};

// One frame of the loop: true while the game should go on
inline bool next_frame(ov::Engine &engine, const Options &o, const Script &script, int &frame) {
    if (o.frames > 0) {
        if (frame >= o.frames) return false;
        script.feed(engine, frame);
    }
    ++frame;
    return engine.poll();
}

inline int finish(ov::Engine &engine, const Options &o, const char *name) {
    if (!o.screenshot.empty()) engine.save_screenshot(o.screenshot);
    if (o.frames > 0) std::printf("%s: %d frames ok (%s)\n", name, o.frames, engine.renderer_name().c_str());
    return 0;
}

inline ov::Config window(const char *title, const Options &o) {
    ov::Config c;
    c.title = title;
    c.width = 320;
    c.height = 180;
    c.scale = 4.0f;
    c.integer_scale = true;
    c.headless = o.frames > 0;
    c.renderer = o.renderer;
    return c;
}

inline float clampf(float v, float lo, float hi) { return v < lo ? lo : (v > hi ? hi : v); }

// Camera centred on `target` but kept inside the map
inline ov::Vec2 follow(ov::Vec2 target, float map_w, float map_h, float view_w, float view_h) {
    float x = map_w <= view_w ? map_w / 2 : clampf(target.x, view_w / 2, map_w - view_w / 2);
    float y = map_h <= view_h ? map_h / 2 : clampf(target.y, view_h / 2, map_h - view_h / 2);
    return {std::floor(x), std::floor(y)};
}

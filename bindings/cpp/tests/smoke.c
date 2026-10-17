/* Headless smoke test of the C API: draws, collides and checks pixels. */
#include <stdio.h>
#include <string.h>

#include "oversight.h"

#define CHECK(cond)                                                    \
    do {                                                               \
        if (!(cond)) {                                                 \
            fprintf(stderr, "%s:%d: failed: %s\n", __FILE__, __LINE__, #cond); \
            return 1;                                                  \
        }                                                              \
    } while (0)

int main(void) {
    OvConfig config = ov_default_config();
    config.width = 32;
    config.height = 32;
    config.headless = true;
    OvEngine *e = ov_engine_create(&config);
    CHECK(e != NULL);

    CHECK(ov_load_texture(e, "missing.png") == 0);
    CHECK(strstr(ov_last_error(), "missing.png") != NULL);

    uint8_t px[2 * 2 * 4] = {255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255};
    uint32_t tex = ov_texture_from_rgba(e, 2, 2, px);
    CHECK(tex != 0);

    /* A floor of solid tiles on row 3 (cells are 8x8, tile 4 = white) */
    uint32_t map = ov_create_tilemap(e, OV_ORTHOGONAL, 4, 4, 8, 8, 1);
    OvSpriteSheet sheet = ov_sheet_grid(2, 2, 1, 1, 0, 0);
    CHECK(sheet.count == 4);
    CHECK(ov_tilemap_add_tileset(e, map, 1, tex, sheet));
    CHECK(ov_tilemap_set_tile_flag(e, map, 4, "solid", true));
    for (int x = 0; x < 4; ++x) ov_tilemap_set_tile(e, map, 0, x, 3, 4);
    OvMoveResult r = ov_move_box(e, map, (OvRect){2, 2, 4, 4}, (OvVec2){0, 50});
    CHECK(r.contacts.ground && !r.contacts.wall_left);
    CHECK(r.rect.y + r.rect.h == 24.0f);
    OvRayHit hit;
    CHECK(ov_raycast(e, map, (OvVec2){4, 0}, (OvVec2){4, 40}, &hit));
    CHECK(hit.cell_y == 3 && hit.normal.y == -1.0f);

    CHECK(ov_poll(e));
    ov_begin_frame(e, OV_BLACK);
    ov_set_space(e, OV_SPACE_SCREEN);
    OvSprite s = ov_default_sprite();
    s.scale = (OvVec2){8, 8};
    s.flip_x = true;
    ov_draw_sprite(e, tex, &s);
    ov_end_frame(e);
    OvColor green = ov_pixel(e, 2, 2);
    CHECK(green.r == 0 && green.g == 255 && green.b == 0); /* flipped: green top-left */
    OvColor red = ov_pixel(e, 12, 2);
    CHECK(red.r == 255 && red.g == 0);

    /* Particles: a burst of 5 dots with no rate, gone after their life */
    CHECK(sizeof(OvEmitter) == 104); /* layout matches the engine */
    OvEmitter cfg = ov_default_emitter();
    cfg.rate = 0;
    cfg.life_min = cfg.life_max = 0.5f;
    uint32_t sparks = ov_create_emitter(e, &cfg, (OvVec2){8, 8});
    ov_emitter_burst(e, sparks, 5);
    CHECK(ov_emitter_count(e, sparks) == 5);
    ov_update_emitters(e, 0.6f);
    CHECK(ov_emitter_count(e, sparks) == 0);
    ov_free_emitter(e, sparks);

    uint32_t frames[] = {7, 8, 9};
    CHECK(ov_anim_frame_at(frames, 3, 10.0f, OV_ANIM_ONCE, 5.0f) == 9);

    ov_inject_key(e, OV_KEY_D, true);
    ov_poll(e);
    CHECK(ov_key_pressed(e, OV_KEY_D));
    CHECK(ov_axis(e, OV_KEY_A, OV_KEY_D) == 1.0f);

    ov_engine_destroy(e);
    printf("smoke_c: ok\n");
    return 0;
}

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
    /* Headless frames last 1/60 s: one logic tick each */
    CHECK(ov_ticks(e) == 1 && ov_tick_count(e) == 1);
    ov_random_seed(e, 1);
    CHECK(ov_random_u32(e) == 0x910A2DECu); /* the same on every machine */
    int32_t die = ov_random_int(e, 1, 6);
    CHECK(die >= 1 && die <= 6);
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

    /* Entities: a component is a plain struct registered by name */
    typedef struct { float x, y; } Pos;
    uint32_t pos = ov_component(e, "Pos", sizeof(Pos), 4);
    uint32_t tag = ov_component(e, "Tag", 0, 1);
    CHECK(pos != 0 && tag != 0 && ov_component(e, "Pos", sizeof(Pos), 4) == pos);
    CHECK(ov_component(e, "Pos", 4, 4) == 0); /* another size: refused */
    OvEntity ents[3];
    for (int i = 0; i < 3; ++i) {
        ents[i] = ov_spawn(e);
        Pos p = {(float)i, 0};
        CHECK(ov_set(e, ents[i], pos, &p));
    }
    CHECK(ov_set(e, ents[1], tag, NULL));
    ((Pos *)ov_get(e, ents[2], pos))->y = 5;
    CHECK(((Pos *)ov_get(e, ents[2], pos))->y == 5);
    uint32_t both[] = {pos, tag};
    OvEntity found[4];
    CHECK(ov_query(e, both, 2, found, 4) == 1 && found[0] == ents[1]);
    CHECK(ov_despawn(e, ents[1]) && !ov_alive(e, ents[1]) && ov_get(e, ents[1], pos) == NULL);
    CHECK(ov_query(e, &pos, 1, NULL, 0) == 2 && ov_entity_count(e) == 2);

    /* Snapshot, change the world, restore: back as it was */
    size_t size = ov_snapshot(e, NULL, 0);
    uint8_t snap[1024];
    CHECK(size <= sizeof snap && ov_snapshot(e, snap, sizeof snap) == size);
    uint64_t hash = ov_state_hash(e);
    ov_despawn(e, ents[0]);
    ov_random_u32(e);
    CHECK(ov_state_hash(e) != hash);
    CHECK(ov_restore(e, snap, size) && ov_state_hash(e) == hash && ov_alive(e, ents[0]));
    CHECK(!ov_restore(e, snap, 3) && ov_alive(e, ents[0]));

    /* Built-in components: fixed ids, moved by the engine */
    CHECK(ov_component(e, "Velocity", sizeof(OvVelocity), _Alignof(OvVelocity)) == OV_VELOCITY);
    CHECK(sizeof(OvSpriteRenderer) == 40 && sizeof(OvBody) == 24);
    OvEntity mover = ov_spawn(e);
    CHECK(ov_set(e, mover, OV_TRANSFORM, NULL)); /* defaults: scale 1 */
    OvVelocity vel = {{4, 0}, 1};
    ov_set(e, mover, OV_VELOCITY, &vel);
    ov_update_motion(e, 0.5f);
    OvTransform *tr = (OvTransform *)ov_get(e, mover, OV_TRANSFORM);
    CHECK(tr->pos.x == 2 && tr->rotation == 0.5f && tr->scale.x == 1);

    /* Colliders */
    CHECK(sizeof(OvCollider) == 40);
    OvVec2 bent[] = {{0, 0}, {4, 0}, {2, 1}, {4, 4}};
    CHECK(ov_create_polygon(e, bent, 4) == 0); /* concave */

    /* Scenes: switched in the middle of the fade */
    CHECK(ov_scene_go(e, 5, OV_TRANSITION_FADE, 0.1f, OV_RGBA(0, 0, 0, 255)));
    int entered = 0;
    for (int i = 0; i < 10; i++) {
        ov_poll(e);
        if (ov_scene_entered(e) == 5) entered = 1;
    }
    CHECK(entered && ov_scene(e) == 5 && !ov_scene_transitioning(e));

    /* Actions */
    uint32_t fire = ov_action(e, "fire");
    OvBinding left_click = {OV_BIND_MOUSE, OV_MOUSE_LEFT, -1, 0};
    CHECK(ov_bind(e, fire, left_click) && ov_binding_count(e, fire) == 1);
    ov_inject_mouse_button(e, OV_MOUSE_LEFT, true);
    ov_poll(e);
    CHECK(ov_action_pressed(e, fire) && ov_action_value(e, fire) == 1.0f);
    char text[64];
    CHECK(ov_bindings_save(e, text, sizeof text) == strlen("fire = mouse:Left\n"));

    ov_engine_destroy(e);
    printf("smoke_c: ok\n");
    return 0;
}

/* Headless smoke test of the C API: draws, collides and checks pixels. */
#include <stddef.h>
#include <stdio.h>
#include <string.h>

#include "orst.h"

#define CHECK(cond)                                                    \
    do {                                                               \
        if (!(cond)) {                                                 \
            fprintf(stderr, "%s:%d: failed: %s\n", __FILE__, __LINE__, #cond); \
            return 1;                                                  \
        }                                                              \
    } while (0)

int main(void) {
    OrstConfig config = orst_default_config();
    config.width = 32;
    config.height = 32;
    config.headless = true;
    OrstEngine *e = orst_engine_create(&config);
    CHECK(e != NULL);

    CHECK(orst_load_texture(e, "missing.png") == 0);
    CHECK(strstr(orst_last_error(), "missing.png") != NULL);

    uint8_t px[2 * 2 * 4] = {255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255};
    uint32_t tex = orst_texture_from_rgba(e, 2, 2, px);
    CHECK(tex != 0);

    /* A floor of solid tiles on row 3 (cells are 8x8, tile 4 = white) */
    uint32_t map = orst_create_tilemap(e, ORST_ORTHOGONAL, 4, 4, 8, 8, 1);
    OrstSpriteSheet sheet = orst_sheet_grid(2, 2, 1, 1, 0, 0);
    CHECK(sheet.count == 4);
    CHECK(orst_tilemap_add_tileset(e, map, 1, tex, sheet));
    CHECK(orst_tilemap_set_tile_flag(e, map, 4, "solid", true));
    for (int x = 0; x < 4; ++x) orst_tilemap_set_tile(e, map, 0, x, 3, 4);
    OrstMoveResult r = orst_move_box(e, map, (OrstRect){2, 2, 4, 4}, (OrstVec2){0, 50});
    CHECK(r.contacts.ground && !r.contacts.wall_left);
    CHECK(r.rect.y + r.rect.h == 24.0f);
    OrstRayHit hit;
    CHECK(orst_raycast(e, map, (OrstVec2){4, 0}, (OrstVec2){4, 40}, &hit));
    CHECK(hit.cell_y == 3 && hit.normal.y == -1.0f);

    CHECK(orst_poll(e));
    /* Headless frames last 1/60 s: one logic tick each */
    CHECK(orst_ticks(e) == 1 && orst_tick_count(e) == 1);
    orst_random_seed(e, 1);
    CHECK(orst_random_u32(e) == 0x910A2DECu); /* the same on every machine */
    int32_t die = orst_random_int(e, 1, 6);
    CHECK(die >= 1 && die <= 6);
    orst_begin_frame(e, ORST_BLACK);
    orst_set_space(e, ORST_SPACE_SCREEN);
    OrstSprite s = orst_default_sprite();
    s.scale = (OrstVec2){8, 8};
    s.flip_x = true;
    orst_draw_sprite(e, tex, &s);
    orst_end_frame(e);
    OrstColor green = orst_pixel(e, 2, 2);
    CHECK(green.r == 0 && green.g == 255 && green.b == 0); /* flipped: green top-left */
    OrstColor red = orst_pixel(e, 12, 2);
    CHECK(red.r == 255 && red.g == 0);

    /* Particles: a burst of 5 dots with no rate, gone after their life */
    CHECK(sizeof(OrstEmitter) == 104); /* layout matches the engine */
    OrstEmitter cfg = orst_default_emitter();
    cfg.rate = 0;
    cfg.life_min = cfg.life_max = 0.5f;
    uint32_t sparks = orst_create_emitter(e, &cfg, (OrstVec2){8, 8});
    orst_emitter_burst(e, sparks, 5);
    CHECK(orst_emitter_count(e, sparks) == 5);
    orst_update_emitters(e, 0.6f);
    CHECK(orst_emitter_count(e, sparks) == 0);
    orst_free_emitter(e, sparks);
    uint32_t dust = orst_create_gpu_emitter(e, &cfg, (OrstVec2){8, 8});
    orst_gpu_emitter_burst(e, dust, 5);
    CHECK(orst_gpu_emitter_count(e, dust) == 5 && orst_gpu_emitter_config(e, dust).rate == 0);
    orst_draw_gpu_emitter(e, dust);
    orst_update_gpu_emitters(e, 0.6f);
    CHECK(orst_gpu_emitter_count(e, dust) == 0);
    orst_free_gpu_emitter(e, dust);

    uint32_t frames[] = {7, 8, 9};
    CHECK(orst_anim_frame_at(frames, 3, 10.0f, ORST_ANIM_ONCE, 5.0f) == 9);

    orst_inject_key(e, ORST_KEY_D, true);
    orst_poll(e);
    CHECK(orst_key_pressed(e, ORST_KEY_D));
    CHECK(orst_axis(e, ORST_KEY_A, ORST_KEY_D) == 1.0f);

    /* Entities: a component is a plain struct registered by name */
    typedef struct { float x, y; } Pos;
    uint32_t pos = orst_component(e, "Pos", sizeof(Pos), 4);
    uint32_t tag = orst_component(e, "Tag", 0, 1);
    CHECK(pos != 0 && tag != 0 && orst_component(e, "Pos", sizeof(Pos), 4) == pos);
    CHECK(orst_component(e, "Pos", 4, 4) == 0); /* another size: refused */
    OrstEntity ents[3];
    for (int i = 0; i < 3; ++i) {
        ents[i] = orst_spawn(e);
        Pos p = {(float)i, 0};
        CHECK(orst_set(e, ents[i], pos, &p));
    }
    CHECK(orst_set(e, ents[1], tag, NULL));
    ((Pos *)orst_get(e, ents[2], pos))->y = 5;
    CHECK(((Pos *)orst_get(e, ents[2], pos))->y == 5);
    uint32_t both[] = {pos, tag};
    OrstEntity found[4];
    CHECK(orst_query(e, both, 2, found, 4) == 1 && found[0] == ents[1]);
    CHECK(orst_despawn(e, ents[1]) && !orst_alive(e, ents[1]) && orst_get(e, ents[1], pos) == NULL);
    CHECK(orst_query(e, &pos, 1, NULL, 0) == 2 && orst_entity_count(e) == 2);

    /* Fields: what is inside a component, for tools */
    OrstField pos_fields[] = {{"x", ORST_FIELD_F32, offsetof(Pos, x), 0, 0, 100, NULL},
                              {"y", ORST_FIELD_U32, offsetof(Pos, y), 0, 0, 0, "Up|Down"}};
    CHECK(orst_set_fields(e, pos, pos_fields, 2) && orst_field_count(e, pos) == 2);
    OrstField got;
    CHECK(orst_get_field(e, pos, 1, &got) && strcmp(got.name, "y") == 0 && strcmp(got.options, "Up|Down") == 0);
    CHECK(orst_get_field(e, ORST_TRANSFORM, 0, &got) && strcmp(got.name, "pos") == 0 && got.type == ORST_FIELD_VEC2);
    CHECK(!orst_get_field(e, pos, 2, &got) && orst_field_count(e, tag) == 0);
    OrstField past = {"z", ORST_FIELD_F32, sizeof(Pos), 0, 0, 0, NULL};
    CHECK(!orst_set_fields(e, pos, &past, 1) && orst_field_count(e, pos) == 2); /* past the end: refused */
    OrstName name = {"Hero"};
    CHECK(sizeof(OrstName) == 64 && orst_set(e, ents[0], ORST_NAME, &name) && orst_find_named(e, "Hero") == ents[0]);
    CHECK(orst_find_named(e, "Nobody") == 0);
    {
        /* A step moves, animates and points the camera */
        OrstEntity cam = orst_spawn(e);
        OrstTransform t = {{0, 0}, 0, {1, 1}};
        OrstVelocity v = {{60, 0}, 0};
        OrstCamera2D c = {2.0f, 1, {0, 0, 0}};
        CHECK(sizeof(OrstSpriteAnimator) == 36 && sizeof(OrstCamera2D) == 8);
        orst_set(e, cam, ORST_TRANSFORM, &t);
        orst_set(e, cam, ORST_VELOCITY, &v);
        orst_set(e, cam, ORST_CAMERA_2D, &c);
        orst_step(e, 0.5f);
        OrstCamera view = orst_get_camera(e);
        CHECK(view.center.x == 30.0f && view.zoom == 2.0f);
        orst_despawn(e, cam);
    }

    /* Snapshot, change the world, restore: back as it was */
    size_t size = orst_snapshot(e, NULL, 0);
    uint8_t snap[1024];
    CHECK(size <= sizeof snap && orst_snapshot(e, snap, sizeof snap) == size);
    uint64_t hash = orst_state_hash(e);
    orst_despawn(e, ents[0]);
    orst_random_u32(e);
    CHECK(orst_state_hash(e) != hash);
    CHECK(orst_restore(e, snap, size) && orst_state_hash(e) == hash && orst_alive(e, ents[0]));
    CHECK(!orst_restore(e, snap, 3) && orst_alive(e, ents[0]));

    /* Built-in components: fixed ids, moved by the engine */
    CHECK(orst_component(e, "Velocity", sizeof(OrstVelocity), _Alignof(OrstVelocity)) == ORST_VELOCITY);
    CHECK(sizeof(OrstSpriteRenderer) == 40 && sizeof(OrstBody) == 24);
    OrstEntity mover = orst_spawn(e);
    CHECK(orst_set(e, mover, ORST_TRANSFORM, NULL)); /* defaults: scale 1 */
    OrstVelocity vel = {{4, 0}, 1};
    orst_set(e, mover, ORST_VELOCITY, &vel);
    orst_update_motion(e, 0.5f);
    OrstTransform *tr = (OrstTransform *)orst_get(e, mover, ORST_TRANSFORM);
    CHECK(tr->pos.x == 2 && tr->rotation == 0.5f && tr->scale.x == 1);

    /* Colliders */
    CHECK(sizeof(OrstCollider) == 40);
    OrstVec2 bent[] = {{0, 0}, {4, 0}, {2, 1}, {4, 4}};
    CHECK(orst_create_polygon(e, bent, 4) == 0); /* concave */
    CHECK(sizeof(OrstContact) == 40 && sizeof(OrstEntityHit) == 32);
    orst_update_collisions(e);
    CHECK(orst_contact_count(e) == 0);

    /* Physics */
    CHECK(sizeof(OrstRigidBody) == 32);
    CHECK(orst_joint_weld(e, 0, 0) == 0);

    /* Scenes: switched in the middle of the fade */
    CHECK(orst_scene_go(e, 5, ORST_TRANSITION_FADE, 0.1f, ORST_RGBA(0, 0, 0, 255)));
    int entered = 0;
    for (int i = 0; i < 10; i++) {
        orst_poll(e);
        if (orst_scene_entered(e) == 5) entered = 1;
    }
    CHECK(entered && orst_scene(e) == 5 && !orst_scene_transitioning(e));

    /* Actions */
    uint32_t fire = orst_action(e, "fire");
    OrstBinding left_click = {ORST_BIND_MOUSE, ORST_MOUSE_LEFT, -1, 0};
    CHECK(orst_bind(e, fire, left_click) && orst_binding_count(e, fire) == 1);
    orst_inject_mouse_button(e, ORST_MOUSE_LEFT, true);
    orst_poll(e);
    CHECK(orst_action_pressed(e, fire) && orst_action_value(e, fire) == 1.0f);
    char text[64];
    CHECK(orst_bindings_save(e, text, sizeof text) == strlen("fire = mouse:Left\n"));

    /* Render targets and post effects */
    CHECK(sizeof(OrstEffect) == 40 && sizeof(OrstLight) == 32 && ORST_LIGHT == 7);
    uint32_t target = orst_create_target(e, 4, 4);
    CHECK(target != 0 && !orst_set_target(e, 12345));
    orst_begin_frame(e, ORST_RGBA(0, 0, 0, 255));
    CHECK(orst_set_target(e, target) && orst_get_target(e) == target);
    orst_clear_target(e, target, ORST_RGBA(0, 0, 255, 255));
    orst_set_target(e, 0);
    orst_set_space(e, ORST_SPACE_SCREEN);
    orst_draw_texture(e, target, 0, 0);
    orst_end_frame(e);
    CHECK(orst_pixel(e, 1, 1).b == 255);
    OrstEffect blur = {ORST_EFFECT_BLUR, 0, {2}};
    CHECK(orst_set_effects(e, target, &blur, 1) && orst_set_effects(e, target, NULL, 0));
    OrstEffect bad = {ORST_EFFECT_SHADER, 777, {0}};
    CHECK(!orst_set_effects(e, 0, &bad, 1));
    CHECK(orst_create_shader(e, "not wgsl") == 0);
    orst_shake(e, 2, 0.1f);
    orst_free_texture(e, target);

    /* Skeletons */
    const char *rig = "{\"bones\": [{\"name\": \"root\"}, {\"name\": \"arm\", \"parent\": \"root\", \"x\": 10}],"
                      " \"slots\": [{\"name\": \"arm\", \"bone\": \"arm\", \"attachment\": \"a\","
                      " \"attachments\": {\"a\": {\"region\": [0, 0, 2, 2]}}}],"
                      " \"animations\": {\"wave\": {\"bones\": {\"arm\": {\"rotate\": [{\"time\": 0, \"value\": 0},"
                      " {\"time\": 1, \"value\": 90}]}}, \"events\": [{\"time\": 0.5, \"name\": \"half\"}]}}}";
    CHECK(orst_skeleton_from_json(e, "{\"bones\": []}") == 0 && orst_skeleton_from_json(e, "nope") == 0);
    uint32_t rig_data = orst_skeleton_from_json(e, rig);
    uint32_t skel = orst_create_skeleton(e, rig_data);
    CHECK(rig_data != 0 && skel != 0 && orst_create_skeleton(e, 999) == 0);
    int32_t arm = orst_skeleton_bone(e, skel, "arm");
    CHECK(arm == 1 && orst_skeleton_bone(e, skel, "leg") == -1);
    CHECK(orst_skeleton_play(e, skel, "wave", false, 0) && !orst_skeleton_play(e, skel, "dance", false, 0));
    orst_update_skeletons(e, 0.6f);
    CHECK(orst_skeleton_fired(e, skel, "half") && !orst_skeleton_done(e, skel));
    char anim[16];
    CHECK(orst_skeleton_animation(e, skel, anim, sizeof anim) == 4 && strcmp(anim, "wave") == 0);
    orst_update_skeletons(e, 1);
    CHECK(orst_skeleton_done(e, skel));
    OrstTransform at = {{100, 0}, 0, {1, 1}};
    orst_set_skeleton_root(e, skel, at);
    OrstVec2 tip = orst_skeleton_bone_point(e, skel, arm, (OrstVec2){5, 0});
    CHECK(orst_skeleton_bone_world(e, skel, arm).pos.x == 110 && tip.x > 109.9f && tip.y > 4.9f);
    orst_set_bone_override(e, skel, arm, 0, 1);
    CHECK(orst_skeleton_bone_point(e, skel, arm, (OrstVec2){5, 0}).x > 114.9f);
    CHECK(orst_skeleton_set_attachment(e, skel, "arm", NULL) && !orst_skeleton_set_attachment(e, skel, "arm", "b"));
    orst_draw_skeleton(e, skel, 0);
    orst_free_skeleton_data(e, rig_data);
    orst_free_skeleton(e, skel);

    orst_engine_destroy(e);
    printf("smoke_c: ok\n");
    return 0;
}

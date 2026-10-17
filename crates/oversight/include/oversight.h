/* OverSight 2D: C API.
 *
 * The game owns its loop:
 *
 *     OvConfig config = ov_default_config();
 *     config.title = "My game";
 *     OvEngine *e = ov_engine_create(&config);
 *     while (ov_poll(e)) {
 *         float dt = ov_dt(e);
 *         ov_begin_frame(e, OV_BLACK);
 *         // update with dt, then draw
 *         ov_end_frame(e);
 *     }
 *     ov_engine_destroy(e);
 *
 * Resources (textures, fonts, maps, sounds) are ids; 0 means "none" or
 * "failed", and ov_last_error() says why. Strings are UTF-8. Every
 * function accepts a NULL engine and then does nothing.
 */
#ifndef OVERSIGHT_H
#define OVERSIGHT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct OvEngine OvEngine;

typedef struct OvColor { uint8_t r, g, b, a; } OvColor;
typedef struct OvVec2 { float x, y; } OvVec2;
/* Top-left corner and size */
typedef struct OvRect { float x, y, w, h; } OvRect;

#define OV_RGBA(r, g, b, a) ((OvColor){(r), (g), (b), (a)})
#define OV_BLACK OV_RGBA(0, 0, 0, 255)
#define OV_WHITE OV_RGBA(255, 255, 255, 255)

/* How a sprite is drawn. src is the texture region (w or h <= 0: the
 * whole texture); origin (in source pixels) lands on pos and is the
 * pivot for rotation and scale. Start from ov_default_sprite(). */
typedef struct OvSprite {
    OvRect src;
    OvVec2 pos;
    OvVec2 origin;
    OvVec2 scale;
    float rotation; /* radians, clockwise */
    bool flip_x;
    bool flip_y;
    OvColor tint;
    int32_t layer;  /* lower layers are drawn first */
    float depth;    /* order within a layer: pass y for y-sorting */
} OvSprite;

typedef struct OvCamera {
    OvVec2 center; /* world point at the centre of the screen */
    float zoom;    /* screen pixels per world unit */
    float rotation;
} OvCamera;

typedef struct OvConfig {
    const char *title;
    uint32_t width, height; /* canvas: the logical resolution */
    float scale;            /* initial window size = canvas * scale */
    bool resizable;
    bool fullscreen;
    bool integer_scale;     /* scale by whole numbers only (pixel art) */
    uint32_t target_fps;    /* 0 = unlimited */
    bool headless;          /* no window: render offscreen */
    int32_t renderer;       /* OvRenderer */
} OvConfig;

typedef enum OvRenderer {
    OV_RENDERER_AUTO = 0,     /* the graphics card for a window (the CPU if it fails), the CPU headless */
    OV_RENDERER_SOFTWARE = 1, /* always the CPU */
    OV_RENDERER_GPU = 2       /* always the graphics card, also headless; creating fails without one */
} OvRenderer;

/* A texture cut into a grid of equal frames, numbered row by row */
typedef struct OvSpriteSheet {
    uint32_t frame_w, frame_h, columns, count, margin, spacing;
} OvSpriteSheet;

/* How an emitter's particles look and move. Get the defaults from
   ov_default_emitter() and change what you need. A min/max pair picks a
   random value between them for every particle. */
typedef struct OvEmitter {
    uint32_t texture;           /* 0: round dots */
    OvRect src;                 /* region of the texture; w or h <= 0: whole texture */
    float rate;                 /* particles per second while emitting */
    uint32_t max;               /* most particles alive at once */
    float life_min, life_max;   /* seconds */
    float speed_min, speed_max; /* pixels per second */
    float angle;                /* direction, radians: 0 right, PI/2 down */
    float spread;               /* width of the cone of directions (2*PI: every way) */
    OvVec2 gravity;             /* pixels per second squared */
    float drag;                 /* share of speed lost per second */
    float size_start, size_end; /* pixels: dot diameter or texture width */
    float spin_min, spin_max;   /* radians per second (textures) */
    OvColor color_start, color_end;
    OvVec2 area;                /* spawn box half-size around the emitter */
    int32_t layer;
    uint32_t seed;              /* same seed and calls: same particles */
} OvEmitter;

typedef enum OvAnimMode { OV_ANIM_LOOP = 0, OV_ANIM_ONCE = 1, OV_ANIM_PING_PONG = 2 } OvAnimMode;
/* Easing curves, as on easings.net */
typedef enum OvEase {
    OV_EASE_LINEAR = 0,
    OV_EASE_QUAD_IN, OV_EASE_QUAD_OUT, OV_EASE_QUAD_IN_OUT,
    OV_EASE_CUBIC_IN, OV_EASE_CUBIC_OUT, OV_EASE_CUBIC_IN_OUT,
    OV_EASE_SINE_IN, OV_EASE_SINE_OUT, OV_EASE_SINE_IN_OUT,
    OV_EASE_EXPO_IN, OV_EASE_EXPO_OUT, OV_EASE_EXPO_IN_OUT,
    OV_EASE_BACK_IN, OV_EASE_BACK_OUT, OV_EASE_BACK_IN_OUT,
    OV_EASE_ELASTIC_IN, OV_EASE_ELASTIC_OUT, OV_EASE_ELASTIC_IN_OUT,
    OV_EASE_BOUNCE_IN, OV_EASE_BOUNCE_OUT, OV_EASE_BOUNCE_IN_OUT
} OvEase;
typedef enum OvRepeat { OV_REPEAT_ONCE = 0, OV_REPEAT_LOOP = 1, OV_REPEAT_PING_PONG = 2 } OvRepeat;
typedef enum OvSpace { OV_SPACE_WORLD = 0, OV_SPACE_SCREEN = 1 } OvSpace;
typedef enum OvOrientation { OV_ORTHOGONAL = 0, OV_ISOMETRIC = 1 } OvOrientation;
typedef enum OvMouseButton { OV_MOUSE_LEFT = 0, OV_MOUSE_RIGHT = 1, OV_MOUSE_MIDDLE = 2 } OvMouseButton;

/* Gamepad buttons by position: SOUTH is A on Xbox, Cross on PlayStation */
typedef enum OvGamepadButton {
    OV_PAD_SOUTH = 0, OV_PAD_EAST, OV_PAD_WEST, OV_PAD_NORTH,
    OV_PAD_LEFT_BUMPER, OV_PAD_RIGHT_BUMPER,
    OV_PAD_BACK, OV_PAD_START, OV_PAD_GUIDE,
    OV_PAD_LEFT_STICK, OV_PAD_RIGHT_STICK,
    OV_PAD_DPAD_UP, OV_PAD_DPAD_DOWN, OV_PAD_DPAD_LEFT, OV_PAD_DPAD_RIGHT
} OvGamepadButton;
/* Sticks -1..1 with y pointing down (up is -1); triggers 0..1 */
typedef enum OvGamepadAxis {
    OV_PAD_LEFT_X = 0, OV_PAD_LEFT_Y, OV_PAD_RIGHT_X, OV_PAD_RIGHT_Y,
    OV_PAD_LEFT_TRIGGER, OV_PAD_RIGHT_TRIGGER
} OvGamepadAxis;

/* Physical keys (WASD works on any layout) */
typedef enum OvKey {
    OV_KEY_UNKNOWN = 0,
    OV_KEY_A = 1,
    OV_KEY_B = 2,
    OV_KEY_C = 3,
    OV_KEY_D = 4,
    OV_KEY_E = 5,
    OV_KEY_F = 6,
    OV_KEY_G = 7,
    OV_KEY_H = 8,
    OV_KEY_I = 9,
    OV_KEY_J = 10,
    OV_KEY_K = 11,
    OV_KEY_L = 12,
    OV_KEY_M = 13,
    OV_KEY_N = 14,
    OV_KEY_O = 15,
    OV_KEY_P = 16,
    OV_KEY_Q = 17,
    OV_KEY_R = 18,
    OV_KEY_S = 19,
    OV_KEY_T = 20,
    OV_KEY_U = 21,
    OV_KEY_V = 22,
    OV_KEY_W = 23,
    OV_KEY_X = 24,
    OV_KEY_Y = 25,
    OV_KEY_Z = 26,
    OV_KEY_NUM0 = 30,
    OV_KEY_NUM1 = 31,
    OV_KEY_NUM2 = 32,
    OV_KEY_NUM3 = 33,
    OV_KEY_NUM4 = 34,
    OV_KEY_NUM5 = 35,
    OV_KEY_NUM6 = 36,
    OV_KEY_NUM7 = 37,
    OV_KEY_NUM8 = 38,
    OV_KEY_NUM9 = 39,
    OV_KEY_SPACE = 40,
    OV_KEY_ENTER = 41,
    OV_KEY_ESCAPE = 42,
    OV_KEY_TAB = 43,
    OV_KEY_BACKSPACE = 44,
    OV_KEY_DELETE = 45,
    OV_KEY_INSERT = 46,
    OV_KEY_HOME = 47,
    OV_KEY_END = 48,
    OV_KEY_PAGE_UP = 49,
    OV_KEY_PAGE_DOWN = 50,
    OV_KEY_LEFT = 60,
    OV_KEY_RIGHT = 61,
    OV_KEY_UP = 62,
    OV_KEY_DOWN = 63,
    OV_KEY_LEFT_SHIFT = 70,
    OV_KEY_RIGHT_SHIFT = 71,
    OV_KEY_LEFT_CTRL = 72,
    OV_KEY_RIGHT_CTRL = 73,
    OV_KEY_LEFT_ALT = 74,
    OV_KEY_RIGHT_ALT = 75,
    OV_KEY_F1 = 80,
    OV_KEY_F2 = 81,
    OV_KEY_F3 = 82,
    OV_KEY_F4 = 83,
    OV_KEY_F5 = 84,
    OV_KEY_F6 = 85,
    OV_KEY_F7 = 86,
    OV_KEY_F8 = 87,
    OV_KEY_F9 = 88,
    OV_KEY_F10 = 89,
    OV_KEY_F11 = 90,
    OV_KEY_F12 = 91,
    OV_KEY_MINUS = 100,
    OV_KEY_EQUALS = 101,
    OV_KEY_LEFT_BRACKET = 102,
    OV_KEY_RIGHT_BRACKET = 103,
    OV_KEY_SEMICOLON = 104,
    OV_KEY_QUOTE = 105,
    OV_KEY_COMMA = 106,
    OV_KEY_PERIOD = 107,
    OV_KEY_SLASH = 108,
    OV_KEY_BACKSLASH = 109,
    OV_KEY_GRAVE = 110,
} OvKey;

/* Tile id flags, as in Tiled */
#define OV_FLIP_H 0x80000000u
#define OV_FLIP_V 0x40000000u
#define OV_FLIP_D 0x20000000u

typedef struct OvTilemapInfo {
    OvOrientation orientation;
    uint32_t width, height;   /* cells */
    uint32_t tile_w, tile_h;  /* cell size in world units */
    uint32_t layer_count;
    uint32_t object_count;
} OvTilemapInfo;

typedef struct OvMapObject {
    uint32_t id;
    OvRect rect; /* world units */
    bool point;
    bool visible;
    float rotation;
    uint32_t gid;
    uint32_t layer; /* index of its object layer */
} OvMapObject;

typedef struct OvContacts {
    bool ground;  /* stopped while moving down */
    bool ceiling; /* stopped while moving up */
    bool wall_left;
    bool wall_right;
} OvContacts;

typedef struct OvMoveResult {
    OvRect rect;
    OvContacts contacts;
} OvMoveResult;

typedef struct OvRayHit {
    OvVec2 point;
    int32_t cell_x, cell_y;
    OvVec2 normal; /* side hit, in cell axes; (0, 0) when starting inside */
    float distance;
} OvRayHit;

#define OV_DEFAULT_FONT 1u

static inline OvConfig ov_default_config(void) {
    OvConfig c = {"OverSight", 960, 540, 1.0f, true, false, false, 60, false, OV_RENDERER_AUTO};
    return c;
}

static inline OvSprite ov_default_sprite(void) {
    OvSprite s = {{0, 0, 0, 0}, {0, 0}, {0, 0}, {1, 1}, 0, false, false, {255, 255, 255, 255}, 0, 0};
    return s;
}

/* --- engine and frame --- */
const char *ov_last_error(void);
OvEngine *ov_engine_create(const OvConfig *config); /* NULL on failure */
void ov_engine_destroy(OvEngine *e);
bool ov_poll(OvEngine *e); /* false when the game should end */
void ov_quit(OvEngine *e);
float ov_dt(OvEngine *e);
double ov_time(OvEngine *e);
float ov_fps(OvEngine *e);
uint64_t ov_frame(OvEngine *e);

/* Fixed logic ticks: run the game logic ov_ticks(e) times per frame with
 * ov_tick_dt(e), so it plays the same at any frame rate (replays, network).
 *     for (uint32_t i = 0; i < ov_ticks(e); ++i) update(ov_tick_dt(e));
 * ov_tick_alpha (0..1) is how far the time is toward the next tick: draw
 * moving things that far between their last two positions. */
void ov_set_tick_rate(OvEngine *e, uint32_t ticks_per_second); /* 60 by default */
uint32_t ov_ticks(OvEngine *e);
float ov_tick_dt(OvEngine *e);
float ov_tick_alpha(OvEngine *e);
uint64_t ov_tick_count(OvEngine *e);

/* Seeded random numbers: the same seed gives the same game on any machine.
 * The seed is 1 until set. */
void ov_random_seed(OvEngine *e, uint64_t seed);
uint32_t ov_random_u32(OvEngine *e);
float ov_random_float(OvEngine *e);                       /* 0 <= x < 1 */
float ov_random_range(OvEngine *e, float min, float max); /* min <= x < max */
int32_t ov_random_int(OvEngine *e, int32_t min, int32_t max); /* min <= x <= max */
void ov_begin_frame(OvEngine *e, OvColor clear);
void ov_end_frame(OvEngine *e);
void ov_set_title(OvEngine *e, const char *title);
void ov_set_fullscreen(OvEngine *e, bool on);
bool ov_is_fullscreen(OvEngine *e);
void ov_set_target_fps(OvEngine *e, uint32_t fps);
bool ov_has_window(OvEngine *e);
void ov_canvas_size(OvEngine *e, uint32_t *w, uint32_t *h);
bool ov_save_screenshot(OvEngine *e, const char *path);
OvColor ov_pixel(OvEngine *e, uint32_t x, uint32_t y);

/* --- entities and components ---
 * An entity is an id (0 = none); a component is a plain struct with no
 * pointers, registered by name, size and alignment:
 *     typedef struct { float x, y; } Pos;
 *     uint32_t pos = ov_component(e, "Pos", sizeof(Pos), _Alignof(Pos));
 *     OvEntity hero = ov_spawn(e);
 *     Pos p = {10, 20};
 *     ov_set(e, hero, pos, &p);
 *     ((Pos *)ov_get(e, hero, pos))->x += 1;
 * Pointers from ov_get stay valid until the next spawn, despawn, set or
 * remove. The same calls in the same order give the same ids and the same
 * query order on every machine. */
typedef uint64_t OvEntity;
uint32_t ov_component(OvEngine *e, const char *name, uint32_t size, uint32_t align); /* 0 on error */
OvEntity ov_spawn(OvEngine *e);
bool ov_despawn(OvEngine *e, OvEntity entity); /* removes its components too */
bool ov_alive(OvEngine *e, OvEntity entity);
uint32_t ov_entity_count(OvEngine *e);
bool ov_set(OvEngine *e, OvEntity entity, uint32_t component, const void *data); /* NULL data: zeroed (built-ins: defaults) */
void *ov_get(OvEngine *e, OvEntity entity, uint32_t component); /* NULL when it has none */
bool ov_has(OvEngine *e, OvEntity entity, uint32_t component);
bool ov_remove(OvEngine *e, OvEntity entity, uint32_t component);
/* Entities that have all `count` components (none: every entity). Fills up
 * to `capacity` of them into `out` and returns how many there are. */
uint32_t ov_query(OvEngine *e, const uint32_t *components, uint32_t count, OvEntity *out, uint32_t capacity);
void ov_world_clear(OvEngine *e); /* despawn everything; component types stay */

/* Built-in components: every engine registers them first, so these ids
 * are fixed. ov_update_motion moves entities with a Transform and a
 * Velocity; with a Body on a tile map they slide along walls instead.
 * ov_draw_entities draws every Transform + SpriteRenderer. ov_set with
 * NULL data gives them their defaults (scale 1, white tint). */
#define OV_TRANSFORM 1
#define OV_VELOCITY 2
#define OV_SPRITE_RENDERER 3
#define OV_BODY 4
#define OV_COLLIDER 5
#define OV_RIGIDBODY 6

typedef struct OvTransform {
    OvVec2 pos;
    float rotation; /* radians, clockwise */
    OvVec2 scale;   /* default {1, 1} */
} OvTransform;

typedef struct OvVelocity {
    OvVec2 linear; /* units per second */
    float angular; /* radians per second */
} OvVelocity;

typedef struct OvSpriteRenderer {
    uint32_t texture;
    OvRect src;    /* texture region; zero size: the whole texture */
    OvVec2 origin; /* point placed at the position, rotated around */
    OvColor tint;  /* default white */
    int32_t layer;
    uint8_t flip_x, flip_y, hidden;
    uint8_t y_sort; /* order within the layer by y */
} OvSpriteRenderer;

typedef struct OvBody {
    OvRect bounds;    /* relative to the position */
    uint32_t tilemap; /* 0: moves freely */
    OvContacts contacts; /* what it stopped at or rests against */
} OvBody;

/* A shape that collides with other colliders; it moves, turns and scales
 * with the Transform */
#define OV_COLLIDER_BOX 0
#define OV_COLLIDER_CIRCLE 1
#define OV_COLLIDER_POLYGON 2
typedef struct OvCollider {
    uint32_t kind;    /* OV_COLLIDER_* */
    uint32_t polygon; /* from ov_create_polygon */
    OvVec2 offset;    /* centre of the shape from the position */
    OvVec2 size;      /* box */
    float radius;     /* circle */
    uint32_t layer;   /* what it is (bits), default 1 */
    uint32_t mask;    /* what it meets: each one's mask has a bit of the other's layer; default all */
    uint8_t trigger;  /* reports overlaps, never pushes */
    uint8_t pad_[3];
} OvCollider;

/* A body with mass for ov_physics_step (with a Transform and a Collider).
 * Dynamic bodies fall and bounce, fixed ones are walls, kinematic ones go
 * where the game puts them (or by their Velocity) and push dynamic ones.
 * The physics step owns these entities: ov_update_motion skips them, and
 * ov_update_collisions reports them but does not push them. */
#define OV_BODY_DYNAMIC 0
#define OV_BODY_FIXED 1
#define OV_BODY_KINEMATIC 2
typedef struct OvRigidBody {
    uint32_t kind;       /* OV_BODY_* */
    float density;       /* mass per square unit, default 1 */
    float friction;      /* default 0.5 */
    float restitution;   /* bounciness 0..1 */
    float linear_damping;
    float angular_damping;
    float gravity_scale; /* default 1 */
    uint8_t fixed_rotation; /* never turns (characters) */
    uint8_t bullet;         /* fast: does not pass through thin walls */
    uint8_t pad_[2];
} OvRigidBody;

/* How two shapes overlap: move the second by normal * depth and they only touch */
typedef struct OvOverlap {
    OvVec2 normal; /* from the first towards the second */
    float depth;
} OvOverlap;

void ov_update_motion(OvEngine *e, float dt); /* dt: ov_tick_dt in fixed ticks */
void ov_draw_entities(OvEngine *e);
uint32_t ov_create_polygon(OvEngine *e, const OvVec2 *points, uint32_t count); /* convex, 3..64 corners; 0 on error */
void ov_free_polygon(OvEngine *e, uint32_t polygon);
bool ov_entity_overlap(OvEngine *e, OvEntity a, OvEntity b, OvOverlap *out); /* layers aside */
void ov_debug_colliders(OvEngine *e); /* outlines in the debug overlay */

/* Physics (Rapier, deterministic): call ov_physics_step once per tick.
 * Moving the Transform or setting the Velocity moves a body; after the
 * step its place and speed are written back. Impulses and joints work on
 * new entities right away. Built without physics: false, 0, ov_last_error. */
void ov_physics_step(OvEngine *e, float dt);
void ov_set_gravity(OvEngine *e, OvVec2 gravity); /* units/s^2, y down; default none (top-down) */
void ov_set_physics_scale(OvEngine *e, float units_per_metre); /* default 32 */
bool ov_apply_impulse(OvEngine *e, OvEntity entity, OvVec2 impulse);
bool ov_apply_force(OvEngine *e, OvEntity entity, OvVec2 force); /* during the next step */
bool ov_apply_torque_impulse(OvEngine *e, OvEntity entity, float torque);
float ov_body_mass(OvEngine *e, OvEntity entity);
uint32_t ov_joint_pin(OvEngine *e, OvEntity a, OvEntity b, OvVec2 at); /* hinge at a world point */
uint32_t ov_joint_weld(OvEngine *e, OvEntity a, OvEntity b);           /* glued as they are now */
uint32_t ov_joint_rope(OvEngine *e, OvEntity a, OvEntity b, float length); /* at most this far apart */
uint32_t ov_joint_spring(OvEngine *e, OvEntity a, OvEntity b, float rest, float stiffness, float damping);
bool ov_remove_joint(OvEngine *e, uint32_t joint);

/* The collision pass: call it after ov_update_motion, once per tick. A grid
 * finds the pairs near each other; solid colliders (not triggers) with a
 * Velocity are pushed out of the others and stop moving into them, ones
 * without stay put like walls. The contacts list every overlapping pair
 * (lower id first, normal from a to b), then the pairs that stopped. */
#define OV_CONTACT_BEGIN 0
#define OV_CONTACT_STAY 1
#define OV_CONTACT_END 2 /* or one of them is gone; no normal or depth */
typedef struct OvContact {
    OvEntity a, b;
    OvVec2 normal;
    float depth;
    int32_t state;   /* OV_CONTACT_* */
    uint8_t trigger; /* either one is a trigger */
    uint8_t pad_[3];
} OvContact;
typedef struct OvEntityHit {
    OvEntity entity;
    OvVec2 point;
    OvVec2 normal;
    float distance;
} OvEntityHit;
void ov_update_collisions(OvEngine *e);
void ov_set_collision_cell(OvEngine *e, float size); /* grid cell, default 64: about a typical collider */
uint32_t ov_contact_count(OvEngine *e);
bool ov_contact(OvEngine *e, uint32_t index, OvContact *out);
/* Entities whose collider has a layer bit in mask and overlaps the point,
 * box or circle; fills up to capacity and returns how many there are */
uint32_t ov_query_point(OvEngine *e, OvVec2 point, uint32_t mask, OvEntity *out, uint32_t capacity);
uint32_t ov_query_rect(OvEngine *e, OvRect rect, uint32_t mask, OvEntity *out, uint32_t capacity);
uint32_t ov_query_circle(OvEngine *e, OvVec2 center, float radius, uint32_t mask, OvEntity *out, uint32_t capacity);
bool ov_raycast_entities(OvEngine *e, OvVec2 from, OvVec2 to, uint32_t mask, OvEntityHit *out); /* the nearest */

/* Snapshots: the entities and components, the random generator and the
 * tick counter as bytes. Restoring one continues the game exactly from
 * there: saves, rollback, replays. Assets (textures, maps, sounds) are not
 * in it. ov_snapshot returns the size and copies the bytes only when they
 * fit, so call it with NULL first to learn the size. */
size_t ov_snapshot(OvEngine *e, uint8_t *buf, size_t capacity);
bool ov_restore(OvEngine *e, const uint8_t *data, size_t size); /* false + ov_last_error: nothing changed */
/* Fingerprint of the snapshot: games running in step have equal hashes */
uint64_t ov_state_hash(OvEngine *e);

/* --- scenes ---
 * A scene is a number the game picks (0 = none): menu, level, pause. The
 * engine keeps a stack of them and covers each switch with a transition;
 * the switch happens in its middle, when the screen is covered, and
 * ov_scene_entered returns the new scene for that one frame:
 *     if (ov_scene_entered(e) == LEVEL) load_level();
 * A change while another runs is refused (false). */
#define OV_TRANSITION_CUT 0  /* at the next frame */
#define OV_TRANSITION_FADE 1 /* to the colour and back */
#define OV_TRANSITION_WIPE 2 /* the colour sweeps across */
bool ov_scene_go(OvEngine *e, uint32_t scene, int32_t transition, float seconds, OvColor color);
bool ov_scene_push(OvEngine *e, uint32_t scene, int32_t transition, float seconds, OvColor color); /* over the current one */
bool ov_scene_pop(OvEngine *e, int32_t transition, float seconds, OvColor color); /* false: nothing below */
uint32_t ov_scene(OvEngine *e);         /* the scene on top */
uint32_t ov_scene_entered(OvEngine *e); /* became current this frame (after a pop: resumed) */
uint32_t ov_scene_left(OvEngine *e);    /* stopped being current this frame */
uint32_t ov_scene_depth(OvEngine *e);
bool ov_scene_transitioning(OvEngine *e);
float ov_scene_cover(OvEngine *e); /* how covered the screen is, 0..1 */

/* --- textures and fonts --- */
uint32_t ov_load_texture(OvEngine *e, const char *path); /* PNG, JPEG */
uint32_t ov_texture_from_rgba(OvEngine *e, uint32_t width, uint32_t height, const uint8_t *rgba);
bool ov_texture_size(OvEngine *e, uint32_t texture, uint32_t *w, uint32_t *h);
void ov_set_texture_smooth(OvEngine *e, uint32_t texture, bool smooth);
void ov_free_texture(OvEngine *e, uint32_t texture);
uint32_t ov_load_font(OvEngine *e, const char *path); /* TTF, OTF */
void ov_free_font(OvEngine *e, uint32_t font);
OvVec2 ov_measure_text(OvEngine *e, uint32_t font, const char *text, float size);

/* --- render targets, post effects and shake ---
 * A render target is a texture draw calls can go into: draw into it after
 * ov_set_target(e, tex), go back with ov_set_target(e, 0), and draw it
 * like any texture. Every frame starts on the screen. Targets are drawn in
 * the order they are first used in the frame, the screen last. One that is
 * not cleared keeps its picture and is drawn over (trails, paint).
 *
 * Post effects run over a target (0: the screen) every frame it is drawn,
 * in order. Both renderers run the built-in ones the same way; custom
 * shaders are WGSL and run only on the graphics card:
 *     fn effect(uv: vec2<f32>, pixel: vec2<f32>) -> vec4<f32> {
 *         let c = load(vec2<i32>(pixel));   // or textureSample(source, samp, uv)
 *         return vec4<f32>(c.rgb * post.p[0].x, c.a); // p[0..2]: the eight numbers
 *     }
 * post.size is the picture size and post.time seconds since start. */
#define OV_EFFECT_GLOW 1   /* p: threshold 0..1, strength, radius px (1..32) */
#define OV_EFFECT_BLUR 2   /* p: radius px (1..32) */
#define OV_EFFECT_GRADE 3  /* p: brightness (+-), contrast (1), saturation (1), gamma (1), tint r, g, b (1) */
#define OV_EFFECT_CRT 4    /* p: scanlines 0..1, curvature 0..0.5, vignette 0..1, aberration px */
#define OV_EFFECT_SHADER 5 /* shader from ov_create_shader; p goes to post.p */
typedef struct OvEffect {
    uint32_t kind;
    uint32_t shader;
    float p[8];
} OvEffect;
uint32_t ov_create_target(OvEngine *e, uint32_t width, uint32_t height); /* a texture id; 0 on error */
bool ov_set_target(OvEngine *e, uint32_t target); /* 0: the screen; false: not a target */
uint32_t ov_get_target(OvEngine *e);
void ov_clear_target(OvEngine *e, uint32_t target, OvColor color); /* this frame, before its draw calls */
void ov_set_target_camera(OvEngine *e, uint32_t target, const OvCamera *camera); /* NULL: the main camera */
bool ov_set_effects(OvEngine *e, uint32_t target, const OvEffect *effects, uint32_t count);
uint32_t ov_create_shader(OvEngine *e, const char *wgsl); /* 0 and ov_last_error on a compile error */
void ov_free_shader(OvEngine *e, uint32_t shader);
void ov_shake(OvEngine *e, float strength, float duration); /* the world on screen, pixels; fades out */

/* --- camera and draw state --- */
OvCamera ov_get_camera(OvEngine *e);
void ov_set_camera(OvEngine *e, OvCamera camera);
OvVec2 ov_world_to_screen(OvEngine *e, OvVec2 p);
OvVec2 ov_screen_to_world(OvEngine *e, OvVec2 p);
void ov_set_layer(OvEngine *e, int32_t layer);  /* for shapes and text */
void ov_set_depth(OvEngine *e, float depth);
void ov_set_space(OvEngine *e, int32_t space);  /* OvSpace */
void ov_set_antialias(OvEngine *e, bool on);

/* --- drawing --- */
void ov_draw_sprite(OvEngine *e, uint32_t texture, const OvSprite *sprite);
void ov_draw_texture(OvEngine *e, uint32_t texture, float x, float y);
void ov_draw_frame(OvEngine *e, uint32_t texture, const OvSpriteSheet *sheet, uint32_t frame, const OvSprite *sprite);
void ov_fill_rect(OvEngine *e, OvRect rect, OvColor color);
void ov_draw_rect(OvEngine *e, OvRect rect, float thickness, OvColor color);
void ov_draw_line(OvEngine *e, OvVec2 from, OvVec2 to, float thickness, OvColor color);
void ov_fill_circle(OvEngine *e, OvVec2 center, float radius, OvColor color);
void ov_draw_circle(OvEngine *e, OvVec2 center, float radius, float thickness, OvColor color);
void ov_draw_text(OvEngine *e, uint32_t font, const char *text, OvVec2 pos, float size, OvColor color);

/* --- input --- */
bool ov_key_down(OvEngine *e, int32_t key);
bool ov_key_pressed(OvEngine *e, int32_t key);  /* this frame only */
bool ov_key_released(OvEngine *e, int32_t key);
float ov_axis(OvEngine *e, int32_t negative, int32_t positive); /* -1, 0, 1 */
bool ov_mouse_down(OvEngine *e, int32_t button);
bool ov_mouse_pressed(OvEngine *e, int32_t button);
bool ov_mouse_released(OvEngine *e, int32_t button);
OvVec2 ov_mouse_pos(OvEngine *e);   /* canvas pixels */
OvVec2 ov_mouse_world(OvEngine *e);
float ov_mouse_wheel(OvEngine *e);
const char *ov_text_input(OvEngine *e); /* typed this frame; valid until the next call */
void ov_inject_key(OvEngine *e, int32_t key, bool down); /* applied at the next poll */
void ov_inject_mouse_button(OvEngine *e, int32_t button, bool down);
void ov_inject_mouse_move(OvEngine *e, OvVec2 pos);

/* --- input actions ---
 * The game asks for "jump", not Space, and players rebind it. Pressed and
 * released are for the action as a whole: two keys on "jump" press once.
 *     uint32_t jump = ov_action(e, "jump");
 *     ov_bind(e, jump, (OvBinding){OV_BIND_KEY, OV_KEY_SPACE, -1, 0});
 *     if (ov_action_pressed(e, jump)) ...
 * A rebinding menu waits for ov_captured_binding (the first input pressed
 * this frame) and stores it with ov_set_binding. ov_bindings_save gives a
 * text players can edit ("jump = key:Space pad:South"). */
#define OV_BIND_KEY 1
#define OV_BIND_MOUSE 2
#define OV_BIND_PAD_BUTTON 3
#define OV_BIND_PAD_AXIS 4
typedef struct OvBinding {
    int32_t kind;      /* OV_BIND_* */
    int32_t code;      /* key, mouse button, pad button or pad axis */
    int32_t pad;       /* pad slot, -1: any pad */
    int32_t direction; /* pad axis: 1 or -1 */
} OvBinding;
uint32_t ov_action(OvEngine *e, const char *name); /* id from 1, made on first use */
bool ov_bind(OvEngine *e, uint32_t action, OvBinding binding); /* added once */
bool ov_set_binding(OvEngine *e, uint32_t action, uint32_t index, OvBinding binding); /* index = count: add */
bool ov_remove_binding(OvEngine *e, uint32_t action, uint32_t index);
void ov_clear_bindings(OvEngine *e, uint32_t action);
uint32_t ov_binding_count(OvEngine *e, uint32_t action);
bool ov_get_binding(OvEngine *e, uint32_t action, uint32_t index, OvBinding *out);
size_t ov_binding_text(OvEngine *e, OvBinding binding, char *buf, size_t size); /* "key:Space" */
bool ov_action_down(OvEngine *e, uint32_t action);
bool ov_action_pressed(OvEngine *e, uint32_t action);  /* this frame only */
bool ov_action_released(OvEngine *e, uint32_t action);
float ov_action_value(OvEngine *e, uint32_t action);   /* 0..1, sticks give how far */
float ov_action_axis(OvEngine *e, uint32_t negative, uint32_t positive); /* -1..1 */
bool ov_captured_binding(OvEngine *e, OvBinding *out); /* the first input pressed this frame */
size_t ov_bindings_save(OvEngine *e, char *buf, size_t size);
/* Actions in the text get exactly its bindings, others keep theirs;
 * false + ov_last_error on a bad line, and nothing changes */
bool ov_bindings_load(OvEngine *e, const char *text);

/* --- gamepads: up to 4, in slots 0..3 that a controller keeps until unplugged --- */
bool ov_gamepad_connected(OvEngine *e, int32_t gamepad);
size_t ov_gamepad_name(OvEngine *e, int32_t gamepad, char *buf, size_t size);
bool ov_gamepad_down(OvEngine *e, int32_t gamepad, int32_t button);
bool ov_gamepad_pressed(OvEngine *e, int32_t gamepad, int32_t button);  /* this frame only */
bool ov_gamepad_released(OvEngine *e, int32_t gamepad, int32_t button);
float ov_gamepad_axis(OvEngine *e, int32_t gamepad, int32_t axis);    /* after the dead zone */
void ov_set_gamepad_deadzone(OvEngine *e, float deadzone);             /* default 0.15 */
void ov_inject_gamepad_connected(OvEngine *e, int32_t gamepad, bool connected);
void ov_inject_gamepad_button(OvEngine *e, int32_t gamepad, int32_t button, bool down);
void ov_inject_gamepad_axis(OvEngine *e, int32_t gamepad, int32_t axis, float value); /* raw */

/* --- sprite sheets and animations --- */
OvSpriteSheet ov_sheet_grid(uint32_t texture_w, uint32_t texture_h, uint32_t frame_w, uint32_t frame_h,
                            uint32_t margin, uint32_t spacing);
OvRect ov_sheet_frame(OvSpriteSheet sheet, uint32_t index);
uint32_t ov_anim_frame_at(const uint32_t *frames, uint32_t count, float fps, int32_t mode, float time);

/* --- tweens and easing --- */
/* Eased progress for t in 0..1 (clamped); ease is an OvEase */
float ov_ease(int32_t ease, float t);
/* Eased progress 0..1 of a tween `time` seconds after its start; a negative
   time is the delay before it. repeat is an OvRepeat */
float ov_tween_progress(float time, float duration, int32_t ease, int32_t repeat);
OvColor ov_lerp_color(OvColor a, OvColor b, float t);

/* "gpu: <graphics card>" or "software" */
size_t ov_renderer_name(OvEngine *e, char *buf, size_t size);

/* --- hot reload: textures and maps reload in place when their files change --- */
void ov_set_hot_reload(OvEngine *e, bool on); /* checked in ov_poll twice a second */
bool ov_hot_reload_enabled(OvEngine *e);
uint32_t ov_reload_changed(OvEngine *e);      /* check now; returns how many reloaded */
uint32_t ov_reloaded(OvEngine *e);            /* reloaded by the last ov_poll */

/* --- tile maps --- */
uint32_t ov_load_tilemap(OvEngine *e, const char *path); /* Tiled .tmj */
uint32_t ov_create_tilemap(OvEngine *e, int32_t orientation, uint32_t width, uint32_t height,
                           uint32_t tile_w, uint32_t tile_h, uint32_t layers);
bool ov_tilemap_add_tileset(OvEngine *e, uint32_t map, uint32_t first_gid, uint32_t texture, OvSpriteSheet sheet);
bool ov_tilemap_set_tile_flag(OvEngine *e, uint32_t map, uint32_t gid, const char *name, bool value);
void ov_free_tilemap(OvEngine *e, uint32_t map);
bool ov_tilemap_info(OvEngine *e, uint32_t map, OvTilemapInfo *info);
int32_t ov_tilemap_layer_index(OvEngine *e, uint32_t map, const char *name); /* -1: none */
uint32_t ov_tilemap_get_tile(OvEngine *e, uint32_t map, uint32_t layer, int32_t x, int32_t y);
void ov_tilemap_set_tile(OvEngine *e, uint32_t map, uint32_t layer, int32_t x, int32_t y, uint32_t gid);
bool ov_tilemap_is_solid(OvEngine *e, uint32_t map, int32_t x, int32_t y);
OvVec2 ov_tilemap_tile_to_world(OvEngine *e, uint32_t map, OvVec2 tile);
OvVec2 ov_tilemap_world_to_tile(OvEngine *e, uint32_t map, OvVec2 p);
OvVec2 ov_tilemap_cell_center(OvEngine *e, uint32_t map, int32_t x, int32_t y);
void ov_draw_tilemap(OvEngine *e, uint32_t map, int32_t base_layer);
void ov_draw_tilemap_layer(OvEngine *e, uint32_t map, uint32_t layer, int32_t draw_layer, float depth,
                           bool y_sort, OvColor tint);
bool ov_tilemap_object(OvEngine *e, uint32_t map, uint32_t index, OvMapObject *out);
int32_t ov_tilemap_find_object(OvEngine *e, uint32_t map, const char *name); /* -1: none */
/* Copy strings into buf; the return value is the full length (pass size 0 to ask) */
size_t ov_tilemap_object_name(OvEngine *e, uint32_t map, uint32_t index, char *buf, size_t size);
size_t ov_tilemap_object_class(OvEngine *e, uint32_t map, uint32_t index, char *buf, size_t size);
double ov_tilemap_object_number(OvEngine *e, uint32_t map, uint32_t index, const char *name, double fallback);
int64_t ov_tilemap_object_string(OvEngine *e, uint32_t map, uint32_t index, const char *name, char *buf,
                                 size_t size); /* -1: missing */

/* --- collisions --- */
OvMoveResult ov_move_box(OvEngine *e, uint32_t map, OvRect rect, OvVec2 delta);
OvContacts ov_touching(OvEngine *e, uint32_t map, OvRect rect);
bool ov_raycast(OvEngine *e, uint32_t map, OvVec2 from, OvVec2 to, OvRayHit *hit);
/* A path around solid cells (A*): cell centres from the cell of `from` to
 * the cell of `to`. A number property `cost` on a tile or layer makes it
 * slower to cross; diagonal steps never cut a wall's corner. max_cost stops
 * early (0: no limit). Fills up to capacity, returns the length (0: no way). */
uint32_t ov_find_path(OvEngine *e, uint32_t map, OvVec2 from, OvVec2 to, bool diagonal, float max_cost,
                      OvVec2 *out, uint32_t capacity);
bool ov_point_solid(OvEngine *e, uint32_t map, OvVec2 p);
bool ov_rect_solid(OvEngine *e, uint32_t map, OvRect rect);
bool ov_rects_overlap(OvRect a, OvRect b);

/* --- debug overlay: drawn on top only while on; F3 toggles it --- */
void ov_set_debug(OvEngine *e, bool on);
bool ov_debug_enabled(OvEngine *e);
void ov_set_debug_key(OvEngine *e, int32_t key); /* 0: no toggle key */
void ov_debug_rect(OvEngine *e, OvRect rect, OvColor color);   /* in the current space */
void ov_debug_line(OvEngine *e, OvVec2 from, OvVec2 to, OvColor color);
void ov_debug_circle(OvEngine *e, OvVec2 center, float radius, OvColor color);
void ov_debug_text(OvEngine *e, const char *text, OvVec2 pos, OvColor color);
void ov_debug_tilemap(OvEngine *e, uint32_t map); /* solid cells, platforms, objects */

/* --- particles --- */
OvEmitter ov_default_emitter(void);
/* config NULL: the defaults. The emitter spawns at its rate right away */
uint32_t ov_create_emitter(OvEngine *e, const OvEmitter *config, OvVec2 pos);
void ov_free_emitter(OvEngine *e, uint32_t emitter);
OvEmitter ov_emitter_config(OvEngine *e, uint32_t emitter);
/* Living particles keep flying; a new seed restarts the random sequence */
bool ov_set_emitter_config(OvEngine *e, uint32_t emitter, const OvEmitter *config);
void ov_set_emitter_pos(OvEngine *e, uint32_t emitter, OvVec2 pos);
OvVec2 ov_emitter_pos(OvEngine *e, uint32_t emitter);
void ov_set_emitter_emitting(OvEngine *e, uint32_t emitter, bool on);
bool ov_emitter_emitting(OvEngine *e, uint32_t emitter);
void ov_emitter_burst(OvEngine *e, uint32_t emitter, uint32_t count);
uint32_t ov_emitter_count(OvEngine *e, uint32_t emitter);
void ov_emitter_clear(OvEngine *e, uint32_t emitter);
void ov_update_emitter(OvEngine *e, uint32_t emitter, float dt);
void ov_update_emitters(OvEngine *e, float dt);
/* On the config's layer, at the current depth and space */
void ov_draw_emitter(OvEngine *e, uint32_t emitter);

/* --- sound (WAV, OGG, MP3) --- */
uint32_t ov_load_sound(OvEngine *e, const char *path);
void ov_free_sound(OvEngine *e, uint32_t sound);
uint32_t ov_play_sound(OvEngine *e, uint32_t sound, float volume, float pitch); /* voice id */
void ov_stop_voice(OvEngine *e, uint32_t voice);
bool ov_voice_playing(OvEngine *e, uint32_t voice);
void ov_stop_all_sounds(OvEngine *e);
bool ov_play_music(OvEngine *e, const char *path, float volume, bool looped);
void ov_stop_music(OvEngine *e);
void ov_pause_music(OvEngine *e, bool paused);
bool ov_music_playing(OvEngine *e);
void ov_set_music_volume(OvEngine *e, float volume);
void ov_set_master_volume(OvEngine *e, float volume);

#ifdef __cplusplus
}
#endif

#endif /* OVERSIGHT_H */

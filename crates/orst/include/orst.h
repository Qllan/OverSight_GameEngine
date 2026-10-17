/* ORST: C API.
 *
 * The game owns its loop:
 *
 *     OrstConfig config = orst_default_config();
 *     config.title = "My game";
 *     OrstEngine *e = orst_engine_create(&config);
 *     while (orst_poll(e)) {
 *         float dt = orst_dt(e);
 *         orst_begin_frame(e, ORST_BLACK);
 *         // update with dt, then draw
 *         orst_end_frame(e);
 *     }
 *     orst_engine_destroy(e);
 *
 * Resources (textures, fonts, maps, sounds) are ids; 0 means "none" or
 * "failed", and orst_last_error() says why. Strings are UTF-8. Every
 * function accepts a NULL engine and then does nothing.
 */
#ifndef ORST_H
#define ORST_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct OrstEngine OrstEngine;

typedef struct OrstColor { uint8_t r, g, b, a; } OrstColor;
typedef struct OrstVec2 { float x, y; } OrstVec2;
/* Top-left corner and size */
typedef struct OrstRect { float x, y, w, h; } OrstRect;

#define ORST_RGBA(r, g, b, a) ((OrstColor){(r), (g), (b), (a)})
#define ORST_BLACK ORST_RGBA(0, 0, 0, 255)
#define ORST_WHITE ORST_RGBA(255, 255, 255, 255)

/* How a sprite is drawn. src is the texture region (w or h <= 0: the
 * whole texture); origin (in source pixels) lands on pos and is the
 * pivot for rotation and scale. Start from orst_default_sprite(). */
typedef struct OrstSprite {
    OrstRect src;
    OrstVec2 pos;
    OrstVec2 origin;
    OrstVec2 scale;
    float rotation; /* radians, clockwise */
    bool flip_x;
    bool flip_y;
    OrstColor tint;
    int32_t layer;  /* lower layers are drawn first */
    float depth;    /* order within a layer: pass y for y-sorting */
} OrstSprite;

typedef struct OrstCamera {
    OrstVec2 center; /* world point at the centre of the screen */
    float zoom;    /* screen pixels per world unit */
    float rotation;
} OrstCamera;

typedef struct OrstConfig {
    const char *title;
    uint32_t width, height; /* canvas: the logical resolution */
    float scale;            /* initial window size = canvas * scale */
    bool resizable;
    bool fullscreen;
    bool integer_scale;     /* scale by whole numbers only (pixel art) */
    uint32_t target_fps;    /* 0 = unlimited */
    bool headless;          /* no window: render offscreen */
    int32_t renderer;       /* OrstRenderer */
} OrstConfig;

typedef enum OrstRenderer {
    ORST_RENDERER_AUTO = 0,     /* the graphics card for a window (the CPU if it fails), the CPU headless */
    ORST_RENDERER_SOFTWARE = 1, /* always the CPU */
    ORST_RENDERER_GPU = 2       /* always the graphics card, also headless; creating fails without one */
} OrstRenderer;

/* A texture cut into a grid of equal frames, numbered row by row */
typedef struct OrstSpriteSheet {
    uint32_t frame_w, frame_h, columns, count, margin, spacing;
} OrstSpriteSheet;

/* How an emitter's particles look and move. Get the defaults from
   orst_default_emitter() and change what you need. A min/max pair picks a
   random value between them for every particle. */
typedef struct OrstEmitter {
    uint32_t texture;           /* 0: round dots */
    OrstRect src;                 /* region of the texture; w or h <= 0: whole texture */
    float rate;                 /* particles per second while emitting */
    uint32_t max;               /* most particles alive at once */
    float life_min, life_max;   /* seconds */
    float speed_min, speed_max; /* pixels per second */
    float angle;                /* direction, radians: 0 right, PI/2 down */
    float spread;               /* width of the cone of directions (2*PI: every way) */
    OrstVec2 gravity;             /* pixels per second squared */
    float drag;                 /* share of speed lost per second */
    float size_start, size_end; /* pixels: dot diameter or texture width */
    float spin_min, spin_max;   /* radians per second (textures) */
    OrstColor color_start, color_end;
    OrstVec2 area;                /* spawn box half-size around the emitter */
    int32_t layer;
    uint32_t seed;              /* same seed and calls: same particles */
} OrstEmitter;

typedef enum OrstAnimMode { ORST_ANIM_LOOP = 0, ORST_ANIM_ONCE = 1, ORST_ANIM_PING_PONG = 2 } OrstAnimMode;
/* Easing curves, as on easings.net */
typedef enum OrstEase {
    ORST_EASE_LINEAR = 0,
    ORST_EASE_QUAD_IN, ORST_EASE_QUAD_OUT, ORST_EASE_QUAD_IN_OUT,
    ORST_EASE_CUBIC_IN, ORST_EASE_CUBIC_OUT, ORST_EASE_CUBIC_IN_OUT,
    ORST_EASE_SINE_IN, ORST_EASE_SINE_OUT, ORST_EASE_SINE_IN_OUT,
    ORST_EASE_EXPO_IN, ORST_EASE_EXPO_OUT, ORST_EASE_EXPO_IN_OUT,
    ORST_EASE_BACK_IN, ORST_EASE_BACK_OUT, ORST_EASE_BACK_IN_OUT,
    ORST_EASE_ELASTIC_IN, ORST_EASE_ELASTIC_OUT, ORST_EASE_ELASTIC_IN_OUT,
    ORST_EASE_BOUNCE_IN, ORST_EASE_BOUNCE_OUT, ORST_EASE_BOUNCE_IN_OUT
} OrstEase;
typedef enum OrstRepeat { ORST_REPEAT_ONCE = 0, ORST_REPEAT_LOOP = 1, ORST_REPEAT_PING_PONG = 2 } OrstRepeat;
typedef enum OrstSpace { ORST_SPACE_WORLD = 0, ORST_SPACE_SCREEN = 1 } OrstSpace;
typedef enum OrstOrientation { ORST_ORTHOGONAL = 0, ORST_ISOMETRIC = 1 } OrstOrientation;
typedef enum OrstMouseButton { ORST_MOUSE_LEFT = 0, ORST_MOUSE_RIGHT = 1, ORST_MOUSE_MIDDLE = 2 } OrstMouseButton;

/* Gamepad buttons by position: SOUTH is A on Xbox, Cross on PlayStation */
typedef enum OrstGamepadButton {
    ORST_PAD_SOUTH = 0, ORST_PAD_EAST, ORST_PAD_WEST, ORST_PAD_NORTH,
    ORST_PAD_LEFT_BUMPER, ORST_PAD_RIGHT_BUMPER,
    ORST_PAD_BACK, ORST_PAD_START, ORST_PAD_GUIDE,
    ORST_PAD_LEFT_STICK, ORST_PAD_RIGHT_STICK,
    ORST_PAD_DPAD_UP, ORST_PAD_DPAD_DOWN, ORST_PAD_DPAD_LEFT, ORST_PAD_DPAD_RIGHT
} OrstGamepadButton;
/* Sticks -1..1 with y pointing down (up is -1); triggers 0..1 */
typedef enum OrstGamepadAxis {
    ORST_PAD_LEFT_X = 0, ORST_PAD_LEFT_Y, ORST_PAD_RIGHT_X, ORST_PAD_RIGHT_Y,
    ORST_PAD_LEFT_TRIGGER, ORST_PAD_RIGHT_TRIGGER
} OrstGamepadAxis;

/* Physical keys (WASD works on any layout) */
typedef enum OrstKey {
    ORST_KEY_UNKNOWN = 0,
    ORST_KEY_A = 1,
    ORST_KEY_B = 2,
    ORST_KEY_C = 3,
    ORST_KEY_D = 4,
    ORST_KEY_E = 5,
    ORST_KEY_F = 6,
    ORST_KEY_G = 7,
    ORST_KEY_H = 8,
    ORST_KEY_I = 9,
    ORST_KEY_J = 10,
    ORST_KEY_K = 11,
    ORST_KEY_L = 12,
    ORST_KEY_M = 13,
    ORST_KEY_N = 14,
    ORST_KEY_O = 15,
    ORST_KEY_P = 16,
    ORST_KEY_Q = 17,
    ORST_KEY_R = 18,
    ORST_KEY_S = 19,
    ORST_KEY_T = 20,
    ORST_KEY_U = 21,
    ORST_KEY_V = 22,
    ORST_KEY_W = 23,
    ORST_KEY_X = 24,
    ORST_KEY_Y = 25,
    ORST_KEY_Z = 26,
    ORST_KEY_NUM0 = 30,
    ORST_KEY_NUM1 = 31,
    ORST_KEY_NUM2 = 32,
    ORST_KEY_NUM3 = 33,
    ORST_KEY_NUM4 = 34,
    ORST_KEY_NUM5 = 35,
    ORST_KEY_NUM6 = 36,
    ORST_KEY_NUM7 = 37,
    ORST_KEY_NUM8 = 38,
    ORST_KEY_NUM9 = 39,
    ORST_KEY_SPACE = 40,
    ORST_KEY_ENTER = 41,
    ORST_KEY_ESCAPE = 42,
    ORST_KEY_TAB = 43,
    ORST_KEY_BACKSPACE = 44,
    ORST_KEY_DELETE = 45,
    ORST_KEY_INSERT = 46,
    ORST_KEY_HOME = 47,
    ORST_KEY_END = 48,
    ORST_KEY_PAGE_UP = 49,
    ORST_KEY_PAGE_DOWN = 50,
    ORST_KEY_LEFT = 60,
    ORST_KEY_RIGHT = 61,
    ORST_KEY_UP = 62,
    ORST_KEY_DOWN = 63,
    ORST_KEY_LEFT_SHIFT = 70,
    ORST_KEY_RIGHT_SHIFT = 71,
    ORST_KEY_LEFT_CTRL = 72,
    ORST_KEY_RIGHT_CTRL = 73,
    ORST_KEY_LEFT_ALT = 74,
    ORST_KEY_RIGHT_ALT = 75,
    ORST_KEY_F1 = 80,
    ORST_KEY_F2 = 81,
    ORST_KEY_F3 = 82,
    ORST_KEY_F4 = 83,
    ORST_KEY_F5 = 84,
    ORST_KEY_F6 = 85,
    ORST_KEY_F7 = 86,
    ORST_KEY_F8 = 87,
    ORST_KEY_F9 = 88,
    ORST_KEY_F10 = 89,
    ORST_KEY_F11 = 90,
    ORST_KEY_F12 = 91,
    ORST_KEY_MINUS = 100,
    ORST_KEY_EQUALS = 101,
    ORST_KEY_LEFT_BRACKET = 102,
    ORST_KEY_RIGHT_BRACKET = 103,
    ORST_KEY_SEMICOLON = 104,
    ORST_KEY_QUOTE = 105,
    ORST_KEY_COMMA = 106,
    ORST_KEY_PERIOD = 107,
    ORST_KEY_SLASH = 108,
    ORST_KEY_BACKSLASH = 109,
    ORST_KEY_GRAVE = 110,
} OrstKey;

/* Tile id flags, as in Tiled */
#define ORST_FLIP_H 0x80000000u
#define ORST_FLIP_V 0x40000000u
#define ORST_FLIP_D 0x20000000u

typedef struct OrstTilemapInfo {
    OrstOrientation orientation;
    uint32_t width, height;   /* cells */
    uint32_t tile_w, tile_h;  /* cell size in world units */
    uint32_t layer_count;
    uint32_t object_count;
} OrstTilemapInfo;

typedef struct OrstMapObject {
    uint32_t id;
    OrstRect rect; /* world units */
    bool point;
    bool visible;
    float rotation;
    uint32_t gid;
    uint32_t layer; /* index of its object layer */
} OrstMapObject;

typedef struct OrstContacts {
    bool ground;  /* stopped while moving down */
    bool ceiling; /* stopped while moving up */
    bool wall_left;
    bool wall_right;
} OrstContacts;

typedef struct OrstMoveResult {
    OrstRect rect;
    OrstContacts contacts;
} OrstMoveResult;

typedef struct OrstRayHit {
    OrstVec2 point;
    int32_t cell_x, cell_y;
    OrstVec2 normal; /* side hit, in cell axes; (0, 0) when starting inside */
    float distance;
} OrstRayHit;

#define ORST_DEFAULT_FONT 1u

static inline OrstConfig orst_default_config(void) {
    OrstConfig c = {"ORST", 960, 540, 1.0f, true, false, false, 60, false, ORST_RENDERER_AUTO};
    return c;
}

static inline OrstSprite orst_default_sprite(void) {
    OrstSprite s = {{0, 0, 0, 0}, {0, 0}, {0, 0}, {1, 1}, 0, false, false, {255, 255, 255, 255}, 0, 0};
    return s;
}

/* --- engine and frame --- */
const char *orst_last_error(void);
OrstEngine *orst_engine_create(const OrstConfig *config); /* NULL on failure */
void orst_engine_destroy(OrstEngine *e);
bool orst_poll(OrstEngine *e); /* false when the game should end */
void orst_quit(OrstEngine *e);
float orst_dt(OrstEngine *e);
double orst_time(OrstEngine *e);
float orst_fps(OrstEngine *e);
uint64_t orst_frame(OrstEngine *e);

/* Fixed logic ticks: run the game logic orst_ticks(e) times per frame with
 * orst_tick_dt(e), so it plays the same at any frame rate (replays, network).
 *     for (uint32_t i = 0; i < orst_ticks(e); ++i) update(orst_tick_dt(e));
 * orst_tick_alpha (0..1) is how far the time is toward the next tick: draw
 * moving things that far between their last two positions. */
void orst_set_tick_rate(OrstEngine *e, uint32_t ticks_per_second); /* 60 by default */
uint32_t orst_ticks(OrstEngine *e);
float orst_tick_dt(OrstEngine *e);
float orst_tick_alpha(OrstEngine *e);
uint64_t orst_tick_count(OrstEngine *e);

/* Seeded random numbers: the same seed gives the same game on any machine.
 * The seed is 1 until set. */
void orst_random_seed(OrstEngine *e, uint64_t seed);
uint32_t orst_random_u32(OrstEngine *e);
float orst_random_float(OrstEngine *e);                       /* 0 <= x < 1 */
float orst_random_range(OrstEngine *e, float min, float max); /* min <= x < max */
int32_t orst_random_int(OrstEngine *e, int32_t min, int32_t max); /* min <= x <= max */
void orst_begin_frame(OrstEngine *e, OrstColor clear);
void orst_end_frame(OrstEngine *e);
void orst_set_title(OrstEngine *e, const char *title);
void orst_set_fullscreen(OrstEngine *e, bool on);
bool orst_is_fullscreen(OrstEngine *e);
void orst_set_target_fps(OrstEngine *e, uint32_t fps);
bool orst_has_window(OrstEngine *e);
void orst_canvas_size(OrstEngine *e, uint32_t *w, uint32_t *h);
bool orst_save_screenshot(OrstEngine *e, const char *path);
OrstColor orst_pixel(OrstEngine *e, uint32_t x, uint32_t y);

/* --- entities and components ---
 * An entity is an id (0 = none); a component is a plain struct with no
 * pointers, registered by name, size and alignment:
 *     typedef struct { float x, y; } Pos;
 *     uint32_t pos = orst_component(e, "Pos", sizeof(Pos), _Alignof(Pos));
 *     OrstEntity hero = orst_spawn(e);
 *     Pos p = {10, 20};
 *     orst_set(e, hero, pos, &p);
 *     ((Pos *)orst_get(e, hero, pos))->x += 1;
 * Pointers from orst_get stay valid until the next spawn, despawn, set or
 * remove. The same calls in the same order give the same ids and the same
 * query order on every machine. */
typedef uint64_t OrstEntity;
uint32_t orst_component(OrstEngine *e, const char *name, uint32_t size, uint32_t align); /* 0 on error */
OrstEntity orst_spawn(OrstEngine *e);
bool orst_despawn(OrstEngine *e, OrstEntity entity); /* removes its components too */
bool orst_alive(OrstEngine *e, OrstEntity entity);
uint32_t orst_entity_count(OrstEngine *e);
bool orst_set(OrstEngine *e, OrstEntity entity, uint32_t component, const void *data); /* NULL data: zeroed (built-ins: defaults) */
void *orst_get(OrstEngine *e, OrstEntity entity, uint32_t component); /* NULL when it has none */
bool orst_has(OrstEngine *e, OrstEntity entity, uint32_t component);
bool orst_remove(OrstEngine *e, OrstEntity entity, uint32_t component);
/* Entities that have all `count` components (none: every entity). Fills up
 * to `capacity` of them into `out` and returns how many there are. */
uint32_t orst_query(OrstEngine *e, const uint32_t *components, uint32_t count, OrstEntity *out, uint32_t capacity);
void orst_world_clear(OrstEngine *e); /* despawn everything; component types stay */
OrstEntity orst_find_named(OrstEngine *e, const char *name); /* oldest with that OrstName; 0: none */

/* What is inside a component, for tools: the editor's inspector shows a
 * widget per field, scene files store fields by name, undo changes one
 * field. Built-ins come described; a game's component works without it,
 * tools then see bytes. Setting fields again replaces them. Fields must
 * lie inside the component without overlapping.
 *     OrstField f[] = {{"speed", ORST_FIELD_F32, offsetof(Enemy, speed), 0, 0, 10},
 *                      {"mode", ORST_FIELD_U32, offsetof(Enemy, mode), 0, 0, 0, "Walk|Run"}};
 *     orst_set_fields(e, enemy, f, 2); */
#define ORST_FIELD_F32 1
#define ORST_FIELD_I32 2
#define ORST_FIELD_U32 3
#define ORST_FIELD_BOOL 4    /* one byte */
#define ORST_FIELD_VEC2 5
#define ORST_FIELD_COLOR 6
#define ORST_FIELD_RECT 7
#define ORST_FIELD_ENTITY 8  /* OrstEntity */
#define ORST_FIELD_TEXTURE 9 /* resource ids (uint32_t); scenes store asset paths */
#define ORST_FIELD_TILEMAP 10
#define ORST_FIELD_POLYGON 11
#define ORST_FIELD_U8 12
#define ORST_FIELD_TEXT 13 /* char[64] of UTF-8, the rest zeros (OrstName) */
#define ORST_FIELD_ANGLE 1  /* flags: an f32 in radians, shown in degrees */
#define ORST_FIELD_LAYERS 2 /* a u32 of layer bits */

typedef struct OrstField {
    const char *name;
    uint32_t type;   /* ORST_FIELD_... */
    uint32_t offset; /* offsetof(Component, field) */
    uint32_t flags;
    float min, max;      /* a slider when min < max */
    const char *options; /* a number that is a choice: "Walk|Run" names 0, 1; NULL: none */
} OrstField;

bool orst_set_fields(OrstEngine *e, uint32_t component, const OrstField *fields, uint32_t count);
uint32_t orst_field_count(OrstEngine *e, uint32_t component);
/* The strings stay valid until the next orst_get_field */
bool orst_get_field(OrstEngine *e, uint32_t component, uint32_t index, OrstField *out);

/* Built-in components: every engine registers them first, so these ids
 * are fixed. orst_update_motion moves entities with a Transform and a
 * Velocity; with a Body on a tile map they slide along walls instead.
 * orst_draw_entities draws every Transform + SpriteRenderer. orst_set with
 * NULL data gives them their defaults (scale 1, white tint). */
#define ORST_TRANSFORM 1
#define ORST_VELOCITY 2
#define ORST_SPRITE_RENDERER 3
#define ORST_BODY 4
#define ORST_COLLIDER 5
#define ORST_RIGIDBODY 6
#define ORST_LIGHT 7
#define ORST_NAME 8

typedef struct OrstTransform {
    OrstVec2 pos;
    float rotation; /* radians, clockwise */
    OrstVec2 scale;   /* default {1, 1} */
} OrstTransform;

typedef struct OrstVelocity {
    OrstVec2 linear; /* units per second */
    float angular; /* radians per second */
} OrstVelocity;

typedef struct OrstSpriteRenderer {
    uint32_t texture;
    OrstRect src;    /* texture region; zero size: the whole texture */
    OrstVec2 origin; /* point placed at the position, rotated around */
    OrstColor tint;  /* default white */
    int32_t layer;
    uint8_t flip_x, flip_y, hidden;
    uint8_t y_sort; /* order within the layer by y */
} OrstSpriteRenderer;

typedef struct OrstBody {
    OrstRect bounds;    /* relative to the position */
    uint32_t tilemap; /* 0: moves freely */
    OrstContacts contacts; /* what it stopped at or rests against */
} OrstBody;

/* A shape that collides with other colliders; it moves, turns and scales
 * with the Transform */
#define ORST_COLLIDER_BOX 0
#define ORST_COLLIDER_CIRCLE 1
#define ORST_COLLIDER_POLYGON 2
typedef struct OrstCollider {
    uint32_t kind;    /* ORST_COLLIDER_* */
    uint32_t polygon; /* from orst_create_polygon */
    OrstVec2 offset;    /* centre of the shape from the position */
    OrstVec2 size;      /* box */
    float radius;     /* circle */
    uint32_t layer;   /* what it is (bits), default 1 */
    uint32_t mask;    /* what it meets: each one's mask has a bit of the other's layer; default all */
    uint8_t trigger;  /* reports overlaps, never pushes */
    uint8_t pad_[3];
} OrstCollider;

/* A body with mass for orst_physics_step (with a Transform and a Collider).
 * Dynamic bodies fall and bounce, fixed ones are walls, kinematic ones go
 * where the game puts them (or by their Velocity) and push dynamic ones.
 * The physics step owns these entities: orst_update_motion skips them, and
 * orst_update_collisions reports them but does not push them. */
#define ORST_BODY_DYNAMIC 0
#define ORST_BODY_FIXED 1
#define ORST_BODY_KINEMATIC 2
typedef struct OrstRigidBody {
    uint32_t kind;       /* ORST_BODY_* */
    float density;       /* mass per square unit, default 1 */
    float friction;      /* default 0.5 */
    float restitution;   /* bounciness 0..1 */
    float linear_damping;
    float angular_damping;
    float gravity_scale; /* default 1 */
    uint8_t fixed_rotation; /* never turns (characters) */
    uint8_t bullet;         /* fast: does not pass through thin walls */
    uint8_t pad_[2];
} OrstRigidBody;

/* A light at the entity's position, seen by targets (or the screen) with
 * the lighting effect. Colliders on the shadow layers and solid cells of
 * shadow maps cast shadows behind them. */
typedef struct OrstLight {
    OrstVec2 offset;    /* from the position */
    OrstColor color;    /* default white */
    float intensity;  /* default 1 */
    float radius;     /* world units it reaches, default 160 */
    float height;     /* above the ground, share of the radius (0.5): low lights bring out normal maps */
    float softness;   /* shadow edge blur in pixels, default 4 (0: hard) */
    uint8_t shadows;  /* default 1 */
    uint8_t pad_[3];
} OrstLight;

/* What an entity is called: the editor's hierarchy shows it and games find
 * entities by it (orst_find_named). UTF-8, the unused bytes zero. */
typedef struct OrstName {
    char text[64];
} OrstName;

/* How two shapes overlap: move the second by normal * depth and they only touch */
typedef struct OrstOverlap {
    OrstVec2 normal; /* from the first towards the second */
    float depth;
} OrstOverlap;

void orst_update_motion(OrstEngine *e, float dt); /* dt: orst_tick_dt in fixed ticks */
void orst_draw_entities(OrstEngine *e);
uint32_t orst_create_polygon(OrstEngine *e, const OrstVec2 *points, uint32_t count); /* convex, 3..64 corners; 0 on error */
void orst_free_polygon(OrstEngine *e, uint32_t polygon);
bool orst_entity_overlap(OrstEngine *e, OrstEntity a, OrstEntity b, OrstOverlap *out); /* layers aside */
void orst_debug_colliders(OrstEngine *e); /* outlines in the debug overlay */

/* Physics (Rapier, deterministic): call orst_physics_step once per tick.
 * Moving the Transform or setting the Velocity moves a body; after the
 * step its place and speed are written back. Impulses and joints work on
 * new entities right away. Built without physics: false, 0, orst_last_error. */
void orst_physics_step(OrstEngine *e, float dt);
void orst_set_gravity(OrstEngine *e, OrstVec2 gravity); /* units/s^2, y down; default none (top-down) */
void orst_set_physics_scale(OrstEngine *e, float units_per_metre); /* default 32 */
bool orst_apply_impulse(OrstEngine *e, OrstEntity entity, OrstVec2 impulse);
bool orst_apply_force(OrstEngine *e, OrstEntity entity, OrstVec2 force); /* during the next step */
bool orst_apply_torque_impulse(OrstEngine *e, OrstEntity entity, float torque);
float orst_body_mass(OrstEngine *e, OrstEntity entity);
uint32_t orst_joint_pin(OrstEngine *e, OrstEntity a, OrstEntity b, OrstVec2 at); /* hinge at a world point */
uint32_t orst_joint_weld(OrstEngine *e, OrstEntity a, OrstEntity b);           /* glued as they are now */
uint32_t orst_joint_rope(OrstEngine *e, OrstEntity a, OrstEntity b, float length); /* at most this far apart */
uint32_t orst_joint_spring(OrstEngine *e, OrstEntity a, OrstEntity b, float rest, float stiffness, float damping);
bool orst_remove_joint(OrstEngine *e, uint32_t joint);

/* The collision pass: call it after orst_update_motion, once per tick. A grid
 * finds the pairs near each other; solid colliders (not triggers) with a
 * Velocity are pushed out of the others and stop moving into them, ones
 * without stay put like walls. The contacts list every overlapping pair
 * (lower id first, normal from a to b), then the pairs that stopped. */
#define ORST_CONTACT_BEGIN 0
#define ORST_CONTACT_STAY 1
#define ORST_CONTACT_END 2 /* or one of them is gone; no normal or depth */
typedef struct OrstContact {
    OrstEntity a, b;
    OrstVec2 normal;
    float depth;
    int32_t state;   /* ORST_CONTACT_* */
    uint8_t trigger; /* either one is a trigger */
    uint8_t pad_[3];
} OrstContact;
typedef struct OrstEntityHit {
    OrstEntity entity;
    OrstVec2 point;
    OrstVec2 normal;
    float distance;
} OrstEntityHit;
void orst_update_collisions(OrstEngine *e);
void orst_set_collision_cell(OrstEngine *e, float size); /* grid cell, default 64: about a typical collider */
uint32_t orst_contact_count(OrstEngine *e);
bool orst_contact(OrstEngine *e, uint32_t index, OrstContact *out);
/* Entities whose collider has a layer bit in mask and overlaps the point,
 * box or circle; fills up to capacity and returns how many there are */
uint32_t orst_query_point(OrstEngine *e, OrstVec2 point, uint32_t mask, OrstEntity *out, uint32_t capacity);
uint32_t orst_query_rect(OrstEngine *e, OrstRect rect, uint32_t mask, OrstEntity *out, uint32_t capacity);
uint32_t orst_query_circle(OrstEngine *e, OrstVec2 center, float radius, uint32_t mask, OrstEntity *out, uint32_t capacity);
bool orst_raycast_entities(OrstEngine *e, OrstVec2 from, OrstVec2 to, uint32_t mask, OrstEntityHit *out); /* the nearest */

/* Snapshots: the entities and components, the random generator and the
 * tick counter as bytes. Restoring one continues the game exactly from
 * there: saves, rollback, replays. Assets (textures, maps, sounds) are not
 * in it. orst_snapshot returns the size and copies the bytes only when they
 * fit, so call it with NULL first to learn the size. */
size_t orst_snapshot(OrstEngine *e, uint8_t *buf, size_t capacity);
bool orst_restore(OrstEngine *e, const uint8_t *data, size_t size); /* false + orst_last_error: nothing changed */
/* Fingerprint of the snapshot: games running in step have equal hashes */
uint64_t orst_state_hash(OrstEngine *e);

/* --- scenes ---
 * A scene is a number the game picks (0 = none): menu, level, pause. The
 * engine keeps a stack of them and covers each switch with a transition;
 * the switch happens in its middle, when the screen is covered, and
 * orst_scene_entered returns the new scene for that one frame:
 *     if (orst_scene_entered(e) == LEVEL) load_level();
 * A change while another runs is refused (false). */
#define ORST_TRANSITION_CUT 0  /* at the next frame */
#define ORST_TRANSITION_FADE 1 /* to the colour and back */
#define ORST_TRANSITION_WIPE 2 /* the colour sweeps across */
bool orst_scene_go(OrstEngine *e, uint32_t scene, int32_t transition, float seconds, OrstColor color);
bool orst_scene_push(OrstEngine *e, uint32_t scene, int32_t transition, float seconds, OrstColor color); /* over the current one */
bool orst_scene_pop(OrstEngine *e, int32_t transition, float seconds, OrstColor color); /* false: nothing below */
uint32_t orst_scene(OrstEngine *e);         /* the scene on top */
uint32_t orst_scene_entered(OrstEngine *e); /* became current this frame (after a pop: resumed) */
uint32_t orst_scene_left(OrstEngine *e);    /* stopped being current this frame */
uint32_t orst_scene_depth(OrstEngine *e);
bool orst_scene_transitioning(OrstEngine *e);
float orst_scene_cover(OrstEngine *e); /* how covered the screen is, 0..1 */

/* --- textures and fonts --- */
uint32_t orst_load_texture(OrstEngine *e, const char *path); /* PNG, JPEG */
uint32_t orst_texture_from_rgba(OrstEngine *e, uint32_t width, uint32_t height, const uint8_t *rgba);
bool orst_texture_size(OrstEngine *e, uint32_t texture, uint32_t *w, uint32_t *h);
void orst_set_texture_smooth(OrstEngine *e, uint32_t texture, bool smooth);
void orst_free_texture(OrstEngine *e, uint32_t texture);
uint32_t orst_load_font(OrstEngine *e, const char *path); /* TTF, OTF */
void orst_free_font(OrstEngine *e, uint32_t font);
OrstVec2 orst_measure_text(OrstEngine *e, uint32_t font, const char *text, float size);

/* --- render targets, post effects and shake ---
 * A render target is a texture draw calls can go into: draw into it after
 * orst_set_target(e, tex), go back with orst_set_target(e, 0), and draw it
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
#define ORST_EFFECT_GLOW 1   /* p: threshold 0..1, strength, radius px (1..32) */
#define ORST_EFFECT_BLUR 2   /* p: radius px (1..32) */
#define ORST_EFFECT_GRADE 3  /* p: brightness (+-), contrast (1), saturation (1), gamma (1), tint r, g, b (1) */
#define ORST_EFFECT_CRT 4    /* p: scanlines 0..1, curvature 0..0.5, vignette 0..1, aberration px */
#define ORST_EFFECT_SHADER 5 /* shader from orst_create_shader; p goes to post.p */
#define ORST_EFFECT_LIGHTING 6 /* p: ambient r, g, b (0..1, more is brighter); lights are OrstLight entities */
typedef struct OrstEffect {
    uint32_t kind;
    uint32_t shader;
    float p[8];
} OrstEffect;
uint32_t orst_create_target(OrstEngine *e, uint32_t width, uint32_t height); /* a texture id; 0 on error */
bool orst_set_target(OrstEngine *e, uint32_t target); /* 0: the screen; false: not a target */
uint32_t orst_get_target(OrstEngine *e);
void orst_clear_target(OrstEngine *e, uint32_t target, OrstColor color); /* this frame, before its draw calls */
void orst_set_target_camera(OrstEngine *e, uint32_t target, const OrstCamera *camera); /* NULL: the main camera */
bool orst_set_effects(OrstEngine *e, uint32_t target, const OrstEffect *effects, uint32_t count);
uint32_t orst_create_shader(OrstEngine *e, const char *wgsl); /* 0 and orst_last_error on a compile error */
void orst_free_shader(OrstEngine *e, uint32_t shader);
void orst_shake(OrstEngine *e, float strength, float duration); /* the world on screen, pixels; fades out */
/* Lighting: a normal map (same size, red right, green up) gives a texture
 * relief; colliders on these layers and solid cells of these maps cast
 * shadows. */
void orst_set_normal_map(OrstEngine *e, uint32_t texture, uint32_t normal_map); /* 0: flat */
void orst_set_shadow_layers(OrstEngine *e, uint32_t layers);
void orst_set_shadow_tilemap(OrstEngine *e, uint32_t map, bool on);

/* --- camera and draw state --- */
OrstCamera orst_get_camera(OrstEngine *e);
void orst_set_camera(OrstEngine *e, OrstCamera camera);
OrstVec2 orst_world_to_screen(OrstEngine *e, OrstVec2 p);
OrstVec2 orst_screen_to_world(OrstEngine *e, OrstVec2 p);
void orst_set_layer(OrstEngine *e, int32_t layer);  /* for shapes and text */
void orst_set_depth(OrstEngine *e, float depth);
void orst_set_space(OrstEngine *e, int32_t space);  /* OrstSpace */
void orst_set_antialias(OrstEngine *e, bool on);

/* --- drawing --- */
void orst_draw_sprite(OrstEngine *e, uint32_t texture, const OrstSprite *sprite);
void orst_draw_texture(OrstEngine *e, uint32_t texture, float x, float y);
void orst_draw_frame(OrstEngine *e, uint32_t texture, const OrstSpriteSheet *sheet, uint32_t frame, const OrstSprite *sprite);
void orst_fill_rect(OrstEngine *e, OrstRect rect, OrstColor color);
void orst_draw_rect(OrstEngine *e, OrstRect rect, float thickness, OrstColor color);
void orst_draw_line(OrstEngine *e, OrstVec2 from, OrstVec2 to, float thickness, OrstColor color);
void orst_fill_circle(OrstEngine *e, OrstVec2 center, float radius, OrstColor color);
void orst_draw_circle(OrstEngine *e, OrstVec2 center, float radius, float thickness, OrstColor color);
void orst_draw_text(OrstEngine *e, uint32_t font, const char *text, OrstVec2 pos, float size, OrstColor color);

/* --- input --- */
bool orst_key_down(OrstEngine *e, int32_t key);
bool orst_key_pressed(OrstEngine *e, int32_t key);  /* this frame only */
bool orst_key_released(OrstEngine *e, int32_t key);
float orst_axis(OrstEngine *e, int32_t negative, int32_t positive); /* -1, 0, 1 */
bool orst_mouse_down(OrstEngine *e, int32_t button);
bool orst_mouse_pressed(OrstEngine *e, int32_t button);
bool orst_mouse_released(OrstEngine *e, int32_t button);
OrstVec2 orst_mouse_pos(OrstEngine *e);   /* canvas pixels */
OrstVec2 orst_mouse_world(OrstEngine *e);
float orst_mouse_wheel(OrstEngine *e);
const char *orst_text_input(OrstEngine *e); /* typed this frame; valid until the next call */
void orst_inject_key(OrstEngine *e, int32_t key, bool down); /* applied at the next poll */
void orst_inject_mouse_button(OrstEngine *e, int32_t button, bool down);
void orst_inject_mouse_move(OrstEngine *e, OrstVec2 pos);

/* --- input actions ---
 * The game asks for "jump", not Space, and players rebind it. Pressed and
 * released are for the action as a whole: two keys on "jump" press once.
 *     uint32_t jump = orst_action(e, "jump");
 *     orst_bind(e, jump, (OrstBinding){ORST_BIND_KEY, ORST_KEY_SPACE, -1, 0});
 *     if (orst_action_pressed(e, jump)) ...
 * A rebinding menu waits for orst_captured_binding (the first input pressed
 * this frame) and stores it with orst_set_binding. orst_bindings_save gives a
 * text players can edit ("jump = key:Space pad:South"). */
#define ORST_BIND_KEY 1
#define ORST_BIND_MOUSE 2
#define ORST_BIND_PAD_BUTTON 3
#define ORST_BIND_PAD_AXIS 4
typedef struct OrstBinding {
    int32_t kind;      /* ORST_BIND_* */
    int32_t code;      /* key, mouse button, pad button or pad axis */
    int32_t pad;       /* pad slot, -1: any pad */
    int32_t direction; /* pad axis: 1 or -1 */
} OrstBinding;
uint32_t orst_action(OrstEngine *e, const char *name); /* id from 1, made on first use */
bool orst_bind(OrstEngine *e, uint32_t action, OrstBinding binding); /* added once */
bool orst_set_binding(OrstEngine *e, uint32_t action, uint32_t index, OrstBinding binding); /* index = count: add */
bool orst_remove_binding(OrstEngine *e, uint32_t action, uint32_t index);
void orst_clear_bindings(OrstEngine *e, uint32_t action);
uint32_t orst_binding_count(OrstEngine *e, uint32_t action);
bool orst_get_binding(OrstEngine *e, uint32_t action, uint32_t index, OrstBinding *out);
size_t orst_binding_text(OrstEngine *e, OrstBinding binding, char *buf, size_t size); /* "key:Space" */
bool orst_action_down(OrstEngine *e, uint32_t action);
bool orst_action_pressed(OrstEngine *e, uint32_t action);  /* this frame only */
bool orst_action_released(OrstEngine *e, uint32_t action);
float orst_action_value(OrstEngine *e, uint32_t action);   /* 0..1, sticks give how far */
float orst_action_axis(OrstEngine *e, uint32_t negative, uint32_t positive); /* -1..1 */
bool orst_captured_binding(OrstEngine *e, OrstBinding *out); /* the first input pressed this frame */
size_t orst_bindings_save(OrstEngine *e, char *buf, size_t size);
/* Actions in the text get exactly its bindings, others keep theirs;
 * false + orst_last_error on a bad line, and nothing changes */
bool orst_bindings_load(OrstEngine *e, const char *text);

/* --- gamepads: up to 4, in slots 0..3 that a controller keeps until unplugged --- */
bool orst_gamepad_connected(OrstEngine *e, int32_t gamepad);
size_t orst_gamepad_name(OrstEngine *e, int32_t gamepad, char *buf, size_t size);
bool orst_gamepad_down(OrstEngine *e, int32_t gamepad, int32_t button);
bool orst_gamepad_pressed(OrstEngine *e, int32_t gamepad, int32_t button);  /* this frame only */
bool orst_gamepad_released(OrstEngine *e, int32_t gamepad, int32_t button);
float orst_gamepad_axis(OrstEngine *e, int32_t gamepad, int32_t axis);    /* after the dead zone */
void orst_set_gamepad_deadzone(OrstEngine *e, float deadzone);             /* default 0.15 */
void orst_inject_gamepad_connected(OrstEngine *e, int32_t gamepad, bool connected);
void orst_inject_gamepad_button(OrstEngine *e, int32_t gamepad, int32_t button, bool down);
void orst_inject_gamepad_axis(OrstEngine *e, int32_t gamepad, int32_t axis, float value); /* raw */

/* --- sprite sheets and animations --- */
OrstSpriteSheet orst_sheet_grid(uint32_t texture_w, uint32_t texture_h, uint32_t frame_w, uint32_t frame_h,
                            uint32_t margin, uint32_t spacing);
OrstRect orst_sheet_frame(OrstSpriteSheet sheet, uint32_t index);
uint32_t orst_anim_frame_at(const uint32_t *frames, uint32_t count, float fps, int32_t mode, float time);

/* --- tweens and easing --- */
/* Eased progress for t in 0..1 (clamped); ease is an OrstEase */
float orst_ease(int32_t ease, float t);
/* Eased progress 0..1 of a tween `time` seconds after its start; a negative
   time is the delay before it. repeat is an OrstRepeat */
float orst_tween_progress(float time, float duration, int32_t ease, int32_t repeat);
OrstColor orst_lerp_color(OrstColor a, OrstColor b, float t);

/* "gpu: <graphics card>" or "software" */
size_t orst_renderer_name(OrstEngine *e, char *buf, size_t size);

/* --- hot reload: textures and maps reload in place when their files change --- */
void orst_set_hot_reload(OrstEngine *e, bool on); /* checked in orst_poll twice a second */
bool orst_hot_reload_enabled(OrstEngine *e);
uint32_t orst_reload_changed(OrstEngine *e);      /* check now; returns how many reloaded */
uint32_t orst_reloaded(OrstEngine *e);            /* reloaded by the last orst_poll */

/* --- tile maps --- */
uint32_t orst_load_tilemap(OrstEngine *e, const char *path); /* Tiled .tmj */
uint32_t orst_create_tilemap(OrstEngine *e, int32_t orientation, uint32_t width, uint32_t height,
                           uint32_t tile_w, uint32_t tile_h, uint32_t layers);
bool orst_tilemap_add_tileset(OrstEngine *e, uint32_t map, uint32_t first_gid, uint32_t texture, OrstSpriteSheet sheet);
bool orst_tilemap_set_tile_flag(OrstEngine *e, uint32_t map, uint32_t gid, const char *name, bool value);
void orst_free_tilemap(OrstEngine *e, uint32_t map);
bool orst_tilemap_info(OrstEngine *e, uint32_t map, OrstTilemapInfo *info);
int32_t orst_tilemap_layer_index(OrstEngine *e, uint32_t map, const char *name); /* -1: none */
uint32_t orst_tilemap_get_tile(OrstEngine *e, uint32_t map, uint32_t layer, int32_t x, int32_t y);
void orst_tilemap_set_tile(OrstEngine *e, uint32_t map, uint32_t layer, int32_t x, int32_t y, uint32_t gid);
bool orst_tilemap_is_solid(OrstEngine *e, uint32_t map, int32_t x, int32_t y);
OrstVec2 orst_tilemap_tile_to_world(OrstEngine *e, uint32_t map, OrstVec2 tile);
OrstVec2 orst_tilemap_world_to_tile(OrstEngine *e, uint32_t map, OrstVec2 p);
OrstVec2 orst_tilemap_cell_center(OrstEngine *e, uint32_t map, int32_t x, int32_t y);
void orst_draw_tilemap(OrstEngine *e, uint32_t map, int32_t base_layer);
void orst_draw_tilemap_layer(OrstEngine *e, uint32_t map, uint32_t layer, int32_t draw_layer, float depth,
                           bool y_sort, OrstColor tint);
bool orst_tilemap_object(OrstEngine *e, uint32_t map, uint32_t index, OrstMapObject *out);
int32_t orst_tilemap_find_object(OrstEngine *e, uint32_t map, const char *name); /* -1: none */
/* Copy strings into buf; the return value is the full length (pass size 0 to ask) */
size_t orst_tilemap_object_name(OrstEngine *e, uint32_t map, uint32_t index, char *buf, size_t size);
size_t orst_tilemap_object_class(OrstEngine *e, uint32_t map, uint32_t index, char *buf, size_t size);
double orst_tilemap_object_number(OrstEngine *e, uint32_t map, uint32_t index, const char *name, double fallback);
int64_t orst_tilemap_object_string(OrstEngine *e, uint32_t map, uint32_t index, const char *name, char *buf,
                                 size_t size); /* -1: missing */

/* --- collisions --- */
OrstMoveResult orst_move_box(OrstEngine *e, uint32_t map, OrstRect rect, OrstVec2 delta);
OrstContacts orst_touching(OrstEngine *e, uint32_t map, OrstRect rect);
bool orst_raycast(OrstEngine *e, uint32_t map, OrstVec2 from, OrstVec2 to, OrstRayHit *hit);
/* A path around solid cells (A*): cell centres from the cell of `from` to
 * the cell of `to`. A number property `cost` on a tile or layer makes it
 * slower to cross; diagonal steps never cut a wall's corner. max_cost stops
 * early (0: no limit). Fills up to capacity, returns the length (0: no way). */
uint32_t orst_find_path(OrstEngine *e, uint32_t map, OrstVec2 from, OrstVec2 to, bool diagonal, float max_cost,
                      OrstVec2 *out, uint32_t capacity);
bool orst_point_solid(OrstEngine *e, uint32_t map, OrstVec2 p);
bool orst_rect_solid(OrstEngine *e, uint32_t map, OrstRect rect);
bool orst_rects_overlap(OrstRect a, OrstRect b);

/* --- debug overlay: drawn on top only while on; F3 toggles it --- */
void orst_set_debug(OrstEngine *e, bool on);
bool orst_debug_enabled(OrstEngine *e);
void orst_set_debug_key(OrstEngine *e, int32_t key); /* 0: no toggle key */
void orst_debug_rect(OrstEngine *e, OrstRect rect, OrstColor color);   /* in the current space */
void orst_debug_line(OrstEngine *e, OrstVec2 from, OrstVec2 to, OrstColor color);
void orst_debug_circle(OrstEngine *e, OrstVec2 center, float radius, OrstColor color);
void orst_debug_text(OrstEngine *e, const char *text, OrstVec2 pos, OrstColor color);
void orst_debug_tilemap(OrstEngine *e, uint32_t map); /* solid cells, platforms, objects */

/* --- particles --- */
OrstEmitter orst_default_emitter(void);
/* config NULL: the defaults. The emitter spawns at its rate right away */
uint32_t orst_create_emitter(OrstEngine *e, const OrstEmitter *config, OrstVec2 pos);
void orst_free_emitter(OrstEngine *e, uint32_t emitter);
OrstEmitter orst_emitter_config(OrstEngine *e, uint32_t emitter);
/* Living particles keep flying; a new seed restarts the random sequence */
bool orst_set_emitter_config(OrstEngine *e, uint32_t emitter, const OrstEmitter *config);
void orst_set_emitter_pos(OrstEngine *e, uint32_t emitter, OrstVec2 pos);
OrstVec2 orst_emitter_pos(OrstEngine *e, uint32_t emitter);
void orst_set_emitter_emitting(OrstEngine *e, uint32_t emitter, bool on);
bool orst_emitter_emitting(OrstEngine *e, uint32_t emitter);
void orst_emitter_burst(OrstEngine *e, uint32_t emitter, uint32_t count);
uint32_t orst_emitter_count(OrstEngine *e, uint32_t emitter);
void orst_emitter_clear(OrstEngine *e, uint32_t emitter);
void orst_update_emitter(OrstEngine *e, uint32_t emitter, float dt);
void orst_update_emitters(OrstEngine *e, float dt);
/* On the config's layer, at the current depth and space */
void orst_draw_emitter(OrstEngine *e, uint32_t emitter);

/* --- GPU emitters ---
 * The same configs as emitters, for huge numbers of particles: only each
 * particle's birth is recorded, and the graphics card works out where it
 * is now (the software renderer does the same maths). Drag follows the
 * exact curve. Particles can be counted but not read one by one. A new
 * max drops the living particles. */
uint32_t orst_create_gpu_emitter(OrstEngine *e, const OrstEmitter *config, OrstVec2 pos);
void orst_free_gpu_emitter(OrstEngine *e, uint32_t emitter);
OrstEmitter orst_gpu_emitter_config(OrstEngine *e, uint32_t emitter);
bool orst_set_gpu_emitter_config(OrstEngine *e, uint32_t emitter, const OrstEmitter *config);
void orst_set_gpu_emitter_pos(OrstEngine *e, uint32_t emitter, OrstVec2 pos);
OrstVec2 orst_gpu_emitter_pos(OrstEngine *e, uint32_t emitter);
void orst_set_gpu_emitter_emitting(OrstEngine *e, uint32_t emitter, bool on);
void orst_gpu_emitter_burst(OrstEngine *e, uint32_t emitter, uint32_t count);
uint32_t orst_gpu_emitter_count(OrstEngine *e, uint32_t emitter);
void orst_gpu_emitter_clear(OrstEngine *e, uint32_t emitter);
void orst_update_gpu_emitters(OrstEngine *e, float dt);
/* On the config's layer, at the current depth and space */
void orst_draw_gpu_emitter(OrstEngine *e, uint32_t emitter);

/* --- skeletons ---
 * A skeleton file (JSON) has bones, slots with attachments (regions of
 * one atlas texture) and animations; angles are degrees, clockwise.
 * See src/skeleton.rs for the format. Data is shared: many skeletons can
 * be made from one file. At runtime angles are radians. */
uint32_t orst_load_skeleton(OrstEngine *e, const char *path);
uint32_t orst_skeleton_from_json(OrstEngine *e, const char *json);
/* Skeletons made from it keep working */
void orst_free_skeleton_data(OrstEngine *e, uint32_t data);
/* In the setup pose */
uint32_t orst_create_skeleton(OrstEngine *e, uint32_t data);
void orst_free_skeleton(OrstEngine *e, uint32_t skeleton);
/* Fades from the current animation over mix seconds; the one already
 * playing keeps going */
bool orst_skeleton_play(OrstEngine *e, uint32_t skeleton, const char *name, bool looped, float mix);
void orst_skeleton_stop(OrstEngine *e, uint32_t skeleton);
/* The current animation's name ("" for none); returns its full length */
size_t orst_skeleton_animation(OrstEngine *e, uint32_t skeleton, char *buf, size_t size);
/* A non-looping animation reached its end */
bool orst_skeleton_done(OrstEngine *e, uint32_t skeleton);
/* The event was passed in the last update */
bool orst_skeleton_fired(OrstEngine *e, uint32_t skeleton, const char *event);
void orst_update_skeletons(OrstEngine *e, float dt);
/* Where the skeleton stands */
void orst_set_skeleton_root(OrstEngine *e, uint32_t skeleton, OrstTransform root);
void orst_set_skeleton_tint(OrstEngine *e, uint32_t skeleton, OrstColor tint);
void orst_set_skeleton_speed(OrstEngine *e, uint32_t skeleton, float speed);
/* Attachments as sprites of texture (the atlas), at the current layer,
 * depth and space */
void orst_draw_skeleton(OrstEngine *e, uint32_t skeleton, uint32_t texture);
/* A bone's index, -1 if none */
int32_t orst_skeleton_bone(OrstEngine *e, uint32_t skeleton, const char *name);
OrstTransform orst_skeleton_bone_world(OrstEngine *e, uint32_t skeleton, int32_t bone);
/* A point in a bone's own space, in the world (a muzzle's tip) */
OrstVec2 orst_skeleton_bone_point(OrstEngine *e, uint32_t skeleton, int32_t bone, OrstVec2 local);
/* Turn a bone by code (radians, relative to its parent), blended over the
 * animation by weight; 0 gives it back */
void orst_set_bone_override(OrstEngine *e, uint32_t skeleton, int32_t bone, float rotation, float weight);
/* attachment NULL hides the slot */
bool orst_skeleton_set_attachment(OrstEngine *e, uint32_t skeleton, const char *slot, const char *attachment);

/* --- sound (WAV, OGG, MP3) --- */
uint32_t orst_load_sound(OrstEngine *e, const char *path);
void orst_free_sound(OrstEngine *e, uint32_t sound);
uint32_t orst_play_sound(OrstEngine *e, uint32_t sound, float volume, float pitch); /* voice id */
void orst_stop_voice(OrstEngine *e, uint32_t voice);
bool orst_voice_playing(OrstEngine *e, uint32_t voice);
void orst_stop_all_sounds(OrstEngine *e);
bool orst_play_music(OrstEngine *e, const char *path, float volume, bool looped);
void orst_stop_music(OrstEngine *e);
void orst_pause_music(OrstEngine *e, bool paused);
bool orst_music_playing(OrstEngine *e);
void orst_set_music_volume(OrstEngine *e, float volume);
void orst_set_master_volume(OrstEngine *e, float volume);

#ifdef __cplusplus
}
#endif

#endif /* ORST_H */

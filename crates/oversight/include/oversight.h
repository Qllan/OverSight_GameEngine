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
} OvConfig;

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
    OvConfig c = {"OverSight", 960, 540, 1.0f, true, false, false, 60, false};
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

/* --- textures and fonts --- */
uint32_t ov_load_texture(OvEngine *e, const char *path); /* PNG, JPEG */
uint32_t ov_texture_from_rgba(OvEngine *e, uint32_t width, uint32_t height, const uint8_t *rgba);
bool ov_texture_size(OvEngine *e, uint32_t texture, uint32_t *w, uint32_t *h);
void ov_set_texture_smooth(OvEngine *e, uint32_t texture, bool smooth);
void ov_free_texture(OvEngine *e, uint32_t texture);
uint32_t ov_load_font(OvEngine *e, const char *path); /* TTF, OTF */
void ov_free_font(OvEngine *e, uint32_t font);
OvVec2 ov_measure_text(OvEngine *e, uint32_t font, const char *text, float size);

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

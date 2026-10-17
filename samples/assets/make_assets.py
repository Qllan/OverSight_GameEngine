"""Draws the sample tilesets and writes the sample Tiled maps.

Run from anywhere: python samples/assets/make_assets.py
Only the standard library is used (PNG via zlib), so the pictures are
simple procedural pixel art; open the .tmj files in Tiled to edit maps.
"""
import json
import os
import random
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))


def png(path, w, h, px):
    raw = b"".join(b"\0" + bytes(c for p in px[y * w:(y + 1) * w] for c in p) for y in range(h))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    with open(os.path.join(HERE, path), "wb") as f:
        f.write(data)


class Image:
    def __init__(self, w, h):
        self.w, self.h = w, h
        self.px = [(0, 0, 0, 0)] * (w * h)

    def put(self, x, y, c):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[y * self.w + x] = c if len(c) == 4 else c + (255,)

    def rect(self, x, y, w, h, c):
        for yy in range(y, y + h):
            for xx in range(x, x + w):
                self.put(xx, yy, c)

    def save(self, path):
        png(path, self.w, self.h, self.px)


def shade(c, k):
    return tuple(max(0, min(255, int(v * k))) for v in c[:3])


# --- orthogonal tiles: 16x16, 4 columns -------------------------------------
# 0 grass, 1 stone wall (solid), 2 water (solid), 3 path,
# 4 plank platform (one-way), 5 crate (solid), 6 flower, 7 goal flag
rng = random.Random(7)
tiles = Image(64, 32)


def tile(i):
    return (i % 4) * 16, (i // 4) * 16


GRASS = (70, 140, 60)
x, y = tile(0)
tiles.rect(x, y, 16, 16, GRASS)
for _ in range(18):
    tiles.put(x + rng.randrange(16), y + rng.randrange(16), shade(GRASS, rng.choice([0.8, 1.2])))

STONE = (120, 120, 130)
x, y = tile(1)
tiles.rect(x, y, 16, 16, shade(STONE, 0.6))
for row in range(4):
    off = 0 if row % 2 == 0 else 4
    for col in range(-1, 3):
        bx = x + col * 8 + off
        for yy in range(y + row * 4, y + row * 4 + 3):
            for xx in range(max(bx, x), min(bx + 7, x + 16)):
                tiles.put(xx, yy, shade(STONE, 1.0 + 0.1 * ((col + row) % 2)))

WATER = (50, 90, 170)
x, y = tile(2)
tiles.rect(x, y, 16, 16, WATER)
for yy in (3, 9, 14):
    for xx in range(2, 14, 4):
        tiles.rect(x + xx + (yy % 3), y + yy, 3, 1, shade(WATER, 1.4))

PATH = (170, 140, 95)
x, y = tile(3)
tiles.rect(x, y, 16, 16, PATH)
for _ in range(14):
    tiles.put(x + rng.randrange(16), y + rng.randrange(16), shade(PATH, rng.choice([0.85, 1.1])))

WOOD = (140, 90, 45)
x, y = tile(4)
tiles.rect(x, y, 16, 5, WOOD)
tiles.rect(x, y, 16, 1, shade(WOOD, 1.3))
tiles.rect(x, y + 4, 16, 1, shade(WOOD, 0.6))
for xx in (3, 12):
    tiles.rect(x + xx, y + 5, 1, 4, shade(WOOD, 0.7))

x, y = tile(5)
tiles.rect(x + 1, y + 1, 14, 14, WOOD)
tiles.rect(x + 1, y + 1, 14, 2, shade(WOOD, 1.3))
tiles.rect(x + 1, y + 13, 14, 2, shade(WOOD, 0.6))
for i in range(12):
    tiles.put(x + 2 + i, y + 2 + i, shade(WOOD, 0.7))

x, y = tile(6)
tiles.rect(x, y, 16, 16, GRASS)
for fx, fy, c in ((4, 5, (230, 80, 90)), (11, 9, (240, 220, 80)), (6, 12, (220, 120, 230))):
    tiles.rect(x + fx - 1, y + fy, 3, 1, c)
    tiles.rect(x + fx, y + fy - 1, 1, 3, c)
    tiles.put(x + fx, y + fy, (255, 255, 255))

x, y = tile(7)
tiles.rect(x + 3, y + 1, 1, 15, (200, 200, 200))
tiles.rect(x + 4, y + 1, 9, 6, (230, 60, 60))
tiles.save("tiles.png")

# --- hero: 16x16 frames, 4 per row; rows: down, up, left/right ----------------
hero = Image(64, 48)
SKIN, SHIRT, PANTS = (240, 200, 160), (60, 110, 200), (50, 50, 70)
for row in range(3):
    for f in range(4):
        ox, oy = f * 16, row * 16
        step = (0, 1, 0, -1)[f]
        hero.rect(ox + 5, oy + 2, 6, 5, SKIN)          # head
        if row == 0:
            hero.put(ox + 6, oy + 4, (20, 20, 20))
            hero.put(ox + 9, oy + 4, (20, 20, 20))
        elif row == 2:
            hero.put(ox + 9, oy + 4, (20, 20, 20))
        hero.rect(ox + 5, oy + 2, 6, 1, (90, 60, 30))  # hair
        hero.rect(ox + 4, oy + 7, 8, 5, SHIRT)         # body
        hero.rect(ox + 5, oy + 12, 2, 3 + max(0, step), PANTS)
        hero.rect(ox + 9, oy + 12, 2, 3 + max(0, -step), PANTS)
hero.save("hero.png")

# --- isometric tiles: 32x32 images on a 32x16 diamond grid ---------------------
# 0 grass floor, 1 stone block (solid), 2 water (solid)
iso = Image(96, 32)


def diamond(ox, top, c, edge):
    for yy in range(16):
        half = (yy + 1) * 2 if yy < 8 else (16 - yy) * 2
        for xx in range(16 - half, 16 + half):
            iso.put(ox + xx, top + yy, edge if xx in (16 - half, 16 + half - 1) else c)


diamond(0, 16, GRASS, shade(GRASS, 0.8))
diamond(64, 16, WATER, shade(WATER, 1.3))
# Block: a bottom diamond, the sides between, then the top diamond
diamond(32, 16, shade(STONE, 0.55), shade(STONE, 0.45))
for yy in range(8, 24):
    for xx in range(32):
        iso.put(32 + xx, yy, shade(STONE, 0.75 if xx < 16 else 0.55))
diamond(32, 0, STONE, shade(STONE, 1.2))
iso.save("iso_tiles.png")


# --- maps -----------------------------------------------------------------------
def tileset(name, image, w, h, tw, th, columns, count, props):
    return {
        "firstgid": 1, "name": name, "image": image, "imagewidth": w, "imageheight": h,
        "tilewidth": tw, "tileheight": th, "columns": columns, "tilecount": count,
        "margin": 0, "spacing": 0,
        "tiles": [{"id": i, "properties": [{"name": p, "type": "bool", "value": True}]}
                  for i, p in props.items()],
    }


def layer(name, w, h, data, **extra):
    d = {"type": "tilelayer", "name": name, "width": w, "height": h, "data": data,
         "opacity": 1, "visible": True, "x": 0, "y": 0}
    d.update(extra)
    return d


def objects(name, objs):
    return {"type": "objectgroup", "name": name, "objects": [
        dict({"id": i + 1, "width": 0, "height": 0, "rotation": 0, "visible": True}, **o)
        for i, o in enumerate(objs)]}


def save_map(path, orientation, w, h, tw, th, tilesets, layers):
    m = {"type": "map", "version": "1.10", "tiledversion": "1.11.0", "orientation": orientation,
         "renderorder": "right-down", "infinite": False, "width": w, "height": h,
         "tilewidth": tw, "tileheight": th, "nextlayerid": len(layers) + 1, "nextobjectid": 100,
         "tilesets": tilesets, "layers": layers}
    with open(os.path.join(HERE, path), "w", encoding="utf-8") as f:
        json.dump(m, f, indent=1)


ORTHO_SET = tileset("tiles", "tiles.png", 64, 32, 16, 16, 4, 8,
                    {1: "solid", 2: "solid", 5: "solid", 4: "platform"})
G, WALL, WATER_T, PATH_T, PLANK, CRATE, FLOWER, FLAG = range(1, 9)

# Top-down: a walled garden with a pond, paths and crates
W, H = 30, 20
ground = [G] * (W * H)
walls = [0] * (W * H)
for yy in range(H):
    for xx in range(W):
        i = yy * W + xx
        if rng.random() < 0.06:
            ground[i] = FLOWER
        if yy == 10 or xx == 8:
            ground[i] = PATH_T
        if 18 <= xx <= 24 and 3 <= yy <= 7:
            ground[i] = WATER_T
        if xx in (0, W - 1) or yy in (0, H - 1):
            walls[i] = WALL
for xx in range(12, 17):
    walls[14 * W + xx] = WALL
for xx, yy in ((5, 4), (6, 4), (5, 5), (22, 14), (23, 15), (14, 3)):
    walls[yy * W + xx] = CRATE
save_map("topdown.tmj", "orthogonal", W, H, 16, 16, [ORTHO_SET], [
    layer("ground", W, H, ground),
    layer("walls", W, H, walls),
    objects("objects", [
        {"name": "player", "type": "spawn", "x": 64, "y": 168, "point": True},
        {"name": "sign", "type": "text", "x": 120, "y": 150, "width": 32, "height": 24,
         "properties": [{"name": "text", "type": "string", "value": "Добро пожаловать в сад!"}]},
    ]),
])

# Platformer: ground with gaps, one-way planks and a flag
W, H = 48, 15
level = [0] * (W * H)
for xx in range(W):
    if not (14 <= xx <= 16 or 30 <= xx <= 32):
        level[(H - 1) * W + xx] = WALL
        level[(H - 2) * W + xx] = WALL
    level[xx] = WALL
for yy in range(H):
    level[yy * W] = WALL
    level[yy * W + W - 1] = WALL
for x0, yy, n in ((6, 10, 4), (13, 8, 5), (20, 6, 4), (27, 9, 6), (36, 7, 4)):
    for xx in range(x0, x0 + n):
        level[yy * W + xx] = PLANK
for xx, yy in ((10, 12), (24, 12), (25, 12), (25, 11)):
    level[yy * W + xx] = CRATE
save_map("platformer.tmj", "orthogonal", W, H, 16, 16, [ORTHO_SET], [
    layer("level", W, H, level),
    objects("objects", [
        {"name": "player", "type": "spawn", "x": 40, "y": 190, "point": True},
        {"name": "goal", "type": "trigger", "x": 704, "y": 176, "width": 16, "height": 32},
    ]),
])

# Isometric: grass, a pond and stone blocks to walk around
W, H = 14, 14
floor = [1] * (W * H)
blocks = [0] * (W * H)
for yy in range(H):
    for xx in range(W):
        i = yy * W + xx
        if 8 <= xx <= 10 and 2 <= yy <= 4:
            floor[i] = 3
        if xx in (0, W - 1) or yy in (0, H - 1) or (xx == 5 and 3 <= yy <= 9) or (yy == 9 and 7 <= xx <= 10):
            blocks[i] = 2
ISO_SET = tileset("iso", "iso_tiles.png", 96, 32, 32, 32, 3, 3, {1: "solid", 2: "solid"})
save_map("iso.tmj", "isometric", W, H, 32, 16, [ISO_SET], [
    layer("floor", W, H, floor),
    layer("blocks", W, H, blocks),
    # Isometric object positions are in grid pixels (tile_h per cell)
    objects("objects", [{"name": "player", "type": "spawn", "x": 2.5 * 16, "y": 2.5 * 16, "point": True}]),
])
print("assets written to", HERE)

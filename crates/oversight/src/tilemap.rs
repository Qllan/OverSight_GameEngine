//! Tile maps: orthogonal (top-down, side view) and isometric grids of
//! tiles from one or more tilesets, plus object layers (spawn points,
//! triggers). Maps are built in code or loaded from Tiled (`tiled`).
//!
//! World coordinates of a cell: orthogonal cells are `tile_w` x `tile_h`
//! rectangles with (0, 0) at the map's top-left; isometric cells are
//! diamonds, and `tile_to_world(0, 0)` is the top corner of the first one.

use std::collections::HashMap;

use crate::anim::SpriteSheet;
use crate::camera::Camera;
use crate::render::{DrawKind, Renderer};
use crate::types::{Color, Rect, Sprite, Vec2};

/// Tile id flags, as in Tiled: mirrored horizontally, vertically, and
/// across the diagonal (the diagonal flip comes first)
pub const FLIP_H: u32 = 0x8000_0000;
pub const FLIP_V: u32 = 0x4000_0000;
pub const FLIP_D: u32 = 0x2000_0000;
/// Tile id without flags (Tiled also uses 0x1000_0000 for hex maps)
pub const GID_MASK: u32 = 0x0FFF_FFFF;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Orthogonal = 0,
    Isometric = 1,
}

/// A custom property value (Tiled's bool, int, float, string, ...)
#[derive(Debug, Clone, PartialEq)]
pub enum Property {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

impl Property {
    /// True for `true`, a non-zero number or the string "true"
    pub fn truthy(&self) -> bool {
        match self {
            Property::Bool(b) => *b,
            Property::Int(i) => *i != 0,
            Property::Float(f) => *f != 0.0,
            Property::String(s) => s == "true",
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Property::Int(i) => Some(*i as f64),
            Property::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Property::String(s) => Some(s),
            _ => None,
        }
    }
}

pub type Properties = HashMap<String, Property>;

/// Tiles cut from one texture. Tile `local` of this set has the map id
/// `first_gid + local`.
#[derive(Debug, Clone)]
pub struct Tileset {
    pub name: String,
    pub first_gid: u32,
    pub texture: u32,
    pub sheet: SpriteSheet,
    /// Properties of single tiles, by local id
    pub tile_properties: HashMap<u32, Properties>,
    pub properties: Properties,
}

impl Tileset {
    pub fn new(first_gid: u32, texture: u32, sheet: SpriteSheet) -> Tileset {
        Tileset {
            name: String::new(),
            first_gid,
            texture,
            sheet,
            tile_properties: HashMap::new(),
            properties: Properties::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TileLayer {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Tile ids with flip flags, row by row; 0 is an empty cell
    pub tiles: Vec<u32>,
    pub visible: bool,
    pub opacity: f32,
    /// Shift of the whole layer in world units
    pub offset: Vec2,
    pub properties: Properties,
}

impl TileLayer {
    pub fn new(name: &str, width: u32, height: u32) -> TileLayer {
        TileLayer {
            name: name.to_string(),
            width,
            height,
            tiles: vec![0; (width * height) as usize],
            visible: true,
            opacity: 1.0,
            offset: Vec2::default(),
            properties: Properties::new(),
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return None;
        }
        Some(y as usize * self.width as usize + x as usize)
    }

    /// Tile id with flags at a cell (0 outside the layer)
    pub fn get(&self, x: i32, y: i32) -> u32 {
        self.index(x, y).map_or(0, |i| self.tiles[i])
    }

    pub fn set(&mut self, x: i32, y: i32, gid: u32) {
        if let Some(i) = self.index(x, y) {
            self.tiles[i] = gid;
        }
    }
}

/// A spawn point, trigger area or other marker placed on the map
#[derive(Debug, Clone, Default)]
pub struct MapObject {
    pub id: u32,
    pub name: String,
    /// Tiled's class (formerly "type"), e.g. "spawn" or "trigger"
    pub class: String,
    /// Position in world units (an isometric map's objects are converted
    /// from Tiled's grid space; `rect.x, rect.y` is the object's anchor)
    pub rect: Rect,
    pub point: bool,
    pub rotation: f32,
    /// Tile drawn for tile objects (0: none)
    pub gid: u32,
    pub visible: bool,
    pub properties: Properties,
}

#[derive(Debug, Clone, Default)]
pub struct ObjectLayer {
    pub name: String,
    pub objects: Vec<MapObject>,
    pub visible: bool,
    pub properties: Properties,
}

#[derive(Debug, Clone, Default)]
pub struct Tilemap {
    pub orientation: Orientation,
    /// Size in cells
    pub width: u32,
    pub height: u32,
    /// Cell size in world units (for isometric maps: the diamond's size)
    pub tile_w: u32,
    pub tile_h: u32,
    /// Sorted by `first_gid`
    pub tilesets: Vec<Tileset>,
    pub layers: Vec<TileLayer>,
    pub object_layers: Vec<ObjectLayer>,
    pub properties: Properties,
}

/// How a tile layer is drawn
#[derive(Debug, Clone, Copy)]
pub struct LayerDraw {
    /// Draw layer of every tile
    pub layer: i32,
    /// Depth of every tile, or the bottom y of its cell when `y_sort` is
    /// on (so tiles sort with sprites whose depth is their feet's y)
    pub depth: f32,
    pub y_sort: bool,
    pub tint: Color,
}

impl Default for LayerDraw {
    fn default() -> Self {
        LayerDraw {
            layer: 0,
            depth: 0.0,
            y_sort: false,
            tint: Color::WHITE,
        }
    }
}

impl Tilemap {
    pub fn new(
        orientation: Orientation,
        width: u32,
        height: u32,
        tile_w: u32,
        tile_h: u32,
    ) -> Tilemap {
        Tilemap {
            orientation,
            width,
            height,
            tile_w,
            tile_h,
            ..Tilemap::default()
        }
    }

    pub fn add_tileset(&mut self, tileset: Tileset) {
        self.tilesets.push(tileset);
        self.tilesets.sort_by_key(|t| t.first_gid);
    }

    /// Add an empty tile layer the size of the map; returns its index
    pub fn add_layer(&mut self, name: &str) -> usize {
        self.layers
            .push(TileLayer::new(name, self.width, self.height));
        self.layers.len() - 1
    }

    pub fn layer_index(&self, name: &str) -> Option<usize> {
        self.layers.iter().position(|l| l.name == name)
    }

    pub fn object_layer(&self, name: &str) -> Option<&ObjectLayer> {
        self.object_layers.iter().find(|l| l.name == name)
    }

    /// Every object on the map with this name
    pub fn objects_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a MapObject> + 'a {
        self.object_layers
            .iter()
            .flat_map(|l| l.objects.iter())
            .filter(move |o| o.name == name)
    }

    /// The tileset a tile id belongs to and the tile's local id in it
    pub fn tileset_for(&self, gid: u32) -> Option<(&Tileset, u32)> {
        let gid = gid & GID_MASK;
        if gid == 0 {
            return None;
        }
        let set = self.tilesets.iter().rev().find(|t| t.first_gid <= gid)?;
        Some((set, gid - set.first_gid))
    }

    pub fn tile_properties(&self, gid: u32) -> Option<&Properties> {
        let (set, local) = self.tileset_for(gid)?;
        set.tile_properties.get(&local)
    }

    /// A tile property of the tile at a cell of a layer
    pub fn tile_property(&self, layer: usize, x: i32, y: i32, name: &str) -> Option<&Property> {
        let gid = self.layers.get(layer)?.get(x, y);
        self.tile_properties(gid)?.get(name)
    }

    /// A cell blocks movement when a tile in any layer has the `solid`
    /// property, or sits in a layer that has it. Cells outside the map are
    /// not solid (games close their maps with walls).
    pub fn is_solid(&self, x: i32, y: i32) -> bool {
        self.layers.iter().any(|layer| {
            let gid = layer.get(x, y);
            if gid == 0 {
                return false;
            }
            if layer.properties.get("solid").is_some_and(Property::truthy) {
                return true;
            }
            self.tile_properties(gid)
                .and_then(|p| p.get("solid"))
                .is_some_and(Property::truthy)
        })
    }

    // --- coordinates -------------------------------------------------------

    /// World position of a (fractional) cell coordinate: an orthogonal
    /// cell's top-left, an isometric cell's top corner
    pub fn tile_to_world(&self, tx: f32, ty: f32) -> Vec2 {
        let (tw, th) = (self.tile_w as f32, self.tile_h as f32);
        match self.orientation {
            Orientation::Orthogonal => Vec2::new(tx * tw, ty * th),
            Orientation::Isometric => Vec2::new((tx - ty) * tw / 2.0, (tx + ty) * th / 2.0),
        }
    }

    /// Fractional cell coordinate of a world point
    pub fn world_to_tile(&self, p: Vec2) -> Vec2 {
        let (tw, th) = (self.tile_w.max(1) as f32, self.tile_h.max(1) as f32);
        match self.orientation {
            Orientation::Orthogonal => Vec2::new(p.x / tw, p.y / th),
            Orientation::Isometric => {
                let (a, b) = (p.x / (tw / 2.0), p.y / (th / 2.0));
                Vec2::new((a + b) / 2.0, (b - a) / 2.0)
            }
        }
    }

    /// The cell a world point is in
    pub fn tile_at(&self, p: Vec2) -> (i32, i32) {
        let t = self.world_to_tile(p);
        (t.x.floor() as i32, t.y.floor() as i32)
    }

    /// World position of a cell's centre
    pub fn cell_center(&self, x: i32, y: i32) -> Vec2 {
        self.tile_to_world(x as f32 + 0.5, y as f32 + 0.5)
    }

    pub fn tile_to_screen(&self, tx: f32, ty: f32, camera: &Camera, size: (u32, u32)) -> Vec2 {
        camera.world_to_screen(self.tile_to_world(tx, ty), size)
    }

    pub fn screen_to_tile(&self, p: Vec2, camera: &Camera, size: (u32, u32)) -> Vec2 {
        self.world_to_tile(camera.screen_to_world(p, size))
    }

    /// World rectangle the map covers
    pub fn bounds(&self) -> Rect {
        let (w, h) = (self.width as f32, self.height as f32);
        let (tw, th) = (self.tile_w as f32, self.tile_h as f32);
        match self.orientation {
            Orientation::Orthogonal => Rect::new(0.0, 0.0, w * tw, h * th),
            Orientation::Isometric => {
                Rect::new(-h * tw / 2.0, 0.0, (w + h) * tw / 2.0, (w + h) * th / 2.0)
            }
        }
    }

    /// Cells that can show inside a world rectangle, as x0..x1, y0..y1
    pub fn cell_range(&self, view: Rect) -> (i32, i32, i32, i32) {
        // Tile images taller or wider than a cell reach into the view from
        // cells around it
        let (mut over_x, mut over_y) = (0, 0);
        for set in &self.tilesets {
            over_x = over_x.max(set.sheet.frame_w.div_ceil(self.tile_w.max(1)) as i32);
            over_y = over_y.max(set.sheet.frame_h.div_ceil(self.tile_h.max(1)) as i32);
        }
        let corners = [
            Vec2::new(view.x, view.y),
            Vec2::new(view.right(), view.y),
            Vec2::new(view.x, view.bottom()),
            Vec2::new(view.right(), view.bottom()),
        ];
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in corners {
            let t = self.world_to_tile(c);
            x0 = x0.min(t.x);
            y0 = y0.min(t.y);
            x1 = x1.max(t.x);
            y1 = y1.max(t.y);
        }
        let clamp = |v: i32, max: u32| v.clamp(0, max as i32);
        (
            clamp(x0.floor() as i32 - over_x, self.width),
            clamp(y0.floor() as i32 - over_y, self.height),
            clamp(x1.floor() as i32 + 1 + over_x, self.width),
            clamp(y1.floor() as i32 + 1 + over_y, self.height),
        )
    }

    // --- drawing -----------------------------------------------------------

    /// Queue the tiles of a layer that can be seen through the camera
    pub fn draw_layer(&self, renderer: &mut Renderer, index: usize, opts: &LayerDraw) {
        let Some(layer) = self.layers.get(index) else {
            return;
        };
        if !layer.visible || layer.opacity <= 0.0 {
            return;
        }
        let view = renderer.camera.visible_world(renderer.size());
        let view = Rect::new(
            view.x - layer.offset.x,
            view.y - layer.offset.y,
            view.w,
            view.h,
        );
        let (x0, y0, x1, y1) = self.cell_range(view);
        let alpha = (opts.tint.a as f32 * layer.opacity.min(1.0)).round() as u8;
        let tint = Color {
            a: alpha,
            ..opts.tint
        };
        let mut put = |x: i32, y: i32| {
            let gid = layer.get(x, y);
            if gid != 0 {
                self.queue_tile(renderer, gid, x, y, layer.offset, tint, opts);
            }
        };
        match self.orientation {
            Orientation::Orthogonal => {
                for y in y0..y1 {
                    for x in x0..x1 {
                        put(x, y);
                    }
                }
            }
            // Back to front: cells on one diagonal share a screen row
            Orientation::Isometric => {
                for d in (x0 + y0)..(x1 + y1 - 1) {
                    for x in x0.max(d - y1 + 1)..x1.min(d - y0 + 1) {
                        put(x, d - x);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn queue_tile(
        &self,
        renderer: &mut Renderer,
        gid: u32,
        x: i32,
        y: i32,
        offset: Vec2,
        tint: Color,
        opts: &LayerDraw,
    ) {
        let Some((set, local)) = self.tileset_for(gid) else {
            return;
        };
        let src = set.sheet.frame(local);
        if src.w <= 0.0 {
            return;
        }
        let th = self.tile_h as f32;
        // Bottom-left of the image on the cell's bottom-left (orthogonal),
        // bottom-centre on the cell's bottom corner (isometric), as Tiled
        let corner = self.tile_to_world(x as f32, y as f32);
        let (left, bottom) = match self.orientation {
            Orientation::Orthogonal => (corner.x, corner.y + th),
            Orientation::Isometric => (corner.x - src.w / 2.0, corner.y + th),
        };
        let center = Vec2::new(
            left + src.w / 2.0 + offset.x,
            bottom - src.h / 2.0 + offset.y,
        );
        let (h, v, d) = (gid & FLIP_H != 0, gid & FLIP_V != 0, gid & FLIP_D != 0);
        // A diagonal flip is a quarter turn of a vertically flipped tile;
        // the later flips then swap axes
        let (flip_x, flip_y, rotation) = if d {
            (v, !h, std::f32::consts::FRAC_PI_2)
        } else {
            (h, v, 0.0)
        };
        let sprite = Sprite {
            src,
            pos: center,
            origin: Vec2::new(src.w / 2.0, src.h / 2.0),
            rotation,
            flip_x,
            flip_y,
            tint,
            layer: opts.layer,
            depth: if opts.y_sort {
                bottom + offset.y
            } else {
                opts.depth
            },
            ..Sprite::default()
        };
        renderer.push(
            opts.layer,
            sprite.depth,
            crate::types::Space::World,
            DrawKind::Sprite {
                texture: set.texture,
                sprite,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;

    const RED: Color = Color::rgba(255, 0, 0, 255);
    const GREEN: Color = Color::rgba(0, 255, 0, 255);
    const BLUE: Color = Color::rgba(0, 0, 255, 255);

    /// 2x2 texture of 1px tiles: red, green / blue, white
    fn tiles(engine: &mut Engine) -> Tileset {
        let px = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let tex = engine.texture_from_rgba(2, 2, &px).unwrap();
        Tileset::new(1, tex, SpriteSheet::grid(2, 2, 1, 1, 0, 0))
    }

    #[test]
    fn orthogonal_tiles_draw_and_cull() {
        let mut e = Engine::headless(16, 16).unwrap();
        let mut map = Tilemap::new(Orientation::Orthogonal, 100, 100, 1, 1);
        map.add_tileset(tiles(&mut e));
        let ground = map.add_layer("ground");
        for y in 0..100 {
            for x in 0..100 {
                map.layers[ground].set(x, y, 1);
            }
        }
        map.layers[ground].set(1, 0, 2);
        map.layers[ground].set(0, 1, 3 | FLIP_H);
        // Camera shows world 0..4 at 4x zoom
        e.set_camera(Camera {
            center: Vec2::new(2.0, 2.0),
            zoom: 4.0,
            rotation: 0.0,
        });
        e.begin_frame(Color::BLACK);
        map.draw_layer(&mut e.renderer, ground, &LayerDraw::default());
        assert!(
            e.renderer.pending() <= 64,
            "queued {}",
            e.renderer.pending()
        );
        e.end_frame();
        assert_eq!(e.pixel(1, 1), Some(RED));
        assert_eq!(e.pixel(5, 1), Some(GREEN));
        assert_eq!(e.pixel(1, 5), Some(BLUE));
    }

    #[test]
    fn diagonal_flip_transposes() {
        // A 2x2 tile, red green / blue white, flipped across the diagonal
        // becomes red blue / green white
        let mut e = Engine::headless(8, 8).unwrap();
        let px = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let tex = e.texture_from_rgba(2, 2, &px).unwrap();
        let mut map = Tilemap::new(Orientation::Orthogonal, 1, 1, 2, 2);
        map.add_tileset(Tileset::new(1, tex, SpriteSheet::grid(2, 2, 2, 2, 0, 0)));
        let l = map.add_layer("l");
        let draw = |e: &mut Engine, map: &Tilemap| {
            e.set_camera(Camera {
                center: Vec2::new(1.0, 1.0),
                zoom: 4.0,
                rotation: 0.0,
            });
            e.begin_frame(Color::BLACK);
            map.draw_layer(&mut e.renderer, l, &LayerDraw::default());
            e.end_frame();
            [e.pixel(2, 2), e.pixel(6, 2), e.pixel(2, 6), e.pixel(6, 6)]
        };
        map.layers[l].set(0, 0, 1 | FLIP_D);
        let w = Some(Color::WHITE);
        assert_eq!(draw(&mut e, &map), [Some(RED), Some(BLUE), Some(GREEN), w]);
        map.layers[l].set(0, 0, 1 | FLIP_D | FLIP_H);
        assert_eq!(draw(&mut e, &map), [Some(BLUE), Some(RED), w, Some(GREEN)]);
        map.layers[l].set(0, 0, 1 | FLIP_D | FLIP_V);
        assert_eq!(draw(&mut e, &map), [Some(GREEN), w, Some(RED), Some(BLUE)]);
    }

    #[test]
    fn isometric_coordinates_round_trip() {
        let map = Tilemap::new(Orientation::Isometric, 10, 10, 64, 32);
        assert_eq!(map.tile_to_world(0.0, 0.0), Vec2::new(0.0, 0.0));
        assert_eq!(map.tile_to_world(1.0, 0.0), Vec2::new(32.0, 16.0));
        assert_eq!(map.tile_to_world(0.0, 1.0), Vec2::new(-32.0, 16.0));
        assert_eq!(map.cell_center(2, 3), Vec2::new(-32.0, 96.0));
        assert_eq!(map.tile_at(Vec2::new(-32.0, 96.0)), (2, 3));
        let t = map.world_to_tile(map.tile_to_world(3.25, 7.5));
        assert!((t.x - 3.25).abs() < 1e-4 && (t.y - 7.5).abs() < 1e-4);
        assert_eq!(map.bounds(), Rect::new(-320.0, 0.0, 640.0, 320.0));
        let camera = Camera {
            center: Vec2::new(100.0, 40.0),
            zoom: 2.0,
            rotation: 0.0,
        };
        let s = map.tile_to_screen(2.0, 1.0, &camera, (320, 180));
        let back = map.screen_to_tile(s, &camera, (320, 180));
        assert!((back.x - 2.0).abs() < 1e-4 && (back.y - 1.0).abs() < 1e-4);
    }

    #[test]
    fn isometric_draws_front_cells_over_back_ones() {
        // Tall 2x4 tiles on a 2x2 diamond grid overlap: cell (1, 0) is in
        // front of (0, 0) and must cover it, sorted or not
        let mut e = Engine::headless(64, 64).unwrap();
        let mut px = Vec::new();
        for _ in 0..4 {
            px.extend_from_slice(&[255, 0, 0, 255, 255, 0, 0, 255]);
            px.extend_from_slice(&[0, 0, 255, 255, 0, 0, 255, 255]);
        }
        let tex = e.texture_from_rgba(4, 4, &px).unwrap();
        let mut map = Tilemap::new(Orientation::Isometric, 3, 3, 2, 2);
        map.add_tileset(Tileset::new(1, tex, SpriteSheet::grid(4, 4, 2, 4, 0, 0)));
        let l = map.add_layer("walls");
        e.set_camera(Camera {
            center: Vec2::new(0.5, 0.5),
            zoom: 8.0,
            rotation: 0.0,
        });
        for (back, front, expected) in [(1, 2, BLUE), (2, 1, RED)] {
            map.layers[l].set(0, 0, back);
            map.layers[l].set(1, 0, front);
            for y_sort in [false, true] {
                e.begin_frame(Color::BLACK);
                let opts = LayerDraw {
                    y_sort,
                    ..LayerDraw::default()
                };
                map.draw_layer(&mut e.renderer, l, &opts);
                assert_eq!(e.renderer.pending(), 2);
                e.end_frame();
                assert_eq!(e.pixel(32, 32), Some(expected), "y_sort {}", y_sort);
            }
        }
    }

    #[test]
    fn solid_from_tile_or_layer_properties() {
        let mut map = Tilemap::new(Orientation::Orthogonal, 4, 4, 16, 16);
        let mut set = Tileset::new(1, 0, SpriteSheet::grid(32, 16, 16, 16, 0, 0));
        set.tile_properties.insert(
            1,
            Properties::from([("solid".to_string(), Property::Bool(true))]),
        );
        map.add_tileset(set);
        let ground = map.add_layer("ground");
        let walls = map.add_layer("walls");
        map.layers[ground].set(0, 0, 1);
        map.layers[ground].set(1, 0, 2);
        map.layers[walls].set(2, 0, 1 | FLIP_H);
        map.layers[walls]
            .properties
            .insert("solid".to_string(), Property::Bool(true));
        map.layers[walls].set(3, 0, 1);
        assert!(!map.is_solid(0, 0));
        assert!(map.is_solid(1, 0));
        assert!(map.is_solid(2, 0));
        assert!(map.is_solid(3, 0));
        assert!(!map.is_solid(-1, 0) && !map.is_solid(0, 1));
        assert_eq!(
            map.tile_property(ground, 1, 0, "solid"),
            Some(&Property::Bool(true))
        );
    }
}

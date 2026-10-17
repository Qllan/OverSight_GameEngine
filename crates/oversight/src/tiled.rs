//! Loading maps made in Tiled (https://www.mapeditor.org), JSON format:
//! `.tmj` maps and `.tsj` external tilesets.
//!
//! Supported: orthogonal and isometric maps of fixed size, tile layers
//! (CSV or uncompressed base64), object layers, groups (flattened, with
//! their offsets), tilesets with one image (embedded or external), and
//! custom properties on the map, layers, tilesets, tiles and objects.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::anim::SpriteSheet;
use crate::tilemap::{
    MapObject, ObjectLayer, Orientation, Properties, Property, TileLayer, Tilemap, Tileset,
};
use crate::types::{Rect, Vec2};

/// Read a `.tmj` file. `load_texture` turns an image path into a texture
/// id; it is called once per image.
pub fn load(
    path: &Path,
    load_texture: &mut dyn FnMut(&Path) -> Result<u32, String>,
) -> Result<Tilemap, String> {
    let text = read(path)?;
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut map =
        parse(&text, dir, load_texture).map_err(|e| format!("{}: {}", path.display(), e))?;
    map.files.insert(0, path.to_path_buf());
    Ok(map)
}

/// Parse map JSON; relative image and tileset paths are taken from `dir`
pub fn parse(
    json: &str,
    dir: &Path,
    load_texture: &mut dyn FnMut(&Path) -> Result<u32, String>,
) -> Result<Tilemap, String> {
    let map: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if map["infinite"].as_bool() == Some(true) {
        return Err("infinite maps are not supported (turn off Infinite in Map Properties)".into());
    }
    let orientation = match map["orientation"].as_str() {
        Some("orthogonal") | None => Orientation::Orthogonal,
        Some("isometric") => Orientation::Isometric,
        Some(other) => return Err(format!("{} maps are not supported", other)),
    };
    let mut out = Tilemap::new(
        orientation,
        uint(&map, "width")?,
        uint(&map, "height")?,
        uint(&map, "tilewidth")?,
        uint(&map, "tileheight")?,
    );
    out.properties = properties(&map);

    let mut textures: HashMap<PathBuf, u32> = HashMap::new();
    for ts in map["tilesets"].as_array().into_iter().flatten() {
        let first_gid = uint(ts, "firstgid")?;
        let tileset = match ts["source"].as_str() {
            Some(source) => {
                let path = dir.join(source);
                let text = read(&path)?;
                let value: Value = serde_json::from_str(&text)
                    .map_err(|e| format!("{}: {}", path.display(), e))?;
                let ts_dir = path.parent().unwrap_or(Path::new(""));
                let ts = tileset(&value, first_gid, ts_dir, &mut textures, load_texture)
                    .map_err(|e| format!("{}: {}", path.display(), e))?;
                out.files.push(path);
                ts
            }
            None => tileset(ts, first_gid, dir, &mut textures, load_texture)?,
        };
        out.add_tileset(tileset);
    }

    let layers = map["layers"].as_array().cloned().unwrap_or_default();
    add_layers(&mut out, &layers, Vec2::default(), 1.0, true)?;
    Ok(out)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))
}

fn uint(v: &Value, key: &str) -> Result<u32, String> {
    v[key]
        .as_u64()
        .map(|n| n as u32)
        .ok_or_else(|| format!("missing or bad \"{}\"", key))
}

fn num(v: &Value, key: &str) -> f32 {
    v[key].as_f64().unwrap_or(0.0) as f32
}

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}

fn properties(v: &Value) -> Properties {
    let mut out = Properties::new();
    for p in v["properties"].as_array().into_iter().flatten() {
        let (Some(name), value) = (p["name"].as_str(), &p["value"]) else {
            continue;
        };
        let prop = match (p["type"].as_str().unwrap_or("string"), value) {
            ("bool", Value::Bool(b)) => Property::Bool(*b),
            ("int" | "object", v) if v.is_i64() => Property::Int(v.as_i64().unwrap_or(0)),
            ("float", v) if v.is_number() => Property::Float(v.as_f64().unwrap_or(0.0)),
            // string, color ("#aarrggbb"), file (a path)
            (_, Value::String(s)) => Property::String(s.clone()),
            _ => continue, // class values: not supported yet
        };
        out.insert(name.to_string(), prop);
    }
    out
}

fn tileset(
    ts: &Value,
    first_gid: u32,
    dir: &Path,
    textures: &mut HashMap<PathBuf, u32>,
    load_texture: &mut dyn FnMut(&Path) -> Result<u32, String>,
) -> Result<Tileset, String> {
    let name = text(ts, "name");
    let Some(image) = ts["image"].as_str() else {
        return Err(format!(
            "tileset \"{}\": collections of images are not supported, use one image",
            name
        ));
    };
    let path = dir.join(image);
    let texture = match textures.get(&path) {
        Some(&id) => id,
        None => {
            let id = load_texture(&path)?;
            textures.insert(path, id);
            id
        }
    };
    let (frame_w, frame_h) = (uint(ts, "tilewidth")?, uint(ts, "tileheight")?);
    let (margin, spacing) = (
        uint(ts, "margin").unwrap_or(0),
        uint(ts, "spacing").unwrap_or(0),
    );
    let mut sheet = SpriteSheet::grid(
        uint(ts, "imagewidth")?,
        uint(ts, "imageheight")?,
        frame_w,
        frame_h,
        margin,
        spacing,
    );
    // Tiled's own counts win when present (an image may have spare room)
    if let (Ok(columns), Ok(count)) = (uint(ts, "columns"), uint(ts, "tilecount")) {
        sheet.columns = columns;
        sheet.count = count;
    }
    let mut out = Tileset::new(first_gid, texture, sheet);
    out.name = name;
    out.properties = properties(ts);
    for tile in ts["tiles"].as_array().into_iter().flatten() {
        let props = properties(tile);
        if !props.is_empty() {
            out.tile_properties.insert(uint(tile, "id")?, props);
        }
    }
    Ok(out)
}

fn add_layers(
    map: &mut Tilemap,
    layers: &[Value],
    offset: Vec2,
    opacity: f32,
    visible: bool,
) -> Result<(), String> {
    for layer in layers {
        let offset = Vec2::new(
            offset.x + num(layer, "offsetx"),
            offset.y + num(layer, "offsety"),
        );
        let opacity = opacity * layer["opacity"].as_f64().unwrap_or(1.0) as f32;
        let visible = visible && layer["visible"].as_bool().unwrap_or(true);
        let name = text(layer, "name");
        match layer["type"].as_str() {
            Some("tilelayer") => {
                let (w, h) = (uint(layer, "width")?, uint(layer, "height")?);
                let tiles = tile_data(layer).map_err(|e| format!("layer \"{}\": {}", name, e))?;
                if tiles.len() != (w * h) as usize {
                    return Err(format!(
                        "layer \"{}\": {} tiles for {}x{}",
                        name,
                        tiles.len(),
                        w,
                        h
                    ));
                }
                let mut out = TileLayer::new(&name, w, h);
                out.tiles = tiles;
                out.visible = visible;
                out.opacity = opacity;
                out.offset = offset;
                out.properties = properties(layer);
                map.layers.push(out);
            }
            Some("objectgroup") => {
                let objects = layer["objects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|o| object(map, o, offset))
                    .collect();
                map.object_layers.push(ObjectLayer {
                    name,
                    objects,
                    visible,
                    properties: properties(layer),
                });
            }
            Some("group") => {
                let children = layer["layers"].as_array().cloned().unwrap_or_default();
                add_layers(map, &children, offset, opacity, visible)?;
            }
            _ => {} // image layers: not supported yet
        }
    }
    Ok(())
}

fn tile_data(layer: &Value) -> Result<Vec<u32>, String> {
    match &layer["data"] {
        Value::Array(cells) => Ok(cells
            .iter()
            .map(|c| c.as_u64().unwrap_or(0) as u32)
            .collect()),
        Value::String(encoded) => {
            let compression = layer["compression"].as_str().unwrap_or("");
            if !compression.is_empty() {
                return Err(format!(
                    "{} compression is not supported (save tile layers as CSV)",
                    compression
                ));
            }
            let bytes = base64(encoded)?;
            Ok(bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| u32::from_le_bytes(*b))
                .collect())
        }
        _ => Err("no tile data".into()),
    }
}

fn base64(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return Err("bad base64 tile data".into()),
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

fn object(map: &Tilemap, o: &Value, offset: Vec2) -> MapObject {
    let (x, y) = (num(o, "x"), num(o, "y"));
    // Isometric objects are stored in grid space: each cell tile_h square
    let pos = match map.orientation {
        Orientation::Orthogonal => Vec2::new(x, y),
        Orientation::Isometric => {
            let th = map.tile_h.max(1) as f32;
            map.tile_to_world(x / th, y / th)
        }
    };
    // "class" since Tiled 1.9, "type" before
    let class = match text(o, "class") {
        c if c.is_empty() => text(o, "type"),
        c => c,
    };
    MapObject {
        id: o["id"].as_u64().unwrap_or(0) as u32,
        name: text(o, "name"),
        class,
        rect: Rect::new(
            pos.x + offset.x,
            pos.y + offset.y,
            num(o, "width"),
            num(o, "height"),
        ),
        point: o["point"].as_bool().unwrap_or(false),
        rotation: num(o, "rotation").to_radians(),
        gid: o["gid"].as_u64().unwrap_or(0) as u32,
        visible: o["visible"].as_bool().unwrap_or(true),
        properties: properties(o),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tilemap::FLIP_H;

    const MAP: &str = r#"{
        "orientation": "orthogonal", "infinite": false,
        "width": 3, "height": 2, "tilewidth": 16, "tileheight": 16,
        "properties": [{"name": "music", "type": "file", "value": "cave.ogg"}],
        "tilesets": [
            {"firstgid": 1, "name": "ground", "image": "tiles.png",
             "imagewidth": 32, "imageheight": 16, "tilewidth": 16, "tileheight": 16,
             "columns": 2, "tilecount": 2, "margin": 0, "spacing": 0,
             "tiles": [{"id": 1, "properties": [{"name": "solid", "type": "bool", "value": true}]}]},
            {"firstgid": 3, "source": "props.tsj"}
        ],
        "layers": [
            {"type": "tilelayer", "name": "ground", "width": 3, "height": 2,
             "data": [1, 1, 2, 1, 3, 2147483649], "opacity": 1, "visible": true, "x": 0, "y": 0},
            {"type": "group", "name": "upper", "offsetx": 4, "offsety": 0, "opacity": 0.5, "layers": [
                {"type": "tilelayer", "name": "base64", "width": 3, "height": 2,
                 "encoding": "base64", "data": "AQAAAAAAAAACAAAAAAAAAAAAAAAAAAAA", "opacity": 1},
                {"type": "objectgroup", "name": "spawns", "objects": [
                    {"id": 7, "name": "player", "type": "spawn", "x": 24, "y": 8,
                     "width": 0, "height": 0, "point": true, "rotation": 0,
                     "properties": [{"name": "facing", "type": "int", "value": -1}]}
                ]}
            ]}
        ]
    }"#;

    const PROPS: &str = r#"{"name": "props", "image": "props.png",
        "imagewidth": 16, "imageheight": 16, "tilewidth": 16, "tileheight": 16,
        "columns": 1, "tilecount": 1}"#;

    #[test]
    fn parses_layers_tilesets_objects_and_properties() {
        let dir = std::env::temp_dir().join(format!("oversight_tiled_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("props.tsj"), PROPS).unwrap();
        let mut loaded = Vec::new();
        let mut loader = |p: &Path| {
            loaded.push(p.file_name().unwrap().to_string_lossy().to_string());
            Ok(loaded.len() as u32 + 10)
        };
        let map = parse(MAP, &dir, &mut loader).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(loaded, vec!["tiles.png", "props.png"]);
        assert_eq!((map.width, map.height, map.tile_w), (3, 2, 16));
        assert_eq!(map.properties["music"], Property::String("cave.ogg".into()));
        assert_eq!(map.tilesets[1].first_gid, 3);
        assert_eq!(map.tilesets[1].texture, 12);
        assert_eq!(map.layers.len(), 2);
        assert_eq!(map.layers[0].get(2, 1), 1 | FLIP_H);
        assert!(map.is_solid(2, 0) && !map.is_solid(0, 0));
        let b64 = &map.layers[1];
        assert_eq!((b64.get(0, 0), b64.get(2, 0)), (1, 2));
        assert_eq!(b64.offset, Vec2::new(4.0, 0.0));
        assert!((b64.opacity - 0.5).abs() < 1e-6);
        let player = map.objects_named("player").next().unwrap();
        assert_eq!(
            (player.id, player.class.as_str(), player.point),
            (7, "spawn", true)
        );
        assert_eq!(player.rect, Rect::new(28.0, 8.0, 0.0, 0.0));
        assert_eq!(player.properties["facing"], Property::Int(-1));
    }

    #[test]
    fn isometric_objects_and_errors() {
        let iso = r#"{"orientation": "isometric", "width": 4, "height": 4,
            "tilewidth": 64, "tileheight": 32, "tilesets": [], "layers": [
            {"type": "objectgroup", "name": "o", "objects": [
                {"id": 1, "name": "chest", "x": 32, "y": 64}]}]}"#;
        let mut none = |_: &Path| -> Result<u32, String> { Err("no images".into()) };
        let map = parse(iso, Path::new(""), &mut none).unwrap();
        // Grid point (1, 2) is the cell corner tile_to_world(1, 2)
        assert_eq!(map.object_layers[0].objects[0].rect.x, -32.0);
        assert_eq!(map.object_layers[0].objects[0].rect.y, 48.0);

        let infinite =
            r#"{"infinite": true, "width": 1, "height": 1, "tilewidth": 1, "tileheight": 1}"#;
        assert!(parse(infinite, Path::new(""), &mut none)
            .unwrap_err()
            .contains("infinite"));
        let zlib = r#"{"width": 1, "height": 1, "tilewidth": 1, "tileheight": 1, "layers": [
            {"type": "tilelayer", "name": "z", "width": 1, "height": 1,
             "encoding": "base64", "compression": "zlib", "data": "eJw="}]}"#;
        assert!(parse(zlib, Path::new(""), &mut none)
            .unwrap_err()
            .contains("CSV"));
    }
}

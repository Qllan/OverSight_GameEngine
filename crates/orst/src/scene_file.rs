//! Scene files: entities and their components as JSON, the way the editor
//! saves a level and a game loads it.
//!
//! ```json
//! {"orst_scene": 1,
//!  "entities": [
//!   {"id": 1, "name": "Hero",
//!    "components": {"Transform": {"pos": [10, 20], "rotation": 0, "scale": [1, 1]},
//!                   "SpriteRenderer": {"texture": "sprites/hero.png", "tint": "#ffffffff"},
//!                   "Follow": {"target": 2}}},
//!   {"id": 2, "components": {"Transform": {"pos": [0, 0]}}}]}
//! ```
//!
//! Components are written by name with their fields by name (schema.rs),
//! so a scene still loads after a component gains, loses or reorders a
//! field: what the file does not mention keeps its default. Components
//! without fields are kept as their bytes in hex. `id` numbers entities
//! inside the file only: entity fields point at them, and loading gives
//! fresh entities. Textures and tile maps are named through `Assets`: by
//! path (Files) or by uid and path (the asset database); polygons are written out as their corners. Keys are sorted,
//! so saving the same world gives the same text, which keeps diffs small.
//!
//! Loading reports what it could not use (a component nobody registered,
//! a field that is gone, a missing texture) as warnings and goes on, so
//! one stale line does not lose a level.

use crate::components::{read, write, Name, PrefabInstance, NAME, PREFAB};
use crate::ecs::Entity;
use crate::engine::Engine;
use crate::schema::{Field, FieldType, Value};
use crate::types::Vec2;
use serde_json::{json, Map, Value as Json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub const SCENE_VERSION: u64 = 1;

/// How scene files name textures and tile maps: a path (Files) or a uid
/// with a path (asset_db::AssetDb)
pub trait Assets {
    /// How a scene names a loaded resource; None when it has no name (made
    /// from pixels, or from a file outside the project)
    fn name(&self, engine: &Engine, ty: FieldType, id: u32) -> Option<Json>;
    /// The resource a name stands for, loaded if it is not yet
    fn load(&mut self, engine: &mut Engine, ty: FieldType, name: &Json) -> Result<u32, String>;
}

/// Names resources by their path under a folder, with '/' between parts
/// on every system ("sprites/hero.png")
pub struct Files {
    pub root: PathBuf,
}

impl Files {
    pub fn new(root: impl Into<PathBuf>) -> Files {
        Files { root: root.into() }
    }

    fn relative(&self, file: &Path) -> Option<String> {
        let root = self.root.canonicalize().ok()?;
        let file = file.canonicalize().ok()?;
        let rel = file.strip_prefix(&root).ok()?;
        let parts: Vec<&str> = rel.iter().map(|p| p.to_str()).collect::<Option<_>>()?;
        Some(parts.join("/"))
    }
}

impl Assets for Files {
    fn name(&self, engine: &Engine, ty: FieldType, id: u32) -> Option<Json> {
        let file = match ty {
            FieldType::Texture => engine.texture_file(id)?,
            FieldType::Tilemap => engine.tilemap_file(id)?,
            FieldType::Prefab => engine.prefab_file(id)?,
            _ => return None,
        };
        self.relative(file).map(Json::String)
    }

    fn load(&mut self, engine: &mut Engine, ty: FieldType, name: &Json) -> Result<u32, String> {
        let name = name.as_str().ok_or("not a file name")?;
        // One already loaded from the same file is shared
        let ids: Vec<u32> = match ty {
            FieldType::Texture => engine.textures.ids().collect(),
            FieldType::Tilemap => engine.tilemaps.ids().collect(),
            FieldType::Prefab => engine.prefabs.ids().collect(),
            _ => return Err(format!("{} is not a file", ty.name())),
        };
        if let Some(id) = ids
            .into_iter()
            .find(|&id| self.name(engine, ty, id).as_ref().and_then(Json::as_str) == Some(name))
        {
            return Ok(id);
        }
        let path = self.root.join(name);
        let path = path.to_str().ok_or("a path that is not UTF-8")?;
        match ty {
            FieldType::Texture => engine.load_texture(path),
            FieldType::Prefab => engine.load_prefab(path),
            _ => engine.load_tilemap(path),
        }
    }
}

/// What load gave
#[derive(Debug, Default)]
pub struct Loaded {
    /// The new entities, in file order
    pub entities: Vec<Entity>,
    /// What was left out and why
    pub warnings: Vec<String>,
}

impl Engine {
    /// The entities as a scene file (see the module docs). Warnings say
    /// what could not be written: a texture without a file, an entity
    /// field pointing outside `entities`.
    pub fn save_scene(&self, entities: &[Entity], assets: &dyn Assets) -> (String, Vec<String>) {
        let mut warnings = Vec::new();
        let ids = numbering(entities);
        let list: Vec<Json> = entities
            .iter()
            .zip(1u64..)
            .map(|(&e, i)| self.entity_json(e, i, &ids, assets, true, &mut warnings))
            .collect();
        let scene = json!({"orst_scene": SCENE_VERSION, "entities": list});
        (serde_json::to_string_pretty(&scene).unwrap(), warnings)
    }

    /// One entity of `scope` as a scene file writes it, numbered as
    /// save_scene numbers `scope`: what refill_entity takes back
    pub fn scene_entity(
        &self,
        e: Entity,
        scope: &[Entity],
        assets: &dyn Assets,
    ) -> (Json, Vec<String>) {
        let mut warnings = Vec::new();
        let mut ids = numbering(scope);
        let next = ids.len() as u64 + 1;
        let i = *ids.entry(e).or_insert(next);
        (
            self.entity_json(e, i, &ids, assets, true, &mut warnings),
            warnings,
        )
    }

    /// The entity as a prefab file: itself alone, not linked to the prefab
    /// it may be an instance of
    pub fn prefab_of(&self, e: Entity, assets: &dyn Assets) -> (String, Vec<String>) {
        let mut warnings = Vec::new();
        let item = self.entity_json(e, 1, &numbering(&[e]), assets, false, &mut warnings);
        let scene = json!({"orst_scene": SCENE_VERSION, "entities": [item]});
        (serde_json::to_string_pretty(&scene).unwrap(), warnings)
    }

    /// Give `e` exactly the components `item` (from scene_entity) holds,
    /// its prefab's included
    pub fn refill_entity(
        &mut self,
        e: Entity,
        item: &Json,
        scope: &[Entity],
        assets: &mut dyn Assets,
    ) -> Vec<String> {
        for c in 1..=self.world.component_types() {
            self.world.remove(e, c);
        }
        let mut entities: HashMap<u64, Entity> =
            numbering(scope).into_iter().map(|(e, i)| (i, e)).collect();
        let id = item.get("id").and_then(Json::as_u64).unwrap_or(0);
        entities.insert(id, e);
        let mut warnings = Vec::new();
        self.fill_entity(e, id, item, &entities, assets, &mut warnings);
        warnings
    }

    fn entity_json(
        &self,
        e: Entity,
        i: u64,
        ids: &HashMap<Entity, u64>,
        assets: &dyn Assets,
        linked: bool,
        warnings: &mut Vec<String>,
    ) -> Json {
        let world = &self.world;
        let mut out = Map::new();
        out.insert("id".into(), json!(i));
        let mut components = Map::new();
        for c in 1..=world.component_types() {
            let Some(bytes) = world.get(e, c) else {
                continue;
            };
            let storage = world.storage(c).unwrap();
            if c == NAME {
                out.insert("name".into(), json!(crate::schema::text_str(bytes)));
                continue;
            }
            if c == PREFAB {
                continue;
            }
            let mut fields = Map::new();
            if storage.fields.is_empty() && !bytes.is_empty() {
                fields.insert("$bytes".into(), json!(hex(bytes)));
            }
            for f in &storage.fields {
                let where_ = || format!("entity {i}, {}.{}", storage.name, f.name);
                let v = match (f.ty, f.read(bytes)) {
                    (_, Value::U32(0))
                        if matches!(
                            f.ty,
                            FieldType::Texture | FieldType::Tilemap | FieldType::Polygon
                        ) =>
                    {
                        Json::Null
                    }
                    (FieldType::Polygon, Value::U32(id)) => match self.polygons.get(id) {
                        Some(points) => {
                            json!(points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>())
                        }
                        None => {
                            warnings.push(format!("{}: no polygon {id}", where_()));
                            Json::Null
                        }
                    },
                    (FieldType::Texture | FieldType::Tilemap, Value::U32(id)) => {
                        match assets.name(self, f.ty, id) {
                            Some(name) => name,
                            None => {
                                warnings.push(format!(
                                    "{}: {} {id} has no file in the project",
                                    where_(),
                                    f.ty.name()
                                ));
                                Json::Null
                            }
                        }
                    }
                    (_, Value::Entity(0)) => json!(0),
                    (_, Value::Entity(target)) => match ids.get(&target) {
                        Some(&id) => json!(id),
                        None => {
                            warnings.push(format!("{}: points outside the scene", where_()));
                            json!(0)
                        }
                    },
                    (_, v) => f.to_json(v),
                };
                fields.insert(f.name.clone(), v);
            }
            components.insert(storage.name.clone(), Json::Object(fields));
        }
        let link = read::<PrefabInstance>(world, e, PREFAB).filter(|_| linked);
        if let Some(p) = link.map(|l| l.prefab) {
            match assets.name(self, FieldType::Prefab, p) {
                Some(name) => {
                    out.insert("prefab".into(), name);
                    let (changed, removed) = self.overrides(p, i, components);
                    components = changed;
                    if !removed.is_empty() {
                        out.insert("removed".into(), json!(removed));
                    }
                }
                None => warnings.push(format!(
                    "entity {i}: prefab {p} has no file in the project, saved unlinked"
                )),
            }
        }
        out.insert("components".into(), Json::Object(components));
        Json::Object(out)
    }

    /// Spawn the entities of a scene file. Fails without spawning anything
    /// when the file is not a scene; smaller problems become warnings.
    pub fn load_scene(&mut self, text: &str, assets: &mut dyn Assets) -> Result<Loaded, String> {
        let scene: Json = serde_json::from_str(text).map_err(|e| format!("scene: {e}"))?;
        match scene.get("orst_scene").and_then(Json::as_u64) {
            Some(v) if v <= SCENE_VERSION => {}
            Some(v) => {
                return Err(format!(
                    "scene version {v} is newer than this engine ({SCENE_VERSION})"
                ))
            }
            None => return Err("not an ORST scene (no \"orst_scene\")".into()),
        }
        let list = scene
            .get("entities")
            .and_then(Json::as_array)
            .ok_or("scene: no \"entities\" list")?;
        let mut file_ids = BTreeMap::new();
        for (n, item) in list.iter().enumerate() {
            let id = item
                .get("id")
                .and_then(Json::as_u64)
                .filter(|&id| id > 0)
                .ok_or_else(|| format!("scene: entity {n} has no positive \"id\""))?;
            if file_ids.insert(id, n).is_some() {
                return Err(format!("scene: id {id} is used twice"));
            }
            if item.get("components").is_some_and(|c| !c.is_object()) {
                return Err(format!(
                    "scene: entity {id}: \"components\" is not an object"
                ));
            }
        }
        let mut out = Loaded::default();
        let mut entities: HashMap<u64, Entity> = HashMap::new();
        for item in list {
            let id = item["id"].as_u64().unwrap();
            entities.insert(id, self.world.spawn());
        }
        for item in list {
            let id = item["id"].as_u64().unwrap();
            let e = entities[&id];
            out.entities.push(e);
            self.fill_entity(e, id, item, &entities, assets, &mut out.warnings);
        }
        Ok(out)
    }

    /// Give `e` the name and components of a scene file entity: its
    /// prefab's, with what the entity changes on top
    fn fill_entity(
        &mut self,
        e: Entity,
        id: u64,
        item: &Json,
        entities: &HashMap<u64, Entity>,
        assets: &mut dyn Assets,
        warnings: &mut Vec<String>,
    ) {
        let mut name = item.get("name").cloned();
        let own = item.get("components").and_then(Json::as_object);
        let mut components = own.cloned().unwrap_or_default();
        if let Some(prefab) = item.get("prefab") {
            match assets.load(self, FieldType::Prefab, prefab) {
                Ok(p) => {
                    write(&mut self.world, e, PREFAB, &PrefabInstance { prefab: p });
                    let removed = item.get("removed").and_then(Json::as_array);
                    components = self.instance_components(p, id, own, removed);
                    if name.is_none() {
                        name = self
                            .prefabs
                            .get(p)
                            .and_then(|p| p.name.clone())
                            .map(Json::from);
                    }
                }
                Err(why) => warnings.push(format!("entity {id}: prefab: {why}")),
            }
        }
        if let Some(name) = name {
            match name.as_str() {
                Some(name) => {
                    write(&mut self.world, e, NAME, &Name::new(name));
                }
                None => warnings.push(format!("entity {id}: the name is not a string")),
            }
        }
        for (cname, data) in &components {
            let what = format!("entity {id}, {cname}");
            let Some(c) = self.world.component_id(cname) else {
                warnings.push(format!("{what}: no such component is registered"));
                continue;
            };
            let storage = self.world.storage(c).unwrap();
            let size = storage.size as usize;
            let fields: Vec<Field> = storage.fields.clone();
            // A field the file lacks keeps the component's default
            // (a script's field initialiser, say)
            let mut bytes = self.world.default_value(c);
            let Some(data) = data.as_object() else {
                warnings.push(format!("{what}: not an object"));
                continue;
            };
            for (key, v) in data {
                if key == "$bytes" {
                    match unhex(v.as_str().unwrap_or("")) {
                        Some(b) if b.len() == size => bytes = b,
                        _ => warnings.push(format!("{what}: $bytes are not {size} bytes in hex")),
                    }
                    continue;
                }
                let Some(f) = fields.iter().find(|f| &f.name == key) else {
                    warnings.push(format!("{what}: no field {key:?}"));
                    continue;
                };
                match self.field_from_json(f, v, entities, assets) {
                    Ok(value) => {
                        f.write(&mut bytes, value);
                    }
                    Err(why) => warnings.push(format!("{what}.{key}: {why}")),
                }
            }
            self.world.set(e, c, &bytes);
        }
    }

    fn field_from_json(
        &mut self,
        f: &Field,
        v: &Json,
        entities: &HashMap<u64, Entity>,
        assets: &mut dyn Assets,
    ) -> Result<Value, String> {
        match f.ty {
            FieldType::Texture | FieldType::Tilemap | FieldType::Polygon if v.is_null() => {
                Ok(Value::U32(0))
            }
            FieldType::Texture | FieldType::Tilemap => assets.load(self, f.ty, v).map(Value::U32),
            FieldType::Polygon => {
                let points: Option<Vec<Vec2>> = v.as_array().and_then(|a| {
                    a.iter()
                        .map(
                            |p| match p.as_array().map(|p| (p.len(), p.first(), p.get(1))) {
                                Some((2, Some(x), Some(y))) => {
                                    Some(Vec2::new(x.as_f64()? as f32, y.as_f64()? as f32))
                                }
                                _ => None,
                            },
                        )
                        .collect()
                });
                let points = points.ok_or("not a list of [x, y] corners")?;
                self.create_polygon(&points).map(Value::U32)
            }
            FieldType::Entity => match v.as_u64() {
                Some(0) => Ok(Value::Entity(0)),
                Some(id) => entities
                    .get(&id)
                    .map(|&e| Value::Entity(e))
                    .ok_or_else(|| format!("no entity {id} in the scene")),
                None => Err("not an entity id".into()),
            },
            _ => f
                .from_json(v)
                .ok_or_else(|| format!("{v} is not a {}", f.ty.name())),
        }
    }
}

/// Scene file ids of `entities`: 1, 2, ... in order
fn numbering(entities: &[Entity]) -> HashMap<Entity, u64> {
    entities
        .iter()
        .enumerate()
        .map(|(i, &e)| (e, i as u64 + 1))
        .collect()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{
        read, write, Collider, SpriteRenderer, Transform, COLLIDER, SPRITE_RENDERER, TRANSFORM,
    };
    use crate::schema::Field;
    use crate::types::Color;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Follow {
        target: u64,
        speed: f32,
        mood: u32,
    }

    fn engine() -> (Engine, u32, u32) {
        let mut e = Engine::headless(8, 8).unwrap();
        let follow = e.world.component("Follow", 16, 8).unwrap();
        e.world
            .set_fields(
                follow,
                vec![
                    Field::new("target", FieldType::Entity, 0),
                    Field::new("speed", FieldType::F32, 8),
                    Field::new("mood", FieldType::U32, 12).options(&["Calm", "Angry"]),
                ],
            )
            .unwrap();
        let tag = e.world.component("Tag", 4, 4).unwrap();
        (e, follow, tag)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orst_scene_{}_{}", name, std::process::id()));
        std::fs::create_dir_all(dir.join("sprites")).unwrap();
        dir
    }

    fn png(path: &Path) {
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        img.save(path).unwrap();
    }

    #[test]
    fn scenes_round_trip_with_references_and_files() {
        let dir = temp_dir("trip");
        png(&dir.join("sprites/hero.png"));
        let (mut e, follow, tag) = engine();
        let texture = e
            .load_texture(dir.join("sprites/hero.png").to_str().unwrap())
            .unwrap();
        let tri = e
            .create_polygon(&[
                Vec2::new(0.0, 0.0),
                Vec2::new(8.0, 0.0),
                Vec2::new(4.0, 6.0),
            ])
            .unwrap();
        let hero = e.world.spawn();
        let dog = e.world.spawn();
        write(&mut e.world, hero, NAME, &Name::new("Герой"));
        write(
            &mut e.world,
            hero,
            TRANSFORM,
            &Transform {
                pos: Vec2::new(10.0, 20.0),
                ..Default::default()
            },
        );
        let sprite = SpriteRenderer {
            texture,
            tint: Color::rgba(255, 128, 0, 255),
            ..Default::default()
        };
        write(&mut e.world, hero, SPRITE_RENDERER, &sprite);
        let poly = Collider {
            kind: crate::components::COLLIDER_POLYGON,
            polygon: tri,
            ..Default::default()
        };
        write(&mut e.world, hero, COLLIDER, &poly);
        write(
            &mut e.world,
            dog,
            follow,
            &Follow {
                target: hero,
                speed: 3.5,
                mood: 1,
            },
        );
        e.world.set(dog, tag, &7u32.to_le_bytes());

        let mut files = Files::new(&dir);
        let (text, warnings) = e.save_scene(&[hero, dog], &files);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(
            text.contains("\"sprites/hero.png\"")
                && text.contains("\"Angry\"")
                && text.contains("\"Герой\"")
        );
        assert!(
            text.contains("\"$bytes\": \"07000000\""),
            "an undescribed component keeps its bytes"
        );

        // Into a fresh engine: same components, references point at the new entities
        let (mut f, follow2, tag2) = engine();
        let loaded = f.load_scene(&text, &mut files).unwrap();
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        let [hero2, dog2] = loaded.entities[..] else {
            panic!()
        };
        assert_eq!(f.world.find_named("Герой"), Some(hero2));
        assert_eq!(
            read::<Transform>(&f.world, hero2, TRANSFORM).unwrap().pos,
            Vec2::new(10.0, 20.0)
        );
        let s: SpriteRenderer = read(&f.world, hero2, SPRITE_RENDERER).unwrap();
        assert_eq!(
            (s.tint, f.texture_file(s.texture).is_some()),
            (sprite.tint, true)
        );
        let c: Collider = read(&f.world, hero2, COLLIDER).unwrap();
        assert_eq!(f.polygons.get(c.polygon).unwrap().len(), 3);
        assert_eq!(
            read::<Follow>(&f.world, dog2, follow2).unwrap(),
            Follow {
                target: hero2,
                speed: 3.5,
                mood: 1
            }
        );
        assert_eq!(f.world.get(dog2, tag2).unwrap(), 7u32.to_le_bytes());

        // Saving the loaded scene gives the same text
        assert_eq!(f.save_scene(&loaded.entities, &files).0, text);
        // A second load shares the texture instead of loading it again
        let again = f.load_scene(&text, &mut files).unwrap();
        let s2: SpriteRenderer = read(&f.world, again.entities[0], SPRITE_RENDERER).unwrap();
        assert_eq!(s2.texture, s.texture);
    }

    #[test]
    fn stale_scenes_load_what_they_can() {
        let (mut e, _, _) = engine();
        let power = Field::new("power", crate::schema::FieldType::F32, 0);
        let glow = e
            .world
            .define("Glow", 4, 4, vec![power], &2.5f32.to_le_bytes())
            .unwrap();
        let text = r#"{"orst_scene": 1, "entities": [
            {"id": 5, "name": "a", "components": {
                "Glow": {},
                "Transform": {"pos": [1, 2], "wings": 2},
                "Gone": {"x": 1},
                "Follow": {"target": 9, "speed": "fast", "mood": "Calm"},
                "SpriteRenderer": {"texture": "missing.png"}}}]}"#;
        let loaded = e
            .load_scene(text, &mut Files::new(std::env::temp_dir()))
            .unwrap();
        let w = loaded.warnings.join("\n");
        for part in [
            "no field \"wings\"",
            "Gone: no such component",
            "no entity 9",
            "\"fast\" is not a f32",
            "missing.png",
        ] {
            assert!(w.contains(part), "{part} in\n{w}");
        }
        let t: Transform = read(&e.world, loaded.entities[0], TRANSFORM).unwrap();
        assert_eq!(
            (t.pos, t.scale),
            (Vec2::new(1.0, 2.0), Vec2::new(1.0, 1.0)),
            "the rest keeps defaults"
        );
        assert_eq!(
            e.world.field(loaded.entities[0], glow, "power"),
            Some(Value::F32(2.5)),
            "game components too"
        );
    }

    #[test]
    fn broken_scenes_spawn_nothing() {
        let (mut e, _, _) = engine();
        let mut files = Files::new(std::env::temp_dir());
        for bad in [
            "not json",
            r#"{"entities": []}"#,
            r#"{"orst_scene": 99, "entities": []}"#,
            r#"{"orst_scene": 1, "entities": [{"id": 1}, {"id": 1}]}"#,
            r#"{"orst_scene": 1, "entities": [{"components": {}}]}"#,
        ] {
            assert!(e.load_scene(bad, &mut files).is_err(), "{bad}");
        }
        assert_eq!(e.world.count(), 0);
    }
}

//! Prefabs: an entity saved as a file, placed in scenes as instances. A
//! prefab file is a scene file with one entity. A scene stores an instance
//! as the prefab's name and only what the instance changes, so editing the
//! prefab changes every instance that left those fields alone:
//!
//! ```json
//! {"id": 3, "prefab": {"uid": "…", "path": "Prefabs/Coin.prefab"},
//!  "components": {"Transform": {"pos": [40, 16]}},
//!  "removed": ["Light"]}
//! ```
//!
//! `removed` lists the prefab's components the instance does without. An
//! entity field of the prefab that points at the prefab's own entity
//! points at the instance; one pointing anywhere else is cleared.

use crate::components::{read, write, PrefabInstance, PREFAB};
use crate::ecs::Entity;
use crate::engine::Engine;
use crate::scene_file::SCENE_VERSION;
use crate::schema::{FieldType, Value};
use serde_json::{json, Map, Value as Json};
use std::path::{Path, PathBuf};

/// File extension of prefabs
pub const EXTENSION: &str = "prefab";

pub struct Prefab {
    /// Where it was loaded from; None for one made in memory
    pub file: Option<PathBuf>,
    /// The entity's name, which instances take unless they have their own
    pub name: Option<String>,
    /// The entity's components as the file writes them
    pub components: Map<String, Json>,
}

impl Prefab {
    /// A prefab file's text (the scene file of one entity, as
    /// Engine::prefab_of writes it)
    pub fn parse(text: &str) -> Result<Prefab, String> {
        let scene: Json = serde_json::from_str(text).map_err(|e| format!("prefab: {e}"))?;
        match scene.get("orst_scene").and_then(Json::as_u64) {
            Some(v) if v <= SCENE_VERSION => {}
            Some(v) => return Err(format!("prefab version {v} is newer than this engine")),
            None => return Err("not an ORST prefab (no \"orst_scene\")".into()),
        }
        let entity = match scene.get("entities").and_then(Json::as_array) {
            Some(list) if list.len() == 1 => &list[0],
            _ => return Err("a prefab holds exactly one entity".into()),
        };
        let components = match entity.get("components") {
            None => Map::new(),
            Some(Json::Object(c)) => c.clone(),
            Some(_) => return Err("prefab: \"components\" is not an object".into()),
        };
        Ok(Prefab {
            file: None,
            name: entity.get("name").and_then(Json::as_str).map(String::from),
            components,
        })
    }
}

impl Prefab {
    /// The prefab as a file
    pub fn text(&self) -> String {
        let mut entity = Map::new();
        entity.insert("id".into(), json!(1));
        if let Some(name) = &self.name {
            entity.insert("name".into(), json!(name));
        }
        entity.insert("components".into(), Json::Object(self.components.clone()));
        let scene = json!({"orst_scene": SCENE_VERSION, "entities": [entity]});
        serde_json::to_string_pretty(&scene).unwrap()
    }
}

impl Engine {
    /// Load a prefab file; one already loaded from the same file is shared
    pub fn load_prefab(&mut self, path: &str) -> Result<u32, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let mut prefab = Prefab::parse(&text).map_err(|e| format!("{path}: {e}"))?;
        prefab.file = Some(PathBuf::from(path));
        Ok(self.prefabs.insert(prefab))
    }

    /// The file a prefab was loaded from
    pub fn prefab_file(&self, prefab: u32) -> Option<&Path> {
        self.prefabs.get(prefab)?.file.as_deref()
    }

    /// The prefab `e` is an instance of
    pub fn prefab_instance(&self, e: Entity) -> Option<u32> {
        read::<PrefabInstance>(&self.world, e, PREFAB).map(|p| p.prefab)
    }

    /// Make `e` an instance of `prefab`, or not (0)
    pub fn set_prefab_instance(&mut self, e: Entity, prefab: u32) {
        if prefab == 0 {
            self.world.remove(e, PREFAB);
        } else {
            write(&mut self.world, e, PREFAB, &PrefabInstance { prefab });
        }
    }

    /// Instances of `prefab` among `entities`
    pub fn instances_of(&self, prefab: u32, entities: &[Entity]) -> Vec<Entity> {
        entities
            .iter()
            .copied()
            .filter(|&e| self.prefab_instance(e) == Some(prefab))
            .collect()
    }

    /// The prefab's components for an instance with scene id `id`: every
    /// field of a registered component written out (defaults where the
    /// file has none) and entity fields pointing at the instance or 0
    fn template(&self, prefab: u32, id: u64) -> Map<String, Json> {
        let Some(p) = self.prefabs.get(prefab) else {
            return Map::new();
        };
        let mut out = Map::new();
        for (cname, data) in &p.components {
            let (Some(c), Some(data)) = (self.world.component_id(cname), data.as_object()) else {
                out.insert(cname.clone(), data.clone());
                continue;
            };
            let storage = self.world.storage(c).unwrap();
            if storage.fields.is_empty() {
                out.insert(cname.clone(), Json::Object(data.clone()));
                continue;
            }
            let defaults = self.world.default_value(c);
            let mut fields = Map::new();
            for f in &storage.fields {
                let v = match (f.ty, data.get(&f.name)) {
                    (FieldType::Entity, Some(v)) => {
                        json!(if v.as_u64() == Some(1) { id } else { 0 })
                    }
                    (_, Some(v)) => v.clone(),
                    (_, None) => match (f.ty, f.read(&defaults)) {
                        (
                            FieldType::Texture | FieldType::Tilemap | FieldType::Polygon,
                            Value::U32(0),
                        ) => Json::Null,
                        (_, Value::Entity(_)) => json!(0),
                        (FieldType::String, _) => json!(""),
                        (_, v) => f.to_json(v),
                    },
                };
                fields.insert(f.name.clone(), v);
            }
            out.insert(cname.clone(), Json::Object(fields));
        }
        out
    }

    /// What an instance's components change against its prefab: the
    /// components and fields that differ, and the prefab's components it
    /// lacks
    pub(crate) fn overrides(
        &self,
        prefab: u32,
        id: u64,
        components: Map<String, Json>,
    ) -> (Map<String, Json>, Vec<String>) {
        let template = self.template(prefab, id);
        let removed: Vec<String> = template
            .keys()
            .filter(|k| !components.contains_key(*k) && self.world.component_id(k).is_some())
            .cloned()
            .collect();
        let mut changed = Map::new();
        for (cname, data) in components {
            let (Some(t), Some(fields)) = (
                template.get(&cname).and_then(Json::as_object),
                data.as_object(),
            ) else {
                changed.insert(cname, data);
                continue;
            };
            let differ: Map<String, Json> = fields
                .iter()
                .filter(|(k, v)| !t.get(*k).is_some_and(|tv| same(tv, v)))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if !differ.is_empty() {
                changed.insert(cname, Json::Object(differ));
            }
        }
        (changed, removed)
    }

    /// An instance's components: the prefab's without `removed`, with the
    /// instance's own fields on top
    pub(crate) fn instance_components(
        &self,
        prefab: u32,
        id: u64,
        own: Option<&Map<String, Json>>,
        removed: Option<&Vec<Json>>,
    ) -> Map<String, Json> {
        let mut out = self.template(prefab, id);
        for name in removed.into_iter().flatten().filter_map(Json::as_str) {
            out.remove(name);
        }
        for (cname, data) in own.into_iter().flatten() {
            match (out.get_mut(cname), data) {
                (Some(Json::Object(base)), Json::Object(fields)) => {
                    for (k, v) in fields {
                        base.insert(k.clone(), v.clone());
                    }
                }
                _ => {
                    out.insert(cname.clone(), data.clone());
                }
            }
        }
        out
    }
}

/// Whether two values from scene files mean the same: numbers equal as
/// the f32 the fields hold (0.1 in a file is 0.10000000149 once saved),
/// assets by uid when both have one and by path when one has only that
fn same(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(x), Json::Number(y)) => match (x.as_i64(), y.as_i64()) {
            (Some(x), Some(y)) => x == y,
            _ => x.as_f64().map(|v| v as f32) == y.as_f64().map(|v| v as f32),
        },
        (Json::Array(x), Json::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        // An asset by path alone and by uid and path
        (Json::String(p), Json::Object(o)) | (Json::Object(o), Json::String(p)) => {
            o.get("path").and_then(Json::as_str) == Some(p.as_str())
        }
        (Json::Object(x), Json::Object(y)) => match (x.get("uid"), y.get("uid")) {
            (Some(u), Some(v)) => u == v,
            _ => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        },
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Light, Transform, LIGHT, TRANSFORM};
    use crate::scene_file::Files;
    use crate::types::Vec2;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orst_prefab_{}_{}", name, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn instances_keep_only_what_they_change() {
        let dir = temp_dir("overrides");
        let mut engine = Engine::headless(8, 8).unwrap();
        let mut files = Files::new(&dir);
        let coin = engine.world.spawn();
        write(
            &mut engine.world,
            coin,
            crate::components::NAME,
            &crate::components::Name::new("Coin"),
        );
        let t = Transform {
            scale: Vec2::new(2.0, 2.0),
            ..Default::default()
        };
        write(&mut engine.world, coin, TRANSFORM, &t);
        let light = Light {
            radius: 0.1,
            ..Default::default()
        };
        write(&mut engine.world, coin, LIGHT, &light);
        let (text, warnings) = engine.prefab_of(coin, &files);
        assert!(warnings.is_empty(), "{warnings:?}");
        std::fs::write(dir.join("Coin.prefab"), &text).unwrap();
        engine.world.despawn(coin);

        let scene = r#"{"orst_scene": 1, "entities": [
            {"id": 1, "prefab": "Coin.prefab", "components": {"Transform": {"pos": [5, 6]}}},
            {"id": 2, "prefab": "Coin.prefab", "removed": ["Light"]}]}"#;
        let loaded = engine.load_scene(scene, &mut files).unwrap();
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        let [a, b] = loaded.entities[..] else {
            panic!()
        };
        let ta: Transform = read(&engine.world, a, TRANSFORM).unwrap();
        assert_eq!(
            (ta.pos, ta.scale),
            (Vec2::new(5.0, 6.0), Vec2::new(2.0, 2.0))
        );
        assert_eq!(read::<Light>(&engine.world, a, LIGHT).unwrap().radius, 0.1);
        assert!(!engine.world.has(b, LIGHT));
        assert_eq!(
            engine.world.find_named("Coin"),
            Some(a),
            "the prefab's name"
        );
        let p = engine.prefab_instance(a).unwrap();
        assert_eq!(engine.prefab_instance(b), Some(p), "one prefab, shared");

        // Saved back, the instances hold their changes and nothing else
        let (saved, warnings) = engine.save_scene(&loaded.entities, &files);
        assert!(warnings.is_empty(), "{warnings:?}");
        let json: Json = serde_json::from_str(&saved).unwrap();
        let list = json["entities"].as_array().unwrap();
        assert_eq!(list[0]["prefab"], "Coin.prefab");
        assert_eq!(
            list[0]["components"],
            json!({"Transform": {"pos": [5.0, 6.0]}})
        );
        assert_eq!(list[1]["components"], json!({}));
        assert_eq!(list[1]["removed"], json!(["Light"]));

        // Changing the prefab changes what instances did not override
        let (mut item_b, _) = engine.scene_entity(b, &loaded.entities, &files);
        let mut t2 = t;
        t2.rotation = 1.5;
        write(&mut engine.world, a, TRANSFORM, &t2);
        let (text, _) = engine.prefab_of(a, &files);
        let mut updated = Prefab::parse(&text).unwrap();
        updated.file = engine.prefabs.get(p).unwrap().file.clone();
        *engine.prefabs.get_mut(p).unwrap() = updated;
        item_b["components"]["Velocity"] = json!({"linear": [1, 0]});
        let warnings = engine.refill_entity(b, &item_b, &loaded.entities, &mut files);
        assert!(warnings.is_empty(), "{warnings:?}");
        let tb: Transform = read(&engine.world, b, TRANSFORM).unwrap();
        assert_eq!(tb.rotation, 1.5);
        assert!(engine.world.has(b, crate::components::VELOCITY));
        assert_eq!(engine.prefab_instance(b), Some(p));
        assert!(!engine.world.has(b, LIGHT), "still removed");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_prefab_is_one_entity() {
        assert!(Prefab::parse(r#"{"orst_scene": 1, "entities": []}"#).is_err());
        let p =
            Prefab::parse(r#"{"orst_scene": 1, "entities": [{"id": 1, "name": "A"}]}"#).unwrap();
        assert_eq!(p.name.as_deref(), Some("A"));
        assert!(same(&json!(0.1), &json!(0.10000000149011612)));
        assert!(!same(&json!(4294967295u32), &json!(4294967294u32)));
        assert!(same(
            &json!({"uid": "x", "path": "a.png"}),
            &json!({"uid": "x", "path": "b.png"})
        ));
    }
}

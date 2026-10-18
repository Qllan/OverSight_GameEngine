//! Prefabs in the editor: making one from an entity, placing instances,
//! and Apply and Revert, which move an instance's changes into its prefab
//! or throw them away. The files and their format are the engine's
//! (orst::prefab).

use crate::inspector::Overrides;
use orst::asset_db::AssetDb;
use orst::components::{Name, NAME};
use orst::ecs::Entity;
use orst::prefab::{Prefab, EXTENSION};
use orst::scene_file::Assets;
use orst::schema::FieldType;
use orst::{Engine, Vec2};
use serde_json::{json, Map, Value as Json};

/// Where prefabs made in the editor go, under Assets
pub const DIR: &str = "Prefabs";

pub fn is_prefab(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case(EXTENSION))
}

/// A free path for a new prefab named `name`
pub fn new_path(assets: &AssetDb, name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " _-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = match name.trim() {
        "" => "Entity",
        n => n,
    };
    (1..)
        .map(|n| match n {
            1 => format!("{DIR}/{name}.{EXTENSION}"),
            n => format!("{DIR}/{name} {n}.{EXTENSION}"),
        })
        .find(|p| !assets.file(p).exists())
        .unwrap()
}

/// Write `e` as a prefab at `path` under Assets
pub fn write_prefab(
    engine: &Engine,
    assets: &AssetDb,
    e: Entity,
    path: &str,
) -> Result<(), String> {
    let (text, _) = engine.prefab_of(e, assets);
    let file = assets.file(path);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{path}: {err}"))?;
    }
    std::fs::write(&file, text).map_err(|err| format!("{path}: {err}"))
}

/// The scene's entities, numbered as the scene file numbers them
fn scope(engine: &Engine) -> Vec<Entity> {
    engine.world.query(&[])
}

/// The fields an instance changes against its prefab
pub fn overrides(engine: &Engine, assets: &AssetDb, e: Entity) -> Overrides {
    if engine.prefab_instance(e).is_none() {
        return Overrides::new();
    }
    let (item, _) = engine.scene_entity(e, &scope(engine), assets);
    let mut out = Overrides::new();
    for (c, fields) in item["components"].as_object().into_iter().flatten() {
        for f in fields.as_object().into_iter().flatten().map(|(f, _)| f) {
            out.insert((c.clone(), f.clone()));
        }
    }
    out
}

/// A new instance of the prefab at `path`, standing at `at`
pub fn place(
    engine: &mut Engine,
    assets: &mut AssetDb,
    path: &str,
    at: Vec2,
) -> Result<Entity, String> {
    let p = assets.load(engine, FieldType::Prefab, &path.into())?;
    let name = assets
        .name(engine, FieldType::Prefab, p)
        .unwrap_or(json!(path));
    let e = engine.world.spawn();
    let all = scope(engine);
    let id = all.iter().position(|&x| x == e).unwrap() + 1;
    let item = json!({"id": id, "prefab": name,
                      "components": {"Transform": {"pos": [at.x, at.y]}}});
    engine.refill_entity(e, &item, &all, assets);
    if !engine.world.has(e, NAME) {
        let stem = path.rsplit('/').next().unwrap_or(path);
        let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
        orst::components::write(&mut engine.world, e, NAME, &Name::new(stem));
    }
    Ok(e)
}

/// Throw away what the instance changes but its position and name
pub fn revert(engine: &mut Engine, assets: &mut AssetDb, e: Entity) -> Vec<String> {
    let all = scope(engine);
    let (mut item, _) = engine.scene_entity(e, &all, assets);
    let pos = item["components"]["Transform"].get("pos").cloned();
    let mut components = Map::new();
    if let Some(pos) = pos {
        components.insert("Transform".into(), json!({ "pos": pos }));
    }
    item["components"] = Json::Object(components);
    if let Some(o) = item.as_object_mut() {
        o.remove("removed");
    }
    engine.refill_entity(e, &item, &all, assets)
}

/// Put what the instance changes into its prefab (its position aside),
/// write the file, and update the other instances, which keep their own
/// changes; how many instances there are besides `e`
pub fn apply(engine: &mut Engine, assets: &mut AssetDb, e: Entity) -> Result<usize, String> {
    let p = engine.prefab_instance(e).ok_or("not a prefab instance")?;
    let file = engine
        .prefab_file(p)
        .ok_or("the prefab has no file")?
        .to_path_buf();
    let all = scope(engine);
    let others: Vec<(Entity, Json)> = engine
        .instances_of(p, &all)
        .into_iter()
        .filter(|&o| o != e)
        .map(|o| (o, engine.scene_entity(o, &all, assets).0))
        .collect();
    let (text, _) = engine.prefab_of(e, assets);
    let mut prefab = Prefab::parse(&text)?;
    // Where the prefab stood stays: each instance has its own position
    let old = engine.prefabs.get(p).unwrap();
    let old_pos = old
        .components
        .get("Transform")
        .and_then(|t| t.get("pos"))
        .cloned();
    if let Some(Json::Object(t)) = prefab.components.get_mut("Transform") {
        match old_pos {
            Some(pos) => t.insert("pos".into(), pos),
            None => t.remove("pos"),
        };
    }
    prefab.file = Some(file.clone());
    std::fs::write(&file, prefab.text()).map_err(|err| format!("{}: {err}", file.display()))?;
    *engine.prefabs.get_mut(p).unwrap() = prefab;
    for (o, item) in &others {
        engine.refill_entity(*o, item, &all, assets);
    }
    Ok(others.len())
}

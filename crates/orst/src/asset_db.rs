//! The asset database: every file of a project folder with a lasting id.
//!
//! Next to each asset lies `<file>.meta`, a small JSON file holding its uid
//! (and later its import settings). Scenes point at assets by uid and path
//! together, so renaming or moving a file in the editor (which moves its
//! .meta along) breaks nothing, and a path still works when a .meta was
//! lost. .meta files belong in git with their assets.
//!
//! `scan` reads the folder: files without a .meta get one; a .meta copied
//! along with its file (two files, one uid) gives the copy a new uid; a
//! .meta whose file is gone is reported, not deleted, since the file may
//! come back with it.

use crate::engine::Engine;
use crate::scene_file::{Assets, Files};
use crate::schema::FieldType;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};

const META: &str = "meta";

/// What scan did
#[derive(Debug, Default, PartialEq)]
pub struct Scan {
    /// Assets that got a new .meta
    pub created: Vec<String>,
    /// Copies that shared a uid with another file and got a new one
    pub renewed: Vec<String>,
    /// .meta files whose asset is gone
    pub orphans: Vec<String>,
    /// .meta files that could not be read, and were replaced
    pub broken: Vec<String>,
}

pub struct AssetDb {
    pub root: PathBuf,
    /// Asset path ('/' between parts) -> uid
    by_path: BTreeMap<String, String>,
    by_uid: BTreeMap<String, String>,
}

impl AssetDb {
    /// Read every asset under `root`, writing the .meta files it lacks
    pub fn scan(root: impl Into<PathBuf>) -> Result<(AssetDb, Scan), String> {
        let root = root.into();
        let mut files = Vec::new();
        walk(&root, &root, &mut files).map_err(|e| format!("{}: {e}", root.display()))?;
        files.sort();
        let mut db = AssetDb {
            root,
            by_path: BTreeMap::new(),
            by_uid: BTreeMap::new(),
        };
        let mut scan = Scan::default();
        let metas: Vec<&String> = files.iter().filter(|f| is_meta(f)).collect();
        for meta in metas {
            let asset = &meta[..meta.len() - META.len() - 1];
            if files.binary_search_by(|f| f.as_str().cmp(asset)).is_err() {
                scan.orphans.push(asset.to_string());
            }
        }
        for path in files.iter().filter(|f| !is_meta(f)) {
            let uid = match db.read_meta(path) {
                Some(Ok(uid)) if !db.by_uid.contains_key(&uid) => uid,
                found => {
                    let uid = new_uid();
                    db.write_meta(path, &uid)?;
                    match found {
                        None => scan.created.push(path.clone()),
                        Some(Ok(_)) => scan.renewed.push(path.clone()),
                        Some(Err(())) => scan.broken.push(path.clone()),
                    }
                    uid
                }
            };
            db.by_uid.insert(uid.clone(), path.clone());
            db.by_path.insert(path.clone(), uid);
        }
        Ok((db, scan))
    }

    pub fn uid(&self, path: &str) -> Option<&str> {
        self.by_path.get(path).map(String::as_str)
    }

    pub fn path(&self, uid: &str) -> Option<&str> {
        self.by_uid.get(uid).map(String::as_str)
    }

    /// Asset paths in order
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.by_path.keys().map(String::as_str)
    }

    /// Rename or move an asset with its .meta, keeping its uid
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        let uid = self
            .by_path
            .get(from)
            .cloned()
            .ok_or_else(|| format!("no asset {from}"))?;
        if self.by_path.contains_key(to) || self.file(to).exists() {
            return Err(format!("{to} already exists"));
        }
        if let Some(dir) = self.file(to).parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{to}: {e}"))?;
        }
        std::fs::rename(self.file(from), self.file(to)).map_err(|e| format!("{from}: {e}"))?;
        let moved = std::fs::rename(self.meta_file(from), self.meta_file(to));
        if moved.is_err() {
            self.write_meta(to, &uid)?;
        }
        self.by_path.remove(from);
        self.by_path.insert(to.to_string(), uid.clone());
        self.by_uid.insert(uid, to.to_string());
        Ok(())
    }

    /// The file of an asset path
    pub fn file(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    fn meta_file(&self, path: &str) -> PathBuf {
        self.root.join(format!("{path}.{META}"))
    }

    /// None: no .meta; Err: one that does not say a uid
    fn read_meta(&self, path: &str) -> Option<Result<String, ()>> {
        let text = std::fs::read_to_string(self.meta_file(path)).ok()?;
        let uid = serde_json::from_str::<Json>(&text)
            .ok()
            .and_then(|m| m.get("uid")?.as_str().map(str::to_string))
            .filter(|u| valid_uid(u));
        Some(uid.ok_or(()))
    }

    fn write_meta(&self, path: &str, uid: &str) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&json!({"uid": uid})).unwrap() + "\n";
        std::fs::write(self.meta_file(path), text).map_err(|e| format!("{path}.{META}: {e}"))
    }
}

/// Scenes name assets as {"uid": ..., "path": ...}: the uid finds the file
/// wherever it moved, the path is for people reading the scene and for
/// assets that lost their .meta. A plain path string also loads.
impl Assets for AssetDb {
    fn name(&self, engine: &Engine, ty: FieldType, id: u32) -> Option<Json> {
        let path = Files::new(&self.root).name(engine, ty, id)?;
        let path = path.as_str()?;
        Some(match self.uid(path) {
            Some(uid) => json!({"uid": uid, "path": path}),
            None => json!(path),
        })
    }

    fn load(&mut self, engine: &mut Engine, ty: FieldType, name: &Json) -> Result<u32, String> {
        let path = match name {
            Json::Object(o) => {
                let by_uid = o
                    .get("uid")
                    .and_then(Json::as_str)
                    .and_then(|u| self.path(u));
                match (by_uid, o.get("path").and_then(Json::as_str)) {
                    (Some(p), _) | (None, Some(p)) => p.to_string(),
                    (None, None) => {
                        return Err("an asset with neither a known uid nor a path".into())
                    }
                }
            }
            _ => name.as_str().ok_or("not an asset")?.to_string(),
        };
        Files::new(&self.root).load(engine, ty, &json!(path))
    }
}

fn is_meta(path: &str) -> bool {
    path.strip_suffix(META).is_some_and(|p| p.ends_with('.'))
}

/// Files under `dir` as paths from `root`, skipping hidden ones (.git)
fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            let parts: Option<Vec<&str>> = rel.iter().map(|p| p.to_str()).collect();
            if let Some(parts) = parts {
                out.push(parts.join("/"));
            }
        }
    }
    Ok(())
}

/// 32 hex digits from the system's random hash keys and the clock
fn new_uid() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let half = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(now);
        h.write_u64(salt);
        h.finish()
    };
    format!("{:016x}{:016x}", half(1), half(2))
}

fn valid_uid(uid: &str) -> bool {
    uid.len() == 32 && uid.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{read, write, SpriteRenderer, SPRITE_RENDERER};

    fn project(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orst_assets_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sprites")).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 255]))
            .save(dir.join("sprites/hero.png"))
            .unwrap();
        std::fs::write(dir.join("notes.txt"), "hi").unwrap();
        dir
    }

    #[test]
    fn scan_gives_every_asset_a_lasting_uid() {
        let dir = project("scan");
        let (db, scan) = AssetDb::scan(&dir).unwrap();
        assert_eq!(scan.created, ["notes.txt", "sprites/hero.png"]);
        let uid = db.uid("sprites/hero.png").unwrap().to_string();
        assert!(valid_uid(&uid) && dir.join("sprites/hero.png.meta").exists());

        // A second scan reads the same uids and writes nothing
        let (db2, scan2) = AssetDb::scan(&dir).unwrap();
        assert_eq!(
            (db2.uid("sprites/hero.png"), scan2),
            (Some(uid.as_str()), Scan::default())
        );

        // A file copied with its .meta gets its own uid; a lost asset leaves an orphan
        std::fs::copy(dir.join("sprites/hero.png"), dir.join("sprites/copy.png")).unwrap();
        std::fs::copy(
            dir.join("sprites/hero.png.meta"),
            dir.join("sprites/copy.png.meta"),
        )
        .unwrap();
        std::fs::remove_file(dir.join("notes.txt")).unwrap();
        std::fs::write(dir.join("sprites/hero.png.meta"), "{oops").unwrap();
        let (db3, scan3) = AssetDb::scan(&dir).unwrap();
        assert_eq!(scan3.orphans, ["notes.txt"]);
        assert_eq!(scan3.broken, ["sprites/hero.png"]);
        assert!(
            scan3.renewed.is_empty(),
            "the copy keeps the uid once the original lost it"
        );
        assert_eq!(db3.uid("sprites/copy.png"), Some(uid.as_str()));
        let (_, scan4) = AssetDb::scan(&dir).unwrap();
        assert_eq!(scan4.orphans, ["notes.txt"]);
        assert!(scan4.broken.is_empty() && scan4.renewed.is_empty());
    }

    #[test]
    fn copies_with_the_same_uid_are_told_apart() {
        let dir = project("copies");
        AssetDb::scan(&dir).unwrap();
        std::fs::copy(dir.join("sprites/hero.png"), dir.join("sprites/hero2.png")).unwrap();
        std::fs::copy(
            dir.join("sprites/hero.png.meta"),
            dir.join("sprites/hero2.png.meta"),
        )
        .unwrap();
        let (db, scan) = AssetDb::scan(&dir).unwrap();
        assert_eq!(
            scan.renewed,
            ["sprites/hero2.png"],
            "the later path in order is the copy"
        );
        assert_ne!(db.uid("sprites/hero.png"), db.uid("sprites/hero2.png"));
    }

    #[test]
    fn scenes_follow_assets_that_moved() {
        let dir = project("moved");
        let (mut db, _) = AssetDb::scan(&dir).unwrap();
        let mut e = Engine::headless(8, 8).unwrap();
        let texture = e
            .load_texture(dir.join("sprites/hero.png").to_str().unwrap())
            .unwrap();
        let hero = e.world.spawn();
        write(
            &mut e.world,
            hero,
            SPRITE_RENDERER,
            &SpriteRenderer {
                texture,
                ..Default::default()
            },
        );
        let (text, warnings) = e.save_scene(&[hero], &db);
        assert!(warnings.is_empty(), "{warnings:?}");
        let uid = db.uid("sprites/hero.png").unwrap().to_string();
        assert!(text.contains(&uid) && text.contains("\"sprites/hero.png\""));

        db.rename("sprites/hero.png", "art/player.png").unwrap();
        assert!(dir.join("art/player.png.meta").exists() && !dir.join("sprites/hero.png").exists());
        assert_eq!(db.uid("art/player.png"), Some(uid.as_str()));
        let mut f = Engine::headless(8, 8).unwrap();
        let loaded = f.load_scene(&text, &mut db).unwrap();
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        let s: SpriteRenderer = read(&f.world, loaded.entities[0], SPRITE_RENDERER).unwrap();
        assert!(f
            .texture_file(s.texture)
            .unwrap()
            .ends_with("art/player.png"));
    }
}

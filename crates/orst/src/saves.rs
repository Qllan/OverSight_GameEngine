//! What a game keeps between runs, in a folder of the player's own (on
//! Windows %APPDATA%\<game>): saved games and preferences.
//!
//! A saved game is the whole world as a scene file (saves/<slot>.json),
//! with resources named by their path under the game's Assets, so it
//! loads in a later run whatever got loaded first. Loading one, like
//! loading another scene, happens at the start of the next step: the
//! script that asked for it finishes its tick in the old world. The
//! entities are new ones; scripts start again on them.
//!
//! Preferences (prefs.json) are text by key: the language, the volume,
//! the controls. Setting one writes the file.

use crate::scene_file::Files;
use crate::Engine;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SAVE_VERSION: u64 = 1;
const PREFS: &str = "prefs.json";
const SAVES: &str = "saves";

/// The folder for a game's own files on this system; None without a home
pub fn user_dir(game: &str) -> Option<PathBuf> {
    let name = clean(game);
    let name = if name.is_empty() { "ORST Game" } else { &name };
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        env("APPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else {
        env("XDG_DATA_HOME").or_else(|| Some(env("HOME")?.join(".local/share")))?
    };
    Some(base.join(name))
}

/// A name that is safe as one file name: letters, digits, space, - and _
pub fn clean(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || " -_".contains(*c))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Write through a temporary file, so a crash leaves the old one whole
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(err)?;
    std::fs::rename(&tmp, path).map_err(err)
}

/// Preferences: text by key, kept in a file
#[derive(Debug, Default)]
pub struct Prefs {
    file: Option<PathBuf>,
    values: BTreeMap<String, String>,
}

impl Prefs {
    /// The preferences in `file` (none yet when it is missing or broken)
    pub fn open(file: PathBuf) -> Prefs {
        let values = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<BTreeMap<String, Json>>(&t).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| match v {
                Json::String(s) => (k, s),
                v => (k, v.to_string()),
            })
            .collect();
        Prefs {
            file: Some(file),
            values,
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }

    /// Set (or with None remove) a preference and write the file
    pub fn set(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        let changed = match value {
            Some(v) => self.values.insert(key.into(), v.into()).as_deref() != Some(v),
            None => self.values.remove(key).is_some(),
        };
        match &self.file {
            Some(file) if changed => {
                let text = serde_json::to_string_pretty(&self.values).unwrap_or_default();
                write_atomic(file, &text)
            }
            _ => Ok(()),
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(|k| k.as_str())
    }
}

/// A world waiting for the next step
pub enum Pending {
    /// A scene file's text and its name for messages
    Scene(String, String),
    /// A saved game
    Save(Json),
}

impl Engine {
    /// Keep saves and preferences in `dir` (games get theirs from the
    /// window title; the editor names it after the project)
    pub fn set_save_dir(&mut self, dir: PathBuf) {
        self.prefs = Prefs::open(dir.join(PREFS));
        self.save_dir = Some(dir);
    }

    fn save_file(&self, slot: &str) -> Result<PathBuf, String> {
        let dir = self
            .save_dir
            .as_ref()
            .ok_or("the game has no save folder")?;
        let name = clean(slot);
        if name.is_empty() {
            return Err(format!("\"{slot}\" is not a save name"));
        }
        Ok(dir.join(SAVES).join(format!("{name}.json")))
    }

    fn files(&self) -> Files {
        Files::new(self.assets_dir.clone().unwrap_or_default())
    }

    /// Save the whole world in `slot`
    pub fn save_game(&mut self, slot: &str) -> Result<(), String> {
        let file = self.save_file(slot)?;
        let entities = self.world.query(&[]);
        let (scene, _) = self.save_scene(&entities, &self.files());
        let scene: Json = serde_json::from_str(&scene).map_err(|e| e.to_string())?;
        let save = json!({
            "orst_save": SAVE_VERSION,
            "rng": self.rng.state().to_string(),
            "tick": self.time.tick_count,
            "scene": scene,
        });
        let text = serde_json::to_string(&save).map_err(|e| e.to_string())?;
        write_atomic(&file, &text)
    }

    /// Load the game saved in `slot` at the next step
    pub fn load_game(&mut self, slot: &str) -> Result<(), String> {
        let file = self.save_file(slot)?;
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let save: Json = serde_json::from_str(&text).map_err(|e| format!("{slot}: {e}"))?;
        match save.get("orst_save").and_then(Json::as_u64) {
            Some(v) if v <= SAVE_VERSION => {}
            _ => return Err(format!("{slot}: not a save of this version")),
        }
        if !save.get("scene").is_some_and(Json::is_object) {
            return Err(format!("{slot}: the save has no world"));
        }
        self.pending_world = Some(Pending::Save(save));
        Ok(())
    }

    pub fn save_exists(&self, slot: &str) -> bool {
        self.save_file(slot).is_ok_and(|f| f.is_file())
    }

    pub fn delete_save(&mut self, slot: &str) -> bool {
        self.save_file(slot)
            .is_ok_and(|f| std::fs::remove_file(f).is_ok())
    }

    /// Put the scene at `path` under the game's Assets in place of the
    /// world at the next step
    pub fn load_scene_file(&mut self, path: &str) -> Result<(), String> {
        let file = self.files().root.join(path);
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{path}: {e}"))?;
        self.pending_world = Some(Pending::Scene(text, path.to_string()));
        Ok(())
    }

    /// A world asked for since the last step goes in (start of step).
    /// What it could not use goes to the last error.
    pub fn swap_world(&mut self) {
        let Some(pending) = self.pending_world.take() else {
            return;
        };
        let (text, name, save) = match pending {
            Pending::Scene(text, name) => (text, name, None),
            Pending::Save(save) => (save["scene"].to_string(), "the save".into(), Some(save)),
        };
        self.world.clear();
        self.entity_emitters.clear();
        self.collisions.set_touching(Vec::new());
        let mut files = self.files();
        let warnings = match self.load_scene(&text, &mut files) {
            Ok(loaded) => loaded.warnings,
            Err(e) => vec![e],
        };
        if let Some(save) = save {
            if let Some(rng) = save["rng"].as_str().and_then(|s| s.parse().ok()) {
                self.rng = crate::rng::Rng::new(rng);
            }
            if let Some(tick) = save["tick"].as_u64() {
                self.time.tick_count = tick;
            }
        }
        if !warnings.is_empty() {
            self.set_error(format!("{name}: {}", warnings.join("; ")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{read, write, Transform, UiText, TRANSFORM, UI_TEXT};
    use crate::types::Vec2;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orst_saves_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_saved_game_comes_back_in_a_new_engine() {
        let dir = temp("game");
        let mut e = Engine::headless(8, 8).unwrap();
        e.set_save_dir(dir.clone());
        let hero = e.world.spawn();
        let t = Transform {
            pos: Vec2::new(3.0, 4.0),
            ..Default::default()
        };
        write(&mut e.world, hero, TRANSFORM, &t);
        let label = UiText {
            text: e.strings.intern("Wave 3"),
            ..Default::default()
        };
        write(&mut e.world, hero, UI_TEXT, &label);
        e.rng = crate::rng::Rng::new(u64::MAX - 5);
        e.save_game("Slot 1/../x").unwrap();
        assert!(dir.join("saves/Slot 1x.json").is_file(), "a safe file name");
        assert!(e.save_exists("Slot 1/../x") && !e.save_exists("other"));

        // Another run: strings got other ids, the world has other things
        let mut f = Engine::headless(8, 8).unwrap();
        f.set_save_dir(dir.clone());
        f.strings.intern("something else");
        f.world.spawn();
        f.load_game("Slot 1x").unwrap();
        assert_eq!(f.world.count(), 1, "not before the step");
        f.step(0.0);
        let loaded = f.world.query(&[TRANSFORM]);
        assert_eq!(loaded.len(), 1);
        assert_eq!(f.world.count(), 1, "the old world went");
        let t2: Transform = read(&f.world, loaded[0], TRANSFORM).unwrap();
        assert_eq!(t2.pos, t.pos);
        let l2: UiText = read(&f.world, loaded[0], UI_TEXT).unwrap();
        assert_eq!(f.strings.get(l2.text), "Wave 3");
        assert_eq!(f.rng.state(), u64::MAX - 5);
        assert!(f.load_game("missing").is_err());
        assert!(f.delete_save("Slot 1x") && !f.save_exists("Slot 1x"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn prefs_persist_and_saves_need_a_folder() {
        let dir = temp("prefs");
        let mut e = Engine::headless(8, 8).unwrap();
        assert!(e.save_game("a").is_err(), "no folder yet");
        e.prefs.set("volume", Some("0.5")).unwrap(); // in memory only
        e.set_save_dir(dir.clone());
        e.prefs.set("language", Some("ru")).unwrap();
        e.prefs.set("gone", Some("1")).unwrap();
        e.prefs.set("gone", None).unwrap();
        let again = Prefs::open(dir.join(PREFS));
        assert_eq!(again.get("language"), Some("ru"));
        assert_eq!(again.keys().count(), 1);
        std::fs::write(dir.join(PREFS), "{broken").unwrap();
        assert_eq!(Prefs::open(dir.join(PREFS)).keys().count(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn user_folders_are_per_game() {
        let dir = user_dir("My: Game?");
        assert!(
            dir.as_ref().is_none_or(|d| d.ends_with("My Game")),
            "{dir:?}"
        );
    }
}

//! A project is a folder:
//!
//! ```text
//! MyGame/
//!   project.orst   name, engine version, script language, first scene
//!   Assets/        everything the game uses, each file with its .meta
//!   Library/       caches the editor can rebuild; not for git
//!   .gitignore
//! ```

use orst::asset_db::{AssetDb, Scan};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PROJECT_FILE: &str = "project.orst";
pub const ASSETS: &str = "Assets";
pub const LIBRARY: &str = "Library";
/// Where new scripts go, under Assets
pub const SCRIPTS: &str = "Scripts";

/// The script a new C# project starts with: the Coins sample's own
const PLAYER_MOVE: &str =
    include_str!("../../../samples/studio/Coins/Assets/Scripts/PlayerMove.cs");
/// The same in C++
const PLAYER_MOVE_CPP: &str = include_str!("../templates/PlayerMove.cpp");

impl Language {
    /// The extension of its script files
    pub fn extension(self) -> &'static str {
        match self {
            Language::CSharp => "cs",
            Language::Cpp => "cpp",
        }
    }
}

/// A new script's source: a Behaviour named `class`
pub fn script_source(class: &str, language: Language) -> String {
    if language == Language::Cpp {
        return format!(
            r#"#include "orst_script.hpp"

using namespace orst;

struct {class} : Behaviour {{
    // Fields listed in fields() show in the Inspector and scenes save them:
    // float speed = 2;
    // static void fields(Fields<{class}> &f) {{ f("Speed", &{class}::speed); }}

    void start() override {{}}

    void update(float dt) override {{}}
}};
ORST_BEHAVIOUR({class});
"#
        );
    }
    format!(
        r#"using ORST;

public class {class} : Behaviour
{{
    public override void Start()
    {{
    }}

    public override void Update(float dt)
    {{
    }}
}}
"#
    )
}

/// What the user typed as a script name, as a class name: "enemy
/// spawner" becomes EnemySpawner
pub fn class_name(name: &str) -> Result<String, String> {
    let name = name.trim().trim_end_matches(".cs").trim_end_matches(".cpp");
    let class: String = name
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect();
    match class.chars().next() {
        Some(c) if !c.is_ascii_digit() && class.is_ascii() => Ok(class),
        _ => Err(format!(
            "\"{name}\" can't be a script name: use English letters"
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    CSharp,
    Cpp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub name: String,
    /// The engine version that last saved it
    pub engine: String,
    pub language: Language,
    /// The scene the game starts with, under Assets
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_scene: Option<String>,
}

pub struct Project {
    pub root: PathBuf,
    pub file: ProjectFile,
    pub assets: AssetDb,
}

impl Project {
    /// A new project in `root` (an empty or missing folder)
    pub fn create(root: &Path, name: &str, language: Language) -> Result<Project, String> {
        if name.trim().is_empty() {
            return Err("the project needs a name".into());
        }
        if root.join(PROJECT_FILE).exists() {
            return Err(format!("{} already holds a project", root.display()));
        }
        if root.is_dir()
            && std::fs::read_dir(root)
                .map_err(|e| e.to_string())?
                .next()
                .is_some()
        {
            return Err(format!("{} is not empty", root.display()));
        }
        let io = |e: std::io::Error| format!("{}: {e}", root.display());
        std::fs::create_dir_all(root.join(ASSETS)).map_err(io)?;
        std::fs::write(root.join(".gitignore"), format!("/{LIBRARY}/\n/Build/\n")).map_err(io)?;
        let file = ProjectFile {
            name: name.trim().to_string(),
            engine: env!("CARGO_PKG_VERSION").to_string(),
            language,
            main_scene: None,
        };
        write_project_file(root, &file)?;
        let scripts = root.join(ASSETS).join(SCRIPTS);
        std::fs::create_dir_all(&scripts).map_err(io)?;
        let (file, text) = match language {
            Language::CSharp => ("PlayerMove.cs", PLAYER_MOVE),
            Language::Cpp => ("PlayerMove.cpp", PLAYER_MOVE_CPP),
        };
        std::fs::write(scripts.join(file), text).map_err(io)?;
        Ok(Project::open(root)?.0)
    }

    /// Open the project in `root`; the scan says what .meta files changed
    pub fn open(root: &Path) -> Result<(Project, Scan), String> {
        let path = root.join(PROJECT_FILE);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file: ProjectFile =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        std::fs::create_dir_all(root.join(ASSETS)).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(root.join(LIBRARY)).map_err(|e| e.to_string())?;
        let (assets, scan) = AssetDb::scan(root.join(ASSETS))?;
        Ok((
            Project {
                root: root.to_path_buf(),
                file,
                assets,
            },
            scan,
        ))
    }

    pub fn save(&self) -> Result<(), String> {
        write_project_file(&self.root, &self.file)
    }

    /// Read Assets again after files changed outside the editor
    pub fn rescan(&mut self) -> Result<Scan, String> {
        let (assets, scan) = AssetDb::scan(self.root.join(ASSETS))?;
        self.assets = assets;
        Ok(scan)
    }
}

pub(crate) fn write_project_file(root: &Path, file: &ProjectFile) -> Result<(), String> {
    let text = serde_json::to_string_pretty(file).unwrap() + "\n";
    let path = root.join(PROJECT_FILE);
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Projects opened lately, newest first, kept in the user's settings
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Recent {
    pub projects: Vec<PathBuf>,
}

impl Recent {
    const MAX: usize = 12;

    pub fn load(file: &Path) -> Recent {
        std::fs::read_to_string(file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, file: &Path) -> Result<(), String> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(file, serde_json::to_string_pretty(self).unwrap()).map_err(|e| e.to_string())
    }

    pub fn add(&mut self, root: &Path) {
        self.projects.retain(|p| p != root);
        self.projects.insert(0, root.to_path_buf());
        self.projects.truncate(Self::MAX);
    }
}

/// Where the editor keeps its own settings: %APPDATA%\ORST on Windows,
/// ~/.config/ORST elsewhere
pub fn settings_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("ORST")
}

#[cfg(test)]
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("orst_studio_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_are_created_and_opened_again() {
        let root = temp_dir("create").join("My Game");
        let p = Project::create(&root, " My Game ", Language::CSharp).unwrap();
        assert_eq!(p.file.name, "My Game");
        assert!(root.join("Assets").is_dir() && root.join("Library").is_dir());
        assert!(root.join("Assets/Scripts/PlayerMove.cs").is_file());
        assert_eq!(
            std::fs::read_to_string(root.join(".gitignore")).unwrap(),
            "/Library/\n/Build/\n"
        );
        std::fs::write(root.join("Assets/hero.png"), b"x").unwrap();
        let (again, scan) = Project::open(&root).unwrap();
        assert_eq!(
            (again.file, scan.created),
            (p.file, vec!["hero.png".to_string()])
        );
        assert!(
            Project::create(&root, "Other", Language::Cpp).is_err(),
            "never over a project"
        );
        let busy = temp_dir("busy");
        std::fs::create_dir_all(&busy).unwrap();
        std::fs::write(busy.join("x.txt"), b"x").unwrap();
        let err = Project::create(&busy, "X", Language::Cpp).err().unwrap();
        assert!(err.contains("not empty"));
    }

    #[test]
    fn script_names_become_class_names() {
        assert_eq!(class_name("enemy spawner").unwrap(), "EnemySpawner");
        assert_eq!(class_name(" Coin.cs ").unwrap(), "Coin");
        assert_eq!(class_name("hp_bar").unwrap(), "Hp_bar");
        assert!(class_name("2d").is_err() && class_name("").is_err());
        assert!(class_name("Монета").is_err());
        assert!(script_source("Coin", Language::CSharp).contains("public class Coin : Behaviour"));
        assert!(script_source("Coin", Language::Cpp).contains("ORST_BEHAVIOUR(Coin);"));
        assert_eq!(class_name("coin.cpp").unwrap(), "Coin");
    }

    #[test]
    fn recent_projects_keep_the_newest_first() {
        let file = temp_dir("recent").join("recent.json");
        let mut r = Recent::load(&file);
        for p in ["a", "b", "a"] {
            r.add(Path::new(p));
        }
        r.save(&file).unwrap();
        assert_eq!(
            Recent::load(&file).projects,
            [PathBuf::from("a"), PathBuf::from("b")]
        );
    }
}

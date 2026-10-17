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
        std::fs::write(root.join(".gitignore"), format!("/{LIBRARY}/\n")).map_err(io)?;
        let file = ProjectFile {
            name: name.trim().to_string(),
            engine: env!("CARGO_PKG_VERSION").to_string(),
            language,
            main_scene: None,
        };
        write_project_file(root, &file)?;
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

fn write_project_file(root: &Path, file: &ProjectFile) -> Result<(), String> {
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
        assert_eq!(
            std::fs::read_to_string(root.join(".gitignore")).unwrap(),
            "/Library/\n"
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

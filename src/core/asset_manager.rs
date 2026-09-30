//! AssetManager - registry of the game's asset files
//!
//! Assets are registered by id, either one by one or by scanning a
//! directory: `scan()` registers every file with a known extension under
//! the id "<relative path without extension>" (e.g. "voice/mia_001").
//! Loading the files is the renderer's job; the engine uses the registry
//! to resolve paths and to validate asset references in graphs.

use pyo3::prelude::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Asset type enumeration
#[derive(Debug, Clone, Copy, PartialEq)]
#[pyclass(skip_from_py_object)]
pub enum AssetType {
    Image,
    Audio,
    Font,
    Data,
    Unknown,
}

impl AssetType {
    /// Type for a file extension (case-insensitive)
    pub fn from_extension(extension: &str) -> AssetType {
        match extension.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" => AssetType::Image,
            "ogg" | "wav" | "mp3" | "flac" | "opus" => AssetType::Audio,
            "ttf" | "otf" | "woff" | "woff2" => AssetType::Font,
            "json" | "txt" | "csv" | "yaml" | "yml" => AssetType::Data,
            _ => AssetType::Unknown,
        }
    }

    /// Name used in parameter types, e.g. "asset:audio"
    pub fn kind(&self) -> &'static str {
        match self {
            AssetType::Image => "image",
            AssetType::Audio => "audio",
            AssetType::Font => "font",
            AssetType::Data => "data",
            AssetType::Unknown => "unknown",
        }
    }
}

/// Asset metadata
#[derive(Debug, Clone)]
#[pyclass(skip_from_py_object)]
pub struct AssetInfo {
    #[pyo3(get)]
    pub id: String,
    #[pyo3(get)]
    pub asset_type: AssetType,
    /// Path relative to the manager's base path
    #[pyo3(get)]
    pub path: String,
    #[pyo3(get)]
    pub loaded: bool,
}

#[pymethods]
impl AssetInfo {
    #[new]
    fn new(id: &str, asset_type: &AssetType, path: &str) -> Self {
        AssetInfo {
            id: id.to_string(),
            asset_type: *asset_type,
            path: path.to_string(),
            loaded: false,
        }
    }

    /// "image", "audio", "font", "data" or "unknown"
    #[getter]
    fn kind(&self) -> &'static str {
        self.asset_type.kind()
    }
}

/// AssetManager handles asset registration and path resolution
#[pyclass]
#[derive(Debug)]
pub struct AssetManager {
    base_path: PathBuf,
    assets: BTreeMap<String, AssetInfo>,
}

#[pymethods]
impl AssetManager {
    #[new]
    pub fn new() -> Self {
        AssetManager {
            base_path: PathBuf::from("assets"),
            assets: BTreeMap::new(),
        }
    }

    /// Set the base path for asset loading
    pub fn set_base_path(&mut self, path: &str) {
        self.base_path = PathBuf::from(path);
        log::info!("Asset base path set to: {:?}", self.base_path);
    }

    #[getter]
    pub fn base_path(&self) -> String {
        self.base_path.to_string_lossy().to_string()
    }

    /// Get the full path for an asset
    pub fn resolve_path(&self, asset_id: &str) -> Option<String> {
        self.assets
            .get(asset_id)
            .map(|info| self.base_path.join(&info.path).to_string_lossy().to_string())
    }

    /// Register an asset by id; `path` is relative to the base path
    pub fn register_asset(&mut self, id: &str, asset_type: &AssetType, path: &str) -> PyResult<()> {
        if self.assets.contains_key(id) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Asset '{}' already registered",
                id
            )));
        }
        log::debug!("Registered asset: {} ({}) -> {}", id, asset_type.kind(), path);
        self.assets.insert(id.to_string(), AssetInfo::new(id, asset_type, path));
        Ok(())
    }

    /// Register every file with a known extension under the base path.
    /// Ids are relative paths without extension, with "/" separators.
    /// Already registered ids are kept. Returns the number of new assets.
    pub fn scan(&mut self) -> PyResult<usize> {
        let mut files = Vec::new();
        collect_files(&self.base_path, &mut files).map_err(|e| {
            pyo3::exceptions::PyOSError::new_err(format!(
                "Cannot scan {}: {}",
                self.base_path.display(),
                e
            ))
        })?;
        files.sort();

        let mut added = 0;
        for file in files {
            let Ok(relative) = file.strip_prefix(&self.base_path) else { continue };
            let asset_type = relative
                .extension()
                .map(|e| AssetType::from_extension(&e.to_string_lossy()))
                .unwrap_or(AssetType::Unknown);
            if asset_type == AssetType::Unknown {
                continue;
            }
            let path = to_slashes(relative);
            let id = to_slashes(&relative.with_extension(""));
            if self.assets.contains_key(&id) {
                log::warn!("Asset id '{}' is taken; skipping {}", id, path);
                continue;
            }
            self.assets.insert(id.clone(), AssetInfo::new(&id, &asset_type, &path));
            added += 1;
        }
        log::info!("Scanned {}: {} assets", self.base_path.display(), added);
        Ok(added)
    }

    /// Check if an asset is registered
    pub fn has_asset(&self, id: &str) -> bool {
        self.assets.contains_key(id)
    }

    /// Get asset info
    pub fn get_asset(&self, id: &str) -> Option<AssetInfo> {
        self.assets.get(id).cloned()
    }

    /// Get all registered assets, sorted by id
    pub fn get_all_assets(&self) -> Vec<AssetInfo> {
        self.assets.values().cloned().collect()
    }

    /// Clear the asset registry
    pub fn clear(&mut self) {
        self.assets.clear();
        log::info!("Cleared asset registry");
    }

    /// Get asset count
    pub fn asset_count(&self) -> usize {
        self.assets.len()
    }
}

impl AssetManager {
    pub fn asset(&self, id: &str) -> Option<&AssetInfo> {
        self.assets.get(id)
    }

    /// Registered ids, sorted
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.assets.keys().map(String::as_str)
    }
}

impl Default for AssetManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Visible files below `dir`, recursively
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let hidden = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn to_slashes(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_registers_known_files_by_relative_id() {
        let dir = std::env::temp_dir().join(format!("vn_assets_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for file in ["bg/park.png", "voice/mia_001.ogg", "notes.md", ".hidden.png", "font/Main.TTF"] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }

        let mut assets = AssetManager::new();
        assets.set_base_path(&dir.to_string_lossy());
        assert_eq!(assets.scan().unwrap(), 3);
        assert_eq!(assets.ids().collect::<Vec<_>>(), vec!["bg/park", "font/Main", "voice/mia_001"]);
        assert_eq!(assets.asset("voice/mia_001").unwrap().asset_type, AssetType::Audio);
        assert_eq!(assets.asset("font/Main").unwrap().path, "font/Main.TTF");
        assert_eq!(assets.scan().unwrap(), 0, "rescanning adds nothing new");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

//! AssetManager - Asset loading and caching (stubs for Phase 1)
//!
//! In Phase 1, this provides path resolution stubs. Actual asset loading
//! (images, audio, fonts) is deferred to Phase 2 with pygame-ce integration.

use pyo3::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

/// Asset type enumeration
#[derive(Debug, Clone, PartialEq)]
#[pyclass(skip_from_py_object)]
pub enum AssetType {
    Image,
    Audio,
    Font,
    Data,
    Unknown,
}

/// Asset metadata
#[derive(Debug, Clone)]
#[pyclass(skip_from_py_object)]
pub struct AssetInfo {
    #[pyo3(get)]
    pub id: String,
    #[pyo3(get)]
    pub asset_type: AssetType,
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
            asset_type: asset_type.clone(),
            path: path.to_string(),
            loaded: false,
        }
    }
}

/// AssetManager handles asset loading, caching, and path resolution
#[pyclass]
pub struct AssetManager {
    base_path: PathBuf,
    assets: HashMap<String, AssetInfo>,
    cache_enabled: bool,
}

#[pymethods]
impl AssetManager {
    #[new]
    fn new() -> Self {
        AssetManager {
            base_path: PathBuf::from("assets"),
            assets: HashMap::new(),
            cache_enabled: true,
        }
    }

    /// Set the base path for asset loading
    fn set_base_path(&mut self, path: &str) {
        self.base_path = PathBuf::from(path);
        log::info!("Asset base path set to: {:?}", self.base_path);
    }

    /// Get the full path for an asset
    fn resolve_path(&self, asset_id: &str) -> Option<String> {
        self.assets.get(asset_id).map(|info| {
            self.base_path.join(&info.path).to_string_lossy().to_string()
        })
    }

    /// Register an asset without loading it (Phase 1 stub)
    fn register_asset(
        &mut self,
        id: &str,
        asset_type: &AssetType,
        path: &str,
    ) -> PyResult<()> {
        if self.assets.contains_key(id) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Asset '{}' already registered",
                id
            )));
        }

        let info = AssetInfo::new(id, asset_type, path);
        log::debug!("Registered asset: {} ({}) -> {}", id, path, match asset_type {
            AssetType::Image => "Image",
            AssetType::Audio => "Audio",
            AssetType::Font => "Font",
            AssetType::Data => "Data",
            AssetType::Unknown => "Unknown",
        });
        self.assets.insert(id.to_string(), info);
        Ok(())
    }

    /// Check if an asset is registered
    fn has_asset(&self, id: &str) -> bool {
        self.assets.contains_key(id)
    }

    /// Get asset info
    fn get_asset(&self, id: &str) -> Option<AssetInfo> {
        self.assets.get(id).cloned()
    }

    /// Get all registered assets
    fn get_all_assets(&self) -> Vec<AssetInfo> {
        self.assets.values().cloned().collect()
    }

    /// Enable or disable caching
    fn set_cache_enabled(&mut self, enabled: bool) {
        self.cache_enabled = enabled;
    }

    /// Clear the asset registry (not loaded assets in Phase 1)
    fn clear(&mut self) {
        self.assets.clear();
        log::info!("Cleared asset registry");
    }

    /// Get asset count
    fn asset_count(&self) -> usize {
        self.assets.len()
    }
}

impl Default for AssetManager {
    fn default() -> Self {
        Self::new()
    }
}

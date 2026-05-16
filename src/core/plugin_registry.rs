//! PluginRegistry - Runtime plugin discovery and management
//!
//! Plugins extend the engine with new block types, events, and functionality.
//! They are discovered at runtime and registered through a trait-based system.

use pyo3::prelude::*;
use std::collections::HashMap;

/// Block definition for visual scripting
#[derive(Debug, Clone)]
#[pyclass]
pub struct BlockDefinition {
    #[pyo3(get)]
    pub block_type: String,
    #[pyo3(get)]
    pub display_name: String,
    #[pyo3(get)]
    pub inputs: Vec<String>,
    #[pyo3(get)]
    pub outputs: Vec<String>,
    #[pyo3(get)]
    pub parameters: Vec<ParameterDefinition>,
}

#[pyclass]
#[derive(Debug, Clone)]
pub struct ParameterDefinition {
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub param_type: String,
    #[pyo3(get)]
    pub default: Option<String>,
}

#[pymethods]
impl ParameterDefinition {
    #[new]
    fn new(name: &str, param_type: &str, default: Option<&str>) -> Self {
        ParameterDefinition {
            name: name.to_string(),
            param_type: param_type.to_string(),
            default: default.map(String::from),
        }
    }
}

#[pymethods]
impl BlockDefinition {
    #[new]
    fn new(
        block_type: &str,
        display_name: &str,
        inputs: Vec<String>,
        outputs: Vec<String>,
        parameters: Vec<ParameterDefinition>,
    ) -> Self {
        BlockDefinition {
            block_type: block_type.to_string(),
            display_name: display_name.to_string(),
            inputs,
            outputs,
            parameters,
        }
    }
}

/// Plugin metadata and registration info
#[derive(Debug, Clone)]
#[pyclass]
pub struct PluginInfo {
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub version: String,
    #[pyo3(get)]
    pub author: String,
}

#[pymethods]
impl PluginInfo {
    #[new]
    fn new(name: &str, version: &str, author: &str) -> Self {
        PluginInfo {
            name: name.to_string(),
            version: version.to_string(),
            author: author.to_string(),
        }
    }
}

/// Registry for managing plugins and their block definitions
#[pyclass]
pub struct PluginRegistry {
    plugins: HashMap<String, PluginInfo>,
    blocks: HashMap<String, BlockDefinition>,
}

#[pymethods]
impl PluginRegistry {
    #[new]
    fn new() -> Self {
        PluginRegistry {
            plugins: HashMap::new(),
            blocks: HashMap::new(),
        }
    }

    /// Register a plugin with its metadata
    fn register_plugin(&mut self, info: &PluginInfo) -> PyResult<()> {
        if self.plugins.contains_key(&info.name) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Plugin '{}' already registered",
                info.name
            )));
        }
        log::info!("Registered plugin: {} v{}", info.name, info.version);
        self.plugins.insert(info.name.clone(), info.clone());
        Ok(())
    }

    /// Register a block definition from a plugin
    fn register_block(&mut self, plugin_name: &str, block: &BlockDefinition) -> PyResult<()> {
        if !self.plugins.contains_key(plugin_name) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Plugin '{}' not found",
                plugin_name
            )));
        }
        if self.blocks.contains_key(&block.block_type) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Block type '{}' already registered",
                block.block_type
            )));
        }
        log::debug!(
            "Registered block '{}' from plugin '{}'",
            block.block_type,
            plugin_name
        );
        self.blocks.insert(block.block_type.clone(), block.clone());
        Ok(())
    }

    /// Get a block definition by type
    fn get_block(&self, block_type: &str) -> Option<BlockDefinition> {
        self.blocks.get(block_type).cloned()
    }

    /// Check if a block type is registered
    fn has_block(&self, block_type: &str) -> bool {
        self.blocks.contains_key(block_type)
    }

    /// Get all registered block types
    fn get_all_blocks(&self) -> Vec<BlockDefinition> {
        self.blocks.values().cloned().collect()
    }

    /// Get all registered plugins
    fn get_all_plugins(&self) -> Vec<PluginInfo> {
        self.plugins.values().cloned().collect()
    }

    /// Unregister a plugin and its blocks
    fn unregister_plugin(&mut self, plugin_name: &str) -> PyResult<()> {
        if !self.plugins.contains_key(plugin_name) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Plugin '{}' not found",
                plugin_name
            )));
        }
        
        // Remove all blocks from this plugin
        let blocks_to_remove: Vec<String> = self
            .blocks
            .iter()
            .filter(|(_, b)| b.display_name.starts_with(plugin_name))
            .map(|(k, _)| k.clone())
            .collect();
        
        for block_type in blocks_to_remove {
            self.blocks.remove(&block_type);
        }
        
        self.plugins.remove(plugin_name);
        log::info!("Unregistered plugin: {}", plugin_name);
        Ok(())
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

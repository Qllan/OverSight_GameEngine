//! PluginRegistry - block definitions provided by plugins
//!
//! Control-flow nodes (entry, End, SetVariable, If, BranchChoice, ...) are
//! built into the compiler. Every other node is a *block*: an action the
//! host performs, described here so the compiler can validate its
//! parameters and generate the call.

use pyo3::prelude::*;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

use crate::pyjson::{json_to_py, py_to_json};

/// Node types handled by the compiler itself; blocks cannot use these names
pub const BUILTIN_NODES: [&str; 9] = [
    "entry",
    "End",
    "EndScene",
    "SetVariable",
    "If",
    "BranchChoice",
    "JumpScene",
    "CallScene",
    "Wait",
];

/// Parameter types a block can declare
pub const PARAM_TYPES: [&str; 10] = [
    "string",     // literal string
    "text",       // string with {expression} placeholders, expanded at runtime
    "expression", // expression evaluated at runtime; the block receives the value
    "number",
    "float", // alias of number
    "int",
    "bool",
    "array",
    "object",
    "any",
];

/// Block parameter: name, type and optional default (None = required)
#[pyclass(from_py_object)]
#[derive(Debug, Clone)]
pub struct ParameterDefinition {
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub param_type: String,
    pub default: Option<JsonValue>,
}

#[pymethods]
impl ParameterDefinition {
    /// `default` is any JSON-compatible value; None makes the parameter required
    #[new]
    #[pyo3(signature = (name, param_type, default = None))]
    fn py_new(name: &str, param_type: &str, default: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let default = default.map(py_to_json).transpose()?;
        Ok(Self::new(name, param_type, default))
    }

    #[getter]
    fn default(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.default.as_ref().map(|v| json_to_py(py, v)).transpose()
    }

    fn is_required(&self) -> bool {
        self.default.is_none()
    }

    fn __repr__(&self) -> String {
        match &self.default {
            Some(d) => format!(
                "ParameterDefinition({}: {} = {})",
                self.name, self.param_type, d
            ),
            None => format!("ParameterDefinition({}: {})", self.name, self.param_type),
        }
    }
}

impl ParameterDefinition {
    pub fn new(name: &str, param_type: &str, default: Option<JsonValue>) -> Self {
        ParameterDefinition {
            name: name.to_string(),
            param_type: param_type.to_string(),
            default,
        }
    }
}

/// Block definition for visual scripting
#[derive(Debug, Clone)]
#[pyclass(skip_from_py_object)]
pub struct BlockDefinition {
    #[pyo3(get)]
    pub block_type: String,
    #[pyo3(get)]
    pub display_name: String,
    #[pyo3(get)]
    pub inputs: Vec<String>,
    /// Output ports: empty for blocks that end the flow, else one ("next")
    #[pyo3(get)]
    pub outputs: Vec<String>,
    /// Arguments, passed to the host in this order
    #[pyo3(get)]
    pub parameters: Vec<ParameterDefinition>,
    /// If set, execution waits for the player after the block (YIELD with
    /// this reason, e.g. "input"), and resume() continues without a value
    #[pyo3(get)]
    pub wait: Option<String>,
}

#[pymethods]
impl BlockDefinition {
    #[new]
    #[pyo3(signature = (block_type, display_name, inputs, outputs, parameters, wait = None))]
    pub fn new(
        block_type: &str,
        display_name: &str,
        inputs: Vec<String>,
        outputs: Vec<String>,
        parameters: Vec<ParameterDefinition>,
        wait: Option<String>,
    ) -> Self {
        BlockDefinition {
            block_type: block_type.to_string(),
            display_name: display_name.to_string(),
            inputs,
            outputs,
            parameters,
            wait,
        }
    }

    fn __repr__(&self) -> String {
        format!("BlockDefinition('{}')", self.block_type)
    }
}

impl BlockDefinition {
    pub fn parameter(&self, name: &str) -> Option<&ParameterDefinition> {
        self.parameters.iter().find(|p| p.name == name)
    }

    /// Check the definition is something the compiler can generate code for
    pub fn check(&self) -> Result<(), String> {
        let name = &self.block_type;
        if name.is_empty() {
            return Err("Block type cannot be empty".to_string());
        }
        if BUILTIN_NODES.contains(&name.as_str()) {
            return Err(format!("'{}' is a built-in node type", name));
        }
        if self.outputs.len() > 1 {
            return Err(format!(
                "Block '{}' has {} outputs; blocks have at most one (use If or BranchChoice to branch)",
                name,
                self.outputs.len()
            ));
        }
        if self.wait.as_deref() == Some("choice") {
            return Err(format!(
                "Block '{}' cannot wait for 'choice'; that is reserved for BranchChoice",
                name
            ));
        }
        for (i, param) in self.parameters.iter().enumerate() {
            let base_type = param.param_type.split(':').next().unwrap_or_default();
            if !PARAM_TYPES.contains(&base_type) && base_type != "asset" {
                return Err(format!(
                    "Parameter '{}' of '{}' has unknown type '{}'",
                    param.name, name, param.param_type
                ));
            }
            if self.parameters[..i].iter().any(|p| p.name == param.name) {
                return Err(format!("Block '{}' declares '{}' twice", name, param.name));
            }
        }
        Ok(())
    }
}

/// Plugin metadata and registration info
#[derive(Debug, Clone)]
#[pyclass(skip_from_py_object)]
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
    pub fn new(name: &str, version: &str, author: &str) -> Self {
        PluginInfo {
            name: name.to_string(),
            version: version.to_string(),
            author: author.to_string(),
        }
    }
}

/// Registry for managing plugins and their block definitions
#[pyclass]
#[derive(Debug, Default)]
pub struct PluginRegistry {
    plugins: HashMap<String, PluginInfo>,
    blocks: HashMap<String, BlockDefinition>,
    /// block_type -> name of the plugin that registered it
    block_owners: HashMap<String, String>,
}

/// The given registry, or a new one with the default blocks
pub fn shared_registry(
    py: Python<'_>,
    registry: Option<Py<PluginRegistry>>,
) -> PyResult<Py<PluginRegistry>> {
    match registry {
        Some(registry) => Ok(registry),
        None => Py::new(py, PluginRegistry::with_defaults().map_err(value_error)?),
    }
}

fn value_error(message: String) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(message)
}

#[pymethods]
impl PluginRegistry {
    /// An empty registry. `with_defaults=True` installs the built-in
    /// novel_plugin (ShowText, PlayVoice).
    #[new]
    #[pyo3(signature = (with_defaults = false))]
    fn py_new(with_defaults: bool) -> PyResult<Self> {
        if with_defaults {
            Self::with_defaults().map_err(value_error)
        } else {
            Ok(Self::default())
        }
    }

    /// Register a plugin with its metadata
    pub fn register_plugin(&mut self, info: &PluginInfo) -> PyResult<()> {
        self.add_plugin(info.clone()).map_err(value_error)
    }

    /// Register a block definition from a plugin
    pub fn register_block(&mut self, plugin_name: &str, block: &BlockDefinition) -> PyResult<()> {
        self.add_block(plugin_name, block.clone())
            .map_err(value_error)
    }

    /// Get a block definition by type
    pub fn get_block(&self, block_type: &str) -> Option<BlockDefinition> {
        self.blocks.get(block_type).cloned()
    }

    /// Check if a block type is registered
    pub fn has_block(&self, block_type: &str) -> bool {
        self.blocks.contains_key(block_type)
    }

    /// Check if a plugin is registered
    pub fn has_plugin(&self, plugin_name: &str) -> bool {
        self.plugins.contains_key(plugin_name)
    }

    /// Get all registered block types
    pub fn get_all_blocks(&self) -> Vec<BlockDefinition> {
        let mut blocks: Vec<BlockDefinition> = self.blocks.values().cloned().collect();
        blocks.sort_by(|a, b| a.block_type.cmp(&b.block_type));
        blocks
    }

    /// Get the name of the plugin that registered a block type
    pub fn get_block_owner(&self, block_type: &str) -> Option<String> {
        self.block_owners.get(block_type).cloned()
    }

    /// Get all registered plugins
    pub fn get_all_plugins(&self) -> Vec<PluginInfo> {
        let mut plugins: Vec<PluginInfo> = self.plugins.values().cloned().collect();
        plugins.sort_by(|a, b| a.name.cmp(&b.name));
        plugins
    }

    /// Unregister a plugin and its blocks
    pub fn unregister_plugin(&mut self, plugin_name: &str) -> PyResult<()> {
        if !self.plugins.contains_key(plugin_name) {
            return Err(value_error(format!("Plugin '{}' not found", plugin_name)));
        }

        let blocks_to_remove: Vec<String> = self
            .block_owners
            .iter()
            .filter(|(_, owner)| owner.as_str() == plugin_name)
            .map(|(block_type, _)| block_type.clone())
            .collect();

        for block_type in blocks_to_remove {
            self.blocks.remove(&block_type);
            self.block_owners.remove(&block_type);
        }

        self.plugins.remove(plugin_name);
        log::info!("Unregistered plugin: {}", plugin_name);
        Ok(())
    }
}

impl PluginRegistry {
    /// Registry with the built-in novel_plugin installed
    pub fn with_defaults() -> Result<Self, String> {
        let mut registry = Self::default();
        crate::plugins::novel_plugin::install(&mut registry)?;
        Ok(registry)
    }

    pub fn block(&self, block_type: &str) -> Option<&BlockDefinition> {
        self.blocks.get(block_type)
    }

    /// Registered block types, sorted
    pub fn block_types(&self) -> Vec<&str> {
        let mut types: Vec<&str> = self.blocks.keys().map(String::as_str).collect();
        types.sort();
        types
    }

    pub fn add_plugin(&mut self, info: PluginInfo) -> Result<(), String> {
        if self.plugins.contains_key(&info.name) {
            return Err(format!("Plugin '{}' already registered", info.name));
        }
        log::info!("Registered plugin: {} v{}", info.name, info.version);
        self.plugins.insert(info.name.clone(), info);
        Ok(())
    }

    pub fn add_block(&mut self, plugin_name: &str, block: BlockDefinition) -> Result<(), String> {
        if !self.plugins.contains_key(plugin_name) {
            return Err(format!("Plugin '{}' not found", plugin_name));
        }
        if self.blocks.contains_key(&block.block_type) {
            return Err(format!(
                "Block type '{}' already registered",
                block.block_type
            ));
        }
        block.check()?;
        log::debug!(
            "Registered block '{}' from plugin '{}'",
            block.block_type,
            plugin_name
        );
        self.block_owners
            .insert(block.block_type.clone(), plugin_name.to_string());
        self.blocks.insert(block.block_type.clone(), block);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn block(
        name: &str,
        outputs: usize,
        wait: Option<&str>,
        params: Vec<ParameterDefinition>,
    ) -> BlockDefinition {
        BlockDefinition::new(
            name,
            name,
            vec![],
            (0..outputs).map(|_| "next".to_string()).collect(),
            params,
            wait.map(String::from),
        )
    }

    #[test]
    fn invalid_definitions_are_rejected() {
        let mut registry = PluginRegistry::default();
        registry
            .add_plugin(PluginInfo::new("p", "1", "me"))
            .unwrap();
        let cases = [
            (block("If", 1, None, vec![]), "built-in"),
            (block("Fork", 2, None, vec![]), "at most one"),
            (block("Ask", 1, Some("choice"), vec![]), "reserved"),
            (
                block(
                    "Bad",
                    1,
                    None,
                    vec![ParameterDefinition::new("x", "colour", None)],
                ),
                "unknown type",
            ),
            (
                block(
                    "Twice",
                    1,
                    None,
                    vec![
                        ParameterDefinition::new("x", "int", None),
                        ParameterDefinition::new("x", "int", Some(json!(1))),
                    ],
                ),
                "twice",
            ),
        ];
        for (definition, message) in cases {
            let err = registry.add_block("p", definition).unwrap_err();
            assert!(err.contains(message), "{}", err);
        }
        registry
            .add_block(
                "p",
                block(
                    "Shake",
                    1,
                    None,
                    vec![ParameterDefinition::new("s", "asset:image", None)],
                ),
            )
            .unwrap();
    }

    #[test]
    fn defaults_include_novel_blocks() {
        let registry = PluginRegistry::with_defaults().unwrap();
        assert_eq!(
            registry.block_types(),
            vec![
                "HideCharacter",
                "PlayMusic",
                "PlaySound",
                "PlayVoice",
                "ShowBackground",
                "ShowCharacter",
                "ShowText",
                "StopMusic"
            ]
        );
        assert_eq!(
            registry.block("ShowText").unwrap().wait.as_deref(),
            Some("input")
        );
    }
}

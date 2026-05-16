//! Novel Plugin - Example plugin for visual novel functionality
//!
//! Provides blocks: ShowText, PlayVoice, BranchChoice, SetVariable

use pyo3::prelude::*;
use crate::core::plugin_registry::{BlockDefinition, ParameterDefinition, PluginInfo};

/// Register the novel plugin with the registry
#[pyclass]
pub struct NovelPlugin;

#[pymethods]
impl NovelPlugin {
    #[new]
    fn new() -> Self {
        NovelPlugin
    }

    /// Get plugin info
    fn get_info(&self) -> PluginInfo {
        PluginInfo::new("novel_plugin", "1.0.0", "VN Engine Team")
    }

    /// Register all novel plugin blocks
    fn register_blocks(&self, registry: &mut crate::core::plugin_registry::PluginRegistry) -> PyResult<()> {
        let plugin_info = self.get_info();
        registry.register_plugin(&plugin_info)?;

        // ShowText block
        let show_text = BlockDefinition::new(
            "ShowText",
            "Show Text",
            vec!["exec".to_string()],
            vec!["next".to_string()],
            vec![
                ParameterDefinition::new("text", "string", Some("")),
                ParameterDefinition::new("speaker", "string", Some("")),
            ],
        );
        registry.register_block("novel_plugin", &show_text)?;

        // PlayVoice block
        let play_voice = BlockDefinition::new(
            "PlayVoice",
            "Play Voice",
            vec!["exec".to_string()],
            vec!["next".to_string()],
            vec![
                ParameterDefinition::new("voice_id", "string", Some("")),
                ParameterDefinition::new("volume", "float", Some("1.0")),
            ],
        );
        registry.register_block("novel_plugin", &play_voice)?;

        // BranchChoice block
        let branch_choice = BlockDefinition::new(
            "BranchChoice",
            "Branch Choice",
            vec!["exec".to_string()],
            vec!["choice_0".to_string(), "choice_1".to_string()],
            vec![
                ParameterDefinition::new("choices", "array", Some("[]")),
            ],
        );
        registry.register_block("novel_plugin", &branch_choice)?;

        // SetVariable block
        let set_var = BlockDefinition::new(
            "SetVariable",
            "Set Variable",
            vec!["exec".to_string()],
            vec!["next".to_string()],
            vec![
                ParameterDefinition::new("name", "string", None),
                ParameterDefinition::new("value", "any", None),
            ],
        );
        registry.register_block("novel_plugin", &set_var)?;

        // EndScene block
        let end_scene = BlockDefinition::new(
            "EndScene",
            "End Scene",
            vec!["exec".to_string()],
            vec![],
            vec![],
        );
        registry.register_block("novel_plugin", &end_scene)?;

        log::info!("Registered novel_plugin blocks");
        Ok(())
    }

    /// Get list of block types provided by this plugin
    fn get_block_types(&self) -> Vec<String> {
        vec![
            "ShowText".to_string(),
            "PlayVoice".to_string(),
            "BranchChoice".to_string(),
            "SetVariable".to_string(),
            "EndScene".to_string(),
        ]
    }
}

impl Default for NovelPlugin {
    fn default() -> Self {
        Self::new()
    }
}

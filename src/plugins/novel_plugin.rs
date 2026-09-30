//! Novel Plugin - the standard visual novel blocks
//!
//! Provides blocks: ShowText (waits for the player), PlayVoice, and the
//! stage blocks ShowBackground, ShowCharacter, HideCharacter, PlayMusic,
//! StopMusic, PlaySound (drawn and played by vn_engine.player).
//! Branching (BranchChoice, If) and variables (SetVariable) are built into
//! the compiler.

use pyo3::prelude::*;
use serde_json::json;

use crate::core::plugin_registry::{
    BlockDefinition, ParameterDefinition, PluginInfo, PluginRegistry,
};

pub const PLUGIN_NAME: &str = "novel_plugin";

pub fn info() -> PluginInfo {
    PluginInfo::new(PLUGIN_NAME, "1.1.0", "VN Engine Team")
}

pub fn blocks() -> Vec<BlockDefinition> {
    vec![
        BlockDefinition::new(
            "ShowText",
            "Show Text",
            vec!["exec".to_string()],
            vec!["next".to_string()],
            vec![
                ParameterDefinition::new("text", "text", None),
                ParameterDefinition::new("speaker", "text", Some(json!(""))),
            ],
            Some("input".to_string()),
        ),
        BlockDefinition::new(
            "PlayVoice",
            "Play Voice",
            vec!["exec".to_string()],
            vec!["next".to_string()],
            vec![
                ParameterDefinition::new("voice_id", "asset:audio", None),
                ParameterDefinition::new("volume", "number", Some(json!(1.0))),
            ],
            None,
        ),
        action(
            "ShowBackground",
            "Show Background",
            vec![ParameterDefinition::new("image", "asset:image", None)],
        ),
        action(
            "ShowCharacter",
            "Show Character",
            vec![
                ParameterDefinition::new("character", "string", None),
                ParameterDefinition::new("image", "asset:image", None),
                // left, center, right, far_left, far_right
                ParameterDefinition::new("position", "string", Some(json!("center"))),
            ],
        ),
        action(
            "HideCharacter",
            "Hide Character",
            vec![ParameterDefinition::new("character", "string", None)],
        ),
        action(
            "PlayMusic",
            "Play Music",
            vec![
                ParameterDefinition::new("music", "asset:audio", None),
                ParameterDefinition::new("volume", "number", Some(json!(0.8))),
                ParameterDefinition::new("loop", "bool", Some(json!(true))),
            ],
        ),
        action(
            "StopMusic",
            "Stop Music",
            vec![ParameterDefinition::new("fade_ms", "int", Some(json!(500)))],
        ),
        action(
            "PlaySound",
            "Play Sound",
            vec![
                ParameterDefinition::new("sound", "asset:audio", None),
                ParameterDefinition::new("volume", "number", Some(json!(1.0))),
            ],
        ),
    ]
}

/// A block that does something and continues
fn action(
    block_type: &str,
    display_name: &str,
    parameters: Vec<ParameterDefinition>,
) -> BlockDefinition {
    BlockDefinition::new(
        block_type,
        display_name,
        vec!["exec".to_string()],
        vec!["next".to_string()],
        parameters,
        None,
    )
}

/// Register the plugin and its blocks
pub fn install(registry: &mut PluginRegistry) -> Result<(), String> {
    registry.add_plugin(info())?;
    for block in blocks() {
        registry.add_block(PLUGIN_NAME, block)?;
    }
    log::info!("Registered novel_plugin blocks");
    Ok(())
}

/// Python handle for the plugin
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
        info()
    }

    /// Register all novel plugin blocks
    fn register_blocks(&self, registry: &mut PluginRegistry) -> PyResult<()> {
        install(registry).map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Get list of block types provided by this plugin
    fn get_block_types(&self) -> Vec<String> {
        blocks().into_iter().map(|b| b.block_type).collect()
    }
}

impl Default for NovelPlugin {
    fn default() -> Self {
        Self::new()
    }
}

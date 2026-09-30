//! Projects - several scenes compiled together
//!
//! ```json
//! {"project_id": "demo", "start": "intro",
//!  "variables": {"affection": 0},
//!  "scenes": [ {graph}, {graph}, ... ]}
//! ```
//! Scenes are graphs; JumpScene / CallScene nodes refer to them by
//! graph_id. `variables` gives initial values, applied when the game starts.
//! (The Python Engine also accepts file paths in `scenes`.)

use pyo3::prelude::*;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

use crate::compiler::codegen::{compile_graph, fnv_hex, scene_param, CompiledProgram};
use crate::compiler::graph_parser::{parse_json_graph, AstGraph};
use crate::compiler::validator::{validate_graph_with_assets, ValidationError};
use crate::core::asset_manager::AssetManager;
use crate::core::plugin_registry::PluginRegistry;
use crate::pyjson::json_to_py;

#[derive(Deserialize)]
struct JsonProject {
    project_id: String,
    start: String,
    #[serde(default)]
    variables: BTreeMap<String, JsonValue>,
    scenes: Vec<JsonValue>,
}

/// Compiled scenes plus the project's start scene and initial variables
#[pyclass(skip_from_py_object)]
#[derive(Debug, Clone)]
pub struct CompiledProject {
    #[pyo3(get)]
    pub project_id: String,
    #[pyo3(get)]
    pub start: String,
    pub variables: BTreeMap<String, JsonValue>,
    pub scenes: BTreeMap<String, CompiledProgram>,
}

#[pymethods]
impl CompiledProject {
    fn __repr__(&self) -> String {
        format!(
            "CompiledProject(id='{}', start='{}', scenes={})",
            self.project_id,
            self.start,
            self.scenes.len()
        )
    }

    /// Scene ids, sorted
    pub fn scene_ids(&self) -> Vec<String> {
        self.scenes.keys().cloned().collect()
    }

    pub fn get_scene(&self, scene_id: &str) -> Option<CompiledProgram> {
        self.scenes.get(scene_id).cloned()
    }

    /// Initial variable values
    #[getter]
    fn variables(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let object: serde_json::Map<String, JsonValue> = self
            .variables
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        json_to_py(py, &JsonValue::Object(object))
    }

    /// Hash of all scenes' bytecode
    pub fn fingerprint(&self) -> String {
        fnv_hex(
            self.scenes
                .iter()
                .map(|(id, p)| format!("{}:{}", id, p.fingerprint())),
        )
    }

    /// Disassembly of every scene
    pub fn disassemble(&self) -> String {
        self.scenes
            .values()
            .map(|p| p.disassemble())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

impl CompiledProject {
    /// A project made of one scene, which is also the start
    pub fn single(program: CompiledProgram) -> Self {
        let id = program.graph_id.clone();
        CompiledProject {
            project_id: id.clone(),
            start: id.clone(),
            variables: BTreeMap::new(),
            scenes: BTreeMap::from([(id, program)]),
        }
    }
}

#[derive(Debug)]
pub enum ProjectError {
    Parse(String),
    Invalid(Vec<ValidationError>),
    Compile(String),
}

fn error(
    kind: &str,
    message: String,
    graph: Option<&str>,
    node: Option<&str>,
    hint: &str,
) -> ValidationError {
    let mut e = ValidationError::new(kind, &message, node, Some(hint));
    e.graph_id = graph.map(String::from);
    e
}

/// Parse, validate and compile a project with inline scenes
pub fn compile_project(
    json: &str,
    registry: &PluginRegistry,
) -> Result<CompiledProject, ProjectError> {
    compile_project_with_assets(json, registry, None)
}

/// Like `compile_project`, also checking asset references
pub fn compile_project_with_assets(
    json: &str,
    registry: &PluginRegistry,
    assets: Option<&AssetManager>,
) -> Result<CompiledProject, ProjectError> {
    let project: JsonProject =
        serde_json::from_str(json).map_err(|e| ProjectError::Parse(format!("Project: {}", e)))?;

    let mut graphs: BTreeMap<String, AstGraph> = BTreeMap::new();
    let mut errors = Vec::new();

    for (index, scene) in project.scenes.iter().enumerate() {
        let graph = parse_json_graph(&scene.to_string())
            .map_err(|e| ProjectError::Parse(format!("Scene #{}: {}", index, e)))?;
        if graphs.contains_key(&graph.graph_id) {
            errors.push(error(
                "DUPLICATE_SCENE",
                format!("Two scenes have graph_id '{}'", graph.graph_id),
                Some(&graph.graph_id),
                None,
                "Give every scene a unique graph_id.",
            ));
            continue;
        }
        for mut e in validate_graph_with_assets(&graph, registry, assets) {
            e.graph_id = Some(graph.graph_id.clone());
            errors.push(e);
        }
        graphs.insert(graph.graph_id.clone(), graph);
    }

    if !graphs.contains_key(&project.start) {
        errors.push(error(
            "UNKNOWN_SCENE",
            format!("Start scene '{}' is not in the project", project.start),
            None,
            None,
            "Set 'start' to the graph_id of one of the scenes.",
        ));
    }

    // Scene references must resolve
    for graph in graphs.values() {
        for node_id in graph.get_node_ids() {
            let node = &graph.nodes[&node_id];
            if !matches!(node.node_type.as_str(), "JumpScene" | "CallScene") {
                continue;
            }
            if let Ok(target) = scene_param(node) {
                if !graphs.contains_key(&target) {
                    errors.push(error(
                        "UNKNOWN_SCENE",
                        format!("{} goes to unknown scene '{}'", node.node_type, target),
                        Some(&graph.graph_id),
                        Some(&node_id),
                        "Use the graph_id of a scene in the project.",
                    ));
                }
            }
        }
    }

    if !errors.is_empty() {
        return Err(ProjectError::Invalid(errors));
    }

    let mut scenes = BTreeMap::new();
    for (id, graph) in &graphs {
        let program = compile_graph(graph, registry)
            .map_err(|e| ProjectError::Compile(format!("Scene '{}': {}", id, e)))?;
        scenes.insert(id.clone(), program);
    }

    Ok(CompiledProject {
        project_id: project.project_id,
        start: project.start,
        variables: project.variables,
        scenes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(id: &str, nodes: &str, edges: &str) -> String {
        format!(
            r#"{{"graph_id":"{}","version":"1.0","nodes":[{}],"edges":[{}]}}"#,
            id, nodes, edges
        )
    }

    fn project(start: &str, scenes: &[String]) -> String {
        format!(
            r#"{{"project_id":"p","start":"{}","variables":{{"gold":10}},"scenes":[{}]}}"#,
            start,
            scenes.join(",")
        )
    }

    #[test]
    fn scene_references_are_checked() {
        let intro = scene(
            "intro",
            r#"{"id":"s","type":"entry"},{"id":"go","type":"JumpScene","parameters":{"scene":"nowhere"}}"#,
            r#"{"from":"s","to":"go"}"#,
        );
        let registry = PluginRegistry::with_defaults().unwrap();
        let Err(ProjectError::Invalid(errors)) =
            compile_project(&project("missing", &[intro.clone(), intro]), &registry)
        else {
            panic!("expected validation errors");
        };
        let kinds: Vec<(&str, Option<&str>)> = errors
            .iter()
            .map(|e| (e.error_type.as_str(), e.node_id.as_deref()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("DUPLICATE_SCENE", None),
                ("UNKNOWN_SCENE", None),
                ("UNKNOWN_SCENE", Some("go"))
            ]
        );
        assert_eq!(errors[2].graph_id.as_deref(), Some("intro"));
    }

    #[test]
    fn valid_project_compiles_every_scene() {
        let intro = scene(
            "intro",
            r#"{"id":"s","type":"entry"},{"id":"go","type":"JumpScene","parameters":{"scene":"end"}}"#,
            r#"{"from":"s","to":"go"}"#,
        );
        let end = scene("end", r#"{"id":"e","type":"End"}"#, "");
        let registry = PluginRegistry::with_defaults().unwrap();
        let compiled = compile_project(&project("intro", &[intro, end]), &registry).unwrap();
        assert_eq!(compiled.scene_ids(), vec!["end", "intro"]);
        assert_eq!(compiled.variables["gold"], serde_json::json!(10));
        assert_eq!(
            compiled.scenes["intro"].get_opcodes(),
            vec!["JUMP_SCENE(end)"]
        );
        assert_eq!(compiled.fingerprint().len(), 16);
    }
}

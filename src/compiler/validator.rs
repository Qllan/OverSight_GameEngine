//! Validator - Graph validation and error reporting
//!
//! Validates the AST graph for structural correctness:
//! - Every loop waits for the player at least once
//! - A single, unambiguous start node
//! - Node type validation
//! - Connection integrity
//! - Parameter validation

use crate::compiler::codegen::{node_waits_for_player, scene_param};
use crate::compiler::expr::{parse_expr, parse_template};
use crate::compiler::graph_parser::{AstGraph, AstNode};
use crate::core::plugin_registry::{
    shared_registry, BlockDefinition, ParameterDefinition, PluginRegistry, BUILTIN_NODES,
};
use pyo3::prelude::*;
use serde_json::Value as JsonValue;
use std::collections::HashSet;

/// Validation error with human-readable message
#[derive(Debug, Clone)]
#[pyclass(skip_from_py_object)]
pub struct ValidationError {
    #[pyo3(get)]
    pub error_type: String,
    #[pyo3(get)]
    pub message: String,
    #[pyo3(get)]
    pub node_id: Option<String>,
    #[pyo3(get)]
    pub hint: Option<String>,
    /// Scene the error is in (set when validating a project)
    #[pyo3(get)]
    pub graph_id: Option<String>,
}

#[pymethods]
impl ValidationError {
    #[new]
    #[pyo3(signature = (error_type, message, node_id = None, hint = None, graph_id = None))]
    fn py_new(
        error_type: &str,
        message: &str,
        node_id: Option<&str>,
        hint: Option<&str>,
        graph_id: Option<&str>,
    ) -> Self {
        let mut error = Self::new(error_type, message, node_id, hint);
        error.graph_id = graph_id.map(String::from);
        error
    }

    fn __repr__(&self) -> String {
        let node_info = match (&self.graph_id, &self.node_id) {
            (Some(g), Some(n)) => format!("{}/{}", g, n),
            (None, Some(n)) => n.clone(),
            (Some(g), None) => g.clone(),
            (None, None) => "unknown".to_string(),
        };
        format!(
            "ValidationError[{}]: {} (node: {})",
            self.error_type, self.message, node_info
        )
    }
}

impl ValidationError {
    pub fn new(error_type: &str, message: &str, node_id: Option<&str>, hint: Option<&str>) -> Self {
        ValidationError {
            error_type: error_type.to_string(),
            message: message.to_string(),
            node_id: node_id.map(String::from),
            hint: hint.map(String::from),
            graph_id: None,
        }
    }
}

/// Validate the AST graph. Errors are returned in a deterministic order.
pub fn validate_graph(graph: &AstGraph, registry: &PluginRegistry) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Check 1: Loops are allowed ("back to the menu"), but each must wait
    // for the player somewhere, or the VM would spin forever
    if let Some(cycle) = graph.find_cycle(|n| !node_waits_for_player(&n.node_type, registry)) {
        errors.push(ValidationError::new(
            "BUSY_LOOP",
            &format!("Loop never waits for the player: {}", cycle.join(" -> ")),
            Some(&cycle[0]),
            Some("Put a ShowText or BranchChoice node inside the loop."),
        ));
    }

    // Check 2: Exactly one place to start
    let explicit_entries: Vec<&String> = graph
        .entry_nodes
        .iter()
        .filter(|id| graph.nodes[*id].node_type == "entry")
        .collect();
    if explicit_entries.len() > 1 {
        errors.push(ValidationError::new(
            "MULTIPLE_ENTRY",
            &format!(
                "Graph has {} 'entry' nodes: {}",
                explicit_entries.len(),
                explicit_entries
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None,
            Some("Keep exactly one 'entry' node."),
        ));
    } else if graph.start_node().is_none() {
        errors.push(ValidationError::new(
            "AMBIGUOUS_ENTRY",
            &format!(
                "Cannot determine the start node: {} nodes have no inputs ({})",
                graph.entry_nodes.len(),
                graph.entry_nodes.join(", ")
            ),
            None,
            Some("Add an 'entry' node and connect it to the first block."),
        ));
    }

    // Check 3: Validate each node
    for node_id in graph.get_node_ids() {
        let node = &graph.nodes[&node_id];

        if node.node_type.is_empty() {
            errors.push(ValidationError::new(
                "INVALID_NODE_TYPE",
                "Node type cannot be empty",
                Some(&node_id),
                Some("Assign a valid block type to this node."),
            ));
        }

        // Same target through the same port twice
        let mut seen = HashSet::new();
        for (successor, port) in node.successors.iter().zip(&node.successor_ports) {
            if !seen.insert((successor, port)) {
                errors.push(ValidationError::new(
                    "DUPLICATE_EDGE",
                    &format!("Duplicate edge to node '{}'", successor),
                    Some(&node_id),
                    Some("Remove the duplicate connection."),
                ));
            }
        }

        if node.successors.contains(&node_id) {
            errors.push(ValidationError::new(
                "SELF_REFERENCE",
                "Node references itself",
                Some(&node_id),
                Some("A node cannot connect to itself."),
            ));
        }

        for successor in &node.successors {
            if !graph.nodes.contains_key(successor) {
                errors.push(ValidationError::new(
                    "BROKEN_EDGE",
                    &format!("Edge references non-existent node '{}'", successor),
                    Some(&node_id),
                    Some("Connect to a valid node or remove the edge."),
                ));
            }
        }

        validate_node_type(&node_id, node, registry, &mut errors);
    }

    errors
}

/// Type-specific node validation
fn validate_node_type(
    node_id: &str,
    node: &AstNode,
    registry: &PluginRegistry,
    errors: &mut Vec<ValidationError>,
) {
    match node.node_type.as_str() {
        "entry" => {
            if node.successors.len() != 1 {
                errors.push(ValidationError::new(
                    "INVALID_ENTRY",
                    &format!(
                        "Entry node must have exactly 1 output, found {}",
                        node.successors.len()
                    ),
                    Some(node_id),
                    Some("Connect the entry node to exactly one other node."),
                ));
            }
        }
        "BranchChoice" => validate_branch(node_id, node, errors),
        "EndScene" | "End" => {
            if !node.successors.is_empty() {
                errors.push(ValidationError::new(
                    "INVALID_END_NODE",
                    &format!(
                        "End node should have no outputs, found {}",
                        node.successors.len()
                    ),
                    Some(node_id),
                    Some("Remove all outgoing connections from end nodes."),
                ));
            }
        }
        "If" => validate_if(node_id, node, errors),
        "SetVariable" => {
            validate_set_variable(node_id, node, errors);
            check_output_count(node_id, node, 1, errors);
        }
        "JumpScene" | "CallScene" => {
            if let Err(message) = scene_param(node) {
                errors.push(ValidationError::new(
                    "MISSING_PARAMETER",
                    &message,
                    Some(node_id),
                    Some("Set 'scene' to the graph_id of a scene in the project."),
                ));
            }
            let outputs = if node.node_type == "JumpScene" { 0 } else { 1 };
            check_output_count(node_id, node, outputs, errors);
        }
        "Wait" => {
            let frames_ok = node
                .param("frames")
                .and_then(|f| f.as_u64())
                .is_some_and(|f| f <= u32::MAX as u64);
            if !frames_ok {
                errors.push(ValidationError::new(
                    "INVALID_PARAMETER",
                    "Wait needs 'frames': a whole number of frames (60 = one second)",
                    Some(node_id),
                    None,
                ));
            }
            check_output_count(node_id, node, 1, errors);
        }
        block_type => match registry.block(block_type) {
            Some(definition) => validate_block(node_id, node, definition, errors),
            None => {
                let hint = match closest_name(block_type, registry) {
                    Some(name) => format!("Did you mean '{}'?", name),
                    None => "Register the plugin that provides this block.".to_string(),
                };
                errors.push(ValidationError::new(
                    "UNKNOWN_BLOCK",
                    &format!("Unknown block type '{}'", block_type),
                    Some(node_id),
                    Some(&hint),
                ));
            }
        },
    }
}

/// A block may have as many outputs as its definition declares (0 or 1)
fn check_output_count(node_id: &str, node: &AstNode, allowed: usize, errors: &mut Vec<ValidationError>) {
    let found = node.successors.len();
    if found <= allowed {
        if found == 0 && allowed > 0 {
            // Falling off a node ends the scene; may be intentional
            log::warn!("Node '{}' has no outputs", node_id);
        }
        return;
    }
    let (kind, message) = if allowed == 0 {
        (
            "TOO_MANY_OUTPUTS",
            format!("'{}' ends the flow and cannot have outputs, found {}", node.node_type, found),
        )
    } else {
        (
            "MULTIPLE_OUTPUTS",
            format!(
                "'{}' node has {} outputs; only BranchChoice and If can have more than one",
                node.node_type, found
            ),
        )
    };
    errors.push(ValidationError::new(
        kind,
        &message,
        Some(node_id),
        Some("Use a BranchChoice or If node to split the flow."),
    ));
}

fn validate_block(
    node_id: &str,
    node: &AstNode,
    definition: &BlockDefinition,
    errors: &mut Vec<ValidationError>,
) {
    for param in &definition.parameters {
        match node.param(&param.name) {
            Some(value) => check_param_type(node_id, param, &value, errors),
            None if param.default.is_none() => errors.push(ValidationError::new(
                "MISSING_PARAMETER",
                &format!("{} requires '{}' ({})", definition.block_type, param.name, param.param_type),
                Some(node_id),
                Some("Set the parameter on the node."),
            )),
            None => {}
        }
    }

    let mut unknown: Vec<&String> = node
        .parameters
        .keys()
        .filter(|name| definition.parameter(name).is_none())
        .collect();
    unknown.sort();
    for name in unknown {
        let known: Vec<&str> = definition.parameters.iter().map(|p| p.name.as_str()).collect();
        errors.push(ValidationError::new(
            "UNKNOWN_PARAMETER",
            &format!("{} has no parameter '{}'", definition.block_type, name),
            Some(node_id),
            Some(&format!("Known parameters: {}", known.join(", "))),
        ));
    }

    check_output_count(node_id, node, definition.outputs.len(), errors);
}

fn type_matches(param_type: &str, value: &JsonValue) -> bool {
    match param_type.split(':').next().unwrap_or_default() {
        "string" | "text" | "asset" => value.is_string(),
        // An expression is source text, or a constant
        "expression" => !value.is_array() && !value.is_object(),
        "number" | "float" => value.is_number(),
        "int" => value.is_i64() || value.is_u64(),
        "bool" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => true,
    }
}

fn check_param_type(
    node_id: &str,
    param: &ParameterDefinition,
    value: &JsonValue,
    errors: &mut Vec<ValidationError>,
) {
    if !type_matches(&param.param_type, value) {
        errors.push(ValidationError::new(
            "INVALID_PARAMETER",
            &format!("'{}' must be {}, got {}", param.name, param.param_type, value),
            Some(node_id),
            None,
        ));
        return;
    }
    match (param.param_type.as_str(), value) {
        ("text", _) => check_text(node_id, &param.name, value, errors),
        ("expression", JsonValue::String(source)) => {
            check_expression(node_id, &param.name, source, errors)
        }
        _ => {}
    }
}

/// Closest known node type, for "did you mean" hints
fn closest_name<'a>(name: &str, registry: &'a PluginRegistry) -> Option<&'a str> {
    let candidates = BUILTIN_NODES.iter().copied().chain(registry.block_types());
    let lower = name.to_lowercase();
    candidates
        .map(|c| (edit_distance(&lower, &c.to_lowercase()), c))
        .filter(|(d, c)| *d <= (c.len() / 3).max(2))
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitute = prev + usize::from(ca != *cb);
            prev = row[j + 1];
            row[j + 1] = substitute.min(prev + 1).min(row[j] + 1);
        }
    }
    row[b.len()]
}

fn check_expression(node_id: &str, param: &str, source: &str, errors: &mut Vec<ValidationError>) {
    if let Err(e) = parse_expr(source) {
        errors.push(ValidationError::new(
            "INVALID_EXPRESSION",
            &format!("'{}' is not a valid expression: {}", param, e),
            Some(node_id),
            Some("Example: affection >= 5 and not met_rival"),
        ));
    }
}

/// Text may contain {expression} placeholders
fn check_text(node_id: &str, param: &str, value: &JsonValue, errors: &mut Vec<ValidationError>) {
    if let JsonValue::String(source) = value {
        if let Err(e) = parse_template(source) {
            errors.push(ValidationError::new(
                "INVALID_TEXT",
                &format!("'{}' has a broken placeholder: {}", param, e),
                Some(node_id),
                Some("Write {name} to insert a variable, {{ and }} for literal braces."),
            ));
        }
    }
}

fn validate_set_variable(node_id: &str, node: &AstNode, errors: &mut Vec<ValidationError>) {
    let name_ok = matches!(node.param("name"), Some(JsonValue::String(ref s)) if !s.is_empty());
    if !name_ok {
        errors.push(ValidationError::new(
            "MISSING_PARAMETER",
            "SetVariable requires a non-empty 'name' parameter",
            Some(node_id),
            Some("Set the variable name."),
        ));
    }
    match (node.param("value"), node.param("expression")) {
        (Some(_), None) => {}
        (None, Some(JsonValue::String(source))) => {
            check_expression(node_id, "expression", &source, errors)
        }
        (None, Some(_)) => errors.push(ValidationError::new(
            "INVALID_PARAMETER",
            "SetVariable 'expression' must be a string",
            Some(node_id),
            Some("Example: \"score + 1\""),
        )),
        (None, None) => errors.push(ValidationError::new(
            "MISSING_PARAMETER",
            "SetVariable requires a 'value' or an 'expression' parameter",
            Some(node_id),
            Some("Set a constant 'value', or an 'expression' such as \"score + 1\"."),
        )),
        (Some(_), Some(_)) => errors.push(ValidationError::new(
            "INVALID_PARAMETER",
            "SetVariable has both 'value' and 'expression'",
            Some(node_id),
            Some("Keep only one of them."),
        )),
    }
}

fn validate_if(node_id: &str, node: &AstNode, errors: &mut Vec<ValidationError>) {
    match node.param("condition") {
        Some(JsonValue::String(source)) => check_expression(node_id, "condition", &source, errors),
        Some(_) => {}
        None => errors.push(ValidationError::new(
            "MISSING_PARAMETER",
            "If requires a 'condition' parameter",
            Some(node_id),
            Some("Example: affection >= 5"),
        )),
    }
    if node.successors.len() != 2 {
        errors.push(ValidationError::new(
            "INVALID_IF",
            &format!(
                "If needs exactly 2 outputs (0: true, 1: false), found {}",
                node.successors.len()
            ),
            Some(node_id),
            Some("Connect output 0 for true and output 1 for false."),
        ));
    } else if node.successor_ports.iter().any(|p| p.is_some())
        && node.successor_ports != [Some(0), Some(1)]
    {
        errors.push(ValidationError::new(
            "INVALID_PORT",
            &format!("If output ports must be 0 and 1, found {:?}", node.successor_ports),
            Some(node_id),
            Some("Connect output 0 for true and output 1 for false."),
        ));
    }
}

fn validate_branch(node_id: &str, node: &AstNode, errors: &mut Vec<ValidationError>) {
    let choices = match node.param("choices") {
        Some(JsonValue::Array(choices)) if !choices.is_empty() => choices,
        Some(_) => {
            errors.push(ValidationError::new(
                "INVALID_PARAMETER",
                "BranchChoice 'choices' must be a non-empty array",
                Some(node_id),
                Some("Provide a list of choice labels."),
            ));
            return;
        }
        None => {
            errors.push(ValidationError::new(
                "MISSING_PARAMETER",
                "BranchChoice requires 'choices' parameter",
                Some(node_id),
                Some("Add a 'choices' array parameter."),
            ));
            return;
        }
    };

    for choice in &choices {
        check_text(node_id, "choices", choice, errors);
    }

    if node.successors.len() != choices.len() {
        errors.push(ValidationError::new(
            "BRANCH_MISMATCH",
            &format!(
                "BranchChoice has {} choices but {} outputs",
                choices.len(),
                node.successors.len()
            ),
            Some(node_id),
            Some("Ensure number of outputs matches number of choices."),
        ));
        return;
    }

    // If ports are given, they must be exactly 0..n-1
    if node.successor_ports.iter().any(|p| p.is_some()) {
        let ports = &node.successor_ports;
        let expected: Vec<Option<usize>> = (0..choices.len()).map(Some).collect();
        if *ports != expected {
            errors.push(ValidationError::new(
                "INVALID_PORT",
                &format!(
                    "BranchChoice output ports must be 0..{} (one edge each), found {:?}",
                    choices.len() - 1,
                    ports
                ),
                Some(node_id),
                Some("Connect each choice output exactly once."),
            ));
        }
    }
}

/// Python wrapper for validation
#[pyclass]
pub struct GraphValidator {
    registry: Py<PluginRegistry>,
}

#[pymethods]
impl GraphValidator {
    /// Validate against `registry`, or the default blocks when none is given
    #[new]
    #[pyo3(signature = (registry = None))]
    fn new(py: Python<'_>, registry: Option<Py<PluginRegistry>>) -> PyResult<Self> {
        Ok(GraphValidator { registry: shared_registry(py, registry)? })
    }

    /// Validate a graph and return list of errors
    fn validate(&self, py: Python<'_>, graph: &AstGraph) -> Vec<ValidationError> {
        validate_graph(graph, &self.registry.bind(py).borrow())
    }

    /// Check if graph is valid (no errors)
    fn is_valid(&self, py: Python<'_>, graph: &AstGraph) -> bool {
        self.validate(py, graph).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::graph_parser::parse_json_graph;

    fn error_types(json: &str) -> Vec<String> {
        let graph = parse_json_graph(json).unwrap();
        validate_graph(&graph, &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap()).into_iter().map(|e| e.error_type).collect()
    }

    #[test]
    fn example_graph_is_valid() {
        let json = include_str!("../../examples/simple_novel.json");
        assert!(error_types(json).is_empty());
    }

    #[test]
    fn branch_mismatch_is_reported() {
        let types = error_types(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},
                         {"id":"b","type":"BranchChoice","parameters":{"choices":["x","y","z"]}},
                         {"id":"e","type":"End"}],
                "edges":[{"from":"s","to":"b"},{"from":"b","to":"e","port":0}]}"#,
        );
        assert_eq!(types, vec!["BRANCH_MISMATCH"]);
    }

    #[test]
    fn plain_node_cannot_fork() {
        let types = error_types(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},{"id":"t","type":"ShowText","parameters":{"text":"Hi"}},
                         {"id":"a","type":"End"},{"id":"b","type":"End"}],
                "edges":[{"from":"s","to":"t"},{"from":"t","to":"a"},{"from":"t","to":"b"}]}"#,
        );
        assert_eq!(types, vec!["MULTIPLE_OUTPUTS"]);
    }

    #[test]
    fn two_roots_without_entry_are_ambiguous() {
        let types = error_types(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"a","type":"End"},{"id":"b","type":"End"}],"edges":[]}"#,
        );
        assert_eq!(types, vec!["AMBIGUOUS_ENTRY"]);
    }

    #[test]
    fn loop_through_a_choice_is_allowed() {
        let types = error_types(include_str!("../../examples/menu_loop.json"));
        assert!(types.is_empty(), "{:?}", types);
    }

    #[test]
    fn loop_without_player_input_is_rejected() {
        let graph = parse_json_graph(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},
                         {"id":"a","type":"SetVariable","parameters":{"name":"x","value":1}},
                         {"id":"b","type":"SetVariable","parameters":{"name":"x","value":2}}],
                "edges":[{"from":"s","to":"a"},{"from":"a","to":"b"},{"from":"b","to":"a"}]}"#,
        )
        .unwrap();
        let errors = validate_graph(&graph, &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].error_type, "BUSY_LOOP");
        assert_eq!(errors[0].message, "Loop never waits for the player: a -> b -> a");
    }

    #[test]
    fn affection_example_is_valid() {
        let types = error_types(include_str!("../../examples/affection.json"));
        assert!(types.is_empty(), "{:?}", types);
    }

    #[test]
    fn expression_and_text_errors() {
        let graph = parse_json_graph(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},
                         {"id":"t","type":"ShowText","parameters":{"text":"Hi {name"}},
                         {"id":"i","type":"If","parameters":{"condition":"a = 1"}},
                         {"id":"x","type":"End"},
                         {"id":"v","type":"SetVariable","parameters":{"name":"n","value":1,"expression":"2"}}],
                "edges":[{"from":"s","to":"t"},{"from":"t","to":"i"},{"from":"i","to":"x"},{"from":"v","to":"x"}]}"#,
        )
        .unwrap();
        let errors: Vec<(String, String)> = validate_graph(&graph, &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap())
            .into_iter()
            .map(|e| (e.error_type, e.node_id.unwrap_or_default()))
            .collect();
        let expected = [
            ("INVALID_EXPRESSION", "i"),
            ("INVALID_IF", "i"),
            ("INVALID_TEXT", "t"),
            ("INVALID_PARAMETER", "v"),
        ];
        for (kind, node) in expected {
            assert!(
                errors.contains(&(kind.to_string(), node.to_string())),
                "missing {} on {}: {:?}",
                kind,
                node,
                errors
            );
        }
    }

    #[test]
    fn set_variable_needs_name_and_value() {
        let types = error_types(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},{"id":"v","type":"SetVariable","parameters":{"name":""}}],
                "edges":[{"from":"s","to":"v"}]}"#,
        );
        assert_eq!(types, vec!["MISSING_PARAMETER", "MISSING_PARAMETER"]);
    }

    #[test]
    fn blocks_are_checked_against_their_definition() {
        let graph = parse_json_graph(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},
                         {"id":"typo","type":"ShowTxt","parameters":{"text":"Hi"}},
                         {"id":"voice","type":"PlayVoice","parameters":{"volume":"loud","pitch":2}},
                         {"id":"line","type":"ShowText","parameters":{}}],
                "edges":[{"from":"s","to":"typo"},{"from":"typo","to":"voice"},{"from":"voice","to":"line"}]}"#,
        )
        .unwrap();
        let errors = validate_graph(&graph, &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap());
        let found: Vec<(&str, &str, &str)> = errors
            .iter()
            .map(|e| {
                (
                    e.error_type.as_str(),
                    e.node_id.as_deref().unwrap_or(""),
                    e.hint.as_deref().unwrap_or(""),
                )
            })
            .collect();
        assert!(found.contains(&("UNKNOWN_BLOCK", "typo", "Did you mean 'ShowText'?")), "{:?}", found);
        assert!(found.iter().any(|f| f.0 == "MISSING_PARAMETER" && f.1 == "voice"));
        assert!(found.iter().any(|f| f.0 == "INVALID_PARAMETER" && f.1 == "voice"));
        assert!(found.iter().any(|f| f.0 == "UNKNOWN_PARAMETER" && f.1 == "voice"));
        assert!(found.iter().any(|f| f.0 == "MISSING_PARAMETER" && f.1 == "line"));
    }

    #[test]
    fn edit_distance_suggestions() {
        let registry = crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap();
        assert_eq!(closest_name("showtext", &registry), Some("ShowText"));
        assert_eq!(closest_name("SetVar", &registry), None);
        assert_eq!(closest_name("Iff", &registry), Some("If"));
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }
}

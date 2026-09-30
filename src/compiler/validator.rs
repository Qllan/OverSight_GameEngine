//! Validator - Graph validation and error reporting
//!
//! Validates the AST graph for structural correctness:
//! - Every loop waits for the player at least once
//! - A single, unambiguous start node
//! - Node type validation
//! - Connection integrity
//! - Parameter validation

use crate::compiler::codegen::node_waits_for_player;
use crate::compiler::graph_parser::{AstGraph, AstNode};
use pyo3::prelude::*;
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
}

#[pymethods]
impl ValidationError {
    #[new]
    #[pyo3(signature = (error_type, message, node_id = None, hint = None))]
    pub fn new(
        error_type: &str,
        message: &str,
        node_id: Option<&str>,
        hint: Option<&str>,
    ) -> Self {
        ValidationError {
            error_type: error_type.to_string(),
            message: message.to_string(),
            node_id: node_id.map(String::from),
            hint: hint.map(String::from),
        }
    }

    fn __repr__(&self) -> String {
        let node_info = self.node_id.as_deref().unwrap_or("unknown");
        format!(
            "ValidationError[{}]: {} (node: {})",
            self.error_type, self.message, node_info
        )
    }
}

/// Validate the AST graph. Errors are returned in a deterministic order.
pub fn validate_graph(graph: &AstGraph) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Check 1: Loops are allowed ("back to the menu"), but each must wait
    // for the player somewhere, or the VM would spin forever
    if let Some(cycle) = graph.find_cycle(|n| !node_waits_for_player(&n.node_type)) {
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

        validate_node_type(&node_id, node, &mut errors);
    }

    errors
}

/// Type-specific node validation
fn validate_node_type(node_id: &str, node: &AstNode, errors: &mut Vec<ValidationError>) {
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
        _ => {
            if node.node_type == "SetVariable" {
                let name_ok = matches!(
                    node.param("name"),
                    Some(serde_json::Value::String(ref s)) if !s.is_empty()
                );
                if !name_ok {
                    errors.push(ValidationError::new(
                        "MISSING_PARAMETER",
                        "SetVariable requires a non-empty 'name' parameter",
                        Some(node_id),
                        Some("Set the variable name."),
                    ));
                }
                if node.param("value").is_none() {
                    errors.push(ValidationError::new(
                        "MISSING_PARAMETER",
                        "SetVariable requires a 'value' parameter",
                        Some(node_id),
                        Some("Set the value to assign."),
                    ));
                }
            }

            // Only BranchChoice may fork the flow
            if node.successors.len() > 1 {
                errors.push(ValidationError::new(
                    "MULTIPLE_OUTPUTS",
                    &format!(
                        "'{}' node has {} outputs; only BranchChoice can have more than one",
                        node.node_type,
                        node.successors.len()
                    ),
                    Some(node_id),
                    Some("Use a BranchChoice node to split the flow."),
                ));
            }
            if node.successors.is_empty() {
                // Falling off a node ends the scene; may be intentional
                log::warn!("Node '{}' has no outputs", node_id);
            }
        }
    }
}

fn validate_branch(node_id: &str, node: &AstNode, errors: &mut Vec<ValidationError>) {
    let choices = match node.param("choices") {
        Some(serde_json::Value::Array(choices)) if !choices.is_empty() => choices,
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
pub struct GraphValidator;

#[pymethods]
impl GraphValidator {
    #[new]
    fn new() -> Self {
        GraphValidator
    }

    /// Validate a graph and return list of errors
    fn validate(&self, graph: &AstGraph) -> Vec<ValidationError> {
        validate_graph(graph)
    }

    /// Check if graph is valid (no errors)
    fn is_valid(&self, graph: &AstGraph) -> bool {
        validate_graph(graph).is_empty()
    }
}

impl Default for GraphValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::graph_parser::parse_json_graph;

    fn error_types(json: &str) -> Vec<String> {
        let graph = parse_json_graph(json).unwrap();
        validate_graph(&graph).into_iter().map(|e| e.error_type).collect()
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
                "nodes":[{"id":"s","type":"entry"},{"id":"t","type":"ShowText"},
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
        let errors = validate_graph(&graph);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].error_type, "BUSY_LOOP");
        assert_eq!(errors[0].message, "Loop never waits for the player: a -> b -> a");
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
}

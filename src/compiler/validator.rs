//! Validator - Graph validation and error reporting
//!
//! Validates the AST graph for structural correctness:
//! - DAG verification (no cycles)
//! - Node type validation
//! - Connection integrity
//! - Parameter validation

use crate::compiler::graph_parser::AstGraph;
use pyo3::prelude::*;

/// Validation error with human-readable message
#[derive(Debug, Clone)]
#[pyclass]
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
    fn new(
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

/// Validate the AST graph
pub fn validate_graph(graph: &AstGraph) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Check 1: Verify it's a DAG (no cycles)
    if !graph.is_dag() {
        errors.push(ValidationError::new(
            "CYCLE_DETECTED",
            "Graph contains cycles. Visual script graphs must be Directed Acyclic Graphs (DAGs).".to_string(),
            None,
            Some("Check your branch connections for circular references.".to_string()),
        ));
    }

    // Check 2: Validate each node
    for (node_id, node) in &graph.nodes {
        // Check for empty node type
        if node.node_type.is_empty() {
            errors.push(ValidationError::new(
                "INVALID_NODE_TYPE",
                "Node type cannot be empty".to_string(),
                Some(node_id),
                Some("Assign a valid block type to this node.".to_string()),
            ));
        }

        // Check for duplicate successors (same edge twice)
        let mut seen_successors = std::collections::HashSet::new();
        for successor in &node.successors {
            if !seen_successors.insert(successor) {
                errors.push(ValidationError::new(
                    "DUPLICATE_EDGE",
                    format!("Duplicate edge to node '{}'", successor),
                    Some(node_id),
                    Some("Remove the duplicate connection.".to_string()),
                ));
            }
        }

        // Check for self-references
        if node.successors.contains(node_id) {
            errors.push(ValidationError::new(
                "SELF_REFERENCE",
                "Node references itself".to_string(),
                Some(node_id),
                Some("A node cannot connect to itself.".to_string()),
            ));
        }

        // Type-specific validation
        validate_node_type(node_id, node, &mut errors);
    }

    // Check 3: Validate edges reference existing nodes
    for (_, node) in &graph.nodes {
        for successor in &node.successors {
            if !graph.nodes.contains_key(successor) {
                errors.push(ValidationError::new(
                    "BROKEN_EDGE",
                    format!("Edge references non-existent node '{}'", successor),
                    Some(&node.id),
                    Some("Connect to a valid node or remove the edge.".to_string()),
                ));
            }
        }
    }

    // Check 4: Ensure entry node exists and is reachable
    if graph.entry_nodes.is_empty() {
        errors.push(ValidationError::new(
            "NO_ENTRY_NODE",
            "Graph has no entry point".to_string(),
            None,
            Some("Add an 'entry' type node or ensure at least one node has no predecessors.".to_string()),
        ));
    }

    errors
}

/// Type-specific node validation
fn validate_node_type(
    node_id: &str,
    node: &crate::compiler::graph_parser::AstNode,
    errors: &mut Vec<ValidationError>,
) {
    match node.node_type.as_str() {
        "entry" => {
            // Entry nodes should have exactly one successor
            if node.successors.len() != 1 {
                errors.push(ValidationError::new(
                    "INVALID_ENTRY",
                    format!(
                        "Entry node must have exactly 1 output, found {}",
                        node.successors.len()
                    ),
                    Some(node_id),
                    Some("Connect the entry node to exactly one other node.".to_string()),
                ));
            }
        }
        "BranchChoice" => {
            // Branch nodes need choices parameter and multiple outputs
            if !node.parameters.contains_key("choices") {
                errors.push(ValidationError::new(
                    "MISSING_PARAMETER",
                    "BranchChoice requires 'choices' parameter".to_string(),
                    Some(node_id),
                    Some("Add a 'choices' array parameter.".to_string()),
                ));
            }
            
            if let Some(choices_val) = node.parameters.get("choices") {
                if let Ok(choices) = serde_json::from_str::<Vec<serde_json::Value>>(choices_val) {
                    if node.successors.len() != choices.len() {
                        errors.push(ValidationError::new(
                            "BRANCH_MISMATCH",
                            format!(
                                "BranchChoice has {} choices but {} outputs",
                                choices.len(),
                                node.successors.len()
                            ),
                            Some(node_id),
                            Some("Ensure number of outputs matches number of choices.".to_string()),
                        ));
                    }
                }
            }
        }
        "EndScene" | "End" => {
            // End nodes should have no successors
            if !node.successors.is_empty() {
                errors.push(ValidationError::new(
                    "INVALID_END_NODE",
                    format!(
                        "End node should have no outputs, found {}",
                        node.successors.len()
                    ),
                    Some(node_id),
                    Some("Remove all outgoing connections from end nodes.".to_string()),
                ));
            }
        }
        _ => {
            // Generic nodes: just check they have at least one output (unless they're ends)
            if node.successors.is_empty() 
                && node.node_type != "EndScene" 
                && node.node_type != "End"
            {
                // This might be intentional for some block types, so just warn
                log::warn!("Node '{}' has no outputs", node_id);
            }
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

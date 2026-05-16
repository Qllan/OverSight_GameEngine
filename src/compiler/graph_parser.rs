//! Graph Parser - JSON to AST conversion
//!
//! Parses the JSON graph format into an intermediate AST representation
//! for validation and code generation.

use serde::{Deserialize, Serialize};
use pyo3::prelude::*;
use std::collections::HashMap;

/// Raw JSON node structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub position: Option<[i32; 2]>,
    #[serde(default)]
    pub parameters: HashMap<String, serde_json::Value>,
}

/// Raw JSON edge structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonEdge {
    pub from: String,
    pub to: String,
    pub port: Option<usize>,
}

/// Raw JSON graph structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonGraph {
    pub graph_id: String,
    pub version: String,
    pub nodes: Vec<JsonNode>,
    pub edges: Vec<JsonEdge>,
}

/// AST Node with resolved connections
#[derive(Debug, Clone)]
#[pyclass]
pub struct AstNode {
    #[pyo3(get)]
    pub id: String,
    #[pyo3(get)]
    pub node_type: String,
    #[pyo3(get)]
    pub position: Option<(i32, i32)>,
    #[pyo3(get)]
    pub parameters: HashMap<String, String>, // Serialized JSON values
    #[pyo3(get)]
    pub successors: Vec<String>,
    #[pyo3(get)]
    pub predecessors: Vec<String>,
}

#[pymethods]
impl AstNode {
    fn __repr__(&self) -> String {
        format!(
            "AstNode(id='{}', type='{}', successors={:?})",
            self.id, self.node_type, self.successors
        )
    }
}

/// AST Graph - intermediate representation
#[derive(Debug, Clone)]
#[pyclass]
pub struct AstGraph {
    #[pyo3(get)]
    pub graph_id: String,
    #[pyo3(get)]
    pub version: String,
    #[pyo3(get)]
    pub nodes: HashMap<String, AstNode>,
    #[pyo3(get)]
    pub entry_nodes: Vec<String>,
}

#[pymethods]
impl AstGraph {
    fn __repr__(&self) -> String {
        format!(
            "AstGraph(id='{}', nodes={}, entries={:?})",
            self.graph_id,
            self.nodes.len(),
            self.entry_nodes
        )
    }

    /// Get node by ID
    fn get_node(&self, id: &str) -> Option<AstNode> {
        self.nodes.get(id).cloned()
    }

    /// Get all node IDs
    fn get_node_ids(&self) -> Vec<String> {
        self.nodes.keys().cloned().collect()
    }

    /// Check if graph has cycles (returns true if acyclic)
    fn is_dag(&self) -> bool {
        // Simple DFS-based cycle detection
        let mut visited = HashMap::new();
        let mut rec_stack = HashMap::new();
        
        for node_id in self.nodes.keys() {
            if !visited.get(node_id).unwrap_or(&false) {
                if self.has_cycle_dfs(node_id, &mut visited, &mut rec_stack) {
                    return false;
                }
            }
        }
        true
    }
}

impl AstGraph {
    fn has_cycle_dfs(
        &self,
        node_id: &str,
        visited: &mut HashMap<String, bool>,
        rec_stack: &mut HashMap<String, bool>,
    ) -> bool {
        visited.insert(node_id.to_string(), true);
        rec_stack.insert(node_id.to_string(), true);

        if let Some(node) = self.nodes.get(node_id) {
            for successor in &node.successors {
                if !visited.get(successor).unwrap_or(&false) {
                    if self.has_cycle_dfs(successor, visited, rec_stack) {
                        return true;
                    }
                } else if rec_stack.get(successor).unwrap_or(&false) {
                    return true;
                }
            }
        }

        rec_stack.insert(node_id.to_string(), false);
        false
    }
}

/// Parse JSON graph into AST
pub fn parse_json_graph(json_str: &str) -> Result<AstGraph, String> {
    let json_graph: JsonGraph = serde_json::from_str(json_str)
        .map_err(|e| format!("JSON parse error: {}", e))?;

    // Build node map
    let mut nodes: HashMap<String, AstNode> = HashMap::new();
    
    for json_node in &json_graph.nodes {
        let params: HashMap<String, String> = json_node
            .parameters
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();

        let ast_node = AstNode {
            id: json_node.id.clone(),
            node_type: json_node.node_type.clone(),
            position: json_node.position.map(|p| (p[0], p[1])),
            parameters: params,
            successors: Vec::new(),
            predecessors: Vec::new(),
        };
        nodes.insert(json_node.id.clone(), ast_node);
    }

    // Build edges
    for edge in &json_graph.edges {
        if let Some(from_node) = nodes.get_mut(&edge.from) {
            from_node.successors.push(edge.to.clone());
        } else {
            return Err(format!("Edge references non-existent node: {}", edge.from));
        }
        
        if let Some(to_node) = nodes.get_mut(&edge.to) {
            to_node.predecessors.push(edge.from.clone());
        } else {
            return Err(format!("Edge references non-existent node: {}", edge.to));
        }
    }

    // Find entry nodes (nodes with no predecessors, or explicit "entry" type)
    let entry_nodes: Vec<String> = nodes
        .iter()
        .filter(|(_, n)| n.predecessors.is_empty() || n.node_type == "entry")
        .map(|(id, _)| id.clone())
        .collect();

    if entry_nodes.is_empty() {
        return Err("Graph has no entry nodes".to_string());
    }

    Ok(AstGraph {
        graph_id: json_graph.graph_id,
        version: json_graph.version,
        nodes,
        entry_nodes,
    })
}

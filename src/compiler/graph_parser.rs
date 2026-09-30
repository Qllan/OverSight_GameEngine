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
#[pyclass(skip_from_py_object)]
pub struct AstNode {
    #[pyo3(get)]
    pub id: String,
    #[pyo3(get)]
    pub node_type: String,
    #[pyo3(get)]
    pub position: Option<(i32, i32)>,
    #[pyo3(get)]
    pub parameters: HashMap<String, String>, // Serialized JSON values
    /// Outgoing connections, ordered by output port
    #[pyo3(get)]
    pub successors: Vec<String>,
    /// Output port of each entry in `successors` (None if the edge had no port)
    #[pyo3(get)]
    pub successor_ports: Vec<Option<usize>>,
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

impl AstNode {
    /// Get a parameter as a JSON value
    pub fn param(&self, name: &str) -> Option<serde_json::Value> {
        self.parameters
            .get(name)
            .and_then(|raw| serde_json::from_str(raw).ok())
    }
}

/// AST Graph - intermediate representation
#[derive(Debug, Clone)]
#[pyclass(skip_from_py_object)]
pub struct AstGraph {
    #[pyo3(get)]
    pub graph_id: String,
    #[pyo3(get)]
    pub version: String,
    #[pyo3(get)]
    pub nodes: HashMap<String, AstNode>,
    /// Nodes with no predecessors or of type "entry", sorted by id
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
    pub fn get_node(&self, id: &str) -> Option<AstNode> {
        self.nodes.get(id).cloned()
    }

    /// Get all node IDs (sorted)
    pub fn get_node_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.nodes.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Check if graph has cycles (returns true if acyclic)
    pub fn is_dag(&self) -> bool {
        self.find_cycle(|_| true).is_none()
    }

    /// The node where execution starts: the "entry" node if there is one,
    /// otherwise the single node without predecessors.
    pub fn start_node(&self) -> Option<String> {
        let explicit: Vec<&String> = self
            .entry_nodes
            .iter()
            .filter(|id| self.nodes[*id].node_type == "entry")
            .collect();
        match (explicit.len(), self.entry_nodes.len()) {
            (1, _) => Some(explicit[0].clone()),
            (0, 1) => Some(self.entry_nodes[0].clone()),
            _ => None,
        }
    }
}

impl AstGraph {
    /// Find a cycle among the nodes accepted by `include`, returned as a
    /// path that starts and ends on the same node. Iterative, so long
    /// linear scripts cannot overflow the stack.
    pub fn find_cycle(&self, include: impl Fn(&AstNode) -> bool) -> Option<Vec<String>> {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            OnPath,
            Done,
        }
        let mut marks: HashMap<&str, Mark> = HashMap::new();
        let mut roots: Vec<&String> = self.nodes.keys().collect();
        roots.sort();

        for root in roots {
            if marks.contains_key(root.as_str()) || !include(&self.nodes[root]) {
                continue;
            }
            // (node, index of the next successor to visit)
            let mut path: Vec<(&str, usize)> = vec![(root, 0)];
            marks.insert(root, Mark::OnPath);

            while let Some(&(id, next_index)) = path.last() {
                let Some(next) = self.nodes[id].successors.get(next_index) else {
                    marks.insert(id, Mark::Done);
                    path.pop();
                    continue;
                };
                if let Some(top) = path.last_mut() {
                    top.1 += 1;
                }
                if !self.nodes.get(next).is_some_and(&include) {
                    continue;
                }
                match marks.get(next.as_str()) {
                    Some(Mark::OnPath) => {
                        let start = path.iter().position(|(n, _)| n == next)?;
                        let mut cycle: Vec<String> =
                            path[start..].iter().map(|(n, _)| n.to_string()).collect();
                        cycle.push(next.clone());
                        return Some(cycle);
                    }
                    Some(Mark::Done) => {}
                    None => {
                        marks.insert(next, Mark::OnPath);
                        path.push((next, 0));
                    }
                }
            }
        }
        None
    }
}

/// Parse JSON graph into AST
pub fn parse_json_graph(json_str: &str) -> Result<AstGraph, String> {
    let json_graph: JsonGraph = serde_json::from_str(json_str)
        .map_err(|e| format!("JSON parse error: {}", e))?;

    // Build node map
    let mut nodes: HashMap<String, AstNode> = HashMap::new();

    for json_node in &json_graph.nodes {
        if nodes.contains_key(&json_node.id) {
            return Err(format!("Duplicate node id: {}", json_node.id));
        }

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
            successor_ports: Vec::new(),
            predecessors: Vec::new(),
        };
        nodes.insert(json_node.id.clone(), ast_node);
    }

    // Collect outgoing edges per node, keeping file order
    let mut outgoing: HashMap<String, Vec<(Option<usize>, String)>> = HashMap::new();
    for edge in &json_graph.edges {
        if !nodes.contains_key(&edge.from) {
            return Err(format!("Edge references non-existent node: {}", edge.from));
        }
        match nodes.get_mut(&edge.to) {
            Some(to_node) => to_node.predecessors.push(edge.from.clone()),
            None => return Err(format!("Edge references non-existent node: {}", edge.to)),
        }
        outgoing
            .entry(edge.from.clone())
            .or_default()
            .push((edge.port, edge.to.clone()));
    }

    // Order outputs by port. Edges without a port keep their file position,
    // so graphs that never set ports behave as before.
    for (from, edges) in outgoing {
        let mut keyed: Vec<(usize, (Option<usize>, String))> = edges
            .into_iter()
            .enumerate()
            .map(|(order, e)| (e.0.unwrap_or(order), e))
            .collect();
        keyed.sort_by_key(|(key, _)| *key);

        let node = nodes.get_mut(&from).expect("checked above");
        for (_, (port, to)) in keyed {
            node.successors.push(to);
            node.successor_ports.push(port);
        }
    }

    // Find entry nodes (nodes with no predecessors, or explicit "entry" type)
    let mut entry_nodes: Vec<String> = nodes
        .iter()
        .filter(|(_, n)| n.predecessors.is_empty() || n.node_type == "entry")
        .map(|(id, _)| id.clone())
        .collect();
    entry_nodes.sort();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successors_are_ordered_by_port() {
        let graph = parse_json_graph(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"a","type":"BranchChoice","parameters":{"choices":["x","y"]}},
                         {"id":"b","type":"End"},{"id":"c","type":"End"}],
                "edges":[{"from":"a","to":"c","port":1},{"from":"a","to":"b","port":0}]}"#,
        )
        .unwrap();
        let a = &graph.nodes["a"];
        assert_eq!(a.successors, vec!["b", "c"]);
        assert_eq!(a.successor_ports, vec![Some(0), Some(1)]);
    }

    #[test]
    fn start_node_prefers_explicit_entry() {
        let graph = parse_json_graph(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"orphan","type":"End"},{"id":"s","type":"entry"},{"id":"e","type":"End"}],
                "edges":[{"from":"s","to":"e"}]}"#,
        )
        .unwrap();
        assert_eq!(graph.start_node().as_deref(), Some("s"));
    }

    #[test]
    fn find_cycle_returns_the_loop_path() {
        let graph = parse_json_graph(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},{"id":"a","type":"X"},{"id":"b","type":"X"},{"id":"c","type":"X"}],
                "edges":[{"from":"s","to":"a"},{"from":"a","to":"b"},{"from":"b","to":"c"},{"from":"c","to":"a"}]}"#,
        )
        .unwrap();
        assert!(!graph.is_dag());
        assert_eq!(graph.find_cycle(|_| true).unwrap(), vec!["a", "b", "c", "a"]);
        assert_eq!(graph.find_cycle(|n| n.id != "b"), None);
    }

    #[test]
    fn long_chain_does_not_overflow() {
        let n = 50_000;
        let nodes: Vec<String> = (0..n)
            .map(|i| format!(r#"{{"id":"n{}","type":"X"}}"#, i))
            .collect();
        let edges: Vec<String> = (1..n)
            .map(|i| format!(r#"{{"from":"n{}","to":"n{}"}}"#, i - 1, i))
            .collect();
        let json = format!(
            r#"{{"graph_id":"g","version":"1.0","nodes":[{}],"edges":[{}]}}"#,
            nodes.join(","),
            edges.join(",")
        );
        assert!(parse_json_graph(&json).unwrap().is_dag());
    }

    #[test]
    fn duplicate_node_ids_are_rejected() {
        let result = parse_json_graph(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"a","type":"End"},{"id":"a","type":"End"}],"edges":[]}"#,
        );
        assert!(result.is_err());
    }
}

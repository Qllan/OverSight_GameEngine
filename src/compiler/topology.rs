//! Topology - Topological sorting for execution order
//!
//! Performs topological sort on the DAG to determine linear execution order.
//! Uses Kahn's algorithm for deterministic ordering.

use crate::compiler::graph_parser::AstGraph;
use std::collections::{HashMap, VecDeque};

/// Perform topological sort on the graph
/// Returns nodes in execution order, or error if cycle detected
pub fn topological_sort(graph: &AstGraph) -> Result<Vec<String>, String> {
    // Calculate in-degrees
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    for node_id in graph.nodes.keys() {
        in_degree.insert(node_id.clone(), 0);
    }

    for (_, node) in &graph.nodes {
        for successor in &node.successors {
            *in_degree.get_mut(successor).unwrap() += 1;
        }
    }

    // Initialize queue with nodes having zero in-degree
    // Sort entry nodes for deterministic ordering
    let mut sorted_entries = graph.entry_nodes.clone();
    sorted_entries.sort();
    
    let mut queue: VecDeque<String> = sorted_entries
        .into_iter()
        .filter(|id| *in_degree.get(id).unwrap_or(&0) == 0)
        .collect();

    // If no zero in-degree nodes but graph has nodes, there's a cycle
    if queue.is_empty() && !graph.nodes.is_empty() {
        return Err("Cycle detected: no nodes with zero in-degree".to_string());
    }

    let mut result: Vec<String> = Vec::with_capacity(graph.nodes.len());

    while let Some(node_id) = queue.pop_front() {
        result.push(node_id.clone());

        if let Some(node) = graph.nodes.get(&node_id) {
            // Sort successors for deterministic ordering
            let mut successors = node.successors.clone();
            successors.sort();

            for successor in successors {
                let in_deg = in_degree.get_mut(&successor).unwrap();
                *in_deg -= 1;
                if *in_deg == 0 {
                    queue.push_back(successor);
                }
            }
        }
    }

    // Check if all nodes were processed
    if result.len() != graph.nodes.len() {
        return Err(format!(
            "Cycle detected: only {} of {} nodes processed",
            result.len(),
            graph.nodes.len()
        ));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::graph_parser::AstNode;

    fn create_test_graph() -> AstGraph {
        let mut nodes = HashMap::new();
        
        nodes.insert("A".to_string(), AstNode {
            id: "A".to_string(),
            node_type: "entry".to_string(),
            position: None,
            parameters: HashMap::new(),
            successors: vec!["B".to_string(), "C".to_string()],
            predecessors: vec![],
        });
        
        nodes.insert("B".to_string(), AstNode {
            id: "B".to_string(),
            node_type: "ShowText".to_string(),
            position: None,
            parameters: HashMap::new(),
            successors: vec!["D".to_string()],
            predecessors: vec!["A".to_string()],
        });
        
        nodes.insert("C".to_string(), AstNode {
            id: "C".to_string(),
            node_type: "ShowText".to_string(),
            position: None,
            parameters: HashMap::new(),
            successors: vec!["D".to_string()],
            predecessors: vec!["A".to_string()],
        });
        
        nodes.insert("D".to_string(), AstNode {
            id: "D".to_string(),
            node_type: "End".to_string(),
            position: None,
            parameters: HashMap::new(),
            successors: vec![],
            predecessors: vec!["B".to_string(), "C".to_string()],
        });

        AstGraph {
            graph_id: "test".to_string(),
            version: "1.0".to_string(),
            nodes,
            entry_nodes: vec!["A".to_string()],
        }
    }

    #[test]
    fn test_topological_sort_valid() {
        let graph = create_test_graph();
        let result = topological_sort(&graph);
        assert!(result.is_ok());
        let sorted = result.unwrap();
        assert_eq!(sorted.len(), 4);
        // A must come before B, C; B and C must come before D
        assert_eq!(sorted[0], "A");
        assert_eq!(sorted[3], "D");
    }
}

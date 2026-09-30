//! Codegen - AST to Opcode compilation
//!
//! Generates stack-based bytecode from the AST. Every node becomes a labelled
//! block of opcodes; control flow between nodes is expressed with JUMP /
//! IF_JUMP, whose targets are resolved in a second pass. Nodes are emitted in
//! depth-first order from the start node, so a node's first successor usually
//! follows it directly and needs no jump.
//!
//! Opcodes: PUSH, VAR_SET, VAR_GET, EQ, IF_JUMP, JUMP, YIELD, WAIT, CALL_BLOCK, END

use crate::compiler::graph_parser::{AstGraph, AstNode};
use pyo3::prelude::*;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Variable that receives the player's choice in BranchChoice nodes
pub const BRANCH_RESULT_VAR: &str = "__branch_result";

/// Bytecode opcodes for the VM
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq)]
pub enum Opcode {
    PUSH(JsonValue),
    VAR_SET { name: String },
    VAR_GET { name: String },
    /// Pop b, pop a, push (a == b)
    EQ,
    /// Pop condition, jump if truthy
    IF_JUMP { target: usize },
    JUMP { target: usize },
    YIELD { wait_type: String },
    WAIT { frames: u32 },
    CALL_BLOCK { block_type: String, args: Vec<String> },
    END,
}

impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Opcode::PUSH(v) => write!(f, "PUSH({})", v),
            Opcode::VAR_SET { name } => write!(f, "VAR_SET({})", name),
            Opcode::VAR_GET { name } => write!(f, "VAR_GET({})", name),
            Opcode::EQ => write!(f, "EQ"),
            Opcode::IF_JUMP { target } => write!(f, "IF_JUMP({})", target),
            Opcode::JUMP { target } => write!(f, "JUMP({})", target),
            Opcode::YIELD { wait_type } => write!(f, "YIELD({})", wait_type),
            Opcode::WAIT { frames } => write!(f, "WAIT({})", frames),
            Opcode::CALL_BLOCK { block_type, args } => {
                write!(f, "CALL_BLOCK({}, {:?})", block_type, args)
            }
            Opcode::END => write!(f, "END"),
        }
    }
}

/// Compiled program with opcodes and metadata
#[derive(Debug, Clone)]
#[pyclass]
pub struct CompiledProgram {
    #[pyo3(get)]
    pub graph_id: String,
    pub opcodes: Vec<Opcode>,
    /// Opcode index → ID of the node that produced it
    #[pyo3(get)]
    pub node_map: Vec<String>,
    /// Node ID → node type
    #[pyo3(get)]
    pub node_types: HashMap<String, String>,
}

#[pymethods]
impl CompiledProgram {
    fn __repr__(&self) -> String {
        format!(
            "CompiledProgram(id='{}', opcodes={}, nodes={})",
            self.graph_id,
            self.opcodes.len(),
            self.node_types.len()
        )
    }

    /// Get opcode count
    pub fn opcode_count(&self) -> usize {
        self.opcodes.len()
    }

    /// Opcodes as strings, e.g. "JUMP(4)"
    pub fn get_opcodes(&self) -> Vec<String> {
        self.opcodes.iter().map(|op| op.to_string()).collect()
    }

    /// Get disassembly string
    pub fn disassemble(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("=== {} ===", self.graph_id));

        for (i, opcode) in self.opcodes.iter().enumerate() {
            let node_info = self
                .node_map
                .get(i)
                .map(|id| format!(" [{}]", id))
                .unwrap_or_default();
            lines.push(format!("{:04}: {}{}", i, opcode, node_info));
        }

        lines.join("\n")
    }
}

/// Compile AST graph to bytecode
pub fn compile_graph(graph: &AstGraph) -> Result<CompiledProgram, String> {
    let start = graph
        .start_node()
        .ok_or_else(|| "Cannot determine the start node".to_string())?;
    let order = emission_order(graph, &start);

    for id in graph.get_node_ids() {
        if !order.contains(&id) {
            log::warn!("Node '{}' is unreachable from '{}' and was skipped", id, start);
        }
    }

    let mut gen = Codegen::default();
    for (i, node_id) in order.iter().enumerate() {
        let node = &graph.nodes[node_id];
        gen.node_start.insert(node_id.clone(), gen.opcodes.len());
        gen.emit_node(node, order.get(i + 1))?;
    }

    if gen.opcodes.is_empty() {
        gen.emit(Opcode::END, "empty_graph");
    }

    gen.resolve_jumps()?;

    Ok(CompiledProgram {
        graph_id: graph.graph_id.clone(),
        opcodes: gen.opcodes,
        node_map: gen.node_map,
        node_types: graph
            .nodes
            .iter()
            .map(|(id, n)| (id.clone(), n.node_type.clone()))
            .collect(),
    })
}

/// Depth-first pre-order from the start node, first successor first.
/// Unreachable nodes are left out.
fn emission_order(graph: &AstGraph, start: &str) -> Vec<String> {
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![start.to_string()];

    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(node) = graph.nodes.get(&id) {
            for successor in node.successors.iter().rev() {
                if !seen.contains(successor) {
                    stack.push(successor.clone());
                }
            }
        }
        order.push(id);
    }
    order
}

#[derive(Default)]
struct Codegen {
    opcodes: Vec<Opcode>,
    node_map: Vec<String>,
    node_start: HashMap<String, usize>,
    /// (opcode index, target node id) of jumps to patch
    patches: Vec<(usize, String)>,
}

impl Codegen {
    fn emit(&mut self, opcode: Opcode, node_id: &str) {
        self.opcodes.push(opcode);
        self.node_map.push(node_id.to_string());
    }

    fn emit_jump(&mut self, opcode: Opcode, target_node: &str, node_id: &str) {
        self.patches.push((self.opcodes.len(), target_node.to_string()));
        self.emit(opcode, node_id);
    }

    /// Continue to the node's single successor, or end if it has none
    fn emit_flow(&mut self, node: &AstNode, next_emitted: Option<&String>) {
        match node.successors.first() {
            None => self.emit(Opcode::END, &node.id),
            Some(succ) if Some(succ) == next_emitted => {} // falls through
            Some(succ) => self.emit_jump(Opcode::JUMP { target: 0 }, succ, &node.id),
        }
    }

    fn emit_node(&mut self, node: &AstNode, next_emitted: Option<&String>) -> Result<(), String> {
        let id = node.id.as_str();

        match node.node_type.as_str() {
            "entry" => {
                // Entry is just a marker
                self.emit_flow(node, next_emitted);
            }

            "ShowText" => {
                let text = node.param("text").unwrap_or(JsonValue::String(String::new()));
                let speaker = node.param("speaker").unwrap_or(JsonValue::Null);
                self.emit(Opcode::PUSH(text), id);
                self.emit(Opcode::PUSH(speaker), id);
                // Args are listed in push order; the VM pops them in reverse
                self.emit(
                    Opcode::CALL_BLOCK {
                        block_type: "ShowText".to_string(),
                        args: vec!["text".to_string(), "speaker".to_string()],
                    },
                    id,
                );
                // Wait for the player to read the line
                self.emit(Opcode::YIELD { wait_type: "input".to_string() }, id);
                self.emit_flow(node, next_emitted);
            }

            "BranchChoice" => {
                let choices = node.param("choices").unwrap_or(JsonValue::Array(Vec::new()));
                self.emit(Opcode::PUSH(choices), id);
                self.emit(
                    Opcode::CALL_BLOCK {
                        block_type: "BranchChoice".to_string(),
                        args: vec!["choices".to_string()],
                    },
                    id,
                );
                // resume(choice) pushes the selected index
                self.emit(Opcode::YIELD { wait_type: "choice".to_string() }, id);
                self.emit(Opcode::VAR_SET { name: BRANCH_RESULT_VAR.to_string() }, id);

                // Jump table: if __branch_result == i goto successor i
                for (i, succ) in node.successors.iter().enumerate() {
                    self.emit(Opcode::VAR_GET { name: BRANCH_RESULT_VAR.to_string() }, id);
                    self.emit(Opcode::PUSH(JsonValue::from(i)), id);
                    self.emit(Opcode::EQ, id);
                    self.emit_jump(Opcode::IF_JUMP { target: 0 }, succ, id);
                }
                // No branch matched
                self.emit(Opcode::END, id);
            }

            "SetVariable" => {
                let name = match node.param("name") {
                    Some(JsonValue::String(name)) if !name.is_empty() => name,
                    _ => return Err(format!("SetVariable '{}' has no variable name", id)),
                };
                let value = node
                    .param("value")
                    .ok_or_else(|| format!("SetVariable '{}' has no value", id))?;
                self.emit(Opcode::PUSH(value), id);
                self.emit(Opcode::VAR_SET { name }, id);
                self.emit_flow(node, next_emitted);
            }

            "EndScene" | "End" => {
                self.emit(Opcode::END, id);
            }

            _ => {
                // Generic block: push parameters in name order and call it
                let mut names: Vec<&String> = node.parameters.keys().collect();
                names.sort();
                for name in &names {
                    let value = node.param(name).unwrap_or(JsonValue::Null);
                    self.emit(Opcode::PUSH(value), id);
                }
                self.emit(
                    Opcode::CALL_BLOCK {
                        block_type: node.node_type.clone(),
                        args: names.into_iter().cloned().collect(),
                    },
                    id,
                );
                self.emit_flow(node, next_emitted);
            }
        }
        Ok(())
    }

    fn resolve_jumps(&mut self) -> Result<(), String> {
        for (index, target_node) in &self.patches {
            let target = *self
                .node_start
                .get(target_node)
                .ok_or_else(|| format!("Jump to unknown node '{}'", target_node))?;
            match &mut self.opcodes[*index] {
                Opcode::JUMP { target: t } | Opcode::IF_JUMP { target: t } => *t = target,
                other => return Err(format!("Opcode {} at {} is not a jump", other, index)),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::graph_parser::parse_json_graph;

    fn compile(json: &str) -> CompiledProgram {
        compile_graph(&parse_json_graph(json).unwrap()).unwrap()
    }

    #[test]
    fn example_compiles_with_resolved_jumps() {
        let program = compile(include_str!("../../examples/simple_novel.json"));
        assert_eq!(program.node_map.len(), program.opcodes.len());

        for (i, op) in program.opcodes.iter().enumerate() {
            if let Opcode::JUMP { target } | Opcode::IF_JUMP { target } = op {
                assert!(*target < program.opcodes.len(), "bad target at {}", i);
                assert_ne!(*target, 0, "unpatched jump at {}", i);
            }
        }
        // Branch targets land on the first opcode of each successor
        let starts: Vec<&str> = program
            .opcodes
            .iter()
            .filter_map(|op| match op {
                Opcode::IF_JUMP { target } => Some(program.node_map[*target].as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(starts, vec!["adventure_path", "exit_path"]);
    }

    #[test]
    fn show_text_args_match_push_order() {
        let program = compile(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"t","type":"ShowText","parameters":{"text":"Hi","speaker":"Bob"}}],
                "edges":[]}"#,
        );
        assert_eq!(
            program.get_opcodes(),
            vec![
                "PUSH(\"Hi\")",
                "PUSH(\"Bob\")",
                "CALL_BLOCK(ShowText, [\"text\", \"speaker\"])",
                "YIELD(input)",
                "END"
            ]
        );
    }

    #[test]
    fn merge_node_is_emitted_once_and_jumped_to() {
        // s -> b(choice) -> {x, y} -> m -> end
        let program = compile(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},
                         {"id":"b","type":"BranchChoice","parameters":{"choices":["x","y"]}},
                         {"id":"x","type":"SetVariable","parameters":{"name":"v","value":1}},
                         {"id":"y","type":"SetVariable","parameters":{"name":"v","value":2}},
                         {"id":"m","type":"ShowText","parameters":{"text":"merged"}},
                         {"id":"e","type":"End"}],
                "edges":[{"from":"s","to":"b"},{"from":"b","to":"x","port":0},{"from":"b","to":"y","port":1},
                         {"from":"x","to":"m"},{"from":"y","to":"m"},{"from":"m","to":"e"}]}"#,
        );
        let m_count = program
            .opcodes
            .iter()
            .filter(|op| matches!(op, Opcode::CALL_BLOCK { block_type, .. } if block_type == "ShowText"))
            .count();
        assert_eq!(m_count, 1);
    }

    #[test]
    fn generic_block_bytecode_is_deterministic() {
        let json = r#"{"graph_id":"g","version":"1.0",
            "nodes":[{"id":"p","type":"PlayVoice","parameters":{"voice_id":"v1","volume":0.5,"a":1,"z":2}}],
            "edges":[]}"#;
        let first = compile(json).get_opcodes();
        for _ in 0..10 {
            assert_eq!(compile(json).get_opcodes(), first);
        }
        assert_eq!(first[4], "CALL_BLOCK(PlayVoice, [\"a\", \"voice_id\", \"volume\", \"z\"])");
    }
}

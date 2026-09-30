//! Codegen - AST to Opcode compilation
//!
//! Generates stack-based bytecode from the AST. Every node becomes a labelled
//! block of opcodes; control flow between nodes is expressed with JUMP /
//! IF_JUMP, whose targets are resolved in a second pass. Nodes are emitted in
//! depth-first order from the start node, so a node's first successor usually
//! follows it directly and needs no jump.
//!
//! Expressions (conditions, assignments, `{...}` in text) compile to stack
//! code; `and`/`or` short-circuit with jumps local to the node.

use crate::compiler::expr::{parse_expr, parse_template, BinaryOp, Expr, TemplatePart, UnaryOp};
use crate::compiler::graph_parser::{AstGraph, AstNode};
use crate::core::plugin_registry::{BlockDefinition, PluginRegistry};
use pyo3::prelude::*;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Variable that receives the player's choice in BranchChoice nodes
pub const BRANCH_RESULT_VAR: &str = "__branch_result";

/// Whether nodes of this type YIELD to wait for the player
pub fn node_waits_for_player(node_type: &str, registry: &PluginRegistry) -> bool {
    node_type == "BranchChoice" || registry.block(node_type).is_some_and(|b| b.wait.is_some())
}

/// Bytecode opcodes for the VM
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq)]
pub enum Opcode {
    PUSH(JsonValue),
    POP,
    DUP,
    VAR_SET { name: String },
    VAR_GET { name: String },
    // Binary operators pop b, pop a and push (a op b)
    ADD,
    SUB,
    MUL,
    DIV,
    MOD,
    EQ,
    NE,
    LT,
    LE,
    GT,
    GE,
    // Unary operators replace the top of the stack
    NEG,
    NOT,
    /// Pop `count` values, push their text joined in push order
    CONCAT { count: usize },
    /// Pop `count` values, push them as an array in push order
    MAKE_ARRAY { count: usize },
    /// Pop condition, jump if truthy
    IF_JUMP { target: usize },
    /// Pop condition, jump if falsy
    JUMP_IF_FALSE { target: usize },
    JUMP { target: usize },
    YIELD { wait_type: String },
    /// Pause for this many frames (see Machine::frame)
    WAIT { frames: u32 },
    /// Continue at the start of another scene
    JUMP_SCENE { scene: String },
    /// Run another scene; its END returns here
    CALL_SCENE { scene: String },
    CALL_BLOCK { block_type: String, args: Vec<String> },
    END,
}

impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Opcode::PUSH(v) => write!(f, "PUSH({})", v),
            Opcode::POP => write!(f, "POP"),
            Opcode::DUP => write!(f, "DUP"),
            Opcode::VAR_SET { name } => write!(f, "VAR_SET({})", name),
            Opcode::VAR_GET { name } => write!(f, "VAR_GET({})", name),
            Opcode::ADD => write!(f, "ADD"),
            Opcode::SUB => write!(f, "SUB"),
            Opcode::MUL => write!(f, "MUL"),
            Opcode::DIV => write!(f, "DIV"),
            Opcode::MOD => write!(f, "MOD"),
            Opcode::EQ => write!(f, "EQ"),
            Opcode::NE => write!(f, "NE"),
            Opcode::LT => write!(f, "LT"),
            Opcode::LE => write!(f, "LE"),
            Opcode::GT => write!(f, "GT"),
            Opcode::GE => write!(f, "GE"),
            Opcode::NEG => write!(f, "NEG"),
            Opcode::NOT => write!(f, "NOT"),
            Opcode::CONCAT { count } => write!(f, "CONCAT({})", count),
            Opcode::MAKE_ARRAY { count } => write!(f, "MAKE_ARRAY({})", count),
            Opcode::IF_JUMP { target } => write!(f, "IF_JUMP({})", target),
            Opcode::JUMP_IF_FALSE { target } => write!(f, "JUMP_IF_FALSE({})", target),
            Opcode::JUMP { target } => write!(f, "JUMP({})", target),
            Opcode::YIELD { wait_type } => write!(f, "YIELD({})", wait_type),
            Opcode::WAIT { frames } => write!(f, "WAIT({})", frames),
            Opcode::JUMP_SCENE { scene } => write!(f, "JUMP_SCENE({})", scene),
            Opcode::CALL_SCENE { scene } => write!(f, "CALL_SCENE({})", scene),
            Opcode::CALL_BLOCK { block_type, args } => {
                write!(f, "CALL_BLOCK({}, {:?})", block_type, args)
            }
            Opcode::END => write!(f, "END"),
        }
    }
}

/// Stable 64-bit FNV-1a hash of newline-terminated lines, as hex
pub fn fnv_hex(lines: impl Iterator<Item = String>) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for line in lines {
        for byte in line.bytes().chain(std::iter::once(b'\n')) {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{:016x}", hash)
}

/// Compiled program with opcodes and metadata
#[derive(Debug, Clone)]
#[pyclass(skip_from_py_object)]
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

    /// Stable 64-bit FNV-1a hash of the bytecode, as hex. Identifies the
    /// exact program a save state belongs to.
    pub fn fingerprint(&self) -> String {
        fnv_hex(self.opcodes.iter().map(|op| op.to_string()))
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
pub fn compile_graph(graph: &AstGraph, registry: &PluginRegistry) -> Result<CompiledProgram, String> {
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
        gen.emit_node(node, order.get(i + 1), registry)?;
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

/// Target scene of a JumpScene / CallScene node
pub fn scene_param(node: &AstNode) -> Result<String, String> {
    match node.param("scene") {
        Some(JsonValue::String(scene)) if !scene.is_empty() => Ok(scene),
        _ => Err(format!("{} '{}' needs a 'scene' name", node.node_type, node.id)),
    }
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

    fn emit_node(
        &mut self,
        node: &AstNode,
        next_emitted: Option<&String>,
        registry: &PluginRegistry,
    ) -> Result<(), String> {
        let id = node.id.as_str();

        match node.node_type.as_str() {
            "entry" => {
                // Entry is just a marker
                self.emit_flow(node, next_emitted);
            }

            "BranchChoice" => {
                let choices = node.param("choices").unwrap_or(JsonValue::Array(Vec::new()));
                self.emit_choices(&choices, id)?;
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
                match (node.param("expression"), node.param("value")) {
                    (Some(JsonValue::String(source)), None) => {
                        let expr = parse_expr(&source)
                            .map_err(|e| format!("SetVariable '{}': {}", id, e))?;
                        self.emit_expr(&expr, id);
                    }
                    (None, Some(value)) => self.emit(Opcode::PUSH(value), id),
                    _ => {
                        return Err(format!(
                            "SetVariable '{}' needs either 'value' or an 'expression' string",
                            id
                        ))
                    }
                }
                self.emit(Opcode::VAR_SET { name }, id);
                self.emit_flow(node, next_emitted);
            }

            "If" => {
                let condition = match node.param("condition") {
                    Some(JsonValue::String(source)) => {
                        parse_expr(&source).map_err(|e| format!("If '{}': {}", id, e))?
                    }
                    Some(value) => Expr::Literal(value),
                    None => return Err(format!("If '{}' has no condition", id)),
                };
                let [when_true, when_false] = node.successors.as_slice() else {
                    return Err(format!("If '{}' needs a true and a false output", id));
                };
                self.emit_expr(&condition, id);
                if Some(when_true) == next_emitted {
                    // True branch follows directly
                    self.emit_jump(Opcode::JUMP_IF_FALSE { target: 0 }, when_false, id);
                } else {
                    self.emit_jump(Opcode::IF_JUMP { target: 0 }, when_true, id);
                    if Some(when_false) != next_emitted {
                        self.emit_jump(Opcode::JUMP { target: 0 }, when_false, id);
                    }
                }
            }

            "EndScene" | "End" => {
                self.emit(Opcode::END, id);
            }

            "JumpScene" => {
                self.emit(Opcode::JUMP_SCENE { scene: scene_param(node)? }, id);
            }

            "CallScene" => {
                self.emit(Opcode::CALL_SCENE { scene: scene_param(node)? }, id);
                self.emit_flow(node, next_emitted);
            }

            "Wait" => {
                let frames = node
                    .param("frames")
                    .and_then(|f| f.as_u64())
                    .and_then(|f| u32::try_from(f).ok())
                    .ok_or_else(|| format!("Wait '{}' needs a whole number of 'frames'", id))?;
                self.emit(Opcode::WAIT { frames }, id);
                self.emit_flow(node, next_emitted);
            }

            block_type => match registry.block(block_type) {
                Some(definition) => self.emit_block(node, definition, next_emitted)?,
                None => return Err(format!("Node '{}' has unknown block type '{}'", id, block_type)),
            },
        }
        Ok(())
    }

    /// A block: push its arguments in declared order, call it, then wait
    /// for the player if the block says so
    fn emit_block(
        &mut self,
        node: &AstNode,
        definition: &BlockDefinition,
        next_emitted: Option<&String>,
    ) -> Result<(), String> {
        let id = node.id.as_str();
        for param in &definition.parameters {
            let value = node
                .param(&param.name)
                .or_else(|| param.default.clone())
                .ok_or_else(|| {
                    format!(
                        "Node '{}' ({}) is missing parameter '{}'",
                        id, definition.block_type, param.name
                    )
                })?;
            match (param.param_type.as_str(), value) {
                ("text", value) => self.emit_text(&value, id)?,
                ("expression", JsonValue::String(source)) => {
                    let expr = parse_expr(&source)
                        .map_err(|e| format!("Node '{}' parameter '{}': {}", id, param.name, e))?;
                    self.emit_expr(&expr, id);
                }
                (_, value) => self.emit(Opcode::PUSH(value), id),
            }
        }
        self.emit(
            Opcode::CALL_BLOCK {
                block_type: definition.block_type.clone(),
                args: definition.parameters.iter().map(|p| p.name.clone()).collect(),
            },
            id,
        );
        if let Some(wait_type) = &definition.wait {
            self.emit(Opcode::YIELD { wait_type: wait_type.clone() }, id);
        }
        if definition.outputs.is_empty() {
            self.emit(Opcode::END, id);
        } else {
            self.emit_flow(node, next_emitted);
        }
        Ok(())
    }

    /// Compile an expression that leaves its value on the stack
    fn emit_expr(&mut self, expr: &Expr, id: &str) {
        match expr {
            Expr::Literal(value) => self.emit(Opcode::PUSH(value.clone()), id),
            Expr::Var(name) => self.emit(Opcode::VAR_GET { name: name.clone() }, id),
            Expr::Unary(op, operand) => {
                self.emit_expr(operand, id);
                let op = match op {
                    UnaryOp::Neg => Opcode::NEG,
                    UnaryOp::Not => Opcode::NOT,
                };
                self.emit(op, id);
            }
            Expr::Binary(op, a, b) => {
                self.emit_expr(a, id);
                self.emit_expr(b, id);
                let op = match op {
                    BinaryOp::Add => Opcode::ADD,
                    BinaryOp::Sub => Opcode::SUB,
                    BinaryOp::Mul => Opcode::MUL,
                    BinaryOp::Div => Opcode::DIV,
                    BinaryOp::Mod => Opcode::MOD,
                    BinaryOp::Eq => Opcode::EQ,
                    BinaryOp::Ne => Opcode::NE,
                    BinaryOp::Lt => Opcode::LT,
                    BinaryOp::Le => Opcode::LE,
                    BinaryOp::Gt => Opcode::GT,
                    BinaryOp::Ge => Opcode::GE,
                };
                self.emit(op, id);
            }
            // a and b: keep a if it is falsy, otherwise evaluate b
            // a or b:  keep a if it is truthy, otherwise evaluate b
            Expr::And(a, b) | Expr::Or(a, b) => {
                self.emit_expr(a, id);
                self.emit(Opcode::DUP, id);
                let skip = self.opcodes.len();
                let jump = if matches!(expr, Expr::And(..)) {
                    Opcode::JUMP_IF_FALSE { target: 0 }
                } else {
                    Opcode::IF_JUMP { target: 0 }
                };
                self.emit(jump, id);
                self.emit(Opcode::POP, id);
                self.emit_expr(b, id);
                let end = self.opcodes.len();
                if let Opcode::IF_JUMP { target } | Opcode::JUMP_IF_FALSE { target } =
                    &mut self.opcodes[skip]
                {
                    *target = end;
                }
            }
        }
    }

    /// Push a text value, expanding `{expression}` placeholders in strings
    fn emit_text(&mut self, value: &JsonValue, id: &str) -> Result<(), String> {
        let JsonValue::String(source) = value else {
            self.emit(Opcode::PUSH(value.clone()), id);
            return Ok(());
        };
        let parts = parse_template(source).map_err(|e| format!("Node '{}': {}", id, e))?;
        if let [TemplatePart::Text(text)] = parts.as_slice() {
            self.emit(Opcode::PUSH(JsonValue::String(text.clone())), id);
            return Ok(());
        }
        for part in &parts {
            match part {
                TemplatePart::Text(text) => {
                    self.emit(Opcode::PUSH(JsonValue::String(text.clone())), id)
                }
                TemplatePart::Expr(expr) => self.emit_expr(expr, id),
            }
        }
        self.emit(Opcode::CONCAT { count: parts.len() }, id);
        Ok(())
    }

    /// Push the choice list; labels may contain placeholders
    fn emit_choices(&mut self, choices: &JsonValue, id: &str) -> Result<(), String> {
        let JsonValue::Array(items) = choices else {
            self.emit(Opcode::PUSH(choices.clone()), id);
            return Ok(());
        };
        let is_static = |item: &JsonValue| match item {
            JsonValue::String(s) => !s.contains(['{', '}']),
            _ => true,
        };
        if items.iter().all(is_static) {
            self.emit(Opcode::PUSH(choices.clone()), id);
            return Ok(());
        }
        for item in items {
            self.emit_text(item, id)?;
        }
        self.emit(Opcode::MAKE_ARRAY { count: items.len() }, id);
        Ok(())
    }

    fn resolve_jumps(&mut self) -> Result<(), String> {
        for (index, target_node) in &self.patches {
            let target = *self
                .node_start
                .get(target_node)
                .ok_or_else(|| format!("Jump to unknown node '{}'", target_node))?;
            match &mut self.opcodes[*index] {
                Opcode::JUMP { target: t }
                | Opcode::IF_JUMP { target: t }
                | Opcode::JUMP_IF_FALSE { target: t } => *t = target,
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
        compile_graph(&parse_json_graph(json).unwrap(), &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap()).unwrap()
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
    fn block_args_follow_the_definition() {
        let json = r#"{"graph_id":"g","version":"1.0",
            "nodes":[{"id":"p","type":"PlayVoice","parameters":{"volume":0.5,"voice_id":"v1"}}],
            "edges":[]}"#;
        let first = compile(json).get_opcodes();
        for _ in 0..10 {
            assert_eq!(compile(json).get_opcodes(), first);
        }
        assert_eq!(
            first,
            vec!["PUSH(\"v1\")", "PUSH(0.5)", "CALL_BLOCK(PlayVoice, [\"voice_id\", \"volume\"])", "END"]
        );
    }

    #[test]
    fn defaults_fill_missing_parameters() {
        let program = compile(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"p","type":"PlayVoice","parameters":{"voice_id":"v1"}}],"edges":[]}"#,
        );
        assert_eq!(program.get_opcodes()[1], "PUSH(1.0)");
    }

    #[test]
    fn custom_blocks_follow_their_definition() {
        use crate::core::plugin_registry::{BlockDefinition, ParameterDefinition, PluginInfo};
        use serde_json::json;

        let mut registry = PluginRegistry::with_defaults().unwrap();
        registry.add_plugin(PluginInfo::new("fx", "1", "me")).unwrap();
        registry
            .add_block(
                "fx",
                BlockDefinition::new(
                    "Shake",
                    "Shake Screen",
                    vec![],
                    vec![],
                    vec![
                        ParameterDefinition::new("strength", "expression", Some(json!(1))),
                        ParameterDefinition::new("caption", "text", Some(json!(""))),
                    ],
                    Some("animation".to_string()),
                ),
            )
            .unwrap();
        let graph = parse_json_graph(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"Shake","parameters":{"strength":"level * 2","caption":"Lv {level}"}}],
                "edges":[]}"#,
        )
        .unwrap();
        let program = compile_graph(&graph, &registry).unwrap();
        assert_eq!(
            program.get_opcodes(),
            vec![
                "VAR_GET(level)",
                "PUSH(2)",
                "MUL",
                "PUSH(\"Lv \")",
                "VAR_GET(level)",
                "CONCAT(2)",
                "CALL_BLOCK(Shake, [\"strength\", \"caption\"])",
                "YIELD(animation)",
                "END"
            ]
        );
    }
}

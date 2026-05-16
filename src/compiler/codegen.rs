//! Codegen - AST to Opcode compilation
//!
//! Generates stack-based bytecode from the sorted AST.
//! Opcodes: PUSH, VAR_SET, VAR_GET, IF_JUMP, JUMP, YIELD, WAIT, CALL_BLOCK, END

use crate::compiler::graph_parser::AstGraph;
use pyo3::prelude::*;
use serde_json::Value as JsonValue;

/// Bytecode opcodes for the VM
#[derive(Debug, Clone)]
#[pyclass]
pub enum Opcode {
    PUSH(JsonValue),
    VAR_SET { name: String },
    VAR_GET { name: String },
    IF_JUMP { target: usize },
    JUMP { target: usize },
    YIELD { wait_type: String },
    WAIT { frames: u32 },
    CALL_BLOCK { block_type: String, args: Vec<String> },
    END,
}

#[pymethods]
impl Opcode {
    fn __repr__(&self) -> String {
        match self {
            Opcode::PUSH(v) => format!("PUSH({})", v),
            Opcode::VAR_SET { name } => format!("VAR_SET({})", name),
            Opcode::VAR_GET { name } => format!("VAR_GET({})", name),
            Opcode::IF_JUMP { target } => format!("IF_JUMP({})", target),
            Opcode::JUMP { target } => format!("JUMP({})", target),
            Opcode::YIELD { wait_type } => format!("YIELD({})", wait_type),
            Opcode::WAIT { frames } => format!("WAIT({})", frames),
            Opcode::CALL_BLOCK { block_type, args } => {
                format!("CALL_BLOCK({}, {:?})", block_type, args
            }
            Opcode::END => "END".to_string(),
        }
    }
}

/// Compiled program with opcodes and metadata
#[derive(Debug, Clone)]
#[pyclass]
pub struct CompiledProgram {
    #[pyo3(get)]
    pub graph_id: String,
    #[pyo3(get)]
    pub opcodes: Vec<Opcode>,
    #[pyo3(get)]
    pub node_map: Vec<String>, // Index → NodeID mapping for debugging
}

#[pymethods]
impl CompiledProgram {
    fn __repr__(&self) -> String {
        format!(
            "CompiledProgram(id='{}', opcodes={}, nodes={})",
            self.graph_id,
            self.opcodes.len(),
            self.node_map.len()
        )
    }

    /// Get opcode count
    fn opcode_count(&self) -> usize {
        self.opcodes.len()
    }

    /// Get disassembly string
    fn disassemble(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("=== {} ===", self.graph_id));
        
        for (i, opcode) in self.opcodes.iter().enumerate() {
            let node_info = self.node_map.get(i)
                .map(|id| format!(" [{}]", id))
                .unwrap_or_default();
            lines.push(format!("{:04}: {:?}{}", i, opcode, node_info));
        }
        
        lines.join("\n")
    }
}

/// Compile AST graph to bytecode
pub fn compile_graph(graph: &AstGraph) -> Result<CompiledProgram, String> {
    use crate::compiler::topology::topological_sort;
    
    // Get execution order
    let sorted_nodes = topological_sort(graph)?;
    
    let mut opcodes: Vec<Opcode> = Vec::new();
    let mut node_map: Vec<String> = Vec::new();
    let mut label_targets: HashMap<String, usize> = HashMap::new();
    
    // First pass: generate opcodes and record label positions
    for node_id in &sorted_nodes {
        if let Some(node) = graph.nodes.get(node_id) {
            let opcode_index = opcodes.len();
            node_map.push(node_id.clone());
            
            // Generate opcodes based on node type
            match node.node_type.as_str() {
                "entry" => {
                    // Entry is just a marker, no opcodes needed
                }
                
                "ShowText" => {
                    // Push text and speaker, then call block
                    if let Some(text) = node.parameters.get("text") {
                        if let Ok(json_val) = serde_json::from_str::<JsonValue>(text) {
                            opcodes.push(Opcode::PUSH(json_val));
                        } else {
                            opcodes.push(Opcode::PUSH(JsonValue::String(text.clone())));
                        }
                    } else {
                        opcodes.push(Opcode::PUSH(JsonValue::String(String::new())));
                    }
                    
                    if let Some(speaker) = node.parameters.get("speaker") {
                        if let Ok(json_val) = serde_json::from_str::<JsonValue>(speaker) {
                            opcodes.push(Opcode::PUSH(json_val));
                        } else {
                            opcodes.push(Opcode::PUSH(JsonValue::String(speaker.clone())));
                        }
                    } else {
                        opcodes.push(Opcode::PUSH(JsonValue::Null));
                    }
                    
                    opcodes.push(Opcode::CALL_BLOCK {
                        block_type: "ShowText".to_string(),
                        args: vec!["speaker".to_string(), "text".to_string()],
                    });
                    
                    // Yield for user to read
                    opcodes.push(Opcode::YIELD {
                        wait_type: "input".to_string(),
                    });
                }
                
                "BranchChoice" => {
                    // Push choices array
                    if let Some(choices) = node.parameters.get("choices") {
                        if let Ok(json_val) = serde_json::from_str::<JsonValue>(choices) {
                            opcodes.push(Opcode::PUSH(json_val));
                        }
                    }
                    
                    opcodes.push(Opcode::CALL_BLOCK {
                        block_type: "BranchChoice".to_string(),
                        args: vec!["choices".to_string()],
                    });
                    
                    opcodes.push(Opcode::YIELD {
                        wait_type: "choice".to_string(),
                    });
                    
                    // Store result in temp variable
                    opcodes.push(Opcode::VAR_SET {
                        name: "__branch_result".to_string(),
                    });
                    
                    // Generate conditional jumps based on number of successors
                    let num_branches = node.successors.len();
                    let mut branch_labels: Vec<String> = Vec::new();
                    
                    for i in 0..num_branches {
                        let label = format!("branch_{}_{}", node_id, i);
                        branch_labels.push(label.clone());
                        
                        // Check if result equals this branch index
                        opcodes.push(Opcode::VAR_GET {
                            name: "__branch_result".to_string(),
                        });
                        opcodes.push(Opcode::PUSH(JsonValue::Number(
                            serde_json::Number::from(i)
                        )));
                        // Placeholder for IF_JUMP - will be fixed in second pass
                        opcodes.push(Opcode::IF_JUMP { target: 0 });
                        label_targets.insert(label, opcodes.len() - 1);
                    }
                    
                    // Jump to end if no branch matched
                    opcodes.push(Opcode::JUMP { target: 0 }); // Placeholder
                    let end_jump_idx = opcodes.len() - 1;
                    
                    // Record where each branch should jump to
                    for (i, label) in branch_labels.iter().enumerate() {
                        let successor = node.successors.get(i);
                        if let Some(succ_id) = successor {
                            // We'll fix these targets in second pass
                            let _ = (label.clone(), succ_id.clone());
                        }
                    }
                    
                    // Placeholder for branch end
                    label_targets.insert(format!("branch_end_{}", node_id), opcodes.len());
                }
                
                "SetVariable" => {
                    // Push value, then store in variable
                    if let Some(value) = node.parameters.get("value") {
                        if let Ok(json_val) = serde_json::from_str::<JsonValue>(value) {
                            opcodes.push(Opcode::PUSH(json_val));
                        }
                    }
                    
                    if let Some(var_name) = node.parameters.get("name") {
                        let name = var_name.trim_matches('"').to_string();
                        opcodes.push(Opcode::VAR_SET { name });
                    }
                }
                
                "EndScene" | "End" => {
                    opcodes.push(Opcode::END);
                }
                
                _ => {
                    // Generic block: push all parameters and call
                    let mut args: Vec<String> = Vec::new();
                    for (param_name, param_value) in &node.parameters {
                        if let Ok(json_val) = serde_json::from_str::<JsonValue>(param_value) {
                            opcodes.push(Opcode::PUSH(json_val));
                        } else {
                            opcodes.push(Opcode::PUSH(JsonValue::String(param_value.clone())));
                        }
                        args.push(param_name.clone());
                    }
                    
                    opcodes.push(Opcode::CALL_BLOCK {
                        block_type: node.node_type.clone(),
                        args,
                    });
                }
            }
        }
    }
    
    // Ensure program ends with END if it doesn't already
    if let Some(last) = opcodes.last() {
        if !matches!(last, Opcode::END) {
            opcodes.push(Opcode::END);
            node_map.push("implicit_end".to_string());
        }
    } else {
        opcodes.push(Opcode::END);
        node_map.push("empty_graph".to_string());
    }
    
    // Second pass: fix up jump targets
    // For Phase 1 MVP, we keep it simple and don't fully resolve all jumps
    // This is a known limitation to be addressed in Phase 2
    
    Ok(CompiledProgram {
        graph_id: graph.graph_id.clone(),
        opcodes,
        node_map,
    })
}

use std::collections::HashMap;

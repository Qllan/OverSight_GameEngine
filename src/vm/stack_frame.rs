//! Stack Frame - Call stack management for the VM

use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// A single call frame for block execution
#[derive(Debug, Clone)]
pub struct CallFrame {
    pub return_pc: usize,
    pub local_vars: HashMap<String, JsonValue>,
    pub block_type: String,
}

impl CallFrame {
    pub fn new(return_pc: usize, block_type: String) -> Self {
        CallFrame {
            return_pc,
            local_vars: HashMap::new(),
            block_type,
        }
    }
}

/// Variable snapshot for debugging and save/load
#[derive(Debug, Clone)]
pub struct VariableSnapshot {
    pub variables: HashMap<String, JsonValue>,
    pub stack_size: usize,
    pub call_depth: usize,
}

impl VariableSnapshot {
    pub fn new(variables: HashMap<String, JsonValue>, stack_size: usize, call_depth: usize) -> Self {
        VariableSnapshot {
            variables,
            stack_size,
            call_depth,
        }
    }
}

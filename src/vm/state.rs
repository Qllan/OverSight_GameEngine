//! VM State - Execution state for debugging and serialization

use pyo3::prelude::*;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Python-exposed VM state for debugging
#[derive(Debug, Clone)]
#[pyclass(skip_from_py_object)]
pub struct PyVMState {
    #[pyo3(get)]
    pub pc: usize,
    #[pyo3(get)]
    pub stack: Vec<String>, // JSON string representations
    #[pyo3(get)]
    pub variables: HashMap<String, String>, // JSON string representations
    #[pyo3(get)]
    pub current_node_id: Option<String>,
    #[pyo3(get)]
    pub is_yielding: bool,
    #[pyo3(get)]
    pub yield_reason: Option<String>,
    #[pyo3(get)]
    pub is_running: bool,
    /// Block type of the most recent CALL_BLOCK
    #[pyo3(get)]
    pub last_block_type: Option<String>,
    /// Arguments of the most recent CALL_BLOCK as a JSON object string
    #[pyo3(get)]
    pub last_block_args: Option<String>,
}

#[pymethods]
impl PyVMState {
    #[new]
    fn new() -> Self {
        PyVMState {
            pc: 0,
            stack: Vec::new(),
            variables: HashMap::new(),
            current_node_id: None,
            is_yielding: false,
            yield_reason: None,
            is_running: false,
            last_block_type: None,
            last_block_args: None,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "PyVMState(pc={}, yielding={}, node={:?}, running={})",
            self.pc,
            self.is_yielding,
            self.current_node_id,
            self.is_running
        )
    }

    /// Get a variable value by name
    fn get_variable(&self, name: &str) -> Option<String> {
        self.variables.get(name).cloned()
    }

    /// Get stack depth
    fn stack_depth(&self) -> usize {
        self.stack.len()
    }

    /// Check if VM is at end of program
    fn is_ended(&self) -> bool {
        !self.is_running && !self.is_yielding
    }
}

impl Default for PyVMState {
    fn default() -> Self {
        Self::new()
    }
}

impl PyVMState {
    /// Create from internal VM state
    pub fn from_state(
        pc: usize,
        stack: &[JsonValue],
        variables: &HashMap<String, JsonValue>,
        current_node_id: Option<&str>,
        yield_reason: Option<&str>,
        is_running: bool,
        last_block: Option<&(String, JsonValue)>,
    ) -> Self {
        PyVMState {
            pc,
            stack: stack.iter().map(|v| v.to_string()).collect(),
            variables: variables.iter().map(|(k, v)| (k.clone(), v.to_string())).collect(),
            current_node_id: current_node_id.map(String::from),
            is_yielding: yield_reason.is_some(),
            yield_reason: yield_reason.map(String::from),
            is_running,
            last_block_type: last_block.map(|(t, _)| t.clone()),
            last_block_args: last_block.map(|(_, a)| a.to_string()),
        }
    }
}

//! VM State - Execution state for debugging and serialization

use pyo3::prelude::*;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Python-exposed VM state for debugging
#[derive(Debug, Clone)]
#[pyclass]
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
        is_yielding: bool,
        yield_reason: Option<&str>,
        is_running: bool,
    ) -> Self {
        PyVMState {
            pc,
            stack: stack.iter().map(|v| v.to_string()).collect(),
            variables: variables.iter().map(|(k, v)| (k.clone(), v.to_string())).collect(),
            current_node_id: current_node_id.map(String::from),
            is_yielding,
            yield_reason: yield_reason.map(String::from),
            is_running,
        }
    }
}

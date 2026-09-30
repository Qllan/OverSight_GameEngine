//! VM State - Execution state for debugging and serialization

use pyo3::prelude::*;
use std::collections::HashMap;

use crate::vm::machine::ExecState;

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
    /// Scene being executed
    #[pyo3(get)]
    pub current_graph_id: String,
    /// Number of CallScene frames to return through
    #[pyo3(get)]
    pub call_depth: usize,
    /// Frames left in a Wait (0 = not waiting)
    #[pyo3(get)]
    pub waiting_frames: u32,
    /// The current yield was reached before, in any playthrough (for skip)
    #[pyo3(get)]
    pub seen_before: bool,
    /// Debugger: "scene:node" where execution is paused
    #[pyo3(get)]
    pub paused_at: Option<String>,
    /// Scenes to return to, innermost last
    #[pyo3(get)]
    pub call_stack: Vec<String>,
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
            current_graph_id: String::new(),
            call_depth: 0,
            waiting_frames: 0,
            seen_before: false,
            paused_at: None,
            call_stack: Vec::new(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "PyVMState(pc={}, yielding={}, node={:?}, running={})",
            self.pc, self.is_yielding, self.current_node_id, self.is_running
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
    /// Snapshot of the machine's execution state
    pub fn from_exec(state: &ExecState, paused_at: Option<&str>) -> Self {
        PyVMState {
            pc: state.pc,
            stack: state.stack.iter().map(|v| v.to_string()).collect(),
            variables: state
                .variables
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
            current_node_id: state.current_node_id.clone(),
            is_yielding: state.yield_reason.is_some(),
            yield_reason: state.yield_reason.clone(),
            is_running: state.running,
            last_block_type: state.last_block.as_ref().map(|(t, _)| t.clone()),
            last_block_args: state.last_block.as_ref().map(|(_, a)| a.to_string()),
            current_graph_id: state.scene.clone(),
            call_depth: state.call_stack.len(),
            waiting_frames: state.wait_frames,
            seen_before: state.yield_seen,
            paused_at: paused_at.map(String::from),
            call_stack: state.call_stack.iter().map(|f| f.scene.clone()).collect(),
        }
    }
}

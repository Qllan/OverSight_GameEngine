//! VM Executor - Main execution loop for bytecode
//!
//! Executes opcodes deterministically, supports YIELD for pausing,
//! and emits events for block execution.

use pyo3::prelude::*;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

use crate::compiler::codegen::{CompiledProgram, Opcode};
use crate::vm::state::PyVMState;
use crate::core::event_bus::EventBus;

/// VM Execution result
#[derive(Debug, Clone)]
pub enum ExecResult {
    Continue,
    Yielded { reason: String },
    Ended,
    Error { message: String },
}

/// Virtual Machine executor
#[pyclass]
pub struct VMExecutor {
    program: Option<CompiledProgram>,
    pc: usize,
    stack: Vec<JsonValue>,
    variables: HashMap<String, JsonValue>,
    is_running: bool,
    is_yielding: bool,
    yield_reason: Option<String>,
    current_node_id: Option<String>,
    event_bus: Option<EventBus>,
    execution_log: Vec<String>,
}

#[pymethods]
impl VMExecutor {
    #[new]
    fn new() -> Self {
        VMExecutor {
            program: None,
            pc: 0,
            stack: Vec::new(),
            variables: HashMap::new(),
            is_running: false,
            is_yielding: false,
            yield_reason: None,
            current_node_id: None,
            event_bus: None,
            execution_log: Vec::new(),
        }
    }

    /// Load a compiled program
    fn load(&mut self, program: &CompiledProgram) {
        self.program = Some(program.clone());
        self.pc = 0;
        self.stack.clear();
        self.variables.clear();
        self.is_running = false;
        self.is_yielding = false;
        self.yield_reason = None;
        self.current_node_id = None;
        self.execution_log.clear();
        log::info!("Loaded program: {}", program.graph_id);
    }

    /// Set event bus for emitting events
    fn set_event_bus(&mut self, event_bus: &EventBus) {
        self.event_bus = Some(event_bus.clone());
    }

    /// Start execution
    fn start(&mut self) -> PyResult<()> {
        if self.program.is_none() {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "No program loaded"
            ));
        }
        self.is_running = true;
        self.is_yielding = false;
        self.pc = 0;
        self.log_event("VM started");
        
        if let Some(eb) = &self.event_bus {
            if let Some(prog) = &self.program {
                eb.publish("GraphStart", &prog.graph_id);
            }
        }
        
        Ok(())
    }

    /// Execute one step, returns VM state or None if yielded/ended
    fn step(&mut self) -> PyResult<Option<PyVMState>> {
        if !self.is_running {
            return Ok(None);
        }

        if self.is_yielding {
            // Return current state while yielding
            return Ok(Some(self.get_state()));
        }

        let program = self.program.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("No program loaded")
        })?;

        if self.pc >= program.opcodes.len() {
            self.stop();
            return Ok(None);
        }

        // Get node ID for current opcode
        self.current_node_id = program.node_map.get(self.pc).cloned();

        // Execute opcode
        let opcode = &program.opcodes[self.pc];
        let result = self.execute_opcode(opcode)?;

        match result {
            ExecResult::Continue => {
                if !self.is_yielding {
                    self.pc += 1;
                }
                Ok(Some(self.get_state()))
            }
            ExecResult::Yielded { reason } => {
                self.is_yielding = true;
                self.yield_reason = Some(reason.clone());
                self.log_event(&format!("Yielded: {}", reason));
                Ok(Some(self.get_state()))
            }
            ExecResult::Ended => {
                self.stop();
                Ok(None)
            }
            ExecResult::Error { message } => {
                self.log_event(&format!("Error: {}", message));
                Err(pyo3::exceptions::PyRuntimeError::new_err(message))
            }
        }
    }

    /// Resume execution after yield
    fn resume(&mut self) -> PyResult<Option<PyVMState>> {
        if !self.is_yielding {
            return Ok(None);
        }

        self.is_yielding = false;
        self.yield_reason = None;
        self.pc += 1; // Move past YIELD opcode
        
        self.step()
    }

    /// Stop execution
    fn stop(&mut self) {
        self.is_running = false;
        self.is_yielding = false;
        self.log_event("VM stopped");
        
        if let Some(eb) = &self.event_bus {
            if let Some(prog) = &self.program {
                eb.publish("GraphEnd", &prog.graph_id);
            }
        }
    }

    /// Get current VM state
    fn get_state(&self) -> PyVMState {
        PyVMState::from_state(
            self.pc,
            &self.stack,
            &self.variables,
            self.current_node_id.as_deref(),
            self.is_yielding,
            self.yield_reason.as_deref(),
            self.is_running,
        )
    }

    /// Get a variable value
    fn get_variable(&self, name: &str) -> Option<String> {
        self.variables.get(name).map(|v| v.to_string())
    }

    /// Set a variable value (for debugging/testing)
    fn set_variable(&mut self, name: &str, json_value: &str) -> PyResult<()> {
        let value: JsonValue = serde_json::from_str(json_value)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid JSON: {}", e)))?;
        self.variables.insert(name.to_string(), value);
        Ok(())
    }

    /// Get execution log
    fn get_log(&self) -> Vec<String> {
        self.execution_log.clone()
    }

    /// Check if VM is running
    fn is_running(&self) -> bool {
        self.is_running
    }

    /// Check if VM is yielding
    fn is_yielding(&self) -> bool {
        self.is_yielding
    }

    /// Reset VM state
    fn reset(&mut self) {
        self.pc = 0;
        self.stack.clear();
        self.variables.clear();
        self.is_running = false;
        self.is_yielding = false;
        self.yield_reason = None;
        self.current_node_id = None;
        self.execution_log.clear();
    }
}

impl Default for VMExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl VMExecutor {
    fn execute_opcode(&mut self, opcode: &Opcode) -> Result<ExecResult, String> {
        match opcode {
            Opcode::PUSH(value) => {
                self.stack.push(value.clone());
                Ok(ExecResult::Continue)
            }
            
            Opcode::VAR_SET { name } => {
                if let Some(value) = self.stack.pop() {
                    self.variables.insert(name.clone(), value);
                    Ok(ExecResult::Continue)
                } else {
                    Err(format!("VAR_SET: Stack underflow for variable '{}'", name))
                }
            }
            
            Opcode::VAR_GET { name } => {
                if let Some(value) = self.variables.get(name).cloned() {
                    self.stack.push(value);
                    Ok(ExecResult::Continue)
                } else {
                    Err(format!("VAR_GET: Undefined variable '{}'", name))
                }
            }
            
            Opcode::IF_JUMP { target } => {
                if let Some(condition) = self.stack.pop() {
                    if condition.as_bool().unwrap_or(false) {
                        self.pc = *target;
                    }
                    Ok(ExecResult::Continue)
                } else {
                    Err("IF_JUMP: Stack underflow".to_string())
                }
            }
            
            Opcode::JUMP { target } => {
                self.pc = *target;
                Ok(ExecResult::Continue)
            }
            
            Opcode::YIELD { wait_type } => {
                Ok(ExecResult::Yielded {
                    reason: wait_type.clone(),
                })
            }
            
            Opcode::WAIT { frames: _ } => {
                // In Phase 1, WAIT is a no-op (just continue)
                // Phase 2 will implement actual frame waiting
                Ok(ExecResult::Continue)
            }
            
            Opcode::CALL_BLOCK { block_type, args } => {
                // Pop arguments from stack (in reverse order)
                let mut block_args: HashMap<String, JsonValue> = HashMap::new();
                for arg_name in args.iter().rev() {
                    if let Some(value) = self.stack.pop() {
                        block_args.insert(arg_name.clone(), value);
                    }
                }
                
                // Log the block call
                self.log_event(&format!(
                    "CALL_BLOCK: {}({:?})",
                    block_type, block_args
                ));
                
                // Emit event
                if let Some(eb) = &self.event_bus {
                    if let Some(node_id) = &self.current_node_id {
                        eb.publish("NodeEnter", &format!("{}:{}", node_id, block_type));
                    }
                }
                
                // In Phase 1, we just log; actual block execution in Phase 2
                Ok(ExecResult::Continue)
            }
            
            Opcode::END => {
                Ok(ExecResult::Ended)
            }
        }
    }

    fn log_event(&mut self, message: &str) {
        log::info!("[VM] {}", message);
        self.execution_log.push(message.to_string());
    }
}

// Clone implementation for EventBus
impl Clone for EventBus {
    fn clone(&self) -> Self {
        EventBus::new()
    }
}

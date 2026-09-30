//! VM Executor - Python interface to the bytecode interpreter
//!
//! Wraps `Machine` and forwards its events to an optional shared EventBus.
//! Events are published after the executor is released, so subscribers may
//! call back into the VM (e.g. `resume()` from a callback).

use pyo3::prelude::*;
use serde_json::Value as JsonValue;

use crate::compiler::codegen::CompiledProgram;
use crate::core::event_bus::EventBus;
use crate::pyjson::py_to_json;
use crate::vm::machine::Machine;
use crate::vm::state::PyVMState;

/// Default step budget for `run_until_yield`
const DEFAULT_MAX_STEPS: usize = 100_000;

/// Virtual Machine executor
#[pyclass]
pub struct VMExecutor {
    machine: Machine,
    event_bus: Option<Py<EventBus>>,
}

#[pymethods]
impl VMExecutor {
    #[new]
    fn new() -> Self {
        VMExecutor {
            machine: Machine::new(),
            event_bus: None,
        }
    }

    /// Load a compiled program (clears variables and state)
    fn load(&mut self, program: &CompiledProgram) {
        self.machine.load(program.clone());
    }

    /// Set the event bus that receives engine events (shared, not copied)
    fn set_event_bus(&mut self, event_bus: Py<EventBus>) {
        self.event_bus = Some(event_bus);
    }

    /// Start execution from the beginning. Variables are kept.
    fn start(slf: &Bound<'_, Self>) -> PyResult<()> {
        Self::with_machine(slf, |m| m.start())
    }

    /// Execute one step. Returns the VM state, or None once the VM has stopped.
    fn step(slf: &Bound<'_, Self>) -> PyResult<Option<PyVMState>> {
        Self::with_machine(slf, |m| m.step())?;
        Ok(Self::state_if_running(slf))
    }

    /// Continue after a yield. For a "choice" yield pass the selected index.
    /// Does not execute anything by itself; call step()/run_until_yield().
    /// Returns None if the VM was not yielding.
    #[pyo3(signature = (choice = None))]
    fn resume(slf: &Bound<'_, Self>, choice: Option<&Bound<'_, PyAny>>) -> PyResult<Option<PyVMState>> {
        let value = choice.map(py_to_json).transpose()?;
        let resumed = Self::with_machine(slf, |m| m.resume(value))?;
        Ok(if resumed { Some(slf.borrow().get_state()) } else { None })
    }

    /// Step until the VM yields or stops. Returns the state, or None if stopped.
    #[pyo3(signature = (max_steps = DEFAULT_MAX_STEPS))]
    fn run_until_yield(slf: &Bound<'_, Self>, max_steps: usize) -> PyResult<Option<PyVMState>> {
        Self::with_machine(slf, |m| m.run_until_blocked(max_steps))?;
        Ok(Self::state_if_running(slf))
    }

    /// Stop execution
    fn stop(slf: &Bound<'_, Self>) -> PyResult<()> {
        Self::with_machine(slf, |m| {
            m.stop();
            Ok(())
        })
    }

    /// Get current VM state
    fn get_state(&self) -> PyVMState {
        let m = &self.machine;
        PyVMState::from_state(
            m.pc,
            &m.stack,
            &m.variables,
            m.current_node_id.as_deref(),
            m.yield_reason(),
            m.is_running(),
            m.last_block.as_ref(),
        )
    }

    /// Get a variable value as a JSON string
    fn get_variable(&self, name: &str) -> Option<String> {
        self.machine.get_variable(name).map(|v| v.to_string())
    }

    /// Set a variable from a JSON string
    fn set_variable(&mut self, name: &str, json_value: &str) -> PyResult<()> {
        let value: JsonValue = serde_json::from_str(json_value)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid JSON: {}", e)))?;
        self.machine.set_variable(name, value);
        Ok(())
    }

    /// Get execution log
    fn get_log(&self) -> Vec<String> {
        self.machine.log().to_vec()
    }

    /// Check if VM is running
    fn is_running(&self) -> bool {
        self.machine.is_running()
    }

    /// Check if VM is yielding
    fn is_yielding(&self) -> bool {
        self.machine.is_yielding()
    }

    /// Serialize the execution state to a JSON string
    fn save_state(&self) -> PyResult<String> {
        let state = self
            .machine
            .save_state()
            .map_err(pyo3::exceptions::PyRuntimeError::new_err)?;
        serde_json::to_string(&state)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Restore a state from `save_state()`. Raises ValueError if it was
    /// saved from a different graph or graph version.
    fn load_state(&mut self, json_state: &str) -> PyResult<()> {
        let state = serde_json::from_str(json_state).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid save state: {}", e))
        })?;
        self.machine
            .load_state(state)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Reset VM state (keeps the loaded program)
    fn reset(&mut self) {
        self.machine.reset();
    }
}

impl Default for VMExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl VMExecutor {
    /// Run `f` on the machine, then publish the queued events with the
    /// executor borrow released.
    fn with_machine<T>(
        slf: &Bound<'_, Self>,
        f: impl FnOnce(&mut Machine) -> Result<T, String>,
    ) -> PyResult<T> {
        let py = slf.py();
        let (result, events, bus) = {
            let mut this = slf.borrow_mut();
            let result = f(&mut this.machine);
            let events = this.machine.take_events();
            let bus = this.event_bus.as_ref().map(|b| b.clone_ref(py));
            (result, events, bus)
        };
        if let Some(bus) = bus {
            let bus = bus.bind(py);
            for event in &events {
                EventBus::emit(bus, event)?;
            }
        }
        result.map_err(pyo3::exceptions::PyRuntimeError::new_err)
    }

    fn state_if_running(slf: &Bound<'_, Self>) -> Option<PyVMState> {
        let this = slf.borrow();
        this.machine.is_running().then(|| this.get_state())
    }
}

//! Machine - the bytecode interpreter, independent of Python
//!
//! One `step()` executes one opcode (or one frame of a WAIT). Execution
//! pauses at YIELD until `resume()` is called; for a "choice" yield the
//! selected index is passed to `resume()` and pushed onto the stack.
//! Engine events are queued and drained with `take_events()`.

use serde_json::Value as JsonValue;
use std::collections::HashMap;

use crate::compiler::codegen::{CompiledProgram, Opcode};
use crate::core::event_bus::Event;

/// What an opcode asks the machine to do next
enum Flow {
    Next,
    Jump(usize),
    Stay,
    Yield(String),
    End,
}

#[derive(Debug, Default)]
pub struct Machine {
    program: Option<CompiledProgram>,
    pub(crate) pc: usize,
    pub(crate) stack: Vec<JsonValue>,
    pub(crate) variables: HashMap<String, JsonValue>,
    running: bool,
    yield_reason: Option<String>,
    wait_remaining: Option<u32>,
    pub(crate) current_node_id: Option<String>,
    /// Most recent CALL_BLOCK: (block type, arguments object)
    pub(crate) last_block: Option<(String, JsonValue)>,
    log: Vec<String>,
    events: Vec<Event>,
}

impl Machine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a compiled program and clear all state
    pub fn load(&mut self, program: CompiledProgram) {
        log::info!("Loaded program: {}", program.graph_id);
        self.reset();
        self.program = Some(program);
    }

    /// Start (or restart) execution from the beginning.
    /// Variables are kept, so they can be preset with `set_variable`.
    pub fn start(&mut self) -> Result<(), String> {
        let graph_id = match &self.program {
            Some(program) => program.graph_id.clone(),
            None => return Err("No program loaded".to_string()),
        };
        self.pc = 0;
        self.stack.clear();
        self.yield_reason = None;
        self.wait_remaining = None;
        self.current_node_id = None;
        self.last_block = None;
        self.running = true;
        self.log_event("VM started");
        self.events.push(Event::GraphStart { graph_id });
        Ok(())
    }

    /// Execute one opcode. Does nothing while stopped or yielding.
    pub fn step(&mut self) -> Result<(), String> {
        if !self.running || self.yield_reason.is_some() {
            return Ok(());
        }

        let program = self.program.as_ref().ok_or("No program loaded")?;
        let Some(opcode) = program.opcodes.get(self.pc).cloned() else {
            self.stop();
            return Ok(());
        };
        let program_len = program.opcodes.len();

        // Track which node we are in
        let node_id = program.node_map.get(self.pc).cloned();
        if node_id.is_some() && node_id != self.current_node_id {
            let id = node_id.clone().unwrap_or_default();
            let node_type = program.node_types.get(&id).cloned().unwrap_or_default();
            self.events.push(Event::NodeEnter { node_id: id, node_type });
        }
        self.current_node_id = node_id;

        let flow = match self.execute(&opcode) {
            Ok(flow) => flow,
            Err(message) => return Err(self.fail(message)),
        };

        match flow {
            Flow::Next => self.pc += 1,
            Flow::Jump(target) if target < program_len => self.pc = target,
            Flow::Jump(target) => {
                return Err(self.fail(format!("Jump target {} out of range", target)))
            }
            Flow::Stay => {}
            Flow::Yield(reason) => {
                self.log_event(&format!("Yielded: {}", reason));
                self.events.push(Event::YieldRequest { reason: reason.clone() });
                self.yield_reason = Some(reason);
            }
            Flow::End => self.stop(),
        }
        Ok(())
    }

    /// Continue after a YIELD. A "choice" yield requires the selected
    /// value, which is pushed onto the stack; other yields take none.
    /// Returns false if the machine was not yielding.
    pub fn resume(&mut self, value: Option<JsonValue>) -> Result<bool, String> {
        let Some(reason) = self.yield_reason.clone() else {
            return Ok(false);
        };
        match (reason.as_str(), value) {
            ("choice", Some(value)) => self.stack.push(value),
            ("choice", None) => return Err("resume() needs the selected choice".to_string()),
            (_, Some(_)) => return Err(format!("Yield '{}' does not take a value", reason)),
            (_, None) => {}
        }
        self.yield_reason = None;
        self.pc += 1; // Move past YIELD
        Ok(true)
    }

    /// Step until the machine yields or ends. Fails after `max_steps`
    /// steps to guard against runaway programs.
    pub fn run_until_blocked(&mut self, max_steps: usize) -> Result<(), String> {
        for _ in 0..max_steps {
            if !self.running || self.yield_reason.is_some() {
                return Ok(());
            }
            self.step()?;
        }
        if self.running && self.yield_reason.is_none() {
            return Err(self.fail(format!("Step limit of {} reached", max_steps)));
        }
        Ok(())
    }

    /// Stop execution
    pub fn stop(&mut self) {
        if !self.running {
            return;
        }
        self.running = false;
        self.yield_reason = None;
        self.wait_remaining = None;
        self.log_event("VM stopped");
        if let Some(program) = &self.program {
            self.events.push(Event::GraphEnd { graph_id: program.graph_id.clone() });
        }
    }

    /// Clear execution state (keeps the loaded program)
    pub fn reset(&mut self) {
        self.pc = 0;
        self.stack.clear();
        self.variables.clear();
        self.running = false;
        self.yield_reason = None;
        self.wait_remaining = None;
        self.current_node_id = None;
        self.last_block = None;
        self.log.clear();
        self.events.clear();
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn is_yielding(&self) -> bool {
        self.yield_reason.is_some()
    }

    pub fn yield_reason(&self) -> Option<&str> {
        self.yield_reason.as_deref()
    }

    pub fn get_variable(&self, name: &str) -> Option<&JsonValue> {
        self.variables.get(name)
    }

    pub fn set_variable(&mut self, name: &str, value: JsonValue) {
        self.variables.insert(name.to_string(), value);
    }

    pub fn log(&self) -> &[String] {
        &self.log
    }

    /// Drain queued events
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    fn execute(&mut self, opcode: &Opcode) -> Result<Flow, String> {
        match opcode {
            Opcode::PUSH(value) => {
                self.stack.push(value.clone());
                Ok(Flow::Next)
            }

            Opcode::VAR_SET { name } => {
                let value = self
                    .stack
                    .pop()
                    .ok_or_else(|| format!("VAR_SET: Stack underflow for variable '{}'", name))?;
                self.events.push(Event::VariableChanged { name: name.clone(), value: value.clone() });
                self.variables.insert(name.clone(), value);
                Ok(Flow::Next)
            }

            Opcode::VAR_GET { name } => {
                let value = self
                    .variables
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("VAR_GET: Undefined variable '{}'", name))?;
                self.stack.push(value);
                Ok(Flow::Next)
            }

            Opcode::EQ => {
                let b = self.stack.pop().ok_or("EQ: Stack underflow")?;
                let a = self.stack.pop().ok_or("EQ: Stack underflow")?;
                self.stack.push(JsonValue::Bool(json_eq(&a, &b)));
                Ok(Flow::Next)
            }

            Opcode::IF_JUMP { target } => {
                let condition = self.stack.pop().ok_or("IF_JUMP: Stack underflow")?;
                Ok(if truthy(&condition) { Flow::Jump(*target) } else { Flow::Next })
            }

            Opcode::JUMP { target } => Ok(Flow::Jump(*target)),

            Opcode::YIELD { wait_type } => Ok(Flow::Yield(wait_type.clone())),

            Opcode::WAIT { frames } => {
                // Occupies `frames` steps (at least one)
                let remaining = self.wait_remaining.unwrap_or(*frames);
                if remaining <= 1 {
                    self.wait_remaining = None;
                    Ok(Flow::Next)
                } else {
                    self.wait_remaining = Some(remaining - 1);
                    Ok(Flow::Stay)
                }
            }

            Opcode::CALL_BLOCK { block_type, args } => {
                if self.stack.len() < args.len() {
                    return Err(format!(
                        "CALL_BLOCK {}: expected {} arguments, stack has {}",
                        block_type,
                        args.len(),
                        self.stack.len()
                    ));
                }
                // Args are listed in push order, so pop them in reverse
                let mut block_args = serde_json::Map::new();
                for arg_name in args.iter().rev() {
                    block_args.insert(arg_name.clone(), self.stack.pop().unwrap_or_default());
                }
                let block_args = JsonValue::Object(block_args);

                self.log_event(&format!("CALL_BLOCK: {}({})", block_type, block_args));
                self.events.push(Event::BlockCall {
                    node_id: self.current_node_id.clone(),
                    block_type: block_type.clone(),
                    args: block_args.clone(),
                });
                self.last_block = Some((block_type.clone(), block_args));
                Ok(Flow::Next)
            }

            Opcode::END => Ok(Flow::End),
        }
    }

    /// Record a runtime error, stop the machine and return the message
    fn fail(&mut self, message: String) -> String {
        self.log_event(&format!("Error: {}", message));
        self.events.push(Event::Error {
            message: message.clone(),
            node_id: self.current_node_id.clone(),
        });
        self.stop();
        message
    }

    fn log_event(&mut self, message: &str) {
        log::info!("[VM] {}", message);
        self.log.push(message.to_string());
    }
}

/// Equality that treats 1 and 1.0 as equal
fn json_eq(a: &JsonValue, b: &JsonValue) -> bool {
    match (a, b) {
        (JsonValue::Number(x), JsonValue::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

fn truthy(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => false,
        JsonValue::Bool(b) => *b,
        JsonValue::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        JsonValue::String(s) => !s.is_empty(),
        JsonValue::Array(a) => !a.is_empty(),
        JsonValue::Object(o) => !o.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::codegen::compile_graph;
    use crate::compiler::graph_parser::parse_json_graph;
    use serde_json::json;

    const EXAMPLE: &str = include_str!("../../examples/simple_novel.json");

    fn machine_for(json: &str) -> Machine {
        let program = compile_graph(&parse_json_graph(json).unwrap()).unwrap();
        let mut m = Machine::new();
        m.load(program);
        m.start().unwrap();
        m
    }

    fn block_calls(events: &[Event]) -> Vec<(String, JsonValue)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::BlockCall { block_type, args, .. } => Some((block_type.clone(), args.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn example_first_choice_reaches_adventure() {
        let mut m = machine_for(EXAMPLE);
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.yield_reason(), Some("input"));
        assert_eq!(
            m.last_block,
            Some(("ShowText".into(), json!({"text": "Welcome to the Visual Novel!", "speaker": "Narrator"})))
        );

        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.yield_reason(), Some("choice"));

        m.resume(Some(json!(0))).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.yield_reason(), Some("input"));
        assert_eq!(m.current_node_id.as_deref(), Some("adventure_path"));
        assert_eq!(m.last_block.as_ref().unwrap().1["text"], "Your adventure begins...");

        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert!(!m.is_running());
        assert!(m.stack.is_empty(), "stack leaked: {:?}", m.stack);
    }

    #[test]
    fn example_second_choice_ends_scene() {
        let mut m = machine_for(EXAMPLE);
        m.run_until_blocked(1000).unwrap();
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        m.resume(Some(json!(1))).unwrap();
        m.run_until_blocked(1000).unwrap();

        assert!(!m.is_running());
        assert_eq!(m.current_node_id.as_deref(), Some("exit_path"));
        let texts: Vec<JsonValue> = block_calls(&m.take_events())
            .into_iter()
            .filter(|(t, _)| t == "ShowText")
            .map(|(_, a)| a["text"].clone())
            .collect();
        assert_eq!(texts, vec![json!("Welcome to the Visual Novel!")]);
    }

    #[test]
    fn choice_resume_requires_value() {
        let mut m = machine_for(EXAMPLE);
        m.run_until_blocked(1000).unwrap();
        assert!(m.resume(Some(json!(0))).is_err(), "input yield takes no value");
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert!(m.resume(None).is_err());
    }

    #[test]
    fn execution_is_deterministic() {
        let run = || {
            let mut m = machine_for(EXAMPLE);
            let mut trace = Vec::new();
            for choice in [None, Some(json!(0)), None] {
                m.run_until_blocked(1000).unwrap();
                trace.push(format!("{:?} {:?}", m.pc, m.current_node_id));
                m.resume(choice).unwrap();
            }
            m.run_until_blocked(1000).unwrap();
            (trace, m.log().to_vec())
        };
        let first = run();
        for _ in 0..5 {
            assert_eq!(run(), first);
        }
    }

    #[test]
    fn set_variable_and_merge() {
        let mut m = machine_for(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},
                         {"id":"b","type":"BranchChoice","parameters":{"choices":["x","y"]}},
                         {"id":"x","type":"SetVariable","parameters":{"name":"route","value":"x"}},
                         {"id":"y","type":"SetVariable","parameters":{"name":"route","value":"y"}},
                         {"id":"e","type":"End"}],
                "edges":[{"from":"s","to":"b"},{"from":"b","to":"x","port":0},{"from":"b","to":"y","port":1},
                         {"from":"x","to":"e"},{"from":"y","to":"e"}]}"#,
        );
        m.run_until_blocked(1000).unwrap();
        m.resume(Some(json!(1))).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert!(!m.is_running());
        assert_eq!(m.get_variable("route"), Some(&json!("y")));
    }

    #[test]
    fn out_of_range_choice_ends_without_error() {
        let mut m = machine_for(EXAMPLE);
        m.run_until_blocked(1000).unwrap();
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        m.resume(Some(json!(7))).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert!(!m.is_running());
    }

    #[test]
    fn wait_occupies_frames() {
        let mut m = Machine::new();
        m.load(CompiledProgram {
            graph_id: "w".into(),
            opcodes: vec![Opcode::WAIT { frames: 3 }, Opcode::END],
            node_map: vec!["w".into(), "w".into()],
            node_types: HashMap::new(),
        });
        m.start().unwrap();
        for _ in 0..3 {
            assert_eq!(m.pc, 0);
            m.step().unwrap();
        }
        assert_eq!(m.pc, 1);
    }

    #[test]
    fn runtime_error_stops_machine() {
        let mut m = Machine::new();
        m.load(CompiledProgram {
            graph_id: "e".into(),
            opcodes: vec![Opcode::VAR_GET { name: "missing".into() }, Opcode::END],
            node_map: vec!["n".into(), "n".into()],
            node_types: HashMap::new(),
        });
        m.start().unwrap();
        assert!(m.step().is_err());
        assert!(!m.is_running());
        assert!(m.take_events().iter().any(|e| matches!(e, Event::Error { .. })));
    }
}

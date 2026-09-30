//! Machine - the bytecode interpreter, independent of Python
//!
//! One `step()` executes one opcode (or one frame of a WAIT). Execution
//! pauses at YIELD until `resume()` is called; for a "choice" yield the
//! selected index is passed to `resume()` and pushed onto the stack.
//! Engine events are queued and drained with `take_events()`.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap};

use crate::compiler::codegen::{CompiledProgram, Opcode};
use crate::compiler::expr::BinaryOp;
use crate::vm::values;
use crate::core::event_bus::Event;

/// What an opcode asks the machine to do next
enum Flow {
    Next,
    Jump(usize),
    Stay,
    Yield(String),
    End,
}

/// Version of the save-state format
pub const SAVE_FORMAT: u32 = 1;

/// Serializable machine state (see `Machine::save_state`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveState {
    pub format: u32,
    pub graph_id: String,
    /// `CompiledProgram::fingerprint()` of the program that was running
    pub fingerprint: String,
    pub pc: usize,
    pub stack: Vec<JsonValue>,
    pub variables: BTreeMap<String, JsonValue>,
    pub running: bool,
    pub yield_reason: Option<String>,
    pub wait_remaining: Option<u32>,
    pub current_node_id: Option<String>,
    pub last_block: Option<(String, JsonValue)>,
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

    /// Capture the execution state. Saves are normally taken while the
    /// machine is yielding (waiting for the player).
    pub fn save_state(&self) -> Result<SaveState, String> {
        let program = self.program.as_ref().ok_or("No program loaded")?;
        Ok(SaveState {
            format: SAVE_FORMAT,
            graph_id: program.graph_id.clone(),
            fingerprint: program.fingerprint(),
            pc: self.pc,
            stack: self.stack.clone(),
            variables: self.variables.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            running: self.running,
            yield_reason: self.yield_reason.clone(),
            wait_remaining: self.wait_remaining,
            current_node_id: self.current_node_id.clone(),
            last_block: self.last_block.clone(),
        })
    }

    /// Restore a state saved from the same compiled program. The state is
    /// checked before anything is changed, so a rejected save leaves the
    /// machine untouched.
    pub fn load_state(&mut self, state: SaveState) -> Result<(), String> {
        let program = self.program.as_ref().ok_or("No program loaded")?;
        if state.format != SAVE_FORMAT {
            return Err(format!(
                "Unsupported save format {} (expected {})",
                state.format, SAVE_FORMAT
            ));
        }
        if state.graph_id != program.graph_id {
            return Err(format!(
                "Save is for graph '{}', but '{}' is loaded",
                state.graph_id, program.graph_id
            ));
        }
        if state.fingerprint != program.fingerprint() {
            return Err(format!(
                "Save was made with a different version of graph '{}'",
                state.graph_id
            ));
        }
        if state.pc > program.opcodes.len() {
            return Err(format!("Saved pc {} is out of range", state.pc));
        }
        if let Some(reason) = &state.yield_reason {
            let at_yield = matches!(
                program.opcodes.get(state.pc),
                Some(Opcode::YIELD { wait_type }) if wait_type == reason
            );
            if !at_yield || !state.running {
                return Err("Saved yield does not match the program".to_string());
            }
        }

        self.pc = state.pc;
        self.stack = state.stack;
        self.variables = state.variables.into_iter().collect();
        self.running = state.running;
        self.yield_reason = state.yield_reason;
        self.wait_remaining = state.wait_remaining;
        self.current_node_id = state.current_node_id;
        self.last_block = state.last_block;
        self.log_event("State loaded");
        Ok(())
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

            Opcode::POP => {
                self.pop(opcode)?;
                Ok(Flow::Next)
            }

            Opcode::DUP => {
                let top = self.stack.last().cloned().ok_or("DUP: Stack underflow")?;
                self.stack.push(top);
                Ok(Flow::Next)
            }

            Opcode::ADD
            | Opcode::SUB
            | Opcode::MUL
            | Opcode::DIV
            | Opcode::MOD
            | Opcode::EQ
            | Opcode::NE
            | Opcode::LT
            | Opcode::LE
            | Opcode::GT
            | Opcode::GE => {
                let b = self.pop(opcode)?;
                let a = self.pop(opcode)?;
                let op = match opcode {
                    Opcode::ADD => BinaryOp::Add,
                    Opcode::SUB => BinaryOp::Sub,
                    Opcode::MUL => BinaryOp::Mul,
                    Opcode::DIV => BinaryOp::Div,
                    Opcode::MOD => BinaryOp::Mod,
                    Opcode::EQ => BinaryOp::Eq,
                    Opcode::NE => BinaryOp::Ne,
                    Opcode::LT => BinaryOp::Lt,
                    Opcode::LE => BinaryOp::Le,
                    Opcode::GT => BinaryOp::Gt,
                    _ => BinaryOp::Ge,
                };
                self.stack.push(values::binary(op, &a, &b)?);
                Ok(Flow::Next)
            }

            Opcode::NEG => {
                let value = self.pop(opcode)?;
                self.stack.push(values::negate(&value)?);
                Ok(Flow::Next)
            }

            Opcode::NOT => {
                let value = self.pop(opcode)?;
                self.stack.push(JsonValue::Bool(!values::truthy(&value)));
                Ok(Flow::Next)
            }

            Opcode::CONCAT { count } => {
                let parts = self.pop_many(opcode, *count)?;
                let text: String = parts.iter().map(values::display).collect();
                self.stack.push(JsonValue::String(text));
                Ok(Flow::Next)
            }

            Opcode::MAKE_ARRAY { count } => {
                let items = self.pop_many(opcode, *count)?;
                self.stack.push(JsonValue::Array(items));
                Ok(Flow::Next)
            }

            Opcode::IF_JUMP { target } => {
                let condition = self.pop(opcode)?;
                Ok(if values::truthy(&condition) { Flow::Jump(*target) } else { Flow::Next })
            }

            Opcode::JUMP_IF_FALSE { target } => {
                let condition = self.pop(opcode)?;
                Ok(if values::truthy(&condition) { Flow::Next } else { Flow::Jump(*target) })
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

    fn pop(&mut self, opcode: &Opcode) -> Result<JsonValue, String> {
        self.stack
            .pop()
            .ok_or_else(|| format!("{}: Stack underflow", opcode))
    }

    /// Pop `count` values, returned in push order
    fn pop_many(&mut self, opcode: &Opcode, count: usize) -> Result<Vec<JsonValue>, String> {
        if self.stack.len() < count {
            return Err(format!("{}: Stack underflow", opcode));
        }
        Ok(self.stack.split_off(self.stack.len() - count))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::codegen::compile_graph;
    use crate::compiler::graph_parser::parse_json_graph;
    use serde_json::json;

    const EXAMPLE: &str = include_str!("../../examples/simple_novel.json");

    fn machine_for(json: &str) -> Machine {
        let program = compile_graph(&parse_json_graph(json).unwrap(), &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap()).unwrap();
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
    fn menu_loop_returns_to_the_menu() {
        let mut m = machine_for(include_str!("../../examples/menu_loop.json"));
        let mut texts = Vec::new();
        let mut choices = vec![json!(0), json!(0), json!(1)].into_iter();
        m.run_until_blocked(1000).unwrap();
        while m.is_running() {
            if m.yield_reason() == Some("choice") {
                m.resume(choices.next()).unwrap();
            } else {
                texts.push(m.last_block.as_ref().unwrap().1["text"].clone());
                m.resume(None).unwrap();
            }
            m.run_until_blocked(1000).unwrap();
        }
        assert_eq!(
            texts,
            vec![
                json!("You stand in a small, dusty room."),
                json!("Nothing but dust."),
                json!("Nothing but dust.")
            ]
        );
        assert_eq!(m.get_variable("left_room"), Some(&json!(true)));
        assert!(m.stack.is_empty());
    }

    #[test]
    fn save_and_load_resume_at_the_same_point() {
        let mut m = machine_for(EXAMPLE);
        m.set_variable("score", json!(3));
        m.run_until_blocked(1000).unwrap();
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.yield_reason(), Some("choice"));

        // Round-trip through JSON, as a save file would
        let saved = serde_json::to_string(&m.save_state().unwrap()).unwrap();

        let mut restored = machine_for(EXAMPLE);
        restored.load_state(serde_json::from_str(&saved).unwrap()).unwrap();
        assert_eq!(restored.save_state().unwrap(), m.save_state().unwrap());

        for vm in [&mut m, &mut restored] {
            vm.resume(Some(json!(0))).unwrap();
            vm.run_until_blocked(1000).unwrap();
        }
        assert_eq!(restored.current_node_id.as_deref(), Some("adventure_path"));
        assert_eq!(restored.get_variable("score"), Some(&json!(3)));
        assert_eq!(restored.save_state().unwrap(), m.save_state().unwrap());
    }

    #[test]
    fn save_from_another_program_is_rejected() {
        let mut m = machine_for(EXAMPLE);
        m.run_until_blocked(1000).unwrap();
        let mut saved = m.save_state().unwrap();

        let mut other = machine_for(include_str!("../../examples/menu_loop.json"));
        let before = other.save_state().unwrap();
        assert!(other.load_state(saved.clone()).unwrap_err().contains("is for graph"));

        // Same graph id, different bytecode
        saved.fingerprint = "0".repeat(16);
        let mut same_id = machine_for(EXAMPLE);
        assert!(same_id.load_state(saved).unwrap_err().contains("different version"));
        assert_eq!(other.save_state().unwrap(), before, "rejected load changed state");
    }

    #[test]
    fn fingerprint_is_stable() {
        let program = compile_graph(&parse_json_graph(EXAMPLE).unwrap(), &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap()).unwrap();
        assert_eq!(program.fingerprint(), compile_graph(&parse_json_graph(EXAMPLE).unwrap(), &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap()).unwrap().fingerprint());
        assert_eq!(program.fingerprint().len(), 16);
    }

    /// Play a graph answering choices in order; returns shown (text, choices)
    fn play(json: &str, vars: &[(&str, JsonValue)], choices: &[i64]) -> (Machine, Vec<JsonValue>) {
        let mut m = machine_for(json);
        for (name, value) in vars {
            m.set_variable(name, value.clone());
        }
        let mut shown = Vec::new();
        let mut choices = choices.iter();
        m.run_until_blocked(1000).unwrap();
        while m.is_running() {
            let args = m.last_block.as_ref().unwrap().1.clone();
            if m.yield_reason() == Some("choice") {
                shown.push(args["choices"].clone());
                m.resume(Some(json!(choices.next().unwrap()))).unwrap();
            } else {
                shown.push(args["text"].clone());
                m.resume(None).unwrap();
            }
            m.run_until_blocked(1000).unwrap();
        }
        (m, shown)
    }

    const AFFECTION: &str = include_str!("../../examples/affection.json");

    #[test]
    fn expressions_drive_the_story() {
        let vars = [("player", json!("Alex")), ("speaker_name", json!("Mia"))];
        let (m, shown) = play(AFFECTION, &vars, &[0]);
        assert_eq!(
            shown,
            vec![
                json!("Hi, Alex! It's been a while."),
                json!(["Missed you, Mia", "Whatever."]),
                json!("She smiles. (affection: 3)"),
            ]
        );
        assert!(m.stack.is_empty(), "stack leaked: {:?}", m.stack);

        let (m, shown) = play(AFFECTION, &vars, &[1]);
        assert_eq!(shown.last().unwrap(), &json!("She frowns."));
        assert_eq!(m.get_variable("affection"), Some(&json!(-1)));
    }

    #[test]
    fn and_short_circuits() {
        // `missing` is never read because the left side is already false
        let graph = r#"{"graph_id":"g","version":"1.0",
            "nodes":[{"id":"s","type":"entry"},
                     {"id":"v","type":"SetVariable","parameters":{"name":"r","expression":"flag and missing"}},
                     {"id":"w","type":"SetVariable","parameters":{"name":"o","expression":"flag or 'fallback'"}}],
            "edges":[{"from":"s","to":"v"},{"from":"v","to":"w"}]}"#;
        let (m, _) = play(graph, &[("flag", json!(false))], &[]);
        assert_eq!(m.get_variable("r"), Some(&json!(false)));
        assert_eq!(m.get_variable("o"), Some(&json!("fallback")));
        assert!(m.stack.is_empty());
    }

    #[test]
    fn expression_errors_stop_at_the_node() {
        let mut m = machine_for(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},
                         {"id":"bad","type":"SetVariable","parameters":{"name":"x","expression":"10 / zero"}}],
                "edges":[{"from":"s","to":"bad"}]}"#,
        );
        m.set_variable("zero", json!(0));
        let err = m.run_until_blocked(100).unwrap_err();
        assert_eq!(err, "division by zero");
        assert!(m.take_events().iter().any(|e| matches!(e,
            Event::Error { node_id: Some(n), .. } if n == "bad")));
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

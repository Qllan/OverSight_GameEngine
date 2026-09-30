//! Machine - the bytecode interpreter, independent of Python
//!
//! Runs a CompiledProject (one or more scenes). One `step()` executes one
//! opcode. Execution pauses at YIELD until `resume()` is called; for a
//! "choice" yield the selected index is passed to `resume()` and pushed
//! onto the stack. WAIT pauses for a number of frames, counted by
//! `frame()`. Engine events are queued and drained with `take_events()`.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

use crate::compiler::codegen::{CompiledProgram, Opcode};
use crate::compiler::expr::BinaryOp;
use crate::compiler::project::CompiledProject;
use crate::core::event_bus::Event;
use crate::vm::values;

/// Deepest CallScene nesting before the VM reports runaway recursion
pub const MAX_CALL_DEPTH: usize = 256;

/// What an opcode asks the machine to do next
enum Flow {
    Next,
    Jump(usize),
    Yield(String),
    Wait(u32),
    End,
    EnterScene { scene: String, call: bool },
}

/// Where execution continues when a called scene ends
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub scene: String,
    pub return_pc: usize,
}

/// Where the game is: everything a save or a rollback restores
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExecState {
    pub scene: String,
    pub pc: usize,
    pub stack: Vec<JsonValue>,
    pub variables: BTreeMap<String, JsonValue>,
    pub call_stack: Vec<Frame>,
    pub running: bool,
    pub yield_reason: Option<String>,
    /// Frames left in a WAIT (0 = not waiting)
    pub wait_frames: u32,
    pub current_node_id: Option<String>,
    /// Most recent CALL_BLOCK: (block type, arguments object)
    pub last_block: Option<(String, JsonValue)>,
}

/// Version of the save-state format
pub const SAVE_FORMAT: u32 = 2;

/// Serializable machine state (see `Machine::save_state`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveState {
    pub format: u32,
    pub project_id: String,
    /// `CompiledProject::fingerprint()` of the project that was running
    pub fingerprint: String,
    pub state: ExecState,
}

#[derive(Debug, Default)]
pub struct Machine {
    project: Option<CompiledProject>,
    pub(crate) exec: ExecState,
    log: Vec<String>,
    events: Vec<Event>,
}

impl Machine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a single compiled graph and clear all state
    pub fn load(&mut self, program: CompiledProgram) {
        self.load_project(CompiledProject::single(program));
    }

    /// Load a compiled project and clear all state
    pub fn load_project(&mut self, project: CompiledProject) {
        log::info!("Loaded project: {}", project.project_id);
        self.reset();
        self.project = Some(project);
    }

    fn program(&self) -> Option<&CompiledProgram> {
        self.project.as_ref()?.scenes.get(&self.exec.scene)
    }

    /// Start (or restart) from the project's start scene. Variables that
    /// are already set are kept (so they can be preset); missing ones get
    /// the project's initial values.
    pub fn start(&mut self) -> Result<(), String> {
        let project = self.project.as_ref().ok_or("No program loaded")?;
        let start = project.start.clone();
        for (name, value) in &project.variables {
            self.exec
                .variables
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
        self.exec.pc = 0;
        self.exec.stack.clear();
        self.exec.call_stack.clear();
        self.exec.yield_reason = None;
        self.exec.wait_frames = 0;
        self.exec.current_node_id = None;
        self.exec.last_block = None;
        self.exec.running = true;
        self.log_event("VM started");
        self.enter_scene(start);
        Ok(())
    }

    /// Nothing to execute until the player acts, a WAIT runs out, or a restart
    fn is_blocked(&self) -> bool {
        !self.exec.running || self.exec.yield_reason.is_some() || self.exec.wait_frames > 0
    }

    /// Execute one opcode. Does nothing while stopped, yielding or waiting.
    pub fn step(&mut self) -> Result<(), String> {
        if self.is_blocked() {
            return Ok(());
        }

        let Some(program) = self.program() else {
            let message = format!("Scene '{}' is not loaded", self.exec.scene);
            return Err(self.fail(message));
        };
        let Some(opcode) = program.opcodes.get(self.exec.pc).cloned() else {
            self.stop();
            return Ok(());
        };
        let program_len = program.opcodes.len();

        // Track which node we are in
        let node_id = program.node_map.get(self.exec.pc).cloned();
        if node_id.is_some() && node_id != self.exec.current_node_id {
            let id = node_id.clone().unwrap_or_default();
            let node_type = program.node_types.get(&id).cloned().unwrap_or_default();
            self.events.push(Event::NodeEnter {
                graph_id: self.exec.scene.clone(),
                node_id: id,
                node_type,
            });
        }
        self.exec.current_node_id = node_id;

        let flow = match self.execute(&opcode) {
            Ok(flow) => flow,
            Err(message) => return Err(self.fail(message)),
        };

        match flow {
            Flow::Next => self.exec.pc += 1,
            Flow::Jump(target) if target < program_len => self.exec.pc = target,
            Flow::Jump(target) => {
                return Err(self.fail(format!("Jump target {} out of range", target)))
            }
            Flow::Yield(reason) => {
                self.log_event(&format!("Yielded: {}", reason));
                self.events.push(Event::YieldRequest { reason: reason.clone() });
                self.exec.yield_reason = Some(reason);
            }
            Flow::Wait(frames) => {
                self.exec.pc += 1;
                self.exec.wait_frames = frames;
            }
            Flow::End => self.end_scene(),
            Flow::EnterScene { scene, call } => {
                let known = self
                    .project
                    .as_ref()
                    .is_some_and(|p| p.scenes.contains_key(&scene));
                if !known {
                    return Err(self.fail(format!("Unknown scene '{}'", scene)));
                }
                if call {
                    if self.exec.call_stack.len() >= MAX_CALL_DEPTH {
                        return Err(self.fail(format!(
                            "CallScene nested more than {} deep",
                            MAX_CALL_DEPTH
                        )));
                    }
                    self.exec.call_stack.push(Frame {
                        scene: self.exec.scene.clone(),
                        return_pc: self.exec.pc + 1,
                    });
                } else {
                    self.events.push(Event::GraphEnd { graph_id: self.exec.scene.clone() });
                }
                self.enter_scene(scene);
            }
        }
        Ok(())
    }

    fn enter_scene(&mut self, scene: String) {
        self.exec.scene = scene.clone();
        self.exec.pc = 0;
        self.exec.current_node_id = None;
        self.events.push(Event::GraphStart { graph_id: scene });
    }

    /// END: return to the calling scene, or stop if there is none
    fn end_scene(&mut self) {
        let Some(frame) = self.exec.call_stack.pop() else {
            self.stop();
            return;
        };
        self.events.push(Event::GraphEnd { graph_id: self.exec.scene.clone() });
        self.exec.scene = frame.scene;
        self.exec.pc = frame.return_pc;
        // Back inside the CallScene node; don't report entering it again
        self.exec.current_node_id = self
            .program()
            .and_then(|p| p.node_map.get(frame.return_pc.saturating_sub(1)).cloned());
    }

    /// Continue after a YIELD. A "choice" yield requires the selected
    /// value, which is pushed onto the stack; other yields take none.
    /// Returns false if the machine was not yielding.
    pub fn resume(&mut self, value: Option<JsonValue>) -> Result<bool, String> {
        let Some(reason) = self.exec.yield_reason.clone() else {
            return Ok(false);
        };
        match (reason.as_str(), value) {
            ("choice", Some(value)) => self.exec.stack.push(value),
            ("choice", None) => return Err("resume() needs the selected choice".to_string()),
            (_, Some(_)) => return Err(format!("Yield '{}' does not take a value", reason)),
            (_, None) => {}
        }
        self.exec.yield_reason = None;
        self.exec.pc += 1; // Move past YIELD
        Ok(true)
    }

    /// Step until the machine yields, waits or ends. Fails after
    /// `max_steps` steps to guard against runaway programs.
    pub fn run_until_blocked(&mut self, max_steps: usize) -> Result<(), String> {
        for _ in 0..max_steps {
            if self.is_blocked() {
                return Ok(());
            }
            self.step()?;
        }
        if !self.is_blocked() {
            return Err(self.fail(format!("Step limit of {} reached", max_steps)));
        }
        Ok(())
    }

    /// Advance one frame: count down a pending WAIT, then run until the
    /// machine blocks again. Call once per frame of the game loop.
    pub fn frame(&mut self, max_steps: usize) -> Result<(), String> {
        if self.exec.running && self.exec.wait_frames > 0 {
            self.exec.wait_frames -= 1;
        }
        self.run_until_blocked(max_steps)
    }

    /// Stop execution
    pub fn stop(&mut self) {
        if !self.exec.running {
            return;
        }
        self.exec.running = false;
        self.exec.yield_reason = None;
        self.exec.wait_frames = 0;
        self.exec.call_stack.clear();
        self.log_event("VM stopped");
        self.events.push(Event::GraphEnd { graph_id: self.exec.scene.clone() });
    }

    /// Clear execution state (keeps the loaded project)
    pub fn reset(&mut self) {
        self.exec = ExecState::default();
        self.log.clear();
        self.events.clear();
    }

    /// Capture the execution state. Saves are normally taken while the
    /// machine is yielding (waiting for the player).
    pub fn save_state(&self) -> Result<SaveState, String> {
        let project = self.project.as_ref().ok_or("No program loaded")?;
        Ok(SaveState {
            format: SAVE_FORMAT,
            project_id: project.project_id.clone(),
            fingerprint: project.fingerprint(),
            state: self.exec.clone(),
        })
    }

    /// Restore a state saved from the same compiled project. The state is
    /// checked before anything is changed, so a rejected save leaves the
    /// machine untouched.
    pub fn load_state(&mut self, save: SaveState) -> Result<(), String> {
        let project = self.project.as_ref().ok_or("No program loaded")?;
        if save.format != SAVE_FORMAT {
            return Err(format!(
                "Unsupported save format {} (expected {})",
                save.format, SAVE_FORMAT
            ));
        }
        if save.project_id != project.project_id {
            return Err(format!(
                "Save is for project '{}', but '{}' is loaded",
                save.project_id, project.project_id
            ));
        }
        if save.fingerprint != project.fingerprint() {
            return Err(format!(
                "Save was made with a different version of project '{}'",
                save.project_id
            ));
        }

        let state = &save.state;
        let scene_len = |scene: &str| project.scenes.get(scene).map(|p| p.opcodes.len());
        let positions = std::iter::once((state.scene.as_str(), state.pc))
            .chain(state.call_stack.iter().map(|f| (f.scene.as_str(), f.return_pc)));
        for (scene, pc) in positions {
            match scene_len(scene) {
                Some(len) if pc <= len => {}
                Some(_) => return Err(format!("Saved position {} in '{}' is out of range", pc, scene)),
                None => return Err(format!("Saved scene '{}' is not in the project", scene)),
            }
        }
        if let Some(reason) = &state.yield_reason {
            let at_yield = matches!(
                project.scenes[&state.scene].opcodes.get(state.pc),
                Some(Opcode::YIELD { wait_type }) if wait_type == reason
            );
            if !at_yield || !state.running {
                return Err("Saved yield does not match the program".to_string());
            }
        }

        self.exec = save.state;
        self.log_event("State loaded");
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.exec.running
    }

    pub fn is_yielding(&self) -> bool {
        self.exec.yield_reason.is_some()
    }

    pub fn yield_reason(&self) -> Option<&str> {
        self.exec.yield_reason.as_deref()
    }

    pub fn state(&self) -> &ExecState {
        &self.exec
    }

    pub fn get_variable(&self, name: &str) -> Option<&JsonValue> {
        self.exec.variables.get(name)
    }

    pub fn set_variable(&mut self, name: &str, value: JsonValue) {
        self.exec.variables.insert(name.to_string(), value);
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
                self.exec.stack.push(value.clone());
                Ok(Flow::Next)
            }

            Opcode::VAR_SET { name } => {
                let value = self
                    .exec
                    .stack
                    .pop()
                    .ok_or_else(|| format!("VAR_SET: Stack underflow for variable '{}'", name))?;
                self.events.push(Event::VariableChanged { name: name.clone(), value: value.clone() });
                self.exec.variables.insert(name.clone(), value);
                Ok(Flow::Next)
            }

            Opcode::VAR_GET { name } => {
                let value = self
                    .exec
                    .variables
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("VAR_GET: Undefined variable '{}'", name))?;
                self.exec.stack.push(value);
                Ok(Flow::Next)
            }

            Opcode::POP => {
                self.pop(opcode)?;
                Ok(Flow::Next)
            }

            Opcode::DUP => {
                let top = self.exec.stack.last().cloned().ok_or("DUP: Stack underflow")?;
                self.exec.stack.push(top);
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
                self.exec.stack.push(values::binary(op, &a, &b)?);
                Ok(Flow::Next)
            }

            Opcode::NEG => {
                let value = self.pop(opcode)?;
                self.exec.stack.push(values::negate(&value)?);
                Ok(Flow::Next)
            }

            Opcode::NOT => {
                let value = self.pop(opcode)?;
                self.exec.stack.push(JsonValue::Bool(!values::truthy(&value)));
                Ok(Flow::Next)
            }

            Opcode::CONCAT { count } => {
                let parts = self.pop_many(opcode, *count)?;
                let text: String = parts.iter().map(values::display).collect();
                self.exec.stack.push(JsonValue::String(text));
                Ok(Flow::Next)
            }

            Opcode::MAKE_ARRAY { count } => {
                let items = self.pop_many(opcode, *count)?;
                self.exec.stack.push(JsonValue::Array(items));
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

            Opcode::WAIT { frames } => Ok(Flow::Wait(*frames)),

            Opcode::JUMP_SCENE { scene } => {
                Ok(Flow::EnterScene { scene: scene.clone(), call: false })
            }

            Opcode::CALL_SCENE { scene } => {
                Ok(Flow::EnterScene { scene: scene.clone(), call: true })
            }

            Opcode::CALL_BLOCK { block_type, args } => {
                if self.exec.stack.len() < args.len() {
                    return Err(format!(
                        "CALL_BLOCK {}: expected {} arguments, stack has {}",
                        block_type,
                        args.len(),
                        self.exec.stack.len()
                    ));
                }
                // Args are listed in push order, so pop them in reverse
                let mut block_args = serde_json::Map::new();
                for arg_name in args.iter().rev() {
                    block_args.insert(arg_name.clone(), self.exec.stack.pop().unwrap_or_default());
                }
                let block_args = JsonValue::Object(block_args);

                self.log_event(&format!("CALL_BLOCK: {}({})", block_type, block_args));
                self.events.push(Event::BlockCall {
                    graph_id: self.exec.scene.clone(),
                    node_id: self.exec.current_node_id.clone(),
                    block_type: block_type.clone(),
                    args: block_args.clone(),
                });
                self.exec.last_block = Some((block_type.clone(), block_args));
                Ok(Flow::Next)
            }

            Opcode::END => Ok(Flow::End),
        }
    }

    fn pop(&mut self, opcode: &Opcode) -> Result<JsonValue, String> {
        self.exec
            .stack
            .pop()
            .ok_or_else(|| format!("{}: Stack underflow", opcode))
    }

    /// Pop `count` values, returned in push order
    fn pop_many(&mut self, opcode: &Opcode, count: usize) -> Result<Vec<JsonValue>, String> {
        let stack = &mut self.exec.stack;
        if stack.len() < count {
            return Err(format!("{}: Stack underflow", opcode));
        }
        Ok(stack.split_off(stack.len() - count))
    }

    /// Record a runtime error, stop the machine and return the message
    fn fail(&mut self, message: String) -> String {
        self.log_event(&format!("Error: {}", message));
        self.events.push(Event::Error {
            message: message.clone(),
            graph_id: Some(self.exec.scene.clone()),
            node_id: self.exec.current_node_id.clone(),
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
    use crate::compiler::project::compile_project;
    use crate::core::plugin_registry::PluginRegistry;
    use serde_json::json;
    use std::collections::HashMap;

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
            m.exec.last_block,
            Some(("ShowText".into(), json!({"text": "Welcome to the Visual Novel!", "speaker": "Narrator"})))
        );

        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.yield_reason(), Some("choice"));

        m.resume(Some(json!(0))).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.yield_reason(), Some("input"));
        assert_eq!(m.exec.current_node_id.as_deref(), Some("adventure_path"));
        assert_eq!(m.exec.last_block.as_ref().unwrap().1["text"], "Your adventure begins...");

        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert!(!m.is_running());
        assert!(m.exec.stack.is_empty(), "stack leaked: {:?}", m.exec.stack);
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
        assert_eq!(m.exec.current_node_id.as_deref(), Some("exit_path"));
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
                trace.push(format!("{:?} {:?}", m.exec.pc, m.exec.current_node_id));
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
                texts.push(m.exec.last_block.as_ref().unwrap().1["text"].clone());
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
        assert!(m.exec.stack.is_empty());
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
        assert_eq!(restored.exec.current_node_id.as_deref(), Some("adventure_path"));
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
        assert!(other.load_state(saved.clone()).unwrap_err().contains("is for project"));

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
            let args = m.exec.last_block.as_ref().unwrap().1.clone();
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
        assert!(m.exec.stack.is_empty(), "stack leaked: {:?}", m.exec.stack);

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
        assert!(m.exec.stack.is_empty());
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
    fn wait_counts_frames_not_steps() {
        let mut m = Machine::new();
        m.load(CompiledProgram {
            graph_id: "w".into(),
            opcodes: vec![Opcode::WAIT { frames: 3 }, Opcode::END],
            node_map: vec!["w".into(), "w".into()],
            node_types: HashMap::new(),
        });
        m.start().unwrap();
        // Running as fast as possible does not skip the wait
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.exec.wait_frames, 3);
        m.step().unwrap();
        assert_eq!((m.exec.pc, m.exec.wait_frames), (1, 3), "step() must not advance a wait");
        m.frame(1000).unwrap();
        m.frame(1000).unwrap();
        assert!(m.is_running());
        m.frame(1000).unwrap();
        assert!(!m.is_running(), "third frame after the WAIT continues");
    }

    fn project_machine(start: &str, scenes: &[&str]) -> Machine {
        let json = format!(
            r#"{{"project_id":"p","start":"{}","variables":{{"gold":10}},"scenes":[{}]}}"#,
            start,
            scenes.join(",")
        );
        let project = compile_project(&json, &PluginRegistry::with_defaults().unwrap()).unwrap();
        let mut m = Machine::new();
        m.load_project(project);
        m.start().unwrap();
        m
    }

    const TOWN: &str = r#"{"graph_id":"town","version":"1.0",
        "nodes":[{"id":"s","type":"entry"},
                 {"id":"hello","type":"ShowText","parameters":{"text":"In town with {gold} gold."}},
                 {"id":"shop","type":"CallScene","parameters":{"scene":"shop"}},
                 {"id":"after","type":"ShowText","parameters":{"text":"Back with {gold} gold."}},
                 {"id":"go","type":"JumpScene","parameters":{"scene":"ending"}}],
        "edges":[{"from":"s","to":"hello"},{"from":"hello","to":"shop"},
                 {"from":"shop","to":"after"},{"from":"after","to":"go"}]}"#;
    const SHOP: &str = r#"{"graph_id":"shop","version":"1.0",
        "nodes":[{"id":"s","type":"entry"},
                 {"id":"buy","type":"SetVariable","parameters":{"name":"gold","expression":"gold - 4"}},
                 {"id":"line","type":"ShowText","parameters":{"text":"Thanks for buying!"}},
                 {"id":"done","type":"End"}],
        "edges":[{"from":"s","to":"buy"},{"from":"buy","to":"line"},{"from":"line","to":"done"}]}"#;
    const ENDING: &str = r#"{"graph_id":"ending","version":"1.0",
        "nodes":[{"id":"s","type":"entry"},{"id":"fin","type":"ShowText","parameters":{"text":"Fin."}}],
        "edges":[{"from":"s","to":"fin"}]}"#;

    #[test]
    fn scenes_call_return_and_jump() {
        let mut m = project_machine("town", &[TOWN, SHOP, ENDING]);
        let mut shown = Vec::new();
        m.run_until_blocked(1000).unwrap();
        while m.is_running() {
            let scene = m.exec.scene.clone();
            shown.push((scene, m.exec.last_block.as_ref().unwrap().1["text"].clone()));
            m.resume(None).unwrap();
            m.run_until_blocked(1000).unwrap();
        }
        assert_eq!(
            shown,
            vec![
                ("town".to_string(), json!("In town with 10 gold.")),
                ("shop".to_string(), json!("Thanks for buying!")),
                ("town".to_string(), json!("Back with 6 gold.")),
                ("ending".to_string(), json!("Fin.")),
            ]
        );
        let scenes: Vec<String> = m
            .take_events()
            .into_iter()
            .filter_map(|e| match e {
                Event::GraphStart { graph_id } => Some(format!("+{}", graph_id)),
                Event::GraphEnd { graph_id } => Some(format!("-{}", graph_id)),
                _ => None,
            })
            .collect();
        assert_eq!(scenes, vec!["+town", "+shop", "-shop", "-town", "+ending", "-ending"]);
    }

    #[test]
    fn save_inside_a_called_scene() {
        let mut m = project_machine("town", &[TOWN, SHOP, ENDING]);
        m.run_until_blocked(1000).unwrap();
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!((m.exec.scene.as_str(), m.exec.call_stack.len()), ("shop", 1));

        let saved = m.save_state().unwrap();
        let mut restored = project_machine("town", &[TOWN, SHOP, ENDING]);
        restored.load_state(saved).unwrap();
        restored.resume(None).unwrap();
        restored.run_until_blocked(1000).unwrap();
        assert_eq!(restored.exec.scene, "town");
        assert_eq!(restored.exec.last_block.as_ref().unwrap().1["text"], "Back with 6 gold.");
    }

    #[test]
    fn runaway_recursion_is_stopped() {
        let looping = r#"{"graph_id":"a","version":"1.0",
            "nodes":[{"id":"s","type":"entry"},{"id":"again","type":"CallScene","parameters":{"scene":"a"}}],
            "edges":[{"from":"s","to":"again"}]}"#;
        let mut m = project_machine("a", &[looping]);
        let err = m.run_until_blocked(10_000).unwrap_err();
        assert!(err.contains("nested more than 256"), "{}", err);
        assert!(!m.is_running());
    }

    #[test]
    fn unknown_scene_in_a_single_graph_is_a_runtime_error() {
        let mut m = machine_for(
            r#"{"graph_id":"g","version":"1.0",
                "nodes":[{"id":"s","type":"entry"},{"id":"go","type":"JumpScene","parameters":{"scene":"x"}}],
                "edges":[{"from":"s","to":"go"}]}"#,
        );
        assert_eq!(m.run_until_blocked(100).unwrap_err(), "Unknown scene 'x'");
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

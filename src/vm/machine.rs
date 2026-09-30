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

/// Most recent lines kept in the backlog
pub const BACKLOG_LIMIT: usize = 200;

/// Yields kept for rollback, unless changed with `set_rollback_limit`
pub const DEFAULT_ROLLBACK_LIMIT: usize = 100;

/// One line of the backlog: what was shown at a yield
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacklogEntry {
    pub scene: String,
    pub node_id: Option<String>,
    pub block_type: Option<String>,
    pub args: JsonValue,
    /// The value picked at a choice
    pub choice: Option<JsonValue>,
}

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
    /// Whether the current yield had been reached before (any playthrough)
    #[serde(default)]
    pub yield_seen: bool,
    /// What the player has seen, oldest first (at most BACKLOG_LIMIT)
    #[serde(default)]
    pub backlog: Vec<BacklogEntry>,
    /// State owned by the host (e.g. what is on screen), saved and rolled
    /// back together with the game
    #[serde(default)]
    pub host: JsonValue,
}

/// Prefix of variables kept across playthroughs (not in saves)
pub const PERSISTENT_PREFIX: &str = "persistent.";

/// Data shared by all playthroughs: `persistent.*` variables and the set
/// of places the player has already seen. Saves and rollback leave it alone.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PersistentData {
    /// Keyed by name without the "persistent." prefix
    pub variables: BTreeMap<String, JsonValue>,
    /// "scene:node" of every yield the player has reached
    pub seen: std::collections::BTreeSet<String>,
}

/// Version of the save-state format
pub const SAVE_FORMAT: u32 = 2;

/// A code position described by its node rather than its index, so it
/// can be found again after the graph is edited: "the n-th opcode of
/// this kind inside this node"
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub scene: String,
    pub node_id: String,
    pub kind: String,
    pub occurrence: usize,
}

/// Serializable machine state (see `Machine::save_state`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveState {
    pub format: u32,
    pub project_id: String,
    /// `CompiledProject::fingerprint()` of the project that was running
    pub fingerprint: String,
    pub state: ExecState,
    /// Anchor of `state.pc` (None at the end of a scene)
    pub position: Option<Anchor>,
    /// Anchors of the CALL_SCENE opcodes the call stack returns to
    pub return_positions: Vec<Option<Anchor>>,
}

/// Opcode identity that survives edits: operands that depend on layout
/// or content (jump targets, pushed values) are left out
fn op_kind(op: &Opcode) -> String {
    match op {
        Opcode::YIELD { wait_type } => format!("YIELD({})", wait_type),
        Opcode::CALL_BLOCK { block_type, .. } => format!("CALL_BLOCK({})", block_type),
        Opcode::CALL_SCENE { scene } => format!("CALL_SCENE({})", scene),
        Opcode::JUMP_SCENE { scene } => format!("JUMP_SCENE({})", scene),
        other => {
            let text = other.to_string();
            text.split('(').next().unwrap_or_default().to_string()
        }
    }
}

/// Opcode indices that belong to a node (nodes are emitted contiguously)
fn node_range(program: &CompiledProgram, node_id: &str) -> Option<std::ops::Range<usize>> {
    let start = program.node_map.iter().position(|n| n == node_id)?;
    let len = program.node_map[start..]
        .iter()
        .take_while(|n| *n == node_id)
        .count();
    Some(start..start + len)
}

fn anchor_for(program: &CompiledProgram, scene: &str, pc: usize) -> Option<Anchor> {
    let op = program.opcodes.get(pc)?;
    let node_id = program.node_map.get(pc)?;
    let range = node_range(program, node_id)?;
    let kind = op_kind(op);
    let occurrence = program.opcodes[range.start..pc]
        .iter()
        .filter(|o| op_kind(o) == kind)
        .count();
    Some(Anchor {
        scene: scene.to_string(),
        node_id: node_id.clone(),
        kind,
        occurrence,
    })
}

fn resolve_anchor(program: &CompiledProgram, anchor: &Anchor) -> Option<usize> {
    let range = node_range(program, &anchor.node_id)?;
    range
        .filter(|&i| op_kind(&program.opcodes[i]) == anchor.kind)
        .nth(anchor.occurrence)
}

#[derive(Debug)]
pub struct Machine {
    project: Option<CompiledProject>,
    pub(crate) exec: ExecState,
    persistent: PersistentData,
    /// State at each recent yield, oldest first, for rollback
    history: std::collections::VecDeque<ExecState>,
    rollback_limit: usize,
    /// Debugger: "scene:node" keys that pause execution on entry
    breakpoints: std::collections::BTreeSet<String>,
    /// Debugger: where execution is paused, if it is
    paused_at: Option<String>,
    /// Debugger: pause when the next node is entered
    pause_on_next_node: bool,
    log: Vec<String>,
    events: Vec<Event>,
}

impl Default for Machine {
    fn default() -> Self {
        Machine {
            project: None,
            exec: ExecState::default(),
            persistent: PersistentData::default(),
            history: Default::default(),
            rollback_limit: DEFAULT_ROLLBACK_LIMIT,
            breakpoints: Default::default(),
            paused_at: None,
            pause_on_next_node: false,
            log: Vec::new(),
            events: Vec::new(),
        }
    }
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
        let defaults = project.variables.clone();
        self.apply_defaults(&defaults);
        self.exec.pc = 0;
        self.exec.stack.clear();
        self.exec.call_stack.clear();
        self.exec.yield_reason = None;
        self.exec.wait_frames = 0;
        self.exec.current_node_id = None;
        self.exec.last_block = None;
        self.exec.backlog.clear();
        self.exec.running = true;
        self.history.clear();
        self.paused_at = None;
        self.log_event("VM started");
        self.enter_scene(start);
        Ok(())
    }

    /// Give variables that are not set yet their initial values
    fn apply_defaults(&mut self, defaults: &BTreeMap<String, JsonValue>) {
        for (name, value) in defaults {
            if self.get_variable(name).is_none() {
                self.set_variable(name, value.clone());
            }
        }
    }

    /// Nothing to execute until the player acts, a WAIT runs out, or a restart
    fn is_blocked(&self) -> bool {
        !self.exec.running
            || self.exec.yield_reason.is_some()
            || self.exec.wait_frames > 0
            || self.paused_at.is_some()
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
        let entered = node_id.is_some() && node_id != self.exec.current_node_id;
        if entered {
            let id = node_id.clone().unwrap_or_default();
            let node_type = program.node_types.get(&id).cloned().unwrap_or_default();
            self.events.push(Event::NodeEnter {
                graph_id: self.exec.scene.clone(),
                node_id: id,
                node_type,
            });
        }
        self.exec.current_node_id = node_id;

        // Debugger: stop before the first opcode of the node
        if entered {
            let key = self.location();
            if self.pause_on_next_node || self.breakpoints.contains(&key) {
                self.pause_on_next_node = false;
                self.log_event(&format!("Paused at {}", key));
                self.events.push(Event::Breakpoint {
                    graph_id: self.exec.scene.clone(),
                    node_id: self.exec.current_node_id.clone().unwrap_or_default(),
                });
                self.paused_at = Some(key);
                return Ok(());
            }
        }

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
                let place = format!(
                    "{}:{}",
                    self.exec.scene,
                    self.exec.current_node_id.as_deref().unwrap_or_default()
                );
                self.exec.yield_seen = !self.persistent.seen.insert(place);
                self.log_event(&format!("Yielded: {}", reason));
                self.events.push(Event::YieldRequest {
                    reason: reason.clone(),
                });
                self.exec.yield_reason = Some(reason);
                self.record_yield();
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
                    self.events.push(Event::GraphEnd {
                        graph_id: self.exec.scene.clone(),
                    });
                }
                self.enter_scene(scene);
            }
        }
        Ok(())
    }

    /// Add the shown block to the backlog and remember the state for rollback
    fn record_yield(&mut self) {
        let (block_type, args) = match &self.exec.last_block {
            Some((block_type, args)) => (Some(block_type.clone()), args.clone()),
            None => (None, JsonValue::Null),
        };
        self.exec.backlog.push(BacklogEntry {
            scene: self.exec.scene.clone(),
            node_id: self.exec.current_node_id.clone(),
            block_type,
            args,
            choice: None,
        });
        if self.exec.backlog.len() > BACKLOG_LIMIT {
            self.exec.backlog.remove(0);
        }
        if self.rollback_limit > 0 {
            if self.history.len() >= self.rollback_limit {
                self.history.pop_front();
            }
            self.history.push_back(self.exec.clone());
        }
    }

    /// Go back to the previous point where the game waited for the player,
    /// undoing everything since (except persistent data). Returns false if
    /// there is nothing to go back to.
    pub fn rollback(&mut self) -> bool {
        // While waiting, the newest snapshot is the current point itself
        let at_newest = self.exec.yield_reason.is_some() && self.history.back() == Some(&self.exec);
        let needed = if at_newest { 2 } else { 1 };
        if self.history.len() < needed {
            return false;
        }
        if at_newest {
            self.history.pop_back();
        }
        if let Some(previous) = self.history.back() {
            self.exec = previous.clone();
        }
        self.log_event("Rolled back");
        true
    }

    /// "scene:node" of the current position
    pub fn location(&self) -> String {
        format!(
            "{}:{}",
            self.exec.scene,
            self.exec.current_node_id.as_deref().unwrap_or_default()
        )
    }

    /// Debugger: pause whenever execution enters this node
    pub fn set_breakpoint(&mut self, scene: &str, node_id: &str, enabled: bool) {
        let key = format!("{}:{}", scene, node_id);
        if enabled {
            self.breakpoints.insert(key);
        } else {
            self.breakpoints.remove(&key);
        }
    }

    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    /// Breakpoints as "scene:node", sorted
    pub fn breakpoints(&self) -> Vec<String> {
        self.breakpoints.iter().cloned().collect()
    }

    /// Debugger: "scene:node" where execution is paused
    pub fn paused_at(&self) -> Option<&str> {
        self.paused_at.as_deref()
    }

    /// Debugger: leave a pause and run on (until the next breakpoint)
    pub fn continue_execution(&mut self) {
        self.paused_at = None;
        self.pause_on_next_node = false;
    }

    /// Debugger: leave a pause and stop again when the next node is entered
    pub fn step_node(&mut self) {
        self.paused_at = None;
        self.pause_on_next_node = true;
    }

    /// How many yields are kept for rollback (0 disables it)
    pub fn set_rollback_limit(&mut self, limit: usize) {
        self.rollback_limit = limit;
        while self.history.len() > limit {
            self.history.pop_front();
        }
    }

    pub fn backlog(&self) -> &[BacklogEntry] {
        &self.exec.backlog
    }

    pub fn host_state(&self) -> &JsonValue {
        &self.exec.host
    }

    /// Replace the host state. Events are delivered after the VM has
    /// stopped at a yield, so a host that updates its state in response
    /// also updates the rollback snapshot of that yield.
    pub fn set_host_state(&mut self, value: JsonValue) {
        if self.exec.yield_reason.is_some() {
            if let Some(snapshot) = self.history.back_mut() {
                if snapshot.pc == self.exec.pc && snapshot.scene == self.exec.scene {
                    snapshot.host = value.clone();
                }
            }
        }
        self.exec.host = value;
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
        self.events.push(Event::GraphEnd {
            graph_id: self.exec.scene.clone(),
        });
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
            ("choice", Some(value)) => {
                if let Some(entry) = self.exec.backlog.last_mut() {
                    entry.choice = Some(value.clone());
                }
                self.exec.stack.push(value)
            }
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
        self.paused_at = None;
        self.pause_on_next_node = false;
        self.log_event("VM stopped");
        self.events.push(Event::GraphEnd {
            graph_id: self.exec.scene.clone(),
        });
    }

    /// Clear execution state (keeps the loaded project and persistent data)
    pub fn reset(&mut self) {
        self.exec = ExecState::default();
        self.history.clear();
        self.log.clear();
        self.events.clear();
    }

    /// Capture the execution state. Saves are normally taken while the
    /// machine is yielding (waiting for the player).
    pub fn save_state(&self) -> Result<SaveState, String> {
        let project = self.project.as_ref().ok_or("No program loaded")?;
        let anchor = |scene: &str, pc: usize| {
            project
                .scenes
                .get(scene)
                .and_then(|p| anchor_for(p, scene, pc))
        };
        Ok(SaveState {
            format: SAVE_FORMAT,
            project_id: project.project_id.clone(),
            fingerprint: project.fingerprint(),
            state: self.exec.clone(),
            position: anchor(&self.exec.scene, self.exec.pc),
            return_positions: self
                .exec
                .call_stack
                .iter()
                .map(|f| anchor(&f.scene, f.return_pc.saturating_sub(1)))
                .collect(),
        })
    }

    /// Restore a saved state. If the project was edited since the save,
    /// positions are moved to the same nodes in the new bytecode (see
    /// `Anchor`); variables added since get their initial values. The
    /// save is checked before anything changes, so a rejected save leaves
    /// the machine untouched.
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
        let state = if save.fingerprint == project.fingerprint() {
            save.state
        } else {
            Self::migrate(project, save)?
        };

        let scene_len = |scene: &str| project.scenes.get(scene).map(|p| p.opcodes.len());
        let positions = std::iter::once((state.scene.as_str(), state.pc)).chain(
            state
                .call_stack
                .iter()
                .map(|f| (f.scene.as_str(), f.return_pc)),
        );
        for (scene, pc) in positions {
            match scene_len(scene) {
                Some(len) if pc <= len => {}
                Some(_) => {
                    return Err(format!(
                        "Saved position {} in '{}' is out of range",
                        pc, scene
                    ))
                }
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

        // Variables added to the project since the save get their initial values
        let defaults = project.variables.clone();
        self.exec = state;
        self.paused_at = None;
        self.apply_defaults(&defaults);
        // Rollback starts over from the loaded point
        self.history.clear();
        if self.exec.yield_reason.is_some() && self.rollback_limit > 0 {
            self.history.push_back(self.exec.clone());
        }
        self.log_event("State loaded");
        Ok(())
    }

    /// Move a save made with an older version of the project onto the
    /// current bytecode, using its node anchors
    fn migrate(project: &CompiledProject, save: SaveState) -> Result<ExecState, String> {
        let mut state = save.state;
        if !state.stack.is_empty() {
            return Err(
                "Save was made in the middle of a computation and the project has changed since"
                    .to_string(),
            );
        }
        let relocate = |scene: &str,
                        anchor: &Option<Anchor>,
                        offset: usize|
         -> Result<usize, String> {
            let program = project
                .scenes
                .get(scene)
                .ok_or_else(|| format!("Scene '{}' was removed from the project", scene))?;
            match anchor {
                // Saved at the very end of the scene
                None => Ok(program.opcodes.len()),
                Some(anchor) => resolve_anchor(program, anchor).map(|pc| pc + offset).ok_or_else(|| {
                    format!(
                        "Node '{}' in scene '{}' was removed or changed too much to continue this save",
                        anchor.node_id, scene
                    )
                }),
            }
        };

        state.pc = relocate(&state.scene, &save.position, 0)?;
        if save.return_positions.len() != state.call_stack.len() {
            return Err("Save is damaged: call stack positions are missing".to_string());
        }
        for (frame, anchor) in state.call_stack.iter_mut().zip(&save.return_positions) {
            frame.return_pc = relocate(&frame.scene, anchor, 1)?;
        }
        Ok(state)
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

    /// Read a variable; `persistent.*` names read the persistent data
    pub fn get_variable(&self, name: &str) -> Option<&JsonValue> {
        match name.strip_prefix(PERSISTENT_PREFIX) {
            Some(key) => self.persistent.variables.get(key),
            None => self.exec.variables.get(name),
        }
    }

    /// Set a variable; `persistent.*` names write the persistent data
    pub fn set_variable(&mut self, name: &str, value: JsonValue) {
        match name.strip_prefix(PERSISTENT_PREFIX) {
            Some(key) => self.persistent.variables.insert(key.to_string(), value),
            None => self.exec.variables.insert(name.to_string(), value),
        };
    }

    pub fn persistent(&self) -> &PersistentData {
        &self.persistent
    }

    /// Replace the persistent data (e.g. loaded from disk)
    pub fn set_persistent(&mut self, data: PersistentData) {
        self.persistent = data;
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
                let value =
                    self.exec.stack.pop().ok_or_else(|| {
                        format!("VAR_SET: Stack underflow for variable '{}'", name)
                    })?;
                self.events.push(Event::VariableChanged {
                    name: name.clone(),
                    value: value.clone(),
                });
                self.set_variable(name, value);
                Ok(Flow::Next)
            }

            Opcode::VAR_GET { name } => {
                let value = self
                    .get_variable(name)
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
                let top = self
                    .exec
                    .stack
                    .last()
                    .cloned()
                    .ok_or("DUP: Stack underflow")?;
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
                self.exec
                    .stack
                    .push(JsonValue::Bool(!values::truthy(&value)));
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
                Ok(if values::truthy(&condition) {
                    Flow::Jump(*target)
                } else {
                    Flow::Next
                })
            }

            Opcode::JUMP_IF_FALSE { target } => {
                let condition = self.pop(opcode)?;
                Ok(if values::truthy(&condition) {
                    Flow::Next
                } else {
                    Flow::Jump(*target)
                })
            }

            Opcode::JUMP { target } => Ok(Flow::Jump(*target)),

            Opcode::YIELD { wait_type } => Ok(Flow::Yield(wait_type.clone())),

            Opcode::WAIT { frames } => Ok(Flow::Wait(*frames)),

            Opcode::JUMP_SCENE { scene } => Ok(Flow::EnterScene {
                scene: scene.clone(),
                call: false,
            }),

            Opcode::CALL_SCENE { scene } => Ok(Flow::EnterScene {
                scene: scene.clone(),
                call: true,
            }),

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
        let program = compile_graph(
            &parse_json_graph(json).unwrap(),
            &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap(),
        )
        .unwrap();
        let mut m = Machine::new();
        m.load(program);
        m.start().unwrap();
        m
    }

    fn block_calls(events: &[Event]) -> Vec<(String, JsonValue)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::BlockCall {
                    block_type, args, ..
                } => Some((block_type.clone(), args.clone())),
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
            Some((
                "ShowText".into(),
                json!({"text": "Welcome to the Visual Novel!", "speaker": "Narrator"})
            ))
        );

        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.yield_reason(), Some("choice"));

        m.resume(Some(json!(0))).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.yield_reason(), Some("input"));
        assert_eq!(m.exec.current_node_id.as_deref(), Some("adventure_path"));
        assert_eq!(
            m.exec.last_block.as_ref().unwrap().1["text"],
            "Your adventure begins..."
        );

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
        assert!(
            m.resume(Some(json!(0))).is_err(),
            "input yield takes no value"
        );
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
        restored
            .load_state(serde_json::from_str(&saved).unwrap())
            .unwrap();
        assert_eq!(restored.save_state().unwrap(), m.save_state().unwrap());

        for vm in [&mut m, &mut restored] {
            vm.resume(Some(json!(0))).unwrap();
            vm.run_until_blocked(1000).unwrap();
        }
        assert_eq!(
            restored.exec.current_node_id.as_deref(),
            Some("adventure_path")
        );
        assert_eq!(restored.get_variable("score"), Some(&json!(3)));
        assert_eq!(restored.save_state().unwrap(), m.save_state().unwrap());
    }

    #[test]
    fn save_from_another_project_is_rejected() {
        let mut m = machine_for(EXAMPLE);
        m.run_until_blocked(1000).unwrap();
        let saved = m.save_state().unwrap();

        let mut other = machine_for(include_str!("../../examples/menu_loop.json"));
        let before = other.save_state().unwrap();
        assert!(other
            .load_state(saved)
            .unwrap_err()
            .contains("is for project"));
        assert_eq!(
            other.save_state().unwrap(),
            before,
            "rejected load changed state"
        );
    }

    /// The example at its choice, saved, then loaded into an edited version
    fn load_into_edited(edit: impl Fn(&mut JsonValue)) -> Result<Machine, String> {
        let mut m = machine_for(EXAMPLE);
        m.run_until_blocked(1000).unwrap();
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.yield_reason(), Some("choice"));
        let saved = m.save_state().unwrap();

        let mut graph: JsonValue = serde_json::from_str(EXAMPLE).unwrap();
        edit(&mut graph);
        let mut edited = machine_for(&graph.to_string());
        edited.load_state(saved)?;
        Ok(edited)
    }

    #[test]
    fn save_survives_edits_elsewhere_in_the_graph() {
        let mut m = load_into_edited(|g| {
            // Reword a line and insert a node before the choice
            g["nodes"][1]["parameters"]["text"] = json!("Welcome, {name}!");
            g["nodes"].as_array_mut().unwrap().push(json!(
                {"id": "flag", "type": "SetVariable", "parameters": {"name": "name", "value": "Ann"}}
            ));
            g["edges"][0] = json!({"from": "start", "to": "flag"});
            g["edges"].as_array_mut().unwrap().push(json!({"from": "flag", "to": "show_title"}));
        })
        .unwrap();
        assert_eq!(m.yield_reason(), Some("choice"));
        m.resume(Some(json!(0))).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.exec.current_node_id.as_deref(), Some("adventure_path"));
    }

    #[test]
    fn save_at_a_removed_node_is_rejected() {
        let err = load_into_edited(|g| {
            g["nodes"][2]["id"] = json!("renamed_choice");
            for edge in g["edges"].as_array_mut().unwrap() {
                for end in ["from", "to"] {
                    if edge[end] == "player_choice" {
                        edge[end] = json!("renamed_choice");
                    }
                }
            }
        })
        .err()
        .unwrap();
        assert!(err.contains("Node 'player_choice'"), "{}", err);
    }

    #[test]
    fn fingerprint_is_stable() {
        let program = compile_graph(
            &parse_json_graph(EXAMPLE).unwrap(),
            &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap(),
        )
        .unwrap();
        assert_eq!(
            program.fingerprint(),
            compile_graph(
                &parse_json_graph(EXAMPLE).unwrap(),
                &crate::core::plugin_registry::PluginRegistry::with_defaults().unwrap()
            )
            .unwrap()
            .fingerprint()
        );
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
        assert_eq!(
            (m.exec.pc, m.exec.wait_frames),
            (1, 3),
            "step() must not advance a wait"
        );
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
        assert_eq!(
            scenes,
            vec!["+town", "+shop", "-shop", "-town", "+ending", "-ending"]
        );
    }

    #[test]
    fn save_inside_a_called_scene() {
        let mut m = project_machine("town", &[TOWN, SHOP, ENDING]);
        m.run_until_blocked(1000).unwrap();
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(
            (m.exec.scene.as_str(), m.exec.call_stack.len()),
            ("shop", 1)
        );

        let saved = m.save_state().unwrap();
        let mut restored = project_machine("town", &[TOWN, SHOP, ENDING]);
        restored.load_state(saved).unwrap();
        restored.resume(None).unwrap();
        restored.run_until_blocked(1000).unwrap();
        assert_eq!(restored.exec.scene, "town");
        assert_eq!(
            restored.exec.last_block.as_ref().unwrap().1["text"],
            "Back with 6 gold."
        );
    }

    #[test]
    fn call_stack_survives_edits_to_the_caller() {
        let mut m = project_machine("town", &[TOWN, SHOP, ENDING]);
        m.run_until_blocked(1000).unwrap();
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.exec.scene, "shop");
        let saved = m.save_state().unwrap();

        // Town gets a new line before the shop visit, and a new variable
        let mut town: JsonValue = serde_json::from_str(TOWN).unwrap();
        town["nodes"].as_array_mut().unwrap().push(
            json!({"id": "extra", "type": "ShowText", "parameters": {"text": "A new line."}}),
        );
        town["edges"][1] = json!({"from": "hello", "to": "extra"});
        town["edges"]
            .as_array_mut()
            .unwrap()
            .push(json!({"from": "extra", "to": "shop"}));
        let json = format!(
            r#"{{"project_id":"p","start":"town","variables":{{"gold":10,"reputation":1}},"scenes":[{},{},{}]}}"#,
            town, SHOP, ENDING
        );
        let project = compile_project(&json, &PluginRegistry::with_defaults().unwrap()).unwrap();
        let mut restored = Machine::new();
        restored.load_project(project);
        restored.load_state(saved).unwrap();

        restored.resume(None).unwrap();
        restored.run_until_blocked(1000).unwrap();
        assert_eq!(
            restored.exec.last_block.as_ref().unwrap().1["text"],
            "Back with 6 gold."
        );
        assert_eq!(restored.get_variable("reputation"), Some(&json!(1)));
    }

    #[test]
    fn persistent_data_outlives_playthroughs_and_saves() {
        let graph = r#"{"graph_id":"g","version":"1.0",
            "nodes":[{"id":"s","type":"entry"},
                     {"id":"line","type":"ShowText","parameters":{"text":"Endings seen: {persistent.endings}"}},
                     {"id":"count","type":"SetVariable","parameters":{"name":"persistent.endings","expression":"persistent.endings + 1"}},
                     {"id":"e","type":"End"}],
            "edges":[{"from":"s","to":"line"},{"from":"line","to":"count"},{"from":"count","to":"e"}]}"#;
        let mut m = machine_for(graph);
        m.stop();
        m.set_variable("persistent.endings", json!(0));

        let mut texts = Vec::new();
        let mut seen = Vec::new();
        let mut first_save = None;
        for _ in 0..2 {
            m.start().unwrap();
            m.run_until_blocked(100).unwrap();
            texts.push(m.exec.last_block.as_ref().unwrap().1["text"].clone());
            seen.push(m.exec.yield_seen);
            first_save.get_or_insert(m.save_state().unwrap());
            m.resume(None).unwrap();
            m.run_until_blocked(100).unwrap();
        }
        assert_eq!(
            texts,
            vec![json!("Endings seen: 0"), json!("Endings seen: 1")]
        );
        assert_eq!(seen, vec![false, true], "second visit is marked as seen");

        // Loading an old save does not roll persistent data back
        m.load_state(first_save.unwrap()).unwrap();
        assert_eq!(m.get_variable("persistent.endings"), Some(&json!(2)));
        assert!(m.persistent().seen.contains("g:line"));
        assert!(!m.exec.variables.contains_key("persistent.endings"));
    }

    #[test]
    fn rollback_returns_to_previous_yields() {
        let mut m = machine_for(AFFECTION);
        m.set_variable("player", json!("Alex"));
        m.set_variable("speaker_name", json!("Mia"));
        assert!(!m.rollback(), "nothing to roll back to yet");
        m.run_until_blocked(1000).unwrap(); // greeting
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap(); // choice
        m.resume(Some(json!(1))).unwrap();
        m.run_until_blocked(1000).unwrap(); // "She frowns."
        assert_eq!(m.get_variable("affection"), Some(&json!(-1)));
        assert_eq!(m.backlog().len(), 3);
        assert_eq!(m.backlog()[1].choice, Some(json!(1)));

        // Back to the choice: the rude answer is undone
        assert!(m.rollback());
        assert_eq!(m.yield_reason(), Some("choice"));
        assert_eq!(m.get_variable("affection"), Some(&json!(0)));
        assert_eq!(m.backlog().len(), 2);
        assert_eq!(m.backlog()[1].choice, None);

        // Choose differently this time
        m.resume(Some(json!(0))).unwrap();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(
            m.exec.last_block.as_ref().unwrap().1["text"],
            "She smiles. (affection: 3)"
        );

        // smile -> choice -> greeting
        assert!(m.rollback());
        assert!(m.rollback());
        assert_eq!(m.backlog().len(), 1, "back at the greeting");
        assert!(!m.rollback());

        // Right after resume (not waiting any more) the latest yield is
        // the point to go back to
        m.resume(None).unwrap();
        assert!(!m.is_yielding());
        assert!(m.rollback());
        assert_eq!(m.yield_reason(), Some("input"));
        assert_eq!(m.backlog().len(), 1);
    }

    #[test]
    fn host_state_follows_saves_and_rollback() {
        let mut m = machine_for(EXAMPLE);
        m.run_until_blocked(1000).unwrap();
        m.set_host_state(json!({"background": "bg/title"}));
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap(); // choice
        m.set_host_state(json!({"background": "bg/forest"}));
        let saved = m.save_state().unwrap();

        assert!(m.rollback());
        assert_eq!(m.host_state(), &json!({"background": "bg/title"}));

        let mut restored = machine_for(EXAMPLE);
        restored.load_state(saved).unwrap();
        assert_eq!(restored.host_state(), &json!({"background": "bg/forest"}));
    }

    #[test]
    fn rollback_limit_bounds_history() {
        let mut m = machine_for(include_str!("../../examples/menu_loop.json"));
        m.set_rollback_limit(2);
        m.run_until_blocked(1000).unwrap();
        for _ in 0..5 {
            m.resume(if m.yield_reason() == Some("choice") {
                Some(json!(0))
            } else {
                None
            })
            .unwrap();
            m.run_until_blocked(1000).unwrap();
        }
        assert!(m.rollback());
        assert!(!m.rollback(), "only two yields are kept");
    }

    #[test]
    fn breakpoints_pause_before_the_node() {
        let mut m = machine_for(AFFECTION);
        m.set_variable("player", json!("Alex"));
        m.set_variable("speaker_name", json!("Mia"));
        m.set_breakpoint("affection", "check", true);
        m.run_until_blocked(1000).unwrap();
        m.resume(None).unwrap();
        m.run_until_blocked(1000).unwrap();
        m.resume(Some(json!(0))).unwrap();
        m.run_until_blocked(1000).unwrap();

        // Stopped on entering "check", before its condition ran
        assert_eq!(m.paused_at(), Some("affection:check"));
        assert!(m.is_running() && !m.is_yielding());
        assert_eq!(m.get_variable("affection"), Some(&json!(3)));
        assert!(m
            .take_events()
            .iter()
            .any(|e| matches!(e, Event::Breakpoint { node_id, .. } if node_id == "check")));
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.paused_at(), Some("affection:check"), "stays paused");

        // Step node by node: check -> smile
        m.step_node();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.paused_at(), Some("affection:smile"));

        m.continue_execution();
        m.run_until_blocked(1000).unwrap();
        assert_eq!(m.paused_at(), None);
        assert_eq!(m.yield_reason(), Some("input"));
        assert_eq!(m.breakpoints(), vec!["affection:check"]);
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
            opcodes: vec![
                Opcode::VAR_GET {
                    name: "missing".into(),
                },
                Opcode::END,
            ],
            node_map: vec!["n".into(), "n".into()],
            node_types: HashMap::new(),
        });
        m.start().unwrap();
        assert!(m.step().is_err());
        assert!(!m.is_running());
        assert!(m
            .take_events()
            .iter()
            .any(|e| matches!(e, Event::Error { .. })));
    }
}

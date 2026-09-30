"""High-level Engine API for Python"""

import json
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Union

from . import _core
from .vm import state_to_dict


class Engine:
    """
    Main engine interface for Python.

    Provides initialization, plugin registration, graph loading,
    and execution control.

    Typical loop::

        engine.run()
        state = engine.run_until_yield()
        while state is not None:
            if state["yield_reason"] == "choice":
                state = engine.resume(pick(state["last_block_args"]["choices"]))
            else:  # "input": show state["last_block_args"], wait for a click
                state = engine.resume()
            state = engine.run_until_yield()
    """

    # Created by init(); methods call _require_init() before using them
    _event_bus: "_core.EventBus"
    _plugin_registry: "_core.PluginRegistry"
    _asset_manager: "_core.AssetManager"
    _tick_loop: "_core.FixedTickLoop"
    _compiler: "_core.GraphCompiler"
    _vm: "_core.VMExecutor"

    def __init__(self) -> None:
        self._initialized = False
        self._current_graph_id: Optional[str] = None
        self._compiled_program: Optional[
            Union["_core.CompiledProgram", "_core.CompiledProject"]
        ] = None
        self._block_handlers: Dict[str, Callable[[Dict[str, Any]], Any]] = {}

    def init(self) -> None:
        """Initialize the engine core systems. The standard blocks
        (novel_plugin: ShowText, PlayVoice) are registered."""
        self._event_bus = _core.EventBus()
        self._plugin_registry = _core.PluginRegistry(with_defaults=True)
        self._asset_manager = _core.AssetManager()
        self._tick_loop = _core.FixedTickLoop()
        self._compiler = _core.GraphCompiler(self._plugin_registry)
        self._vm = _core.VMExecutor()
        self._vm.set_event_bus(self._event_bus)
        self._event_bus.subscribe("BlockCall", self._dispatch_block)

        self._initialized = True

    def _require_init(self) -> None:
        if not self._initialized:
            raise RuntimeError("Engine not initialized. Call init() first.")

    def register_plugin(self, plugin_name: str) -> None:
        """
        Register a built-in plugin by name.

        Args:
            plugin_name: Name of plugin to register (e.g., "novel_plugin")
        """
        self._require_init()

        if plugin_name != "novel_plugin":
            raise ValueError(f"Unknown plugin: {plugin_name}")
        if not self._plugin_registry.has_plugin(plugin_name):
            _core.NovelPlugin().register_blocks(self._plugin_registry)

    def register_block(
        self,
        definition: "_core.BlockDefinition",
        handler: Optional[Callable[[Dict[str, Any]], Any]] = None,
        plugin: str = "game",
    ) -> None:
        """
        Add a block that graphs can use.

        Args:
            definition: Parameters, outputs and wait behaviour of the block.
            handler: Called with the block's arguments (a dict) each time
                the block runs.
            plugin: Plugin the block belongs to; created if needed.
        """
        self._require_init()
        if not self._plugin_registry.has_plugin(plugin):
            self._plugin_registry.register_plugin(_core.PluginInfo(plugin, "0.0.0", ""))
        self._plugin_registry.register_block(plugin, definition)
        if handler is not None:
            self._block_handlers[definition.block_type] = handler

    def _dispatch_block(self, payload: str) -> None:
        call = json.loads(payload)
        handler = self._block_handlers.get(call["block_type"])
        if handler is not None:
            handler(call["args"])

    @property
    def plugin_registry(self) -> "_core.PluginRegistry":
        self._require_init()
        return self._plugin_registry

    @property
    def asset_manager(self) -> "_core.AssetManager":
        self._require_init()
        return self._asset_manager

    def scan_assets(self, directory: str) -> int:
        """
        Register every asset file under `directory` (ids are relative
        paths without extension, e.g. "voice/mia_001") and from now on
        check asset references when graphs are loaded.

        Returns:
            Number of assets found
        """
        self._require_init()
        self._asset_manager.set_base_path(directory)
        count = self._asset_manager.scan()
        self._compiler.set_assets(self._asset_manager)
        return count

    def asset_path(self, asset_id: str) -> Optional[str]:
        """Full path of a registered asset, or None if it is unknown."""
        self._require_init()
        return self._asset_manager.resolve_path(asset_id)

    @property
    def tick_loop(self) -> "_core.FixedTickLoop":
        self._require_init()
        return self._tick_loop

    def subscribe(self, event_type: str, callback: Callable[..., Any]) -> None:
        """
        Subscribe to engine events.

        The callback receives the decoded payload dict. With event_type "*"
        it receives (event_type, payload) for every event.
        Event types: GraphStart, GraphEnd, NodeEnter, BlockCall,
        VariableChanged, YieldRequest, Error.
        """
        self._require_init()
        if event_type == "*":
            self._event_bus.subscribe(
                "*", lambda kind, payload: callback(kind, json.loads(payload))
            )
        else:
            self._event_bus.subscribe(
                event_type, lambda payload: callback(json.loads(payload))
            )

    def load_graph(self, json_path: str) -> str:
        """
        Load and compile a graph from JSON file.

        Args:
            json_path: Path to JSON graph file

        Returns:
            Graph ID string
        """
        path = Path(json_path)
        if not path.exists():
            raise FileNotFoundError(f"Graph file not found: {json_path}")

        with open(path, "r", encoding="utf-8") as f:
            return self.load_graph_from_dict(json.load(f))

    def load_graph_from_dict(self, graph_data: Dict[str, Any]) -> str:
        """
        Load and compile a graph from dictionary.

        Args:
            graph_data: Graph dictionary

        Returns:
            Graph ID string
        """
        self._require_init()

        program = self._compiler.compile_json(json.dumps(graph_data))
        self._compiled_program = program
        self._current_graph_id = program.graph_id
        self._vm.load(program)
        return program.graph_id

    def load_project(self, project_path: str) -> str:
        """
        Load and compile a project: several scenes, a start scene and
        initial variables. Scene entries may be file paths, relative to
        the project file, or inline graphs. An "assets" entry names the
        asset folder, which is scanned (see scan_assets).

        Returns:
            Project ID string
        """
        path = Path(project_path)
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
        return self.load_project_from_dict(data, base_dir=str(path.parent))

    def load_project_from_dict(
        self, project_data: Dict[str, Any], base_dir: Optional[str] = None
    ) -> str:
        """Load a project from a dict; see load_project()."""
        self._require_init()
        base = Path(base_dir) if base_dir is not None else Path.cwd()
        scenes = []
        for scene in project_data.get("scenes", []):
            if isinstance(scene, str):
                with open(base / scene, "r", encoding="utf-8") as f:
                    scene = json.load(f)
            scenes.append(scene)
        data = {**project_data, "scenes": scenes}

        # "assets": "<folder>" registers the folder and checks references
        assets = project_data.get("assets")
        if isinstance(assets, str) and (base / assets).is_dir():
            self.scan_assets(str(base / assets))

        project = self._compiler.compile_project_json(json.dumps(data))
        self._compiled_program = project
        self._current_graph_id = project.start
        self._vm.load_project(project)
        return project.project_id

    def get_last_errors(self) -> List[str]:
        """Validation errors from the last load attempt."""
        self._require_init()
        return self._compiler.get_last_errors()

    def disassemble(self) -> str:
        """Bytecode listing of the loaded graph."""
        if self._compiled_program is None:
            raise RuntimeError("No graph loaded. Call load_graph() first.")
        return self._compiled_program.disassemble()

    def run(self) -> None:
        """Start graph execution."""
        if self._compiled_program is None:
            raise RuntimeError("No graph loaded. Call load_graph() first.")
        self._vm.start()

    def step(self) -> Optional[Dict[str, Any]]:
        """
        Execute one VM step.

        Returns:
            VM state dict, or None once execution has ended
        """
        self._require_init()
        return state_to_dict(self._vm.step())

    def tick(self, max_steps: int = 100_000) -> Optional[Dict[str, Any]]:
        """
        Advance one frame; call once per frame of the game loop. Counts
        down Wait nodes, then runs until the graph waits for the player,
        waits for frames, or ends.

        Returns:
            VM state dict, or None once execution has ended
        """
        self._require_init()
        return state_to_dict(self._vm.tick(max_steps))

    def run_until_yield(self, max_steps: int = 100_000) -> Optional[Dict[str, Any]]:
        """
        Run until the graph waits for the player or ends.

        Returns:
            VM state dict while waiting, or None once execution has ended
        """
        self._require_init()
        return state_to_dict(self._vm.run_until_yield(max_steps))

    def resume(self, choice: Optional[int] = None) -> Optional[Dict[str, Any]]:
        """
        Resume execution after a yield.

        Args:
            choice: Index of the selected option when waiting on a choice.

        Returns:
            VM state dict, or None if the engine was not waiting
        """
        self._require_init()
        state = self._vm.get_state()
        if state.yield_reason == "choice":
            args = json.loads(state.last_block_args or "{}")
            count = len(args.get("choices", []))
            if not isinstance(choice, int) or isinstance(choice, bool) or not 0 <= choice < count:
                raise ValueError(f"choice must be an int in range 0..{count - 1}, got {choice!r}")
        return state_to_dict(self._vm.resume(choice))

    def get_state(self) -> Dict[str, Any]:
        """Current VM state."""
        self._require_init()
        state = state_to_dict(self._vm.get_state())
        assert state is not None
        return state

    def stop(self) -> None:
        """Stop execution."""
        self._require_init()
        self._vm.stop()

    def save_state(self) -> Dict[str, Any]:
        """
        Capture the game state as a JSON-compatible dict.

        Take it while the engine waits for the player (after
        run_until_yield()); loading it continues from the same point.
        """
        self._require_init()
        state: Dict[str, Any] = json.loads(self._vm.save_state())
        return state

    def load_state(self, state: Dict[str, Any]) -> None:
        """
        Restore a state from save_state(). The same graph must be loaded;
        a save from a different graph or an edited version of it raises
        ValueError and leaves the current state unchanged.
        """
        self._require_init()
        if self._compiled_program is None:
            raise RuntimeError("No graph loaded. Call load_graph() first.")
        self._vm.load_state(json.dumps(state))

    def save_game(self, path: str) -> None:
        """Write save_state() to a JSON file."""
        state = self.save_state()
        with open(path, "w", encoding="utf-8") as f:
            json.dump(state, f, indent=2, ensure_ascii=False)

    def load_game(self, path: str) -> None:
        """Restore a state written by save_game()."""
        with open(path, "r", encoding="utf-8") as f:
            self.load_state(json.load(f))

    # --- debugging -------------------------------------------------------

    def add_breakpoint(self, scene: str, node_id: str) -> None:
        """Pause whenever execution enters this node (state["paused_at"])."""
        self._require_init()
        self._vm.set_breakpoint(scene, node_id, True)

    def remove_breakpoint(self, scene: str, node_id: str) -> None:
        self._require_init()
        self._vm.set_breakpoint(scene, node_id, False)

    def clear_breakpoints(self) -> None:
        self._require_init()
        self._vm.clear_breakpoints()

    def breakpoints(self) -> List[str]:
        """Breakpoints as "scene:node"."""
        self._require_init()
        return self._vm.get_breakpoints()

    def continue_execution(self) -> None:
        """Leave a breakpoint pause; the next tick() runs on."""
        self._require_init()
        self._vm.continue_execution()

    def step_node(self) -> None:
        """Leave a pause and stop again when the next node is entered."""
        self._require_init()
        self._vm.step_node()

    def debug_info(self) -> Dict[str, Any]:
        """Everything a variable inspector shows: position, call stack,
        variables, persistent variables, operand stack, breakpoints."""
        self._require_init()
        state = self.get_state()
        return {
            "scene": state["current_graph_id"],
            "node_id": state["current_node_id"],
            "pc": state["pc"],
            "paused_at": state["paused_at"],
            "yield_reason": state["yield_reason"],
            "call_stack": state["call_stack"],
            "variables": state["variables"],
            "persistent": json.loads(self._vm.get_persistent_variables()),
            "stack": state["stack"],
            "breakpoints": self.breakpoints(),
        }

    def rollback(self) -> Optional[Dict[str, Any]]:
        """
        Go back to the previous point where the game waited for the player
        (the previous line or choice), undoing everything since except
        persistent data.

        Returns:
            State dict at that point, or None if there is nothing to go back to
        """
        self._require_init()
        return state_to_dict(self._vm.rollback())

    def set_rollback_limit(self, limit: int) -> None:
        """How many steps back rollback() can go (default 100; 0 disables)."""
        self._require_init()
        self._vm.set_rollback_limit(limit)

    def backlog(self) -> List[Dict[str, Any]]:
        """
        Lines and choices shown so far, oldest first (last 200). Each entry
        has scene, node_id, block_type, args and, for choices, the picked
        choice. Saved with the game.
        """
        self._require_init()
        entries: List[Dict[str, Any]] = json.loads(self._vm.get_backlog())
        return entries

    def persistent_state(self) -> Dict[str, Any]:
        """
        Data shared by all playthroughs: ``persistent.*`` variables and the
        places already seen (for skipping read text). Not part of saves.
        """
        self._require_init()
        data: Dict[str, Any] = json.loads(self._vm.save_persistent())
        return data

    def load_persistent_state(self, data: Dict[str, Any]) -> None:
        """Replace the persistent data with a dict from persistent_state()."""
        self._require_init()
        self._vm.load_persistent(json.dumps(data))

    def save_persistent(self, path: str) -> None:
        """Write persistent_state() to a JSON file."""
        with open(path, "w", encoding="utf-8") as f:
            json.dump(self.persistent_state(), f, indent=2, ensure_ascii=False)

    def load_persistent(self, path: str) -> None:
        """Load persistent data written by save_persistent(); a missing
        file (first launch) is ignored."""
        try:
            with open(path, "r", encoding="utf-8") as f:
                data = json.load(f)
        except FileNotFoundError:
            return
        self.load_persistent_state(data)

    def get_variable(self, name: str) -> Optional[Any]:
        """Get a variable value."""
        self._require_init()
        value_str = self._vm.get_variable(name)
        if value_str is None:
            return None
        return json.loads(value_str)

    def set_variable(self, name: str, value: Any) -> None:
        """Set a variable value."""
        self._require_init()
        self._vm.set_variable(name, json.dumps(value))

    def get_log(self) -> List[str]:
        """Get execution log."""
        self._require_init()
        return self._vm.get_log()

    @property
    def is_running(self) -> bool:
        """Check if engine is running."""
        return self._initialized and self._vm.is_running()

    @property
    def is_yielding(self) -> bool:
        """Check if engine is yielding."""
        return self._initialized and self._vm.is_yielding()

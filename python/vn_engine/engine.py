"""High-level Engine API for Python"""

import json
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional

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
        self._compiled_program: Optional["_core.CompiledProgram"] = None

    def init(self) -> None:
        """Initialize the engine core systems."""
        self._event_bus = _core.EventBus()
        self._plugin_registry = _core.PluginRegistry()
        self._asset_manager = _core.AssetManager()
        self._tick_loop = _core.FixedTickLoop()
        self._compiler = _core.GraphCompiler()
        self._vm = _core.VMExecutor()
        self._vm.set_event_bus(self._event_bus)

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

        if plugin_name == "novel_plugin":
            _core.NovelPlugin().register_blocks(self._plugin_registry)
        else:
            raise ValueError(f"Unknown plugin: {plugin_name}")

    @property
    def plugin_registry(self) -> "_core.PluginRegistry":
        self._require_init()
        return self._plugin_registry

    @property
    def asset_manager(self) -> "_core.AssetManager":
        self._require_init()
        return self._asset_manager

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

        self._compiled_program = self._compiler.compile_json(json.dumps(graph_data))
        self._current_graph_id = self._compiled_program.graph_id
        self._vm.load(self._compiled_program)
        return self._current_graph_id

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

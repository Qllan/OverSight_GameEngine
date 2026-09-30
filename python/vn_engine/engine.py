"""High-level Engine API for Python"""

from typing import Optional, List, Dict, Any
import json
from pathlib import Path


class Engine:
    """
    Main engine interface for Python.
    
    Provides initialization, plugin registration, graph loading,
    and execution control.
    """
    
    def __init__(self):
        self._initialized = False
        self._event_bus = None
        self._plugin_registry = None
        self._asset_manager = None
        self._tick_loop = None
        self._compiler = None
        self._vm = None
        self._current_graph_id: Optional[str] = None
        self._compiled_program = None
    
    def init(self) -> None:
        """Initialize the engine core systems."""
        from vn_engine import EventBus, PluginRegistry, AssetManager, FixedTickLoop
        from vn_engine import GraphCompiler as RustGraphCompiler
        from vn_engine import VMExecutor as RustVMExecutor
        
        self._event_bus = EventBus()
        self._plugin_registry = PluginRegistry()
        self._asset_manager = AssetManager()
        self._tick_loop = FixedTickLoop()
        self._compiler = RustGraphCompiler()
        self._vm = RustVMExecutor()
        
        self._initialized = True
        print("[Engine] Initialized")
    
    def register_plugin(self, plugin_name: str) -> None:
        """
        Register a built-in plugin by name.
        
        Args:
            plugin_name: Name of plugin to register (e.g., "novel_plugin")
        """
        if not self._initialized:
            raise RuntimeError("Engine not initialized. Call init() first.")
        
        if plugin_name == "novel_plugin":
            from vn_engine import NovelPlugin
            plugin = NovelPlugin()
            plugin.register_blocks(self._plugin_registry)
            print(f"[Engine] Registered plugin: {plugin_name}")
        else:
            raise ValueError(f"Unknown plugin: {plugin_name}")
    
    def load_graph(self, json_path: str) -> str:
        """
        Load and compile a graph from JSON file.
        
        Args:
            json_path: Path to JSON graph file
            
        Returns:
            Graph ID string
        """
        if not self._initialized:
            raise RuntimeError("Engine not initialized. Call init() first.")
        
        path = Path(json_path)
        if not path.exists():
            raise FileNotFoundError(f"Graph file not found: {json_path}")
        
        with open(path, 'r') as f:
            json_data = json.load(f)
        
        # Compile the graph
        json_str = json.dumps(json_data)
        self._compiled_program = self._compiler.compile_json(json_str)
        self._current_graph_id = self._compiled_program.graph_id
        
        # Load into VM
        self._vm.load(self._compiled_program)
        self._vm.set_event_bus(self._event_bus)
        
        print(f"[Engine] Loaded graph: {self._current_graph_id}")
        return self._current_graph_id
    
    def load_graph_from_dict(self, graph_data: Dict[str, Any]) -> str:
        """
        Load and compile a graph from dictionary.
        
        Args:
            graph_data: Graph dictionary
            
        Returns:
            Graph ID string
        """
        if not self._initialized:
            raise RuntimeError("Engine not initialized. Call init() first.")
        
        json_str = json.dumps(graph_data)
        self._compiled_program = self._compiler.compile_json(json_str)
        self._current_graph_id = self._compiled_program.graph_id
        
        self._vm.load(self._compiled_program)
        self._vm.set_event_bus(self._event_bus)
        
        print(f"[Engine] Loaded graph: {self._current_graph_id}")
        return self._current_graph_id
    
    def run(self) -> None:
        """Start graph execution."""
        if self._compiled_program is None:
            raise RuntimeError("No graph loaded. Call load_graph() first.")
        
        self._vm.start()
        print(f"[Engine] Running graph: {self._current_graph_id}")
    
    def step(self) -> Optional[Dict[str, Any]]:
        """
        Execute one VM step.
        
        Returns:
            VM state dict or None if ended/yielded
        """
        if not self._vm.is_running():
            return None
        
        state = self._vm.step()
        if state is None:
            return None
        
        return {
            "pc": state.pc,
            "stack": state.stack,
            "variables": state.variables,
            "current_node_id": state.current_node_id,
            "is_yielding": state.is_yielding,
            "yield_reason": state.yield_reason,
            "is_running": state.is_running,
        }
    
    def resume(self) -> Optional[Dict[str, Any]]:
        """Resume execution after yield."""
        if not self._vm.is_yielding():
            return None
        
        state = self._vm.resume()
        if state is None:
            return None
        
        return {
            "pc": state.pc,
            "stack": state.stack,
            "variables": state.variables,
            "current_node_id": state.current_node_id,
            "is_yielding": state.is_yielding,
            "yield_reason": state.yield_reason,
            "is_running": state.is_running,
        }
    
    def stop(self) -> None:
        """Stop execution."""
        self._vm.stop()
        print("[Engine] Stopped")
    
    def get_variable(self, name: str) -> Optional[Any]:
        """Get a variable value."""
        value_str = self._vm.get_variable(name)
        if value_str is None:
            return None
        return json.loads(value_str)
    
    def set_variable(self, name: str, value: Any) -> None:
        """Set a variable value."""
        json_value = json.dumps(value)
        self._vm.set_variable(name, json_value)
    
    def get_log(self) -> List[str]:
        """Get execution log."""
        return self._vm.get_log()
    
    @property
    def is_running(self) -> bool:
        """Check if engine is running."""
        return self._vm.is_running()
    
    @property
    def is_yielding(self) -> bool:
        """Check if engine is yielding."""
        return self._vm.is_yielding()

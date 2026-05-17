"""VM Executor Python wrapper"""

from typing import Optional, Dict, Any, List


class VMExecutor:
    """
    Python wrapper for the Rust VMExecutor.
    
    Provides step-based execution, variable access, and state inspection.
    """
    
    def __init__(self):
        from vn_engine import VMExecutor as RustVMExecutor
        self._vm = RustVMExecutor()
    
    def load(self, program: Dict[str, Any]) -> None:
        """
        Load a compiled program.
        
        Note: In Phase 1, programs are loaded via Engine.load_graph().
        This is for advanced use cases.
        """
        # Placeholder - actual implementation requires PyO3 binding for CompiledProgram
        pass
    
    def start(self) -> None:
        """Start execution."""
        self._vm.start()
    
    def step(self) -> Optional[Dict[str, Any]]:
        """
        Execute one step.
        
        Returns:
            State dict or None if ended/yielded
        """
        return self._vm.step()
    
    def resume(self) -> Optional[Dict[str, Any]]:
        """Resume after yield."""
        return self._vm.resume()
    
    def stop(self) -> None:
        """Stop execution."""
        self._vm.stop()
    
    def get_variable(self, name: str) -> Optional[Any]:
        """Get variable value."""
        import json
        value_str = self._vm.get_variable(name)
        if value_str is None:
            return None
        return json.loads(value_str)
    
    def set_variable(self, name: str, value: Any) -> None:
        """Set variable value."""
        import json
        json_value = json.dumps(value)
        self._vm.set_variable(name, json_value)
    
    def get_log(self) -> List[str]:
        """Get execution log."""
        return self._vm.get_log()
    
    def is_running(self) -> bool:
        """Check if running."""
        return self._vm.is_running()
    
    def is_yielding(self) -> bool:
        """Check if yielding."""
        return self._vm.is_yielding()
    
    def reset(self) -> None:
        """Reset VM state."""
        self._vm.reset()

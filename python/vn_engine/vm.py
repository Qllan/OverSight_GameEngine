"""VM Executor Python wrapper"""

import json
from typing import Any, Dict, List, Optional, Union

from . import _core


def state_to_dict(state: Optional["_core.PyVMState"]) -> Optional[Dict[str, Any]]:
    """Convert a Rust PyVMState into a plain dict with decoded JSON values."""
    if state is None:
        return None
    return {
        "pc": state.pc,
        "stack": [json.loads(v) for v in state.stack],
        "variables": {k: json.loads(v) for k, v in state.variables.items()},
        "current_node_id": state.current_node_id,
        "is_yielding": state.is_yielding,
        "yield_reason": state.yield_reason,
        "is_running": state.is_running,
        "last_block_type": state.last_block_type,
        "last_block_args": (
            json.loads(state.last_block_args) if state.last_block_args is not None else None
        ),
    }


class VMExecutor:
    """
    Python wrapper for the Rust VMExecutor.

    Provides step-based execution, variable access, and state inspection.
    """

    def __init__(self) -> None:
        self._vm = _core.VMExecutor()

    def load(self, program: Union["_core.CompiledProgram", Dict[str, Any]]) -> None:
        """
        Load a compiled program.

        Args:
            program: A CompiledProgram, or a graph dict to compile first.
        """
        if isinstance(program, dict):
            program = _core.GraphCompiler().compile_json(json.dumps(program))
        self._vm.load(program)

    def set_event_bus(self, event_bus: "_core.EventBus") -> None:
        """Send VM events to this bus."""
        self._vm.set_event_bus(event_bus)

    def start(self) -> None:
        """Start execution from the beginning (variables are kept)."""
        self._vm.start()

    def step(self) -> Optional[Dict[str, Any]]:
        """
        Execute one step.

        Returns:
            State dict, or None once the VM has stopped
        """
        return state_to_dict(self._vm.step())

    def run_until_yield(self, max_steps: int = 100_000) -> Optional[Dict[str, Any]]:
        """Step until the VM yields or stops. Returns None once stopped."""
        return state_to_dict(self._vm.run_until_yield(max_steps))

    def resume(self, choice: Any = None) -> Optional[Dict[str, Any]]:
        """
        Continue after a yield. Pass the selected index for a "choice" yield.

        Returns:
            State dict, or None if the VM was not yielding
        """
        return state_to_dict(self._vm.resume(choice))

    def stop(self) -> None:
        """Stop execution."""
        self._vm.stop()

    def get_state(self) -> Dict[str, Any]:
        """Get the current state."""
        state = state_to_dict(self._vm.get_state())
        assert state is not None
        return state

    def get_variable(self, name: str) -> Optional[Any]:
        """Get variable value."""
        value_str = self._vm.get_variable(name)
        if value_str is None:
            return None
        return json.loads(value_str)

    def set_variable(self, name: str, value: Any) -> None:
        """Set variable value."""
        self._vm.set_variable(name, json.dumps(value))

    def get_log(self) -> List[str]:
        """Get execution log."""
        return self._vm.get_log()

    def is_running(self) -> bool:
        """Check if running."""
        return self._vm.is_running()

    def is_yielding(self) -> bool:
        """Check if yielding."""
        return self._vm.is_yielding()

    def save_state(self) -> Dict[str, Any]:
        """Execution state as a JSON-compatible dict."""
        return json.loads(self._vm.save_state())

    def load_state(self, state: Dict[str, Any]) -> None:
        """Restore a state from save_state() of the same compiled graph."""
        self._vm.load_state(json.dumps(state))

    def reset(self) -> None:
        """Reset VM state."""
        self._vm.reset()

"""Graph Compiler Python wrapper"""

import json
from typing import Any, Dict, List, Optional

from . import _core


class GraphCompiler:
    """
    Python wrapper for the Rust GraphCompiler.

    Provides JSON parsing, validation, and compilation to bytecode.
    """

    def __init__(self, registry: Optional["_core.PluginRegistry"] = None) -> None:
        """Blocks are resolved against `registry` (default: novel_plugin)."""
        self._compiler = _core.GraphCompiler(registry)
        self._validator = _core.GraphValidator(self._compiler.registry)

    def parse(self, json_data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Parse JSON data into AST graph.

        Args:
            json_data: Graph dictionary

        Returns:
            AST graph info dict
        """
        ast = self._compiler.parse(json.dumps(json_data))
        return {
            "graph_id": ast.graph_id,
            "version": ast.version,
            "node_count": len(ast.nodes),
            "entry_nodes": ast.entry_nodes,
            "is_dag": ast.is_dag(),
        }

    def validate(self, json_data: Dict[str, Any]) -> List[str]:
        """
        Validate a graph and return error messages.

        Args:
            json_data: Graph dictionary

        Returns:
            List of error messages (empty if valid)
        """
        ast = self._compiler.parse(json.dumps(json_data))
        return self._compiler.validate(ast)

    def validate_detailed(self, json_data: Dict[str, Any]) -> List[Dict[str, Any]]:
        """Validate a graph and return errors with type, node and hint."""
        ast = self._compiler.parse(json.dumps(json_data))
        return [
            {
                "error_type": e.error_type,
                "message": e.message,
                "node_id": e.node_id,
                "hint": e.hint,
            }
            for e in self._validator.validate(ast)
        ]

    def compile_program(self, json_data: Dict[str, Any]) -> "_core.CompiledProgram":
        """Compile a graph and return the program object for the VM."""
        return self._compiler.compile_json(json.dumps(json_data))

    def compile(self, json_data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Compile a graph to bytecode.

        Args:
            json_data: Graph dictionary

        Returns:
            Compiled program info dict
        """
        program = self.compile_program(json_data)
        return {
            "graph_id": program.graph_id,
            "opcode_count": program.opcode_count(),
            "opcodes": program.get_opcodes(),
            "disassembly": program.disassemble(),
        }

    def compile_project(self, project_data: Dict[str, Any]) -> "_core.CompiledProject":
        """Compile a project whose scenes are inline graph dicts."""
        return self._compiler.compile_project_json(json.dumps(project_data))

    def get_last_errors(self) -> List[str]:
        """Get errors from last compilation."""
        return self._compiler.get_last_errors()

    def has_errors(self) -> bool:
        """Check if last compilation had errors."""
        return self._compiler.has_errors()

    def clear_errors(self) -> None:
        """Clear error state."""
        self._compiler.clear_errors()

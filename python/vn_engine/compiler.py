"""Graph Compiler Python wrapper"""

from typing import List, Dict, Any, Optional


class GraphCompiler:
    """
    Python wrapper for the Rust GraphCompiler.
    
    Provides JSON parsing, validation, and compilation to bytecode.
    """
    
    def __init__(self):
        from vn_engine import GraphCompiler as RustGraphCompiler
        self._compiler = RustGraphCompiler()
    
    def parse(self, json_data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Parse JSON data into AST graph.
        
        Args:
            json_data: Graph dictionary
            
        Returns:
            AST graph info dict
        """
        import json
        json_str = json.dumps(json_data)
        ast = self._compiler.parse(json_str)
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
        import json
        json_str = json.dumps(json_data)
        ast = self._compiler.parse(json_str)
        return self._compiler.validate(ast)
    
    def compile(self, json_data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Compile a graph to bytecode.
        
        Args:
            json_data: Graph dictionary
            
        Returns:
            Compiled program info dict
        """
        import json
        json_str = json.dumps(json_data)
        program = self._compiler.compile_json(json_str)
        return {
            "graph_id": program.graph_id,
            "opcode_count": program.opcode_count(),
            "disassembly": program.disassemble(),
        }
    
    def get_last_errors(self) -> List[str]:
        """Get errors from last compilation."""
        return self._compiler.get_last_errors()
    
    def has_errors(self) -> bool:
        """Check if last compilation had errors."""
        return self._compiler.has_errors()
    
    def clear_errors(self) -> None:
        """Clear error state."""
        self._compiler.clear_errors()

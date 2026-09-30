//! GraphCompiler - Main compiler interface
//!
//! High-level API for compiling JSON graphs to executable bytecode.

use pyo3::prelude::*;
use crate::compiler::graph_parser::{parse_json_graph, AstGraph};
use crate::compiler::validator::validate_graph;
use crate::compiler::codegen::compile_graph;
use crate::compiler::codegen::CompiledProgram;

/// Main compiler class exposed to Python
#[pyclass]
pub struct GraphCompiler {
    last_errors: Vec<String>,
}

#[pymethods]
impl GraphCompiler {
    #[new]
    fn new() -> Self {
        GraphCompiler {
            last_errors: Vec::new(),
        }
    }

    /// Parse JSON string into AST graph
    fn parse(&self, json_str: &str) -> PyResult<AstGraph> {
        parse_json_graph(json_str)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Validate an AST graph, returns list of error messages
    fn validate(&mut self, graph: &AstGraph) -> Vec<String> {
        let errors = validate_graph(graph);
        self.last_errors = errors.iter().map(|e| e.message.clone()).collect();
        self.last_errors.clone()
    }

    /// Compile AST graph to bytecode
    fn compile(&self, graph: &AstGraph) -> PyResult<CompiledProgram> {
        compile_graph(graph)
            .map_err(pyo3::exceptions::PyRuntimeError::new_err)
    }

    /// Full compile pipeline: JSON → AST → Validate → Bytecode
    fn compile_json(&mut self, json_str: &str) -> PyResult<CompiledProgram> {
        self.last_errors.clear();

        // Parse
        let ast = parse_json_graph(json_str)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Parse error: {}", e)))?;
        
        // Validate
        let errors = validate_graph(&ast);
        if !errors.is_empty() {
            self.last_errors = errors.iter().map(|e| {
                format!("[{}] {}: {} (node: {:?})", 
                    e.error_type,
                    e.message,
                    e.hint.as_deref().unwrap_or(""),
                    e.node_id
                )
            }).collect();
            
            let error_msg = errors.iter()
                .map(|e| format!("{}: {}", e.error_type, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            
            return Err(pyo3::exceptions::PyValueError::new_err(
                format!("Validation failed: {}", error_msg)
            ));
        }
        
        // Compile
        compile_graph(&ast)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("Compile error: {}", e)))
    }

    /// Get errors from last validation/compilation
    fn get_last_errors(&self) -> Vec<String> {
        self.last_errors.clone()
    }

    /// Check if last compilation was successful
    fn has_errors(&self) -> bool {
        !self.last_errors.is_empty()
    }

    /// Clear error state
    fn clear_errors(&mut self) {
        self.last_errors.clear();
    }
}

impl Default for GraphCompiler {
    fn default() -> Self {
        Self::new()
    }
}

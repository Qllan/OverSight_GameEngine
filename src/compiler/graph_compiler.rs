//! GraphCompiler - Main compiler interface
//!
//! High-level API for compiling JSON graphs to executable bytecode.
//! Blocks are resolved against a PluginRegistry.

use pyo3::prelude::*;

use crate::compiler::codegen::{compile_graph, CompiledProgram};
use crate::compiler::graph_parser::{parse_json_graph, AstGraph};
use crate::compiler::validator::validate_graph;
use crate::core::plugin_registry::{shared_registry, PluginRegistry};

/// Main compiler class exposed to Python
#[pyclass]
pub struct GraphCompiler {
    registry: Py<PluginRegistry>,
    last_errors: Vec<String>,
}

#[pymethods]
impl GraphCompiler {
    /// Compile against `registry`, or against the default blocks
    /// (novel_plugin) when none is given. The registry is shared, so
    /// blocks registered later are picked up.
    #[new]
    #[pyo3(signature = (registry = None))]
    fn new(py: Python<'_>, registry: Option<Py<PluginRegistry>>) -> PyResult<Self> {
        Ok(GraphCompiler {
            registry: shared_registry(py, registry)?,
            last_errors: Vec::new(),
        })
    }

    /// The registry blocks are resolved against
    #[getter]
    fn registry(&self, py: Python<'_>) -> Py<PluginRegistry> {
        self.registry.clone_ref(py)
    }

    /// Parse JSON string into AST graph
    fn parse(&self, json_str: &str) -> PyResult<AstGraph> {
        parse_json_graph(json_str).map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Validate an AST graph, returns list of error messages
    fn validate(&mut self, py: Python<'_>, graph: &AstGraph) -> Vec<String> {
        let errors = validate_graph(graph, &self.registry.bind(py).borrow());
        self.last_errors = errors.iter().map(|e| e.message.clone()).collect();
        self.last_errors.clone()
    }

    /// Compile AST graph to bytecode (without validating it)
    fn compile(&self, py: Python<'_>, graph: &AstGraph) -> PyResult<CompiledProgram> {
        compile_graph(graph, &self.registry.bind(py).borrow())
            .map_err(pyo3::exceptions::PyRuntimeError::new_err)
    }

    /// Full compile pipeline: JSON → AST → Validate → Bytecode
    fn compile_json(&mut self, py: Python<'_>, json_str: &str) -> PyResult<CompiledProgram> {
        self.last_errors.clear();

        let ast = parse_json_graph(json_str)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Parse error: {}", e)))?;

        let registry = self.registry.bind(py).borrow();
        let errors = validate_graph(&ast, &registry);
        if !errors.is_empty() {
            self.last_errors = errors
                .iter()
                .map(|e| {
                    format!(
                        "[{}] {}: {} (node: {:?})",
                        e.error_type,
                        e.message,
                        e.hint.as_deref().unwrap_or(""),
                        e.node_id
                    )
                })
                .collect();

            let error_msg = errors
                .iter()
                .map(|e| format!("{}: {}", e.error_type, e.message))
                .collect::<Vec<_>>()
                .join("; ");

            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Validation failed: {}",
                error_msg
            )));
        }

        compile_graph(&ast, &registry).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Compile error: {}", e))
        })
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

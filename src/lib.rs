//! Visual Novel Engine - Rust Core
//! 
//! This crate provides the core functionality for the VN Engine:
//! - EventBus: Publish/subscribe event system
//! - PluginRegistry: Runtime plugin discovery and management
//! - AssetManager: Asset loading and caching (stubs for Phase 1)
//! - FixedTickLoop: Deterministic 60 FPS game loop
//! - GraphCompiler: JSON → AST → Opcodes compilation
//! - VM: Stack-based bytecode executor

pub mod core;
pub mod compiler;
pub mod vm;
pub mod plugins;

use pyo3::prelude::*;

/// Python module entry point
#[pymodule]
fn vn_engine(_py: Python, m: &PyModule) -> PyResult<()> {
    // Core modules
    m.add_class::<core::event_bus::EventBus>()?;
    m.add_class::<core::plugin_registry::PluginRegistry>()?;
    m.add_class::<core::asset_manager::AssetManager>()?;
    
    // Compiler
    m.add_class::<compiler::graph_compiler::GraphCompiler>()?;
    
    // VM
    m.add_class::<vm::executor::VMExecutor>()?;
    m.add_class::<vm::state::PyVMState>()?;
    
    Ok(())
}

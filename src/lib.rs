//! Visual Novel Engine - Rust Core
//!
//! This crate provides the core functionality for the VN Engine:
//! - EventBus: Publish/subscribe event system
//! - PluginRegistry: Runtime plugin discovery and management
//! - AssetManager: Asset loading and caching (stubs for Phase 1)
//! - FixedTickLoop: Deterministic 60 FPS game loop
//! - GraphCompiler: JSON → AST → Opcodes compilation
//! - VM: Stack-based bytecode executor

pub mod compiler;
pub mod core;
pub mod plugins;
pub mod pyjson;
pub mod vm;

use pyo3::prelude::*;

/// Python extension module, imported as `vn_engine._core`
#[pymodule]
#[pyo3(name = "_core")]
fn vn_engine_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Core modules
    m.add_class::<core::event_bus::EventBus>()?;
    m.add_class::<core::plugin_registry::PluginRegistry>()?;
    m.add_class::<core::plugin_registry::PluginInfo>()?;
    m.add_class::<core::plugin_registry::BlockDefinition>()?;
    m.add_class::<core::plugin_registry::ParameterDefinition>()?;
    m.add_class::<core::asset_manager::AssetManager>()?;
    m.add_class::<core::asset_manager::AssetInfo>()?;
    m.add_class::<core::asset_manager::AssetType>()?;
    m.add_class::<core::tick_loop::FixedTickLoop>()?;

    // Compiler
    m.add_class::<compiler::graph_compiler::GraphCompiler>()?;
    m.add_class::<compiler::graph_parser::AstGraph>()?;
    m.add_class::<compiler::graph_parser::AstNode>()?;
    m.add_class::<compiler::validator::GraphValidator>()?;
    m.add_class::<compiler::validator::ValidationError>()?;
    m.add_class::<compiler::codegen::CompiledProgram>()?;
    m.add_class::<compiler::project::CompiledProject>()?;

    // VM
    m.add_class::<vm::executor::VMExecutor>()?;
    m.add_class::<vm::state::PyVMState>()?;

    // Plugins
    m.add_class::<plugins::novel_plugin::NovelPlugin>()?;

    Ok(())
}

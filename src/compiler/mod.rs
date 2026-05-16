//! Compiler module - Graph compilation pipeline
//!
//! Transforms JSON graph definitions into executable bytecode:
//! JSON → AST → Validation (DAG) → Topological Sort → Opcodes

pub mod graph_parser;
pub mod validator;
pub mod topology;
pub mod codegen;
pub mod graph_compiler;

//! Compiler module - Graph compilation pipeline
//!
//! Transforms JSON graph definitions into executable bytecode:
//! JSON → AST → Validation → Opcodes

pub mod graph_parser;
pub mod validator;
pub mod codegen;
pub mod graph_compiler;

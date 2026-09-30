//! Compiler module - Graph compilation pipeline
//!
//! Transforms JSON graph definitions into executable bytecode:
//! JSON → AST → Validation → Opcodes

pub mod codegen;
pub mod expr;
pub mod graph_compiler;
pub mod graph_parser;
pub mod project;
pub mod validator;

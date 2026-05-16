//! VM Opcodes - Re-export from compiler with additional runtime info

pub use crate::compiler::codegen::Opcode;

/// Runtime opcode with resolved jump targets
#[derive(Debug, Clone)]
pub struct ResolvedOpcode {
    pub opcode: Opcode,
    pub source_node: Option<String>,
}

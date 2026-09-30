//! VM module - Virtual Machine for executing bytecode
//!
//! Stack-based VM that executes compiled opcodes deterministically.
//! Supports YIELD for pausing execution and waiting for user input.

pub mod opcodes;
pub mod machine;
pub mod executor;
pub mod stack_frame;
pub mod state;

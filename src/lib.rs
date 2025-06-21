//! Metashrew - Bitcoin indexer framework with WebAssembly virtual machine

#[cfg(test)]
mod tests;

// Re-export the SMT trait
pub use crate::smt_trait::{SMTOperations, BSTOperations, StateManager};

// Include the SMT trait module
pub mod smt_trait;
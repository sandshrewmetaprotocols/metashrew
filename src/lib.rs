//! Metashrew - Bitcoin indexer framework with WebAssembly virtual machine

#[cfg(test)]
mod tests;

// Re-export the SMT trait from rockshrew_smt
pub use rockshrew_smt::{SMTOperations, BSTOperations, StateManager};

// Legacy module - will be removed in future versions
// Use rockshrew_smt instead
#[deprecated(since = "9.1.0", note = "Use rockshrew_smt crate instead")]
pub mod smt_trait;

// WASM module handling
pub mod wasm;
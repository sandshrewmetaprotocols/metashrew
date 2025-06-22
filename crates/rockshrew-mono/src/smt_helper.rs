//! This module has been removed to eliminate code duplication.
//! All SMT functionality now uses the generic, well-tested implementation
//! from metashrew-runtime::smt::SMTHelper to ensure consistency between
//! test and production environments.
//!
//! Use metashrew_runtime::smt::SMTHelper instead.

// Re-export the generic SMT implementation for backward compatibility
pub use metashrew_runtime::smt::SMTHelper;

// Re-export constants that were used by the old implementation
pub use metashrew_runtime::smt::{SMT_ROOT_PREFIX, BST_PREFIX as BST_KEY_PREFIX, BST_HEIGHT_PREFIX as BST_HEIGHT_INDEX_PREFIX};

//! Metashrew Test Suite
//! 
//! This crate provides comprehensive testing for the Metashrew indexer framework
//! using the memshrew in-memory adapter and metashrew-minimal WASM module.

pub mod tests;

// Re-export test utilities for external use
pub use tests::{TestConfig, TestUtils};
pub use tests::block_builder::{BlockBuilder, ChainBuilder};
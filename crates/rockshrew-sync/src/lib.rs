//! Generic Bitcoin indexer synchronization framework
//!
//! This crate provides a generic framework for building Bitcoin indexers that can work
//! with different storage backends and Bitcoin node interfaces. It abstracts the core
//! synchronization logic from rockshrew-mono to enable testing and modularity.

pub mod traits;
pub mod sync;
pub mod error;
pub mod types;
pub mod adapters;

#[cfg(any(test, feature = "mock"))]
pub mod mock;

pub use traits::*;
pub use sync::*;
pub use error::*;
pub use types::*;
pub use adapters::*;

#[cfg(any(test, feature = "mock"))]
pub use mock::*;
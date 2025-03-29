//! # Metashrew Library
//!
//! A library for building Metashrew indexer programs with convenient macros and primitives.
//! This library simplifies the development of WASM modules for the Metashrew Bitcoin indexer framework.

#[macro_use]
extern crate log;

pub mod macros;
pub mod host;
pub mod indexer;
pub mod view;
pub mod proto;
pub mod utils;
pub mod native;

/// Re-export key components for easier access
pub use macros::*;
pub use host::*;
pub use indexer::*;
pub use view::*;

// Re-export the declare_indexer macro for backward compatibility
#[doc(hidden)]
pub use declare_indexer as metashrew_proto_program;
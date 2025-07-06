//! # WASM Compatibility and Memory Management Utilities
//!
//! This module provides essential utilities for WebAssembly (WASM) host-guest communication
//! and memory management. These functions handle the low-level details of passing data
//! between the Metashrew runtime (host) and WASM indexer modules (guest), ensuring
//! safe and efficient memory operations across the WASM boundary.
//!
//! ## Core Concepts
//!
//! ### WASM Memory Layout
//! WASM modules use linear memory that must be carefully managed when passing data
//! between host and guest. This module implements the ArrayBuffer layout convention:
//! - **Length prefix**: 4-byte little-endian length at the beginning
//! - **Data payload**: Actual data following the length prefix
//! - **Pointer arithmetic**: Safe conversion between Rust pointers and WASM addresses
//!
//! ### Memory Safety
//! All functions in this module handle the unsafe aspects of WASM memory management:
//! - **Pointer conversion**: Safe casting between Rust and WASM address spaces
//! - **Memory layout**: Consistent data structure layout across boundaries
//! - **Lifetime management**: Proper handling of memory ownership transfer
//!
//! ## Usage Examples
//!
//! ```rust,ignore
//! use metashrew_support::compat::*;
//!
//! // Prepare data for export to WASM
//! let data = vec![1, 2, 3, 4, 5];
//! let wasm_ptr = export_bytes(data);
//!
//! // Convert data to ArrayBuffer layout
//! let buffer = to_arraybuffer_layout(&[1, 2, 3]);
//! // Result: [3, 0, 0, 0, 1, 2, 3] (length + data)
//!
//! // Get pointer for passing to WASM
//! let mut data = vec![1, 2, 3];
//! let ptr = to_ptr(&mut data);
//! ```
//!
//! ## Integration with Metashrew
//!
//! These utilities are fundamental to Metashrew's WASM execution model:
//! - **Host functions**: Passing data from runtime to WASM modules
//! - **Return values**: Getting results back from WASM indexer functions
//! - **Memory management**: Safe handling of dynamic data across boundaries

use metashrew_println::wasm::{to_arraybuffer_layout, to_passback_ptr};

/// Export bytes to WASM memory with ArrayBuffer layout and return pointer.
///
/// This function is the primary interface for returning data from host functions
/// to WASM modules. It converts the input data to ArrayBuffer layout, allocates
/// it in WASM-accessible memory, and returns a pointer that the WASM module
/// can use to access the data.
///
/// # Parameters
/// - `v`: Vector of bytes to export to WASM memory
///
/// # Returns
/// 32-bit integer pointer to the data in WASM memory (pointing past length prefix)
///
/// # Memory Management
/// This function uses `Box::leak()` to transfer ownership of the data to the
/// WASM memory space. The caller (typically the WASM module) becomes responsible
/// for the memory's lifetime. The returned pointer points to the data payload,
/// not the length prefix.
///
/// # Usage Pattern
/// This function is typically used in host function implementations:
/// ```rust,ignore
/// // In a host function that returns data to WASM
/// fn host_function() -> i32 {
///     let result_data = vec![1, 2, 3, 4];
///     export_bytes(result_data) // Returns pointer for WASM
/// }
/// ```
///
/// # Memory Layout
/// The allocated memory has ArrayBuffer layout:
/// - Bytes -4 to -1: Length as little-endian u32
/// - Bytes 0+: Data payload (returned pointer points here)
pub fn export_bytes(v: Vec<u8>) -> i32 {
    let response: Vec<u8> = to_arraybuffer_layout(&v);
    Box::leak(Box::new(response)).as_mut_ptr() as usize as i32 + 4
}

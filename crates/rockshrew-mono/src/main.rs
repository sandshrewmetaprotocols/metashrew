//! # Rockshrew-Mono: Combined Bitcoin Indexer and View Layer
//!
//! ## ARCHITECTURE OVERVIEW
//!
//! This is a monolithic Bitcoin indexer that combines both indexing and view layer functionality
//! into a single binary. It uses an **append-only database architecture** for reliable Bitcoin
//! blockchain indexing with full historical state access.
//!
//! ## APPEND-ONLY DATABASE DESIGN
//!
//! **IMPORTANT**: This system NO LONGER uses BST (Binary Search Tree) indexing. All BST code
//! has been removed as it was a flawed design. We now use a pure append-only approach:
//!
//! ### Key-Value Structure:
//! - `"key/length"`: Total number of updates for a key since indexing began
//! - `"key/0"`, `"key/1"`, `"key/2"`, etc.: Individual update entries
//! - Values stored as: `"height:hex_encoded_value"`
//!
//! ### Benefits:
//! - **Reorg Safety**: No data loss during blockchain reorganizations
//! - **Historical Access**: Binary search through updates for any block height
//! - **Debugging**: Human-readable keys and height-prefixed values
//! - **Consistency**: Deterministic state at any point in blockchain history
//!
//! ## CRATE HIERARCHY & CODE ORGANIZATION
//!
//! Code should be factored to the lowest common denominator in this hierarchy:
//!
//! ```
//! rockshrew-mono
//! ├── rockshrew-sync    (sync framework, adapters)
//! ├── rockshrew-runtime (RocksDB integration)
//! ├── metashrew-runtime (core WASM runtime, append-only SMT)
//! └── metashrew-core    (WASM bindings, fundamental types)
//! ```
//!
//! **Rule**: Always implement behavior in the lowest possible crate to maximize reusability.
//! Most core logic should live in `metashrew-runtime` and `metashrew-core`.

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder().format_timestamp_secs().init();
    let args = rockshrew_mono::Args::parse();
    rockshrew_mono::run_prod(args).await
}

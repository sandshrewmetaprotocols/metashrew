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
//! ### Key-Value Structure (v10 binary format):
//! - `"key/length"`: u32 LE — total number of updates for a key since indexing began
//! - `"key/0"`, `"key/1"`, `"key/2"`, etc.: Individual update entries
//! - Values stored as: `[u32 LE height | raw_value_bytes]` (v10; replaces the
//!   v9 `"{height}:{hex_value}"` UTF-8 string format — saves 50% on disk and
//!   removes per-read string parsing)
//!
//! ### Benefits:
//! - **Reorg Safety**: No data loss during blockchain reorganizations
//! - **Historical Access**: Binary search through updates for any block height
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

/// Initialize logging/tracing based on build features and environment
fn init_tracing() {
    // Check if console debugging is requested via environment variable
    let _use_console = std::env::var("ROCKSHREW_CONSOLE").is_ok();
    
    #[cfg(feature = "console")]
    if use_console {
        console_subscriber::init();
        return;
    }
    
    // Check if JSON tracing is requested
    #[cfg(feature = "debug-tracing")]
    if std::env::var("ROCKSHREW_JSON_TRACING").is_ok() {
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
        
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().json())
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .init();
        return;
    }
    
    // Enhanced tracing for debugging
    if std::env::var("ROCKSHREW_DEBUG").is_ok() {
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
        
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .with_thread_ids(true)
                    .with_line_number(true)
                    .with_file(true)
            )
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .init();
        return;
    }
    
    // Default: use env_logger for backward compatibility
    env_logger::builder().format_timestamp_secs().init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = rockshrew_mono::Args::parse();
    rockshrew_mono::run_prod(args).await
}

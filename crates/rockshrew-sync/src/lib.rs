//! # Generic Bitcoin Indexer Synchronization Framework
//!
//! This crate provides a comprehensive, modular framework for building Bitcoin indexers
//! that can work with different storage backends, Bitcoin node interfaces, and WASM
//! runtime environments. It abstracts the core synchronization logic to enable testing,
//! modularity, and reusability across different Bitcoin indexing applications.
//!
//! ## Core Architecture
//!
//! The framework is built around several key abstractions:
//!
//! ### Adapter Pattern
//! - **[`BitcoinNodeAdapter`]**: Abstracts Bitcoin node communication (RPC, REST, etc.)
//! - **[`StorageAdapter`]**: Abstracts storage backends (RocksDB, PostgreSQL, etc.)
//! - **[`RuntimeAdapter`]**: Abstracts WASM runtime execution environments
//! - **[`JsonRpcProvider`]**: Abstracts JSON-RPC API endpoints for external access
//!
//! ### Synchronization Engine
//! - **[`SyncEngine`]**: Coordinates all components for blockchain synchronization
//! - **Chain reorganization detection**: Automatic handling of blockchain forks
//! - **Atomic block processing**: Ensures data consistency during indexing
//! - **Pipeline processing**: Parallel block processing for improved performance
//!
//! ### Snapshot System
//! - **Snapshot creation**: Generate portable database snapshots
//! - **Snapshot restoration**: Fast bootstrap from existing snapshots
//! - **Incremental sync**: Efficient updates from snapshot points
//!
//! ## Usage Examples
//!
//! ### Basic Synchronization
//! ```rust
//! use rockshrew_sync::*;
//!
//! // Configure synchronization parameters
//! let config = SyncConfig {
//!     start_block: 0,
//!     exit_at: Some(100000),
//!     pipeline_size: Some(10),
//!     max_reorg_depth: 100,
//!     reorg_check_threshold: 6,
//! };
//!
//! // Create adapters for your specific environment
//! let node_adapter = MyBitcoinNodeAdapter::new();
//! let storage_adapter = MyStorageAdapter::new();
//! let runtime_adapter = MyRuntimeAdapter::new();
//!
//! // Initialize and start synchronization
//! let mut sync_engine = MySyncEngine::new(node_adapter, storage_adapter, runtime_adapter);
//! sync_engine.start().await?;
//! ```
//!
//! ### Snapshot Operations
//! ```rust
//! use rockshrew_sync::*;
//!
//! // Create a snapshot at current height
//! let snapshot = create_snapshot(&storage_adapter).await?;
//!
//! // Restore from snapshot
//! restore_from_snapshot(&storage_adapter, &snapshot_data).await?;
//! ```
//!
//! ## Key Features
//!
//! ### Modular Design
//! - **Pluggable components**: Swap implementations without changing core logic
//! - **Testing support**: Mock implementations for unit and integration testing
//! - **Multiple backends**: Support for various storage and node interfaces
//!
//! ### Reliability
//! - **Atomic operations**: All-or-nothing block processing
//! - **Reorg handling**: Automatic detection and recovery from chain reorganizations
//! - **Error recovery**: Graceful handling of network and storage failures
//!
//! ### Performance
//! - **Pipeline processing**: Parallel block fetching and processing
//! - **Efficient storage**: Optimized database operations and batching
//! - **Memory management**: Controlled memory usage for long-running processes
//!
//! ### Observability
//! - **Comprehensive metrics**: Storage, runtime, and sync statistics
//! - **Status monitoring**: Real-time sync progress and health checks
//! - **Error reporting**: Detailed error information for debugging
//!
//! ## Integration with Metashrew
//!
//! This framework serves as the foundation for:
//! - **rockshrew-mono**: Production Bitcoin indexer implementation
//! - **Custom indexers**: Application-specific Bitcoin data processing
//! - **Testing infrastructure**: Reliable testing of indexing logic
//! - **Development tools**: Rapid prototyping of new indexing strategies

pub mod adapters;

#[cfg(any(test, feature = "test-utils"))]
pub mod mock;

#[cfg(any(test, feature = "test-utils"))]
pub mod mock_snapshot;

pub use adapters::*;
pub use metashrew_sync::*;

#[cfg(any(test, feature = "test-utils"))]
pub use mock::*;

#[cfg(any(test, feature = "test-utils"))]
pub use mock_snapshot::*;

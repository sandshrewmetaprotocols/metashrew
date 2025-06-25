//! RocksDB-specific implementation of MetashrewRuntime

pub mod adapter;

// Re-export the adapter and related types
pub use adapter::{query_height, RocksDBBatch, RocksDBRuntimeAdapter};

// Re-export core runtime with RocksDB adapter
pub use metashrew_runtime::{MetashrewRuntime, MetashrewRuntimeContext};

/// Type alias for MetashrewRuntime using RocksDB backend
pub type RocksDBRuntime = MetashrewRuntime<RocksDBRuntimeAdapter>;

/// Type alias for MetashrewRuntimeContext using RocksDB backend
pub type RocksDBRuntimeContext = MetashrewRuntimeContext<RocksDBRuntimeAdapter>;

// Re-export other useful types from metashrew-runtime
pub use metashrew_runtime::{
    get_label, has_label, set_label, to_labeled_key, wait_timeout, BSTHelper, BSTStatistics,
    BatchLike, KVTrackerFn, KeyValueStoreLike,
};

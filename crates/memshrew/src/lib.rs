//! In-memory implementation of MetashrewRuntime for fast testing

pub mod adapter;

// Re-export the adapter and related types
pub use adapter::{MemStoreAdapter, MemStoreBatch};

// Re-export core runtime with MemStore adapter
pub use metashrew_runtime::{MetashrewRuntime, MetashrewRuntimeContext};

/// Type alias for MetashrewRuntime using in-memory backend
pub type MemStoreRuntime = MetashrewRuntime<MemStoreAdapter>;

/// Type alias for MetashrewRuntimeContext using in-memory backend
pub type MemStoreRuntimeContext = MetashrewRuntimeContext<MemStoreAdapter>;

// Re-export other useful types from metashrew-runtime
pub use metashrew_runtime::{
    BatchLike, KeyValueStoreLike, KVTrackerFn,
    BSTHelper, BSTStatistics,
    OptimizedBST, OptimizedBSTStatistics,
    set_label, get_label, has_label, to_labeled_key, wait_timeout
};

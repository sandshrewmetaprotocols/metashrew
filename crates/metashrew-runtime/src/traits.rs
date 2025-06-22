//! Core traits for generic key-value storage backends

use anyhow::Result;

/// Trait for batch operations on key-value stores
pub trait BatchLike {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V);
    fn delete<K: AsRef<[u8]>>(&mut self, key: K);
    fn default() -> Self;
}

/// Generic trait for key-value storage backends
/// This allows the runtime to work with different storage implementations
/// like RocksDB, in-memory stores, or other databases
pub trait KeyValueStoreLike {
    type Error: std::fmt::Debug + Send + Sync + std::error::Error + 'static;
    type Batch: BatchLike;
    
    /// Write a batch of operations to the store
    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error>;
    
    /// Get a value by key
    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error>;
    
    /// Get a value by key (immutable version for read-only operations)
    fn get_immutable<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error>;
    
    /// Put a single key-value pair
    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>;
    
    /// Delete a key
    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error>;
    
    /// Scan keys with a given prefix, returning key-value pairs
    fn scan_prefix<K: AsRef<[u8]>>(&self, prefix: K) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error>;
    
    /// Scan keys with a given prefix (mutable version for compatibility)
    fn scan_prefix_mut<K: AsRef<[u8]>>(&mut self, prefix: K) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error> {
        self.scan_prefix(prefix)
    }
    
    /// Create a new batch for atomic operations
    fn create_batch(&self) -> Self::Batch;
    
    /// Write a batch atomically (alias for write method)
    fn write_batch(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        self.write(batch)
    }
    
    /// Get an iterator over all keys
    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error>;
    
    /// Optional method to track key-value updates for snapshots or other purposes
    /// Default implementation does nothing
    fn track_kv_update(&mut self, _key: Vec<u8>, _value: Vec<u8>) {
        // Default implementation does nothing
    }
    
    /// Check if the store is open/connected
    fn is_open(&self) -> bool {
        true // Default implementation assumes always open
    }
    
    /// Set the current height for height-aware operations
    fn set_height(&mut self, _height: u32) {
        // Default implementation does nothing
    }
    
    /// Get the current height
    fn get_height(&self) -> u32 {
        0 // Default implementation returns 0
    }
    
    /// Create an isolated copy for preview operations (default implementation just clones)
    fn create_isolated_copy(&self) -> Self where Self: Clone {
        self.clone()
    }
}

/// Type definition for key-value tracker function
pub type KVTrackerFn = Box<dyn Fn(Vec<u8>, Vec<u8>) + Send + Sync>;
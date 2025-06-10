//! RocksDB adapter for Metashrew runtime
//!
//! This crate provides a RocksDB adapter for the Metashrew runtime.

use runtime::{KeyValueStoreLike, BatchLike};
use std::sync::Arc;
use rocksdb::{DB, Options, WriteBatch};

mod merkleized_adapter;
pub use merkleized_adapter::{
    MerkleizedRocksDBRuntimeAdapter,
    MerkleizedRocksDBBatch,
    SparseMerkleTree,
    Hash,
};

/// RocksDB adapter for Metashrew runtime
#[derive(Clone)]
pub struct RocksDBRuntimeAdapter {
    /// The underlying RocksDB instance
    pub db: Arc<DB>,
    /// Current block height
    pub height: u32,
}

/// Batch operations for RocksDB
pub struct RocksDBBatch {
    /// The underlying RocksDB batch
    batch: WriteBatch,
}

impl RocksDBRuntimeAdapter {
    /// Create a new RocksDB adapter
    pub fn new(db_path: &str) -> Result<Self, rocksdb::Error> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_max_open_files(10000);
        opts.set_use_fsync(false);
        opts.set_bytes_per_sync(8388608); // 8MB
        opts.optimize_for_point_lookup(1024);
        opts.set_table_cache_num_shard_bits(6);
        opts.set_max_write_buffer_number(6);
        opts.set_write_buffer_size(256 * 1024 * 1024);
        opts.set_target_file_size_base(256 * 1024 * 1024);
        opts.set_min_write_buffer_number_to_merge(2);
        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_level_zero_slowdown_writes_trigger(20);
        opts.set_level_zero_stop_writes_trigger(30);
        opts.set_max_background_jobs(4);
        opts.set_max_background_compactions(4);
        opts.set_disable_auto_compactions(false);
        
        let db = DB::open(&opts, db_path)?;
        
        Ok(Self {
            db: Arc::new(db),
            height: 0,
        })
    }
    
    /// Create a new RocksDB adapter with custom options
    pub fn with_options(db_path: &str, opts: Options) -> Result<Self, rocksdb::Error> {
        let db = DB::open(&opts, db_path)?;
        
        Ok(Self {
            db: Arc::new(db),
            height: 0,
        })
    }
    
    /// Open a secondary instance of RocksDB
    pub fn open_secondary(
        primary_path: String,
        secondary_path: String, 
        opts: Options
    ) -> Result<Self, rocksdb::Error> {
        let db = DB::open_as_secondary(&opts, &primary_path, &secondary_path)?;
        
        Ok(Self {
            db: Arc::new(db),
            height: 0,
        })
    }
    
    /// Create an iterator over key-value pairs with a prefix
    pub fn prefix_iterator(&self, prefix: &[u8]) -> impl Iterator<Item = (Vec<u8>, Vec<u8>)> {
        let prefix = prefix.to_vec();
        let iter = self.db.prefix_iterator(&prefix);
        
        iter.map(|result| {
            match result {
                Ok((key, value)) => (key.to_vec(), value.to_vec()),
                Err(_) => (Vec::new(), Vec::new()),
            }
        })
        .filter(|(key, value)| !key.is_empty() && !value.is_empty())
    }
}

impl KeyValueStoreLike for RocksDBRuntimeAdapter {
    type Batch = RocksDBBatch;
    type Error = rocksdb::Error;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        self.db.write(batch.batch)
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        self.db.get(key)
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.db.delete(key)
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>
    {
        self.db.put(key, value)
    }
}

impl RocksDBBatch {
    /// Create a new batch
    pub fn new() -> Self {
        Self {
            batch: WriteBatch::default(),
        }
    }
}

impl BatchLike for RocksDBBatch {
    fn insert<K, V>(&mut self, key: K, value: V)
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.batch.put(key, value);
    }

    fn remove<K>(&mut self, key: K)
    where
        K: AsRef<[u8]>,
    {
        self.batch.delete(key);
    }
}

impl Default for RocksDBBatch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_rocksdb_adapter() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().to_str().unwrap();
        
        // Create a new adapter
        let mut adapter = RocksDBRuntimeAdapter::new(db_path).unwrap();
        
        // Insert a key-value pair
        adapter.put(b"test_key", b"test_value").unwrap();
        
        // Get the value back
        let value = adapter.get(b"test_key").unwrap().unwrap();
        assert_eq!(value, b"test_value");
        
        // Delete the key
        adapter.delete(b"test_key").unwrap();
        
        // Value should be gone
        assert!(adapter.get(b"test_key").unwrap().is_none());
    }
    
    #[test]
    fn test_rocksdb_batch() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().to_str().unwrap();
        
        // Create a new adapter
        let mut adapter = RocksDBRuntimeAdapter::new(db_path).unwrap();
        
        // Create a batch
        let mut batch = RocksDBBatch::new();
        
        // Add some operations
        batch.insert(b"key1", b"value1");
        batch.insert(b"key2", b"value2");
        batch.insert(b"key3", b"value3");
        
        // Write the batch
        adapter.write(batch).unwrap();
        
        // Check that all keys were written
        assert_eq!(adapter.get(b"key1").unwrap().unwrap(), b"value1");
        assert_eq!(adapter.get(b"key2").unwrap().unwrap(), b"value2");
        assert_eq!(adapter.get(b"key3").unwrap().unwrap(), b"value3");
    }
}

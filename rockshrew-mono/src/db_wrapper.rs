use anyhow::Result;
use rocksdb::{DB, WriteBatch};
use std::sync::{Arc, Mutex};

/// A wrapper for tracking database changes
pub struct DBChangeTracker {
    pub db: Arc<DB>,
    pub modified_keys: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl DBChangeTracker {
    pub fn new(db: Arc<DB>) -> Self {
        Self {
            db,
            modified_keys: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) -> Result<(), rocksdb::Error> {
        // Track the key
        if let Ok(mut keys) = self.modified_keys.lock() {
            keys.push(key.as_ref().to_vec());
        }
        
        // Perform the actual put operation
        self.db.put(key, value)
    }

    pub fn get_modified_keys(&self) -> Vec<Vec<u8>> {
        match self.modified_keys.lock() {
            Ok(keys) => keys.clone(),
            Err(_) => Vec::new(),
        }
    }

    pub fn write(&self, batch: WriteBatch) -> Result<(), rocksdb::Error> {
        // Note: This doesn't track keys in the batch
        // For a complete implementation, you would need to iterate through the batch
        self.db.write(batch)
    }
}
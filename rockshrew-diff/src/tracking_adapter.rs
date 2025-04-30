use metashrew_runtime::KeyValueStoreLike;
use rockshrew_runtime::RocksDBRuntimeAdapter;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// A wrapper around RocksDBRuntimeAdapter that tracks key-value updates
/// for specific prefixes during flush operations
pub struct TrackingAdapter {
    /// The underlying RocksDB adapter
    pub inner: RocksDBRuntimeAdapter,
    /// The prefix to track
    pub prefix: Vec<u8>,
    /// Tracked keys for the current block
    pub tracked_keys: Arc<Mutex<HashSet<Vec<u8>>>>,
}

/// Batch implementation for TrackingAdapter
pub struct TrackingBatch {
    /// The underlying RocksDB batch
    pub inner: <RocksDBRuntimeAdapter as KeyValueStoreLike>::Batch,
    /// Keys being written in this batch
    pub keys: HashMap<Vec<u8>, Vec<u8>>,
    /// The prefix to track
    pub prefix: Vec<u8>,
    /// Reference to the tracked keys set
    pub tracked_keys: Arc<Mutex<HashSet<Vec<u8>>>>,
}

impl metashrew_runtime::BatchLike for TrackingBatch {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        // Track the key if it starts with our prefix
        let key_bytes = key.as_ref().to_vec();
        if key_bytes.starts_with(&self.prefix) {
            // Add to tracked keys
            if let Ok(mut tracked_keys) = self.tracked_keys.lock() {
                tracked_keys.insert(key_bytes.clone());
            }
        }
        
        // Store in our local map for debugging/inspection
        self.keys.insert(key_bytes, value.as_ref().to_vec());
        
        // Forward to the inner batch
        self.inner.put(key, value);
    }

    fn default() -> Self {
        // This should never be called directly, use new() instead
        panic!("TrackingBatch::default() should not be called directly");
    }
}

impl TrackingBatch {
    pub fn new(inner: <RocksDBRuntimeAdapter as KeyValueStoreLike>::Batch, prefix: Vec<u8>, tracked_keys: Arc<Mutex<HashSet<Vec<u8>>>>) -> Self {
        Self {
            inner,
            keys: HashMap::new(),
            prefix,
            tracked_keys,
        }
    }
}

impl Clone for TrackingAdapter {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            prefix: self.prefix.clone(),
            tracked_keys: self.tracked_keys.clone(),
        }
    }
}

impl KeyValueStoreLike for TrackingAdapter {
    type Error = <RocksDBRuntimeAdapter as KeyValueStoreLike>::Error;
    type Batch = TrackingBatch;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        // Forward to the inner adapter
        self.inner.write(batch.inner)
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        // Forward to the inner adapter
        self.inner.get(key)
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        // Forward to the inner adapter
        self.inner.delete(key)
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        // Track the key if it starts with our prefix
        let key_bytes = key.as_ref().to_vec();
        if key_bytes.starts_with(&self.prefix) {
            // Add to tracked keys
            if let Ok(mut tracked_keys) = self.tracked_keys.lock() {
                tracked_keys.insert(key_bytes);
            }
        }
        
        // Forward to the inner adapter
        self.inner.put(key, value)
    }
}

impl TrackingAdapter {
    pub fn new(inner: RocksDBRuntimeAdapter, prefix: Vec<u8>) -> Self {
        Self {
            inner,
            prefix,
            tracked_keys: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn get_tracked_keys(&self) -> HashSet<Vec<u8>> {
        match self.tracked_keys.lock() {
            Ok(tracked_keys) => tracked_keys.clone(),
            Err(_) => HashSet::new(),
        }
    }

    pub fn clear_tracked_keys(&self) {
        if let Ok(mut tracked_keys) = self.tracked_keys.lock() {
            tracked_keys.clear();
        }
    }

    pub fn set_height(&mut self, height: u32) {
        self.inner.set_height(height);
    }
}
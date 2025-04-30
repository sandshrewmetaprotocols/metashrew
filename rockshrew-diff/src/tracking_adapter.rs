use metashrew_runtime::{KeyValueStoreLike, BatchLike};
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

/// Enhanced batch implementation that tracks keys being written
pub struct TrackingBatch {
    /// The underlying RocksDB batch
    inner: <RocksDBRuntimeAdapter as KeyValueStoreLike>::Batch,
    /// Keys being written in this batch
    keys: HashMap<Vec<u8>, Vec<u8>>,
}

impl BatchLike for TrackingBatch {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        // Store the key for later tracking
        let key_bytes = key.as_ref().to_vec();
        self.keys.insert(key_bytes, value.as_ref().to_vec());
        
        // Forward to the inner batch
        self.inner.put(key, value);
    }

    fn default() -> Self {
        Self {
            inner: <RocksDBRuntimeAdapter as KeyValueStoreLike>::Batch::default(),
            keys: HashMap::new(),
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
        // Track keys from the batch that match our prefix
        for (key, _) in batch.keys.iter() {
            if key.starts_with(&self.prefix) {
                if let Ok(mut tracked_keys) = self.tracked_keys.lock() {
                    tracked_keys.insert(key.clone());
                }
            }
        }
        
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
        // Forward to the inner adapter - no tracking here as it's not used by __flush
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
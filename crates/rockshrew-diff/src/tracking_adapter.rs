use metashrew_runtime::{BatchLike, KeyValueStoreLike, RocksDBRuntimeAdapter};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A wrapper around RocksDBRuntimeAdapter that tracks key-value updates
/// for specific prefixes during flush operations
pub struct TrackingAdapter {
    /// The underlying RocksDB adapter
    pub inner: RocksDBRuntimeAdapter,
    /// The prefix to track
    pub prefix: Vec<u8>,
    /// Tracked updates (key-value pairs) for the current block
    pub tracked_updates: Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
}

/// Enhanced batch implementation that tracks key-value pairs being written
pub struct TrackingBatch {
    /// The underlying RocksDB batch
    inner: <RocksDBRuntimeAdapter as KeyValueStoreLike>::Batch,
    /// Key-value pairs being written in this batch
    updates: HashMap<Vec<u8>, Vec<u8>>,
}

/// Process a Metashrew key by removing the last 4 bytes (version/list suffix)
fn process_metashrew_key(key: &[u8]) -> Vec<u8> {
    if key.len() >= 4 {
        key[..key.len() - 4].to_vec()
    } else {
        key.to_vec()
    }
}

/// Process a Metashrew value by removing the last 4 bytes (block height)
fn process_metashrew_value(value: &[u8]) -> Vec<u8> {
    if value.len() >= 4 {
        value[..value.len() - 4].to_vec()
    } else {
        value.to_vec()
    }
}

impl BatchLike for TrackingBatch {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        // Store the key-value pair for later tracking
        let key_bytes = key.as_ref().to_vec();
        let value_bytes = value.as_ref().to_vec();
        self.updates.insert(key_bytes.clone(), value_bytes);

        // Forward to the inner batch
        self.inner.put(key, value);
    }

    fn default() -> Self {
        Self {
            inner: <RocksDBRuntimeAdapter as KeyValueStoreLike>::Batch::default(),
            updates: HashMap::new(),
        }
    }
}

impl Clone for TrackingAdapter {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            prefix: self.prefix.clone(),
            tracked_updates: self.tracked_updates.clone(),
        }
    }
}

impl KeyValueStoreLike for TrackingAdapter {
    type Error = <RocksDBRuntimeAdapter as KeyValueStoreLike>::Error;
    type Batch = TrackingBatch;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        // Track key-value pairs from the batch that match our prefix
        for (key, value) in batch.updates.iter() {
            if key.starts_with(&self.prefix) {
                if let Ok(mut tracked_updates) = self.tracked_updates.lock() {
                    // Process the key and value according to Metashrew's format
                    let processed_key = process_metashrew_key(key);
                    let processed_value = process_metashrew_value(value);

                    // Only track if it's not a size key (ending with ffffffff)
                    if key.len() >= 4 && &key[key.len() - 4..] != &[0xff, 0xff, 0xff, 0xff] {
                        tracked_updates.insert(processed_key, processed_value);
                    }
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
    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
        self.inner.keys()
    }
}

impl TrackingAdapter {
    pub fn new(inner: RocksDBRuntimeAdapter, prefix: Vec<u8>) -> Self {
        Self {
            inner,
            prefix,
            tracked_updates: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get_tracked_updates(&self) -> HashMap<Vec<u8>, Vec<u8>> {
        match self.tracked_updates.lock() {
            Ok(tracked_updates) => tracked_updates.clone(),
            Err(_) => HashMap::new(),
        }
    }

    pub fn clear_tracked_updates(&self) {
        if let Ok(mut tracked_updates) = self.tracked_updates.lock() {
            tracked_updates.clear();
        }
    }

    pub fn set_height(&mut self, height: u32) {
        self.inner.set_height(height);
    }
}

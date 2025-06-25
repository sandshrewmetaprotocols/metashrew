use metashrew_runtime::{BatchLike, KeyValueStoreLike};
use rockshrew_runtime::RocksDBRuntimeAdapter;
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

/// Process a Metashrew key for the new minimal append-only format
/// Keys are now human-readable strings like "key/0", "key/1", "key/length"
fn process_metashrew_key(key: &[u8]) -> Vec<u8> {
    let key_str = String::from_utf8_lossy(key);
    
    // For the new format, extract the base key from patterns like "key/0", "key/1", etc.
    if let Some(slash_pos) = key_str.rfind('/') {
        let base_key = &key_str[..slash_pos];
        // Skip length keys and numeric indices, return the base key
        if !base_key.is_empty() && !key_str.ends_with("/length") {
            return base_key.as_bytes().to_vec();
        }
    }
    
    // For other keys (like SMT roots), return as-is
    key.to_vec()
}

/// Process a Metashrew value for the new minimal append-only format
/// Values are now in format "height:hex_value"
fn process_metashrew_value(value: &[u8]) -> Vec<u8> {
    let value_str = String::from_utf8_lossy(value);
    
    // For the new format, extract the hex value from "height:hex_value"
    if let Some(colon_pos) = value_str.find(':') {
        let hex_value = &value_str[colon_pos + 1..];
        if let Ok(decoded) = hex::decode(hex_value) {
            return decoded;
        }
    }
    
    // For other values (like SMT roots), return as-is
    value.to_vec()
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

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) {
        // Forward to the inner batch
        self.inner.delete(key);
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
                    let key_str = String::from_utf8_lossy(key);
                    
                    // For the new minimal append-only format, only track actual key-value updates
                    // Skip length keys and SMT nodes to avoid tracking metadata
                    if !key_str.ends_with("/length") && !key_str.starts_with("smt:node:") {
                        // Process the key and value according to the new format
                        let processed_key = process_metashrew_key(key);
                        let processed_value = process_metashrew_value(value);

                        // Only track non-empty processed keys
                        if !processed_key.is_empty() {
                            tracked_updates.insert(processed_key, processed_value);
                        }
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

    fn get_immutable<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        // Forward to the inner adapter
        self.inner.get_immutable(key)
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

    fn scan_prefix<K: AsRef<[u8]>>(&self, prefix: K) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error> {
        // Forward to the inner adapter
        self.inner.scan_prefix(prefix)
    }

    fn create_batch(&self) -> Self::Batch {
        TrackingBatch {
            inner: self.inner.create_batch(),
            updates: HashMap::new(),
        }
    }

    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
        self.inner.keys()
    }

    fn is_open(&self) -> bool {
        self.inner.is_open()
    }

    fn set_height(&mut self, height: u32) {
        self.inner.set_height(height);
    }

    fn get_height(&self) -> u32 {
        self.inner.get_height()
    }

    fn track_kv_update(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.inner.track_kv_update(key, value);
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

//! RocksDB implementation of KeyValueStoreLike trait

use anyhow::Result;
use rocksdb::{DB, Options, WriteBatch, WriteBatchIterator};
use std::sync::{Arc, Mutex};
use metashrew_runtime::{BatchLike, KeyValueStoreLike, KVTrackerFn, to_labeled_key, TIP_HEIGHT_KEY};

#[derive(Clone)]
pub struct RocksDBRuntimeAdapter {
    pub db: Arc<DB>,
    pub height: u32,
    pub kv_tracker: Arc<Mutex<Option<KVTrackerFn>>>,
}

impl RocksDBRuntimeAdapter {
    pub fn open_secondary(
        primary_path: String,
        secondary_path: String,
        opts: rocksdb::Options
    ) -> Result<Self, rocksdb::Error> {
        let db = rocksdb::DB::open_as_secondary(&opts, &primary_path, &secondary_path)?;
        Ok(RocksDBRuntimeAdapter {
            db: Arc::new(db),
            height: 0,
            kv_tracker: Arc::new(Mutex::new(None)),
        })
    }
    
    pub fn open(path: String, opts: Options) -> Result<RocksDBRuntimeAdapter> {
        let db = DB::open(&opts, path)?;
        Ok(RocksDBRuntimeAdapter {
            db: Arc::new(db),
            height: 0,
            kv_tracker: Arc::new(Mutex::new(None)),
        })
    }
    
    /// Create a new adapter from an existing DB handle
    pub fn from_db(db: Arc<DB>) -> Self {
        RocksDBRuntimeAdapter {
            db,
            height: 0,
            kv_tracker: Arc::new(Mutex::new(None)),
        }
    }

    /// Set a key-value tracker function that will be called for each key-value update
    pub fn set_kv_tracker(&mut self, tracker: Option<KVTrackerFn>) {
        if let Ok(mut guard) = self.kv_tracker.lock() {
            *guard = tracker;
        }
    }
    
    /// Track a key-value update using the registered tracker function
    pub fn track_kv_update_internal(&self, key: Vec<u8>, value: Vec<u8>) {
        if let Ok(guard) = self.kv_tracker.lock() {
            if let Some(tracker) = &*guard {
                tracker(key, value);
            }
        }
    }
}

pub struct RocksDBBatch(pub WriteBatch);

impl BatchLike for RocksDBBatch {
    fn default() -> Self {
        Self(WriteBatch::default())
    }
    
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, k: K, v: V) {
        self.0.put(to_labeled_key(&k.as_ref().to_vec()), v);
    }
    
    fn delete<K: AsRef<[u8]>>(&mut self, k: K) {
        self.0.delete(to_labeled_key(&k.as_ref().to_vec()));
    }
}

// BatchTracker captures key-value pairs during batch operations for tracking
pub struct BatchTracker<'a> {
    inner_batch: &'a mut WriteBatch,
    kv_tracker: Arc<Mutex<Option<KVTrackerFn>>>,
}

impl<'a> WriteBatchIterator for BatchTracker<'a> {
    fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
        // Track the key-value update if a tracker is registered
        if let Ok(guard) = self.kv_tracker.lock() {
            if let Some(tracker) = &*guard {
                // Clone the key and value for tracking
                let key_vec = key.to_vec();
                let value_vec = value.to_vec();
                tracker(key_vec, value_vec);
            }
        }
        
        // Forward to the inner batch
        self.inner_batch.put(key.as_ref(), value.as_ref());
    }
    
    fn delete(&mut self, key: Box<[u8]>) {
        // Forward to the inner batch
        self.inner_batch.delete(key.as_ref());
    }
}

impl KeyValueStoreLike for RocksDBRuntimeAdapter {
    type Batch = RocksDBBatch;
    type Error = rocksdb::Error;
    
    fn track_kv_update(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.track_kv_update_internal(key, value);
    }

    fn write(&mut self, batch: RocksDBBatch) -> Result<(), Self::Error> {
        let key_bytes: Vec<u8> = TIP_HEIGHT_KEY.as_bytes().to_vec();
        let height_bytes: Vec<u8> = (self.height + 1).to_le_bytes().to_vec();
        
        let mut final_batch = WriteBatch::default();
        final_batch.put(&to_labeled_key(&key_bytes), &height_bytes);
        
        // Create a batch tracker to capture key-value pairs for tracking
        let kv_tracker_clone = self.kv_tracker.clone();
        let mut batch_tracker = BatchTracker {
            inner_batch: &mut final_batch,
            kv_tracker: kv_tracker_clone,
        };
        
        // Use the batch tracker to capture key-value pairs
        batch.0.iterate(&mut batch_tracker);
        
        self.db.write(final_batch)
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        self.db.get(to_labeled_key(&key.as_ref().to_vec())).map(|opt| opt.map(|v| v.to_vec()))
    }
    
    fn get_immutable<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        self.db.get(to_labeled_key(&key.as_ref().to_vec())).map(|opt| opt.map(|v| v.to_vec()))
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.db.delete(to_labeled_key(&key.as_ref().to_vec()))
    }

    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) -> Result<(), Self::Error> {
        let key_vec = key.as_ref().to_vec();
        let value_vec = value.as_ref().to_vec();
        
        // Track the key-value update if a tracker is registered
        self.track_kv_update(key_vec.clone(), value_vec.clone());
        
        // Perform the actual database update
        self.db.put(to_labeled_key(&key_vec), value_vec)
    }

    fn scan_prefix<K: AsRef<[u8]>>(&self, prefix: K) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error> {
        let mut results = Vec::new();
        let prefix_bytes = to_labeled_key(&prefix.as_ref().to_vec());
        
        let mut iter = self.db.raw_iterator();
        iter.seek(&prefix_bytes);
        
        while iter.valid() {
            if let Some(key) = iter.key() {
                if !key.starts_with(&prefix_bytes) {
                    break;
                }
                if let Some(value) = iter.value() {
                    results.push((key.to_vec(), value.to_vec()));
                }
            }
            iter.next();
        }
        
        Ok(results)
    }
    
    fn create_batch(&self) -> Self::Batch {
        RocksDBBatch::default()
    }

    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        Ok(Box::new(iter.map(|item| {
            let (key, _) = item.unwrap();
            key.to_vec()
        })))
    }

    fn is_open(&self) -> bool {
        true // RocksDB doesn't need connection management like Redis
    }
    
    fn set_height(&mut self, height: u32) {
        self.height = height;
    }
    
    fn get_height(&self) -> u32 {
        self.height
    }
}

/// Query height from RocksDB
pub async fn query_height(db: Arc<DB>, start_block: u32) -> Result<u32> {
    let height_key = TIP_HEIGHT_KEY.as_bytes().to_vec();
    let bytes = match db.get(&to_labeled_key(&height_key))? {
        Some(v) => v,
        None => {
            return Ok(start_block);
        }
    };
    if bytes.len() == 0 {
        return Ok(start_block);
    }
    let bytes_ref: &[u8] = &bytes;
    Ok(u32::from_le_bytes(bytes_ref.try_into().unwrap()))
}
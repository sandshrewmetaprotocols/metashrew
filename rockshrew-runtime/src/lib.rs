use anyhow::Result;
use metashrew_runtime::{TIP_HEIGHT_KEY, BatchLike, KeyValueStoreLike};
use rocksdb::{DB, Options, WriteBatch, WriteBatchIterator};
use std::sync::{Arc, Mutex};
pub mod smt;
pub mod runtime;
pub mod helpers;
pub mod optimized_bst;

// Re-export important types from the runtime module
pub use runtime::{MetashrewRuntime, MetashrewRuntimeContext};

// Re-export helper types
pub use helpers::{BSTHelper, BSTStatistics};
pub use optimized_bst::{OptimizedBST, OptimizedBSTStatistics};


// Type definition for key-value tracker function
pub type KVTrackerFn = Box<dyn Fn(Vec<u8>, Vec<u8>) + Send + Sync>;

#[derive(Clone)]
pub struct RocksDBRuntimeAdapter {
    pub db: Arc<DB>,
    pub height: u32,
    pub kv_tracker: Arc<Mutex<Option<KVTrackerFn>>>,
}

static mut _LABEL: Option<String> = None;

const TIMEOUT: u64 = 1500;

use std::{thread, time};

pub fn wait_timeout() {
    thread::sleep(time::Duration::from_millis(TIMEOUT));
}

pub fn set_label(s: String) -> () {
    unsafe {
        _LABEL = Some(s + "://");
    }
}

#[allow(static_mut_refs)]
pub fn get_label() -> &'static String {
    unsafe { _LABEL.as_ref().unwrap() }
}

#[allow(static_mut_refs)]
pub fn has_label() -> bool {
    unsafe { _LABEL.is_some() }
}

pub fn to_labeled_key(key: &Vec<u8>) -> Vec<u8> {
    if has_label() {
        let mut result: Vec<u8> = vec![];
        result.extend(get_label().as_str().as_bytes());
        result.extend(key);
        result
    } else {
        key.clone()
    }
}

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

    pub fn is_open(&self) -> bool {
        true // RocksDB doesn't need connection management like Redis
    }
    pub fn set_height(&mut self, height: u32) {
        self.height = height;
    }

    pub fn clone(&self) -> Self {
        RocksDBRuntimeAdapter {
            db: self.db.clone(),
            height: self.height,
            kv_tracker: self.kv_tracker.clone(),
        }
    }
    
    /// Set a key-value tracker function that will be called for each key-value update
    pub fn set_kv_tracker(&mut self, tracker: Option<KVTrackerFn>) {
        if let Ok(mut guard) = self.kv_tracker.lock() {
            *guard = tracker;
        }
    }
    
    /// Track a key-value update using the registered tracker function
    pub fn track_kv_update(&self, key: Vec<u8>, value: Vec<u8>) {
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

// Legacy batch cloner for compatibility
pub struct RocksDBBatchCloner<'a>(&'a mut WriteBatch);

impl<'a> WriteBatchIterator for RocksDBBatchCloner<'a> {
  fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
    self.0.put(key.as_ref(), value.as_ref());
  }
  fn delete(&mut self, _key: Box<[u8]>) {
    //no-op
  }
}

impl KeyValueStoreLike for RocksDBRuntimeAdapter {
    type Batch = RocksDBBatch;
    type Error = rocksdb::Error;
    
    // Implement the track_kv_update method from the KeyValueStoreLike trait
    fn track_kv_update(&mut self, key: Vec<u8>, value: Vec<u8>) {
        // Use the existing implementation
        RocksDBRuntimeAdapter::track_kv_update(self, key, value);
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

    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        Ok(Box::new(iter.map(|item| {
            let (key, _) = item.unwrap();
            key.to_vec()
        })))
    }
}

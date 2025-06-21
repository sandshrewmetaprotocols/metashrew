use anyhow::{anyhow, Result};
use metashrew_runtime::{BatchLike, KeyValueStoreLike};
use rockshrew_smt::{BSTOperations, SMTOperations, StateManager};
use rocksdb::{DB, Options, WriteBatch, WriteBatchIterator};
use std::sync::{Arc, Mutex};
use sha2::Digest;

const TIP_HEIGHT_KEY: &'static str = "/__INTERNAL/tip-height";

// Constants for SMT and BST operations
const SMT_ROOT_PREFIX: &str = "smt:root:";
const BST_KEY_PREFIX: &str = "bst:";
const BST_HEIGHT_INDEX_PREFIX: &str = "bst:height:";
const EMPTY_NODE_HASH: [u8; 32] = [0; 32];

// Type definition for key-value tracker function
pub type KVTrackerFn = Box<dyn Fn(Vec<u8>, Vec<u8>) + Send + Sync>;

#[derive(Clone)]
pub struct RocksDBAdapter {
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

impl RocksDBAdapter {
    pub fn open_secondary(
        primary_path: String,
        secondary_path: String,
        opts: rocksdb::Options
    ) -> Result<Self, rocksdb::Error> {
        let db = rocksdb::DB::open_as_secondary(&opts, &primary_path, &secondary_path)?;
        Ok(RocksDBAdapter {
            db: Arc::new(db),
            height: 0,
            kv_tracker: Arc::new(Mutex::new(None)),
        })
    }
    
    pub fn open(path: String, opts: Options) -> Result<RocksDBAdapter> {
        let db = DB::open(&opts, path)?;
        Ok(RocksDBAdapter {
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
        RocksDBAdapter {
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

impl KeyValueStoreLike for RocksDBAdapter {
    type Batch = RocksDBBatch;
    type Error = rocksdb::Error;
    
    // Implement the track_kv_update method from the KeyValueStoreLike trait
    fn track_kv_update(&mut self, key: Vec<u8>, value: Vec<u8>) {
        // Use the existing implementation
        RocksDBAdapter::track_kv_update(self, key, value);
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

// Implementation of SMT operations for RocksDB
impl SMTOperations for RocksDBAdapter {
    fn get_smt_root_at_height(&self, height: u32) -> Result<[u8; 32]> {
        // Try direct lookup first with double colon format
        let double_key = format!("{}::{}", SMT_ROOT_PREFIX, height).into_bytes();
        log::debug!("Looking up state root for height {}", height);
        
        if let Ok(Some(root_data)) = self.db.get(&double_key) {
            log::debug!("Found state root for height {}", height);
            if root_data.len() == 32 {
                let mut root = [0u8; 32];
                root.copy_from_slice(&root_data);
                return Ok(root);
            }
        }
        
        // Try with triple colon format
        let triple_key = format!("{}:::{}", SMT_ROOT_PREFIX, height).into_bytes();
        log::debug!("Checking alternative format for state root");
        
        if let Ok(Some(root_data)) = self.db.get(&triple_key) {
            log::debug!("Found state root using alternative format for height {}", height);
            if root_data.len() == 32 {
                let mut root = [0u8; 32];
                root.copy_from_slice(&root_data);
                return Ok(root);
            }
        }
        
        // If direct lookup fails, fall back to binary search
        if height == 0 {
            log::debug!("Height is 0, returning empty state root");
            return Ok(EMPTY_NODE_HASH);
        }
        
        log::debug!("Direct lookup failed, searching for nearest state root for height {}", height);
        
        let mut low = 0;
        let mut high = height;
        let mut best_match = 0;
        let mut found = false;
        
        // Binary search for the closest height
        while low <= high {
            let mid = low + (high - low) / 2;
            if mid == 0 {
                // Skip height 0
                low = 1;
                continue;
            }
            
            let root_key = format!("{}::{}", SMT_ROOT_PREFIX, mid).into_bytes();
            let get_result = self.db.get(&root_key);
            if let Ok(Some(_)) = get_result {
                // Found a valid height, but continue searching for a closer one
                found = true;
                best_match = mid;
                
                if mid == height {
                    // Exact match found
                    break;
                } else if mid < height {
                    // Look for a closer match in the upper half
                    low = mid + 1;
                } else {
                    // This shouldn't happen in our search pattern, but just in case
                    high = mid - 1;
                }
            } else {
                // No root at this height, check lower heights
                high = mid - 1;
            }
        }
        
        if found {
            // Retrieve the actual root data
            let root_key = format!("{}::{}", SMT_ROOT_PREFIX, best_match).into_bytes();
            if let Ok(Some(root_data)) = self.db.get(&root_key) {
                log::debug!("Found nearest state root at height {} for requested height {}", best_match, height);
                if root_data.len() == 32 {
                    let mut root = [0u8; 32];
                    root.copy_from_slice(&root_data);
                    return Ok(root);
                }
            }
        }
        
        // If no root found, return the default (empty) root
        log::debug!("No state root found for height {}, returning empty state root", height);
        Ok(EMPTY_NODE_HASH)
    }
    
    fn calculate_and_store_state_root(&self, height: u32) -> Result<[u8; 32]> {
        // Get the previous root
        let prev_height = if height > 0 { height - 1 } else { 0 };
        let prev_root = self.get_smt_root_at_height(prev_height)?;
        
        // Get all keys updated at this height
        let updated_keys = self.list_keys_at_height(height)?;
        
        // Calculate the new root based on the previous root and updated keys
        let mut hasher = sha2::Sha256::new();
        hasher.update(&prev_root);
        hasher.update(&height.to_le_bytes());
        
        // Add each updated key and its value to the hash
        for key in &updated_keys {
            hasher.update(key);
            if let Ok(Some(value)) = self.bst_get_at_height(key, height) {
                hasher.update(&value);
            }
        }
        
        // Finalize the new root
        let mut new_root = [0u8; 32];
        new_root.copy_from_slice(&hasher.finalize());
        
        // Store the new root
        let root_key = format!("{}::{}", SMT_ROOT_PREFIX, height).into_bytes();
        if let Err(e) = self.db.put(&root_key, &new_root) {
            log::error!("Failed to store SMT root for height {}: {}", height, e);
            return Err(anyhow!("Failed to store SMT root: {}", e));
        }
        
        log::info!("Generated new state root for block {}", height);
        Ok(new_root)
    }
    
    fn list_keys_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>> {
        let prefix = format!("{}{}", BST_HEIGHT_INDEX_PREFIX, height);
        let mut keys = Vec::new();
        
        let iter = self.db.prefix_iterator(prefix.as_bytes());
        for item in iter {
            match item {
                Ok((key, _)) => {
                    // Extract the original key from the height index key
                    let key_str = String::from_utf8_lossy(&key);
                    if let Some(hex_key) = key_str.split(':').nth(1) {
                        if let Ok(original_key) = hex::decode(hex_key) {
                            keys.push(original_key);
                        }
                    }
                },
                Err(e) => {
                    log::error!("Error iterating over keys at height {}: {}", height, e);
                }
            }
        }
        
        Ok(keys)
    }
}

impl BSTOperations for RocksDBAdapter {
    fn bst_put(&self, key: &[u8], value: &[u8], height: u32) -> Result<()> {
        let mut batch = WriteBatch::default();
        
        // Create the BST key with the original key
        let bst_key = [BST_KEY_PREFIX.as_bytes(), key].concat();
        
        // Create the height index key
        let height_index_key = format!("{}{}:{}", BST_HEIGHT_INDEX_PREFIX, height, hex::encode(key)).into_bytes();
        
        log::debug!("BST PUT: height={}, key={}, bst_key={}, height_index_key={}",
               height, hex::encode(key), hex::encode(&bst_key), hex::encode(&height_index_key));
        
        // Store the value with the BST key
        batch.put(&bst_key, value);
        
        // Store a reference in the height index
        batch.put(&height_index_key, &[0u8; 0]); // Empty value, just for indexing
        
        // Write the batch
        self.db.write(batch).map_err(|e| anyhow!("Failed to write to database: {}", e))?;
        
        log::debug!("Stored value in BST at height {}: key={}", height, hex::encode(key));
        
        Ok(())
    }
    
    fn bst_get_at_height(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>> {
        // First, check if the key exists in the BST
        let bst_key = [BST_KEY_PREFIX.as_bytes(), key].concat();
        
        log::debug!("BST GET: height={}, key={}, bst_key={}",
               height, hex::encode(key), hex::encode(&bst_key));
        
        // Find the closest height less than or equal to the requested height
        // that has this key using binary search
        let mut low = 0;
        let mut high = height;
        let mut best_match = 0;
        let mut found = false;
        
        log::debug!("BST GET: Starting binary search from {} to {}", low, high);
        
        while low <= high {
            let mid = low + (high - low) / 2;
            if mid == 0 {
                // Skip height 0
                low = 1;
                continue;
            }
            
            let height_index_key = format!("{}{}:{}", BST_HEIGHT_INDEX_PREFIX, mid, hex::encode(key)).into_bytes();
            log::debug!("BST GET: Checking height {} with index key: {}", mid, hex::encode(&height_index_key));
            
            if let Ok(Some(_)) = self.db.get(&height_index_key) {
                // Found a valid height, but continue searching for a closer one
                log::debug!("BST GET: Found height index at height {}", mid);
                found = true;
                best_match = mid;
                
                if mid == height {
                    // Exact match found
                    log::debug!("BST GET: Exact height match at {}", mid);
                    break;
                } else if mid < height {
                    // Look for a closer match in the upper half
                    low = mid + 1;
                } else {
                    // This shouldn't happen in our search pattern, but just in case
                    high = mid - 1;
                }
            } else {
                log::debug!("BST GET: No height index found at height {}", mid);
                // No entry at this height, check lower heights
                high = mid - 1;
            }
        }
        
        if found {
            log::debug!("BST GET: Best match found at height {}, retrieving value with bst_key={}", best_match, hex::encode(&bst_key));
            // Get the value using the BST key
            match self.db.get(&bst_key) {
                Ok(Some(value)) => {
                    log::debug!("Found value in BST at height {}: key={}", best_match, hex::encode(key));
                    return Ok(Some(value.to_vec()));
                },
                Ok(None) => {
                    log::error!("Inconsistent BST state: height index exists but value not found for key={}", hex::encode(key));
                    return Ok(None);
                },
                Err(e) => {
                    return Err(anyhow!("Database error: {}", e));
                }
            }
        }
        
        // Key not found at or before the requested height
        log::debug!("No value found in BST at or before height {}: key={}", height, hex::encode(key));
        Ok(None)
    }
}

impl StateManager for RocksDBAdapter {
    fn get_db(&self) -> Arc<dyn std::any::Any + Send + Sync> {
        self.db.clone()
    }
}
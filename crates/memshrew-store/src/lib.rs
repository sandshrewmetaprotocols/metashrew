use std::io::{Error, Result};
use metashrew_runtime::{BatchLike, KeyValueStoreLike};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use anyhow::{anyhow, Result as AnyhowResult};
use sha2::Digest;
use metashrew_smt_trait::{SMTOperations, BSTOperations, StateManager};

#[derive(Clone, Default)]
pub struct MemStore {
    pub db: Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Default)]
pub struct MemStoreBatch {
    operations: Vec<(Vec<u8>, Vec<u8>)>,
}

impl BatchLike for MemStoreBatch {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        self.operations
            .push((key.as_ref().to_vec(), value.as_ref().to_vec()));
    }
    fn default() -> Self {
        Default::default()
    }
}

impl KeyValueStoreLike for MemStore {
    type Batch = MemStoreBatch;
    type Error = Error;

    fn write(&mut self, batch: Self::Batch) -> Result<()> {
        let mut db = self.db.lock().unwrap();
        for (key, value) in batch.operations {
            db.insert(key, value);
        }
        Ok(())
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>> {
        let db = self.db.lock().unwrap();
        Ok(db.get(key.as_ref()).cloned())
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<()>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let mut db = self.db.lock().unwrap();
        db.insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        Ok(())
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<()> {
        let mut db = self.db.lock().unwrap();
        db.remove(key.as_ref());
        Ok(())
    }

    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>> {
        let db = self.db.lock().unwrap();
        let keys = db.keys().cloned().collect::<Vec<Vec<u8>>>();
        Ok(Box::new(keys.into_iter()))
    }
}

// Constants for SMT and BST operations
const SMT_ROOT_PREFIX: &str = "smt:root:";
const BST_KEY_PREFIX: &str = "bst:";
const BST_HEIGHT_INDEX_PREFIX: &str = "bst:height:";
const EMPTY_NODE_HASH: [u8; 32] = [0; 32];

// Implementation of the SMT trait for MemStore
impl SMTOperations for MemStore {
    fn get_smt_root_at_height(&self, height: u32) -> AnyhowResult<[u8; 32]> {
        let db = self.db.lock().unwrap();
        
        // Try direct lookup first with double colon format
        let double_key = format!("{}::{}", SMT_ROOT_PREFIX, height).into_bytes();
        
        if let Some(root_data) = db.get(&double_key) {
            if root_data.len() == 32 {
                let mut root = [0u8; 32];
                root.copy_from_slice(&root_data);
                return Ok(root);
            }
        }
        
        // Try with triple colon format
        let triple_key = format!("{}:::{}", SMT_ROOT_PREFIX, height).into_bytes();
        
        if let Some(root_data) = db.get(&triple_key) {
            if root_data.len() == 32 {
                let mut root = [0u8; 32];
                root.copy_from_slice(&root_data);
                return Ok(root);
            }
        }
        
        // If direct lookup fails, fall back to binary search
        if height == 0 {
            return Ok(EMPTY_NODE_HASH);
        }
        
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
            if db.get(&root_key).is_some() {
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
            if let Some(root_data) = db.get(&root_key) {
                if root_data.len() == 32 {
                    let mut root = [0u8; 32];
                    root.copy_from_slice(&root_data);
                    return Ok(root);
                }
            }
        }
        
        // If no root found, return the default (empty) root
        Ok(EMPTY_NODE_HASH)
    }
    
    fn calculate_and_store_state_root(&self, height: u32) -> AnyhowResult<[u8; 32]> {
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
        let mut db = self.db.lock().unwrap();
        db.insert(root_key, new_root.to_vec());
        
        Ok(new_root)
    }
    
    fn list_keys_at_height(&self, height: u32) -> AnyhowResult<Vec<Vec<u8>>> {
        let prefix = format!("{}{}", BST_HEIGHT_INDEX_PREFIX, height);
        let mut keys = Vec::new();
        
        let db = self.db.lock().unwrap();
        
        for (key, _) in db.iter() {
            let key_str = String::from_utf8_lossy(key);
            if key_str.starts_with(&prefix) {
                if let Some(hex_key) = key_str.split(':').nth(1) {
                    if let Ok(original_key) = hex::decode(hex_key) {
                        keys.push(original_key);
                    }
                }
            }
        }
        
        Ok(keys)
    }
}

impl BSTOperations for MemStore {
    fn bst_put(&self, key: &[u8], value: &[u8], height: u32) -> AnyhowResult<()> {
        let mut db = self.db.lock().unwrap();
        
        // Create the BST key with the original key
        let bst_key = [BST_KEY_PREFIX.as_bytes(), key].concat();
        
        // Create the height index key
        let height_index_key = format!("{}{}:{}", BST_HEIGHT_INDEX_PREFIX, height, hex::encode(key)).into_bytes();
        
        // Store the value with the BST key
        db.insert(bst_key, value.to_vec());
        
        // Store a reference in the height index
        db.insert(height_index_key, vec![]);
        
        Ok(())
    }
    
    fn bst_get_at_height(&self, key: &[u8], height: u32) -> AnyhowResult<Option<Vec<u8>>> {
        // First, check if the key exists in the BST
        let bst_key = [BST_KEY_PREFIX.as_bytes(), key].concat();
        
        let db = self.db.lock().unwrap();
        
        // Find the closest height less than or equal to the requested height
        // that has this key using binary search
        let mut low = 0;
        let mut high = height;
        let mut best_match = 0;
        let mut found = false;
        
        while low <= high {
            let mid = low + (high - low) / 2;
            if mid == 0 {
                // Skip height 0
                low = 1;
                continue;
            }
            
            let height_index_key = format!("{}{}:{}", BST_HEIGHT_INDEX_PREFIX, mid, hex::encode(key)).into_bytes();
            
            if db.get(&height_index_key).is_some() {
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
                // No entry at this height, check lower heights
                high = mid - 1;
            }
        }
        
        if found {
            // Get the value using the BST key
            return Ok(db.get(&bst_key).cloned());
        }
        
        // Key not found at or before the requested height
        Ok(None)
    }
}

impl StateManager for MemStore {
    fn get_db(&self) -> Arc<dyn std::any::Any + Send + Sync> {
        self.db.clone()
    }
}

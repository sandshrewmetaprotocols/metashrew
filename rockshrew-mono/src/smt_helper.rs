use anyhow::{anyhow, Result};
use rocksdb::{DB, WriteBatch};
use std::sync::Arc;
use log::{debug, info, error};
use sha2::Digest;

// Prefixes for different types of keys in the database
pub const SMT_ROOT_PREFIX: &str = "smt:root:";
pub const BST_KEY_PREFIX: &str = "bst:";
pub const BST_HEIGHT_INDEX_PREFIX: &str = "bst:height:";

// Default empty hash (represents empty nodes)
const EMPTY_NODE_HASH: [u8; 32] = [0; 32];

/// Helper functions for SMT and BST operations
pub struct SMTHelper {
    db: Arc<DB>,
}

/// Represents a key-value pair with height information
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HeightIndexedValue {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub height: u32,
}

impl SMTHelper {
    pub fn new(db: Arc<DB>) -> Self {
        Self { db }
    }

    /// Get the SMT root for a specific height using binary search
    pub fn get_smt_root_at_height(&self, height: u32) -> Result<[u8; 32]> {
        // Try direct lookup first with double colon format
        let double_key = format!("{}::{}", SMT_ROOT_PREFIX, height).into_bytes();
        debug!("Looking up state root for height {}", height);
        
        if let Ok(Some(root_data)) = self.db.get(&double_key) {
            debug!("Found state root for height {}", height);
            if root_data.len() == 32 {
                let mut root = [0u8; 32];
                root.copy_from_slice(&root_data);
                return Ok(root);
            }
        }
        
        // Try with triple colon format
        let triple_key = format!("{}:::{}", SMT_ROOT_PREFIX, height).into_bytes();
        debug!("Checking alternative format for state root");
        
        if let Ok(Some(root_data)) = self.db.get(&triple_key) {
            debug!("Found state root using alternative format for height {}", height);
            if root_data.len() == 32 {
                let mut root = [0u8; 32];
                root.copy_from_slice(&root_data);
                return Ok(root);
            }
        }
        
        // If direct lookup fails, fall back to binary search
        if height == 0 {
            debug!("Height is 0, returning empty state root");
            return Ok(EMPTY_NODE_HASH);
        }
        
        debug!("Direct lookup failed, searching for nearest state root for height {}", height);
        
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
                debug!("Found nearest state root at height {} for requested height {}", best_match, height);
                if root_data.len() == 32 {
                    let mut root = [0u8; 32];
                    root.copy_from_slice(&root_data);
                    return Ok(root);
                }
            }
        }
        
        // If no root found, return the default (empty) root
        debug!("No state root found for height {}, returning empty state root", height);
        Ok(EMPTY_NODE_HASH)
    }
    
    /// Store a value in the BST with height indexing
    pub fn bst_put(&self, key: &[u8], value: &[u8], height: u32) -> Result<()> {
        let mut batch = WriteBatch::default();
        
        // Create the BST key with the original key
        let bst_key = [BST_KEY_PREFIX.as_bytes(), key].concat();
        
        // Create the height index key
        let height_index_key = format!("{}{}:{}", BST_HEIGHT_INDEX_PREFIX, height, hex::encode(key)).into_bytes();
        
        debug!("BST PUT: height={}, key={}, bst_key={}, height_index_key={}",
               height, hex::encode(key), hex::encode(&bst_key), hex::encode(&height_index_key));
        
        // Store the value with the BST key
        batch.put(&bst_key, value);
        
        // Store a reference in the height index
        batch.put(&height_index_key, &[0u8; 0]); // Empty value, just for indexing
        
        // Write the batch
        self.db.write(batch).map_err(|e| anyhow!("Failed to write to database: {}", e))?;
        
        debug!("Stored value in BST at height {}: key={}", height, hex::encode(key));
        
        // Immediately verify the write
        match self.db.get(&bst_key) {
            Ok(Some(stored_value)) => {
                debug!("BST PUT verification: Successfully stored and retrieved value for key={}", hex::encode(key));
            },
            Ok(None) => {
                error!("BST PUT verification: Value not found immediately after storage for key={}", hex::encode(key));
            },
            Err(e) => {
                error!("BST PUT verification: Error retrieving value after storage for key={}: {}", hex::encode(key), e);
            }
        }
        
        Ok(())
    }
    
    /// Get a value from the BST at a specific height
    pub fn bst_get_at_height(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>> {
        // First, check if the key exists in the BST
        let bst_key = [BST_KEY_PREFIX.as_bytes(), key].concat();
        
        debug!("BST GET: height={}, key={}, bst_key={}",
               height, hex::encode(key), hex::encode(&bst_key));
        
        // Find the closest height less than or equal to the requested height
        // that has this key using binary search
        let mut low = 0;
        let mut high = height;
        let mut best_match = 0;
        let mut found = false;
        
        debug!("BST GET: Starting binary search from {} to {}", low, high);
        
        while low <= high {
            let mid = low + (high - low) / 2;
            if mid == 0 {
                // Skip height 0
                low = 1;
                continue;
            }
            
            let height_index_key = format!("{}{}:{}", BST_HEIGHT_INDEX_PREFIX, mid, hex::encode(key)).into_bytes();
            debug!("BST GET: Checking height {} with index key: {}", mid, hex::encode(&height_index_key));
            
            if let Ok(Some(_)) = self.db.get(&height_index_key) {
                // Found a valid height, but continue searching for a closer one
                debug!("BST GET: Found height index at height {}", mid);
                found = true;
                best_match = mid;
                
                if mid == height {
                    // Exact match found
                    debug!("BST GET: Exact height match at {}", mid);
                    break;
                } else if mid < height {
                    // Look for a closer match in the upper half
                    low = mid + 1;
                } else {
                    // This shouldn't happen in our search pattern, but just in case
                    high = mid - 1;
                }
            } else {
                debug!("BST GET: No height index found at height {}", mid);
                // No entry at this height, check lower heights
                high = mid - 1;
            }
        }
        
        if found {
            debug!("BST GET: Best match found at height {}, retrieving value with bst_key={}", best_match, hex::encode(&bst_key));
            // Get the value using the BST key
            match self.db.get(&bst_key) {
                Ok(Some(value)) => {
                    debug!("Found value in BST at height {}: key={}", best_match, hex::encode(key));
                    return Ok(Some(value));
                },
                Ok(None) => {
                    error!("Inconsistent BST state: height index exists but value not found for key={}", hex::encode(key));
                    return Ok(None);
                },
                Err(e) => {
                    return Err(anyhow!("Database error: {}", e));
                }
            }
        }
        
        // Key not found at or before the requested height
        debug!("No value found in BST at or before height {}: key={}", height, hex::encode(key));
        Ok(None)
    }
    
    /// List all keys updated at a specific height
    pub fn list_keys_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>> {
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
                    error!("Error iterating over keys at height {}: {}", height, e);
                }
            }
        }
        
        Ok(keys)
    }
    
    /// Calculate and store a new state root for the given height
    pub fn calculate_and_store_state_root(&self, height: u32) -> Result<[u8; 32]> {
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
            error!("Failed to store SMT root for height {}: {}", height, e);
            return Err(anyhow!("Failed to store SMT root: {}", e));
        }
        
        info!("Generated new state root for block {}", height);
        Ok(new_root)
    }
}
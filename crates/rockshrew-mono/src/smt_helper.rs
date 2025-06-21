use anyhow::{anyhow, Result};
use rocksdb::{DB, WriteBatch};
use std::sync::Arc;
use log::{trace, debug, info, error};
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
        trace!("Looking up state root for height {}", height);
        
        if let Ok(Some(root_data)) = self.db.get(&double_key) {
            trace!("Found state root for height {}", height);
            if root_data.len() == 32 {
                let mut root = [0u8; 32];
                root.copy_from_slice(&root_data);
                return Ok(root);
            }
        }
        
        // Try with triple colon format
        let triple_key = format!("{}:::{}", SMT_ROOT_PREFIX, height).into_bytes();
        trace!("Checking alternative format for state root");
        
        if let Ok(Some(root_data)) = self.db.get(&triple_key) {
            trace!("Found state root using alternative format for height {}", height);
            if root_data.len() == 32 {
                let mut root = [0u8; 32];
                root.copy_from_slice(&root_data);
                return Ok(root);
            }
        }
        
        // If direct lookup fails, fall back to binary search
        if height == 0 {
            trace!("Height is 0, returning empty state root");
            return Ok(EMPTY_NODE_HASH);
        }
        
        trace!("Direct lookup failed, searching for nearest state root for height {}", height);
        
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
                trace!("Found nearest state root at height {} for requested height {}", best_match, height);
                if root_data.len() == 32 {
                    let mut root = [0u8; 32];
                    root.copy_from_slice(&root_data);
                    return Ok(root);
                }
            }
        }
        
        // If no root found, return the default (empty) root
        trace!("No state root found for height {}, returning empty state root", height);
        Ok(EMPTY_NODE_HASH)
    }
    
    /// Store a value in the BST with height indexing
    #[allow(dead_code)]
    pub fn bst_put(&self, key: &[u8], value: &[u8], height: u32) -> Result<()> {
        let mut batch = WriteBatch::default();
        
        // Create the BST key with the original key
        let bst_key = [BST_KEY_PREFIX.as_bytes(), key].concat();
        
        // Create the height index key
        let height_index_key = format!("{}{}:{}", BST_HEIGHT_INDEX_PREFIX, height, hex::encode(key)).into_bytes();
        
        trace!("BST PUT: height={}, key={}, bst_key={}, height_index_key={}",
               height, hex::encode(key), hex::encode(&bst_key), hex::encode(&height_index_key));
        
        // Store the value with the BST key
        batch.put(&bst_key, value);
        
        // Store a reference in the height index
        batch.put(&height_index_key, &[0u8; 0]); // Empty value, just for indexing
        
        // Write the batch
        self.db.write(batch).map_err(|e| anyhow!("Failed to write to database: {}", e))?;
        
        trace!("Stored value in BST at height {}: key={}", height, hex::encode(key));
        
        // Immediately verify the write
        match self.db.get(&bst_key) {
            Ok(Some(_stored_value)) => {
                trace!("BST PUT verification: Successfully stored and retrieved value for key={}", hex::encode(key));
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
        
        trace!("BST GET: height={}, key={}, bst_key={}",
               height, hex::encode(key), hex::encode(&bst_key));
        
        // Find the closest height less than or equal to the requested height
        // that has this key using binary search
        let mut low = 0;
        let mut high = height;
        let mut best_match = 0;
        let mut found = false;
        
        trace!("BST GET: Starting binary search from {} to {}", low, high);
        
        while low <= high {
            let mid = low + (high - low) / 2;
            if mid == 0 {
                // Skip height 0
                low = 1;
                continue;
            }
            
            let height_index_key = format!("{}{}:{}", BST_HEIGHT_INDEX_PREFIX, mid, hex::encode(key)).into_bytes();
            trace!("BST GET: Checking height {} with index key: {}", mid, hex::encode(&height_index_key));
            
            if let Ok(Some(_)) = self.db.get(&height_index_key) {
                // Found a valid height, but continue searching for a closer one
                trace!("BST GET: Found height index at height {}", mid);
                found = true;
                best_match = mid;
                
                if mid == height {
                    // Exact match found
                    trace!("BST GET: Exact height match at {}", mid);
                    break;
                } else if mid < height {
                    // Look for a closer match in the upper half
                    low = mid + 1;
                } else {
                    // This shouldn't happen in our search pattern, but just in case
                    high = mid - 1;
                }
            } else {
                trace!("BST GET: No height index found at height {}", mid);
                // No entry at this height, check lower heights
                high = mid - 1;
            }
        }
        
        if found {
            trace!("BST GET: Best match found at height {}, retrieving value with bst_key={}", best_match, hex::encode(&bst_key));
            // Get the value using the BST key
            match self.db.get(&bst_key) {
                Ok(Some(value)) => {
                    trace!("Found value in BST at height {}: key={}", best_match, hex::encode(key));
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
        trace!("No value found in BST at or before height {}: key={}", height, hex::encode(key));
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
        use metashrew_runtime::OptimizedBST;
        
        info!("Starting state root calculation for height {}", height);
        
        // Use OptimizedBST to get keys updated at this height
        let optimized_bst = OptimizedBST::new(self.db.clone());
        let updated_keys = optimized_bst.get_keys_at_height(height)?;
        
        info!("Found {} keys updated at height {}", updated_keys.len(), height);
        
        // If no keys were updated at this height, get all current keys to calculate a complete state root
        let keys_to_process = if updated_keys.is_empty() {
            info!("No keys updated at height {}, getting all current keys for state root calculation", height);
            // Get all current keys from the optimized BST
            match optimized_bst.get_all_current_keys() {
                Ok(all_keys) => {
                    info!("Found {} total current keys for state root calculation", all_keys.len());
                    all_keys
                },
                Err(e) => {
                    error!("Failed to get all current keys: {}", e);
                    Vec::new()
                }
            }
        } else {
            updated_keys
        };
        
        info!("Processing {} keys for state root calculation at height {}", keys_to_process.len(), height);
        
        // If we still have no keys, return a meaningful empty state root
        if keys_to_process.is_empty() {
            info!("No keys to process for state root at height {}, returning empty state root", height);
            let empty_root = [0u8; 32];
            
            // Store the empty root
            let root_key = format!("{}::{}", SMT_ROOT_PREFIX, height).into_bytes();
            if let Err(e) = self.db.put(&root_key, &empty_root) {
                error!("Failed to store empty SMT root for height {}: {}", height, e);
                return Err(anyhow!("Failed to store SMT root: {}", e));
            }
            
            info!("Stored empty state root for height {}", height);
            return Ok(empty_root);
        }
        
        // Calculate the new root based on all relevant keys
        let mut hasher = sha2::Sha256::new();
        hasher.update(&height.to_le_bytes());
        
        // Sort keys for deterministic hashing
        let mut sorted_keys = keys_to_process;
        sorted_keys.sort();
        
        info!("Sorted {} keys for deterministic hashing", sorted_keys.len());
        
        // Add each key and its current value to the hash
        let mut processed_keys = 0;
        for key in &sorted_keys {
            hasher.update(key);
            // Use OptimizedBST to get the current value for this key
            if let Ok(Some(value)) = optimized_bst.get_current(key) {
                // Remove height annotation if present (last 4 bytes)
                let clean_value = if value.len() >= 4 {
                    &value[..value.len()-4]
                } else {
                    &value
                };
                hasher.update(clean_value);
                processed_keys += 1;
                info!("Added to state root hash - key: {}, value: {} (processed: {}/{})",
                      hex::encode(key), hex::encode(clean_value), processed_keys, sorted_keys.len());
            } else {
                error!("No current value found for key: {} (this should not happen)", hex::encode(key));
            }
        }
        
        // Finalize the new root
        let mut new_root = [0u8; 32];
        new_root.copy_from_slice(&hasher.finalize());
        
        // Store the new root
        let root_key = format!("{}::{}", SMT_ROOT_PREFIX, height).into_bytes();
        info!("About to store state root for height {} with key: {} and root: {}",
              height, hex::encode(&root_key), hex::encode(&new_root));
        
        if let Err(e) = self.db.put(&root_key, &new_root) {
            error!("Failed to store SMT root for height {}: {}", height, e);
            return Err(anyhow!("Failed to store SMT root: {}", e));
        }
        
        // Immediately verify the storage
        match self.db.get(&root_key) {
            Ok(Some(stored_value)) => {
                info!("Verification: Successfully stored and retrieved state root for height {}: {}",
                      height, hex::encode(&stored_value));
            },
            Ok(None) => {
                error!("Verification: State root not found immediately after storage for height {}", height);
            },
            Err(e) => {
                error!("Verification: Error retrieving state root after storage for height {}: {}", height, e);
            }
        }
        
        info!("Generated and stored state root for block {} with {} keys (root: {})",
              height, processed_keys, hex::encode(&new_root));
        Ok(new_root)
    }
    
    /// Get the current state root (most recent)
    pub fn get_current_state_root(&self) -> Result<[u8; 32]> {
        // Find the highest height with a state root
        let prefix = SMT_ROOT_PREFIX.as_bytes();
        let iter = self.db.prefix_iterator(prefix);
        
        let mut max_height = 0;
        let mut found = false;
        
        for item in iter {
            match item {
                Ok((key, _)) => {
                    let key_str = String::from_utf8_lossy(&key);
                    // Parse height from key format "smt:root::HEIGHT"
                    if let Some(height_str) = key_str.strip_prefix(SMT_ROOT_PREFIX).and_then(|s| s.strip_prefix("::")) {
                        if let Ok(height) = height_str.parse::<u32>() {
                            if height > max_height {
                                max_height = height;
                                found = true;
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Error iterating over state roots: {}", e);
                }
            }
        }
        
        if found {
            self.get_smt_root_at_height(max_height)
        } else {
            // No state root found, return empty hash
            Ok(EMPTY_NODE_HASH)
        }
    }
    
    /// Get all heights at which a key was updated (for backward compatibility)
    pub fn bst_get_heights_for_key(&self, key: &[u8]) -> Result<Vec<u32>> {
        use metashrew_runtime::OptimizedBST;
        
        // Use OptimizedBST to get heights for this key
        let optimized_bst = OptimizedBST::new(self.db.clone());
        optimized_bst.get_heights_for_key(key)
    }
    
    /// Delete the state root for a specific height
    pub fn delete_state_root_at_height(&self, height: u32) -> Result<()> {
        // Try both key formats to ensure we delete any existing state root
        let double_key = format!("{}::{}", SMT_ROOT_PREFIX, height).into_bytes();
        let triple_key = format!("{}:::{}", SMT_ROOT_PREFIX, height).into_bytes();
        
        debug!("Deleting state root for height {}", height);
        
        // Delete both possible key formats
        if let Err(e) = self.db.delete(&double_key) {
            debug!("Failed to delete state root with double colon format for height {}: {}", height, e);
        }
        
        if let Err(e) = self.db.delete(&triple_key) {
            debug!("Failed to delete state root with triple colon format for height {}: {}", height, e);
        }
        
        debug!("Deleted state root for height {}", height);
        Ok(())
    }
}

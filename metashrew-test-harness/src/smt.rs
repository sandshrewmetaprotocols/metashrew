use anyhow::{anyhow, Result};
use metashrew_runtime::KeyValueStoreLike;
use sha2::{Digest, Sha256};
use log::{debug, info, error};

// Prefixes for different types of keys in the database
pub const SMT_ROOT_PREFIX: &str = "smt:root:";
pub const BST_KEY_PREFIX: &str = "bst:";
pub const BST_HEIGHT_INDEX_PREFIX: &str = "bst:height:";

// Default empty hash (represents empty nodes)
const EMPTY_NODE_HASH: [u8; 32] = [0; 32];

/// Helper functions for SMT and BST operations
pub struct SMTHelper<T: KeyValueStoreLike> {
    db: T,
}

impl<T> SMTHelper<T>
where
    T: KeyValueStoreLike,
    T::Error: std::error::Error + Send + Sync + 'static,
{
    pub fn new(db: T) -> Self {
        Self { db }
    }

    /// Get the SMT root for a specific height using binary search
    pub fn get_smt_root_at_height(&mut self, height: u32) -> Result<[u8; 32]> {
        if height == 0 {
            return Ok(EMPTY_NODE_HASH);
        }
        
        let mut low = 0;
        let mut high = height;
        let mut best_match = 0;
        let mut found = false;
        
        while low <= high {
            let mid = low + (high - low) / 2;
            if mid == 0 {
                low = 1;
                continue;
            }
            
            let root_key = format!("{}{}", SMT_ROOT_PREFIX, mid).into_bytes();
            if let Ok(Some(_)) = self.db.get(&root_key) {
                found = true;
                best_match = mid;
                
                if mid == height {
                    break;
                } else if mid < height {
                    low = mid + 1;
                } else {
                    high = mid - 1;
                }
            } else {
                high = mid - 1;
            }
        }
        
        if found {
            let root_key = format!("{}{}", SMT_ROOT_PREFIX, best_match).into_bytes();
            if let Ok(Some(root_data)) = self.db.get(&root_key) {
                if root_data.len() == 32 {
                    let mut root = [0u8; 32];
                    root.copy_from_slice(&root_data);
                    return Ok(root);
                }
            }
        }
        
        Ok(EMPTY_NODE_HASH)
    }

    pub fn bst_put(&mut self, key: &[u8], value: &[u8], height: u32) -> Result<()> {
        let bst_key = [BST_KEY_PREFIX.as_bytes(), key].concat();
        let height_index_key = format!("{}{}:{}", BST_HEIGHT_INDEX_PREFIX, height, hex::encode(key)).into_bytes();
        self.db.put(&bst_key, value)?;
        self.db.put(&height_index_key, &[0u8; 0])?;
        debug!("Stored value in BST at height {}: key={}", height, hex::encode(key));
        Ok(())
    }

    pub fn bst_get_at_height(&mut self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>> {
        let bst_key = [BST_KEY_PREFIX.as_bytes(), key].concat();
        let mut low = 0;
        let mut high = height;
        let mut best_match = 0;
        let mut found = false;
        
        while low <= high {
            let mid = low + (high - low) / 2;
            if mid == 0 {
                low = 1;
                continue;
            }
            
            let height_index_key = format!("{}{}:{}", BST_HEIGHT_INDEX_PREFIX, mid, hex::encode(key)).into_bytes();
            if let Ok(Some(_)) = self.db.get(&height_index_key) {
                found = true;
                best_match = mid;
                
                if mid == height {
                    break;
                } else if mid < height {
                    low = mid + 1;
                } else {
                    high = mid - 1;
                }
            } else {
                high = mid - 1;
            }
        }
        
        if found {
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
        
        debug!("No value found in BST at or before height {}: key={}", height, hex::encode(key));
        Ok(None)
    }

    pub fn list_keys_at_height(&mut self, height: u32) -> Result<Vec<Vec<u8>>> {
        let prefix = format!("{}{}:", BST_HEIGHT_INDEX_PREFIX, height);
        let mut keys = Vec::new();
        for key in self.db.keys()? {
            if key.starts_with(prefix.as_bytes()) {
                let key_str = String::from_utf8_lossy(&key);
                if let Some(hex_key) = key_str.split(':').nth(2) {
                    if let Ok(original_key) = hex::decode(hex_key) {
                        keys.push(original_key);
                    }
                }
            }
        }
        Ok(keys)
    }

    pub fn calculate_and_store_state_root(&mut self, height: u32) -> Result<[u8; 32]> {
        let prev_height = if height > 0 { height - 1 } else { 0 };
        let prev_root = self.get_smt_root_at_height(prev_height)?;
        let updated_keys = self.list_keys_at_height(height)?;
        let mut hasher = Sha256::new();
        hasher.update(&prev_root);
        hasher.update(&height.to_le_bytes());
        for key in &updated_keys {
            hasher.update(key);
            if let Ok(Some(value)) = self.bst_get_at_height(key, height) {
                hasher.update(&value);
            }
        }
        let mut new_root = [0u8; 32];
        new_root.copy_from_slice(&hasher.finalize());
        let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
        self.db.put(&root_key, &new_root)?;
        info!("Calculated and stored new SMT root for height {}: {}", height, hex::encode(&new_root));
        Ok(new_root)
    }
}
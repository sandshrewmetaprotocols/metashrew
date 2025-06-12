use anyhow::Result;
use rocksdb::{DB, WriteBatch};
use std::sync::Arc;
use std::cmp::Ordering;

// Prefixes for different types of keys in the database
pub const SMT_ROOT_PREFIX: &str = "smt:root:";
pub const HEIGHT_TREE_PREFIX: &str = "height_tree:";
pub const HEIGHT_LIST_SUFFIX: &str = ":heights";
pub const MODIFIED_KEYS_PREFIX: &str = "modified_keys:";

// Default empty hash (represents empty nodes)
const EMPTY_NODE_HASH: [u8; 32] = [0; 32];

/// Helper functions for SMT operations
pub struct SMTHelper {
    db: Arc<DB>,
}

impl SMTHelper {
    pub fn new(db: Arc<DB>) -> Self {
        Self { db }
    }

    /// Get the SMT root for a specific height
    pub fn get_smt_root_at_height(&self, height: u32) -> Result<[u8; 32]> {
        // Find the closest height less than or equal to the requested height
        let mut target_height = height;
        
        while target_height > 0 {
            let root_key = format!("{}:{}", SMT_ROOT_PREFIX, target_height).into_bytes();
            if let Ok(Some(root_data)) = self.db.get(&root_key) {
                if root_data.len() == 32 {
                    let mut root = [0u8; 32];
                    root.copy_from_slice(&root_data);
                    return Ok(root);
                }
            }
            target_height -= 1;
        }
        
        // If no root found, return the default (empty) root
        Ok(EMPTY_NODE_HASH)
    }
    
    /// Get a value at a specific height using binary search
    pub fn get_value_at_height(&self, key: &[u8], target_height: u32) -> Result<Option<Vec<u8>>> {
        // 1. Get the height list
        let heights_key = format!("{}:{}", hex::encode(key), HEIGHT_LIST_SUFFIX).into_bytes();
        
        let heights = match self.db.get(&heights_key)? {
            Some(data) => {
                let mut heights = Vec::new();
                let mut i = 0;
                while i < data.len() {
                    if i + 4 <= data.len() {
                        let mut height_bytes = [0u8; 4];
                        height_bytes.copy_from_slice(&data[i..i+4]);
                        heights.push(u32::from_le_bytes(height_bytes));
                    }
                    i += 4;
                }
                heights
            },
            None => return Ok(None), // No history for this key
        };
        
        // 2. Binary search for the closest height less than or equal to target_height
        let closest_height = match heights.binary_search_by(|&h| {
            if h <= target_height {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }) {
            Ok(idx) => heights[idx],
            Err(idx) => {
                if idx == 0 {
                    return Ok(None); // No value before target_height
                }
                heights[idx - 1] // Get the closest height less than target_height
            }
        };
        
        // 3. Get the value at the closest height
        let height_key = format!("{}:{}:{}", 
            HEIGHT_TREE_PREFIX, 
            hex::encode(key), 
            closest_height
        ).into_bytes();
        
        self.db.get(&height_key).map_err(|e| anyhow::anyhow!(e))
    }
    
    /// Migrate existing data to the height-indexed structure
    pub fn migrate_to_height_indexed(&self, start_height: u32, end_height: u32) -> Result<()> {
        let mut batch = WriteBatch::default();
        
        // For each height
        for height in start_height..=end_height {
            // Get all keys modified at this height
            let modified_keys_key = format!("{}:{}", "smt:height:", height).into_bytes();
            
            if let Ok(Some(keys_data)) = self.db.get(&modified_keys_key) {
                let mut i = 0;
                let mut modified_keys = Vec::new();
                
                while i < keys_data.len() {
                    if i + 4 <= keys_data.len() {
                        let mut len_bytes = [0u8; 4];
                        len_bytes.copy_from_slice(&keys_data[i..i+4]);
                        let key_len = u32::from_le_bytes(len_bytes) as usize;
                        i += 4;
                        
                        if i + key_len <= keys_data.len() {
                            let key = keys_data[i..i+key_len].to_vec();
                            modified_keys.push(key.clone());
                            
                            // Get the value at this height
                            let value_key = format!("{}:{}:{}", "smt:height:", hex::encode(&key), height).into_bytes();
                            
                            if let Ok(Some(value)) = self.db.get(&value_key) {
                                // Store in the new format
                                self.put_with_height_to_batch(&mut batch, &key, &value, height)?;
                            }
                            
                            i += key_len;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                
                // Track modified keys in the new format
                self.track_modified_keys_to_batch(&mut batch, &modified_keys, height)?;
            }
        }
        
        // Apply all changes
        self.db.write(batch)?;
        
        Ok(())
    }
    
    /// Store a value with height indexing in a batch
    fn put_with_height_to_batch(&self, batch: &mut WriteBatch, key: &[u8], value: &[u8], height: u32) -> Result<()> {
        // 1. Store the actual value with height in the key
        let height_key = format!("{}:{}:{}", 
            HEIGHT_TREE_PREFIX, 
            hex::encode(key), 
            height
        ).into_bytes();
        batch.put(&height_key, value);
        
        // 2. Update the height list for binary search
        self.update_height_list_to_batch(batch, key, height)?;
        
        Ok(())
    }
    
    /// Update the height list for a key in a batch
    fn update_height_list_to_batch(&self, batch: &mut WriteBatch, key: &[u8], height: u32) -> Result<()> {
        let heights_key = format!("{}:{}", hex::encode(key), HEIGHT_LIST_SUFFIX).into_bytes();
        
        // Get existing heights
        let mut heights = match self.db.get(&heights_key)? {
            Some(data) => {
                let mut heights = Vec::new();
                let mut i = 0;
                while i < data.len() {
                    if i + 4 <= data.len() {
                        let mut height_bytes = [0u8; 4];
                        height_bytes.copy_from_slice(&data[i..i+4]);
                        heights.push(u32::from_le_bytes(height_bytes));
                    }
                    i += 4;
                }
                heights
            },
            None => Vec::new(),
        };
        
        // Add new height if not already present
        if !heights.contains(&height) {
            heights.push(height);
            heights.sort(); // Keep sorted for binary search
            
            // Serialize heights back to bytes
            let mut data = Vec::with_capacity(heights.len() * 4);
            for h in heights {
                data.extend_from_slice(&h.to_le_bytes());
            }
            
            batch.put(&heights_key, &data);
        }
        
        Ok(())
    }
    
    /// Track keys modified at a specific height in a batch
    fn track_modified_keys_to_batch(&self, batch: &mut WriteBatch, keys: &[Vec<u8>], height: u32) -> Result<()> {
        let modified_keys_key = format!("{}:{}", MODIFIED_KEYS_PREFIX, height).into_bytes();
        
        // Serialize the list of keys
        let mut data = Vec::new();
        for key in keys {
            // Write key length (4 bytes)
            data.extend_from_slice(&(key.len() as u32).to_le_bytes());
            // Write key
            data.extend_from_slice(key);
        }
        
        batch.put(&modified_keys_key, &data);
        Ok(())
    }
    
    /// Get keys modified at a specific height
    fn get_keys_modified_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>> {
        let modified_keys_key = format!("{}:{}", MODIFIED_KEYS_PREFIX, height).into_bytes();
        
        match self.db.get(&modified_keys_key)? {
            Some(data) => {
                let mut keys = Vec::new();
                let mut i = 0;
                
                while i < data.len() {
                    if i + 4 <= data.len() {
                        let mut len_bytes = [0u8; 4];
                        len_bytes.copy_from_slice(&data[i..i+4]);
                        let key_len = u32::from_le_bytes(len_bytes) as usize;
                        i += 4;
                        
                        if i + key_len <= data.len() {
                            keys.push(data[i..i+key_len].to_vec());
                            i += key_len;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                
                Ok(keys)
            },
            None => Ok(Vec::new()),
        }
    }
    
    /// Get all keys affected by a reorg
    fn get_keys_affected_by_reorg(&self, fork_height: u32, current_height: u32) -> Result<Vec<Vec<u8>>> {
        let mut affected_keys = Vec::new();
        
        // For each height from fork_height+1 to current_height
        for height in (fork_height + 1)..=current_height {
            // Get all keys modified at this height
            let keys = self.get_keys_modified_at_height(height)?;
            affected_keys.extend(keys);
        }
        
        // Remove duplicates
        affected_keys.sort();
        affected_keys.dedup();
        
        Ok(affected_keys)
    }
    
    /// Handle chain reorganization
    pub fn handle_reorg(&self, fork_height: u32, current_height: u32) -> Result<()> {
        // Get all keys affected by the reorg
        let affected_keys = self.get_keys_affected_by_reorg(fork_height, current_height)?;
        
        let mut batch = WriteBatch::default();
        
        // For each affected key
        for key in &affected_keys {
            // Get the value at the fork height
            let _fork_value = match self.get_value_at_height(key, fork_height)? {
                Some(value) => value,
                None => Vec::new(), // Key didn't exist at fork height
            };
            
            // Update the height list to mark this key as not modified after fork_height
            let heights_key = format!("{}:{}", hex::encode(key), HEIGHT_LIST_SUFFIX).into_bytes();
            
            if let Some(heights_data) = self.db.get(&heights_key)? {
                let mut heights = Vec::new();
                let mut i = 0;
                
                while i < heights_data.len() {
                    if i + 4 <= heights_data.len() {
                        let mut height_bytes = [0u8; 4];
                        height_bytes.copy_from_slice(&heights_data[i..i+4]);
                        let height = u32::from_le_bytes(height_bytes);
                        
                        // Only keep heights up to fork_height
                        if height <= fork_height {
                            heights.push(height);
                        }
                        
                        i += 4;
                    } else {
                        break;
                    }
                }
                
                // Serialize heights back to bytes
                let mut new_heights_data = Vec::with_capacity(heights.len() * 4);
                for h in heights {
                    new_heights_data.extend_from_slice(&h.to_le_bytes());
                }
                
                batch.put(&heights_key, &new_heights_data);
            }
        }
        
        // Delete height index entries for heights > fork_height
        for height in (fork_height + 1)..=current_height {
            let modified_keys_key = format!("{}:{}", MODIFIED_KEYS_PREFIX, height).into_bytes();
            batch.delete(&modified_keys_key);
            
            // Delete the stateroot for this height
            let root_key = format!("{}:{}", SMT_ROOT_PREFIX, height).into_bytes();
            batch.delete(&root_key);
        }
        
        // Apply all changes atomically
        self.db.write(batch)?;
        
        Ok(())
    }
}
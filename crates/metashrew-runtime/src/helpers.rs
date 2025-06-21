//! Helper functions for working with the append-only BST database structure
//! 
//! This module provides high-level functions for:
//! - Querying values at specific block heights
//! - Iterating backwards through key history
//! - Binary search for historical values
//! - Tree traversal across nodes
//! - Reorg handling with complete rollback

use anyhow::{anyhow, Result};
use std::sync::{Arc, Mutex};
use crate::{RocksDBRuntimeAdapter, smt::SMTHelper};

/// Helper struct for working with the BST database
pub struct BSTHelper {
    db: Arc<rocksdb::DB>,
    smt_helper: SMTHelper,
}

impl BSTHelper {
    /// Create a new BST helper
    pub fn new(db: Arc<rocksdb::DB>) -> Self {
        let smt_helper = SMTHelper::new(db.clone());
        Self { db, smt_helper }
    }

    /// Choose a key to look up in the database and get its current value
    pub fn get_current_value(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Get the current tip height
        let tip_height = self.get_tip_height()?;
        self.get_value_at_height(key, tip_height)
    }

    /// Get the value of a key at a specific block height using binary search
    pub fn get_value_at_height(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>> {
        match self.smt_helper.bst_get_at_height(key, height)? {
            Some(value) => {
                // Remove height annotation if present (last 4 bytes)
                if value.len() >= 4 {
                    Ok(Some(value[..value.len()-4].to_vec()))
                } else {
                    Ok(Some(value))
                }
            },
            None => Ok(None)
        }
    }

    /// Iterate backwards from the most recent update to visit all values of a key
    /// Returns tuples of (height, value) in descending height order
    pub fn iterate_backwards(&self, key: &[u8], from_height: Option<u32>) -> Result<Vec<(u32, Vec<u8>)>> {
        let start_height = match from_height {
            Some(h) => h,
            None => self.get_tip_height()?,
        };

        let results = self.smt_helper.bst_iterate_backwards(key, start_height)?;
        
        // Remove height annotations from values
        let mut clean_results = Vec::new();
        for (height, value) in results {
            let clean_value = if value.len() >= 4 {
                value[..value.len()-4].to_vec()
            } else {
                value
            };
            clean_results.push((height, clean_value));
        }
        
        Ok(clean_results)
    }

    /// Binary search to find the value of a key at any given point in time
    /// This is more efficient than iterating when you know the target height
    pub fn binary_search_at_height(&self, key: &[u8], target_height: u32) -> Result<Option<Vec<u8>>> {
        self.get_value_at_height(key, target_height)
    }

    /// Get all keys that were touched at a specific block height
    pub fn get_keys_touched_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>> {
        self.smt_helper.get_keys_at_height(height)
    }

    /// Get all heights at which a key was updated
    pub fn get_key_update_heights(&self, key: &[u8]) -> Result<Vec<u32>> {
        self.smt_helper.bst_get_heights_for_key(key)
    }

    /// Tree traversal: get all nodes for a key across all heights
    pub fn traverse_key_nodes(&self, key: &[u8]) -> Result<Vec<(u32, Vec<u8>)>> {
        self.iterate_backwards(key, None)
    }

    /// Get the current tip height from the database
    pub fn get_tip_height(&self) -> Result<u32> {
        let height_key = crate::runtime::TIP_HEIGHT_KEY.as_bytes();
        match self.db.get(crate::to_labeled_key(&height_key.to_vec()))? {
            Some(bytes) => {
                if bytes.len() >= 4 {
                    Ok(u32::from_le_bytes(bytes[..4].try_into().unwrap()))
                } else {
                    Ok(0)
                }
            },
            None => Ok(0),
        }
    }

    /// Get the current state root (merkle root of entire state)
    pub fn get_current_state_root(&self) -> Result<[u8; 32]> {
        self.smt_helper.get_current_state_root()
    }

    /// Get the state root at a specific height
    pub fn get_state_root_at_height(&self, height: u32) -> Result<[u8; 32]> {
        self.smt_helper.get_smt_root_at_height(height)
    }

    /// Perform a complete rollback to a specific height
    /// This handles reorg by rolling back all state changes after the target height
    pub fn rollback_to_height(&self, target_height: u32) -> Result<()> {
        log::info!("Rolling back database state to height {}", target_height);
        
        // Rollback all BST entries
        self.smt_helper.bst_rollback_to_height(target_height)?;
        
        // Recalculate and store the state root
        self.smt_helper.calculate_and_store_state_root(target_height)?;
        
        log::info!("Rollback completed to height {}", target_height);
        Ok(())
    }

    /// Iteratively walk back through all BST structures at each key to roll back state
    /// This provides complete reorg handling
    pub fn complete_reorg_rollback(&self, target_height: u32) -> Result<()> {
        log::info!("Starting complete reorg rollback to height {}", target_height);
        
        // Get all heights that need to be rolled back
        let current_height = self.get_tip_height()?;
        if target_height >= current_height {
            log::info!("No rollback needed: target height {} >= current height {}", target_height, current_height);
            return Ok(());
        }

        // For each height from current down to target+1, get all keys and roll them back
        for height in ((target_height + 1)..=current_height).rev() {
            log::debug!("Rolling back height {}", height);
            
            let keys = self.get_keys_touched_at_height(height)?;
            log::debug!("Found {} keys to rollback at height {}", keys.len(), height);
            
            for key in keys {
                // Roll back this specific key
                self.rollback_key_to_height(&key, target_height)?;
            }
        }

        // Recalculate the state root at the target height
        self.smt_helper.calculate_and_store_state_root(target_height)?;
        
        log::info!("Complete reorg rollback finished");
        Ok(())
    }

    /// Roll back a specific key to its state before a target height
    fn rollback_key_to_height(&self, key: &[u8], target_height: u32) -> Result<()> {
        self.smt_helper.bst_rollback_key(key, target_height)
    }

    /// Verify the integrity of the BST structure
    pub fn verify_integrity(&self) -> Result<bool> {
        log::info!("Verifying BST integrity");
        
        // Get current state root
        let current_root = self.get_current_state_root()?;
        
        // Recalculate state root and compare
        let tip_height = self.get_tip_height()?;
        let calculated_root = self.smt_helper.calculate_and_store_state_root(tip_height)?;
        
        let is_valid = current_root == calculated_root;
        
        if is_valid {
            log::info!("BST integrity verification passed");
        } else {
            log::error!("BST integrity verification failed: stored root {:?} != calculated root {:?}", 
                       current_root, calculated_root);
        }
        
        Ok(is_valid)
    }

    /// Get statistics about the BST structure
    pub fn get_statistics(&self) -> Result<BSTStatistics> {
        let tip_height = self.get_tip_height()?;
        let current_root = self.get_current_state_root()?;
        
        // Count total keys by scanning the database
        let mut total_keys = 0;
        let mut total_entries = 0;
        
        let prefix = crate::smt::BST_HEIGHT_PREFIX;
        let mut iter = self.db.raw_iterator();
        iter.seek(prefix.as_bytes());
        
        let mut unique_keys = std::collections::HashSet::new();
        
        while iter.valid() {
            if let Some(db_key) = iter.key() {
                if !db_key.starts_with(prefix.as_bytes()) {
                    break;
                }
                
                total_entries += 1;
                
                // Extract the original key
                let key_str = String::from_utf8_lossy(db_key);
                if let Some(rest) = key_str.strip_prefix(prefix) {
                    if let Some(colon_pos) = rest.find(':') {
                        let hex_key = &rest[..colon_pos];
                        if let Ok(original_key) = hex::decode(hex_key) {
                            unique_keys.insert(original_key);
                        }
                    }
                }
            }
            iter.next();
        }
        
        total_keys = unique_keys.len();
        
        Ok(BSTStatistics {
            tip_height,
            total_keys,
            total_entries,
            current_state_root: current_root,
        })
    }
}

/// Statistics about the BST structure
#[derive(Debug)]
pub struct BSTStatistics {
    pub tip_height: u32,
    pub total_keys: usize,
    pub total_entries: usize,
    pub current_state_root: [u8; 32],
}

impl std::fmt::Display for BSTStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, 
            "BST Statistics:\n  Tip Height: {}\n  Total Keys: {}\n  Total Entries: {}\n  State Root: {}",
            self.tip_height,
            self.total_keys, 
            self.total_entries,
            hex::encode(self.current_state_root)
        )
    }
}

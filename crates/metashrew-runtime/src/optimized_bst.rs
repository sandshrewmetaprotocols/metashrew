//! Optimized BST implementation for efficient key-value operations
//!
//! This module provides an optimized BST that:
//! - Stores the most recent value directly for O(1) current state access
//! - Only uses binary search for historical queries
//! - Properly tracks keys per block for efficient reorg handling
//! - Maintains append-only structure for historical queries

use crate::traits::{BatchLike, KeyValueStoreLike};
use anyhow::Result;
use log::{info, trace};

// Prefixes for different types of keys in the database
pub const CURRENT_VALUE_PREFIX: &str = "current:";
pub const HISTORICAL_VALUE_PREFIX: &str = "hist:";
pub const HEIGHT_INDEX_PREFIX: &str = "height:";
pub const KEYS_AT_HEIGHT_PREFIX: &str = "keys:";

/// Optimized BST helper that provides fast current state access
/// and efficient historical queries
pub struct OptimizedBST<T: KeyValueStoreLike> {
    storage: T,
}

impl<T: KeyValueStoreLike> OptimizedBST<T> {
    pub fn new(storage: T) -> Self {
        Self { storage }
    }

    /// Store a key-value pair at a specific height
    /// This is the main write operation used during indexing
    pub fn put(&mut self, key: &[u8], value: &[u8], height: u32) -> Result<()> {
        let mut batch = self.storage.create_batch();

        // 1. Store the current value for O(1) access
        let current_key = format!("{}{}", CURRENT_VALUE_PREFIX, hex::encode(key)).into_bytes();
        batch.put(&current_key, value);

        // 2. Store the historical value for historical queries
        let historical_key =
            format!("{}{}:{}", HISTORICAL_VALUE_PREFIX, hex::encode(key), height).into_bytes();
        batch.put(&historical_key, value);

        // 3. Track that this key was updated at this height
        let height_index_key =
            format!("{}{}:{}", HEIGHT_INDEX_PREFIX, height, hex::encode(key)).into_bytes();
        batch.put(&height_index_key, b"");

        // 4. Track keys updated at this height for reorg handling
        let keys_at_height_key =
            format!("{}{}:{}", KEYS_AT_HEIGHT_PREFIX, height, hex::encode(key)).into_bytes();
        batch.put(&keys_at_height_key, b"");

        // Write the batch atomically
        self.storage.write_batch(batch)?;

        trace!(
            "Stored key {} at height {} with optimized BST",
            hex::encode(key),
            height
        );
        Ok(())
    }

    /// Get the current (most recent) value of a key - O(1) operation
    /// This should be used for all current state queries during indexing
    pub fn get_current(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let current_key = format!("{}{}", CURRENT_VALUE_PREFIX, hex::encode(key)).into_bytes();

        match self
            .storage
            .get_immutable(&current_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            Some(value) => {
                trace!(
                    "Retrieved current value for key {} (O(1) lookup)",
                    hex::encode(key)
                );
                Ok(Some(value))
            }
            None => {
                trace!("No current value found for key {}", hex::encode(key));
                Ok(None)
            }
        }
    }

    /// Get the value of a key at a specific height - uses binary search only when needed
    /// This should only be used for historical queries (view functions with specific heights)
    pub fn get_at_height(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>> {
        trace!("OptimizedBST: get_at_height for key {} at height {}", hex::encode(key), height);
        
        // For genesis block (height 0), we need to be more careful
        // During indexing of the genesis block, keys might not exist yet
        if height == 0 {
            // First try direct lookup at height 0
            let exact_key =
                format!("{}{}:{}", HISTORICAL_VALUE_PREFIX, hex::encode(key), height).into_bytes();
            if let Some(value) = self
                .storage
                .get_immutable(&exact_key)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
            {
                trace!("Found exact value for key {} at genesis height 0", hex::encode(key));
                return Ok(Some(value));
            }
            
            // For genesis block, if no exact match, return None (key doesn't exist yet)
            trace!("No value found for key {} at genesis height 0", hex::encode(key));
            return Ok(None);
        }

        // Note: We removed the current value check optimization here because during indexing,
        // the WASM module needs to be able to query keys that don't exist yet and get None/empty results.
        // The optimization was preventing proper indexing behavior.

        // First try direct lookup at the exact height
        let exact_key =
            format!("{}{}:{}", HISTORICAL_VALUE_PREFIX, hex::encode(key), height).into_bytes();
        if let Some(value) = self
            .storage
            .get_immutable(&exact_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            trace!(
                "Found exact value for key {} at height {} (direct lookup)",
                hex::encode(key),
                height
            );
            return Ok(Some(value));
        }

        // If not found at exact height, use binary search to find the most recent value
        // at or before the requested height (only if we know the key exists)
        trace!(
            "Using binary search for key {} at height {} (key exists)",
            hex::encode(key),
            height
        );

        let mut low = 0u32;
        let mut high = height;
        let mut best_height: Option<u32> = None;

        while low <= high {
            let mid = low + (high - low) / 2;
            let mid_key =
                format!("{}{}:{}", HISTORICAL_VALUE_PREFIX, hex::encode(key), mid).into_bytes();

            if let Some(_) = self
                .storage
                .get_immutable(&mid_key)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
            {
                // Found a value at this height, but continue searching for a more recent one
                best_height = Some(mid);
                if mid == u32::MAX {
                    break; // Prevent overflow
                }
                low = mid + 1;
            } else {
                // Handle underflow when mid is 0
                if mid == 0 {
                    break;
                }
                high = mid - 1;
            }
            
            // Additional safety check to prevent infinite loops
            if low > high {
                break;
            }
        }

        if let Some(found_height) = best_height {
            let found_key = format!(
                "{}{}:{}",
                HISTORICAL_VALUE_PREFIX,
                hex::encode(key),
                found_height
            )
            .into_bytes();
            if let Some(value) = self
                .storage
                .get_immutable(&found_key)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
            {
                trace!(
                    "Found value for key {} at height {} (binary search result)",
                    hex::encode(key),
                    found_height
                );
                return Ok(Some(value));
            }
        }

        trace!(
            "No value found for key {} at or before height {}",
            hex::encode(key),
            height
        );
        Ok(None)
    }

    /// Get all keys that were updated at a specific height
    /// This is used for reorg handling
    pub fn get_keys_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>> {
        let prefix = format!("{}{}:", KEYS_AT_HEIGHT_PREFIX, height);
        let mut keys = Vec::new();

        // Get all keys with this prefix
        for (key, _) in self.storage.scan_prefix(prefix.as_bytes())? {
            let key_str = String::from_utf8_lossy(&key);
            if let Some(hex_key) = key_str.strip_prefix(&prefix) {
                if let Ok(original_key) = hex::decode(hex_key) {
                    keys.push(original_key);
                }
            }
        }

        trace!("Found {} keys updated at height {}", keys.len(), height);
        Ok(keys)
    }

    /// Rollback a specific key to its state before a target height
    /// This removes all updates after the target height
    pub fn rollback_key(&mut self, key: &[u8], target_height: u32) -> Result<()> {
        let mut batch = self.storage.create_batch();

        // Get all heights for this key
        let heights = self.get_heights_for_key(key)?;

        // Find the most recent value at or before target_height
        let mut latest_valid_value: Option<Vec<u8>> = None;
        let mut latest_valid_height: Option<u32> = None;

        for &height in &heights {
            if height <= target_height {
                let historical_key =
                    format!("{}{}:{}", HISTORICAL_VALUE_PREFIX, hex::encode(key), height)
                        .into_bytes();
                if let Some(value) = self
                    .storage
                    .get_immutable(&historical_key)
                    .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
                {
                    latest_valid_value = Some(value);
                    latest_valid_height = Some(height);
                }
            } else {
                // Remove this historical entry
                let historical_key =
                    format!("{}{}:{}", HISTORICAL_VALUE_PREFIX, hex::encode(key), height)
                        .into_bytes();
                batch.delete(&historical_key);

                // Remove from height index
                let height_index_key =
                    format!("{}{}:{}", HEIGHT_INDEX_PREFIX, height, hex::encode(key)).into_bytes();
                batch.delete(&height_index_key);

                // Remove from keys-at-height tracking
                let keys_at_height_key =
                    format!("{}{}:{}", KEYS_AT_HEIGHT_PREFIX, height, hex::encode(key))
                        .into_bytes();
                batch.delete(&keys_at_height_key);
            }
        }

        // Update the current value to the latest valid value
        let current_key = format!("{}{}", CURRENT_VALUE_PREFIX, hex::encode(key)).into_bytes();
        if let Some(value) = latest_valid_value {
            batch.put(&current_key, &value);
            trace!(
                "Rolled back key {} to height {} with value",
                hex::encode(key),
                latest_valid_height.unwrap_or(0)
            );
        } else {
            // No valid value found, remove the current entry
            batch.delete(&current_key);
            trace!(
                "Rolled back key {} - no valid value found, removed current entry",
                hex::encode(key)
            );
        }

        self.storage
            .write_batch(batch)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
        Ok(())
    }

    /// Rollback all keys to their state before a target height
    /// This is the main reorg handling function
    pub fn rollback_to_height(&mut self, target_height: u32) -> Result<()> {
        info!(
            "Starting optimized BST rollback to height {}",
            target_height
        );

        // Get all heights that need to be rolled back
        let mut heights_to_rollback = Vec::new();

        // Scan for all heights greater than target_height
        let prefix = format!("{}:", KEYS_AT_HEIGHT_PREFIX);

        // Get all keys with this prefix and extract heights
        for (key, _) in self.storage.scan_prefix(prefix.as_bytes())? {
            let key_str = String::from_utf8_lossy(&key);
            if let Some(rest) = key_str.strip_prefix(&prefix) {
                if let Some(colon_pos) = rest.find(':') {
                    let height_str = &rest[..colon_pos];
                    if let Ok(height) = height_str.parse::<u32>() {
                        if height > target_height {
                            heights_to_rollback.push(height);
                        }
                    }
                }
            }
        }

        // Remove duplicates and sort
        heights_to_rollback.sort();
        heights_to_rollback.dedup();

        info!(
            "Found {} heights to rollback: {:?}",
            heights_to_rollback.len(),
            heights_to_rollback
        );

        // Rollback each height
        for height in heights_to_rollback {
            let keys = self.get_keys_at_height(height)?;
            info!("Rolling back {} keys at height {}", keys.len(), height);

            for key in keys {
                self.rollback_key(&key, target_height)?;
            }
        }

        info!(
            "Optimized BST rollback completed to height {}",
            target_height
        );
        Ok(())
    }

    /// Get all heights at which a key was updated
    pub fn get_heights_for_key(&self, key: &[u8]) -> Result<Vec<u32>> {
        let mut heights = Vec::new();
        let prefix = format!("{}{}:", HISTORICAL_VALUE_PREFIX, hex::encode(key));

        // Get all keys with this prefix and extract heights
        for (key, _) in self.storage.scan_prefix(prefix.as_bytes())? {
            let key_str = String::from_utf8_lossy(&key);
            if let Some(height_str) = key_str.strip_prefix(&prefix) {
                if let Ok(height) = height_str.parse::<u32>() {
                    heights.push(height);
                }
            }
        }

        heights.sort();
        Ok(heights)
    }

    /// Iterate backwards through all values of a key from most recent
    pub fn iterate_backwards(&self, key: &[u8], from_height: u32) -> Result<Vec<(u32, Vec<u8>)>> {
        let heights = self.get_heights_for_key(key)?;
        let mut results = Vec::new();

        // Filter heights to only include those <= from_height and sort in descending order
        let mut filtered_heights: Vec<u32> =
            heights.into_iter().filter(|&h| h <= from_height).collect();
        filtered_heights.sort_by(|a, b| b.cmp(a)); // Descending order

        for height in filtered_heights {
            let historical_key =
                format!("{}{}:{}", HISTORICAL_VALUE_PREFIX, hex::encode(key), height).into_bytes();
            if let Some(value) = self
                .storage
                .get_immutable(&historical_key)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
            {
                results.push((height, value));
            }
        }

        Ok(results)
    }

    /// Get all current keys in the OptimizedBST
    pub fn get_all_current_keys(&self) -> Result<Vec<Vec<u8>>> {
        let mut keys = Vec::new();

        // Iterate over all current values
        let current_prefix = CURRENT_VALUE_PREFIX;

        // Get all keys with this prefix
        for (key, _) in self.storage.scan_prefix(current_prefix.as_bytes())? {
            // Extract the original key by removing the prefix
            if key.len() > current_prefix.len() {
                let original_key_hex = &key[current_prefix.len()..];
                if let Ok(original_key) = hex::decode(original_key_hex) {
                    keys.push(original_key);
                }
            }
        }

        trace!("Retrieved {} current keys from OptimizedBST", keys.len());
        Ok(keys)
    }

    /// Get statistics about the optimized BST
    pub fn get_statistics(&self) -> Result<OptimizedBSTStatistics> {
        let mut current_keys = 0;
        let mut historical_entries = 0;

        // Count current keys
        let current_prefix = CURRENT_VALUE_PREFIX;
        for (_, _) in self.storage.scan_prefix(current_prefix.as_bytes())? {
            current_keys += 1;
        }

        // Count historical entries
        let historical_prefix = HISTORICAL_VALUE_PREFIX;
        for (_, _) in self.storage.scan_prefix(historical_prefix.as_bytes())? {
            historical_entries += 1;
        }

        Ok(OptimizedBSTStatistics {
            current_keys,
            historical_entries,
        })
    }
}

/// Statistics about the optimized BST structure
#[derive(Debug)]
pub struct OptimizedBSTStatistics {
    pub current_keys: usize,
    pub historical_entries: usize,
}

impl std::fmt::Display for OptimizedBSTStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Optimized BST Statistics:\n  Current Keys: {}\n  Historical Entries: {}",
            self.current_keys, self.historical_entries
        )
    }
}

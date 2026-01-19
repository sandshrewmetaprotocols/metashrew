//! SMT Rollback Trait and Implementation
//!
//! This module provides a trait for properly rolling back Sparse Merkle Tree (SMT) data
//! during blockchain reorganizations. Both RocksDB and in-memory storage adapters must
//! implement this trait to ensure consistent reorg handling.

use anyhow::Result;
use log::{debug, info};
use std::collections::HashSet;

/// Trait for rolling back SMT data during blockchain reorganizations
pub trait SmtRollback {
    /// Get all keys from storage
    fn get_all_keys(&self) -> Result<Vec<Vec<u8>>>;

    /// Delete a key from storage
    fn delete_key(&mut self, key: &[u8]) -> Result<()>;

    /// Put a key-value pair into storage
    fn put_key(&mut self, key: &[u8], value: &[u8]) -> Result<()>;

    /// Get value for a key
    fn get_value(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
}

/// Parse height from an SMT update key
///
/// SMT keys are stored as: `base_key/index`
/// SMT values are stored as: `height:data`
///
/// This function extracts the height from the value.
fn parse_height_from_smt_value(value: &[u8]) -> Option<u32> {
    let value_str = String::from_utf8_lossy(value);
    if let Some(colon_pos) = value_str.find(':') {
        let height_str = &value_str[..colon_pos];
        height_str.parse::<u32>().ok()
    } else {
        None
    }
}

/// Roll back SMT data to a specific height
///
/// This is the correct implementation that BOTH RocksDB and MemStore should use.
///
/// The rollback process:
/// 1. Delete metadata keys (block_hash_*, state_root_*, smt:root:*) for heights > rollback_height
/// 2. Roll back append-only SMT data structures (keys with /length suffix)
/// 3. This ensures all WASM-indexed data is properly cleaned up during reorgs
pub fn rollback_smt_data<S: SmtRollback>(
    storage: &mut S,
    rollback_height: u32,
    current_height: u32,
) -> Result<()> {
    info!("Starting SMT rollback from height {} to height {}", current_height, rollback_height);

    if rollback_height >= current_height {
        debug!("Rollback height {} >= current height {}, nothing to do", rollback_height, current_height);
        return Ok(());
    }

    let all_keys = storage.get_all_keys()?;

    // --- Step 1: Delete metadata keys for heights > rollback_height ---
    let mut metadata_keys_deleted = 0;
    for key in &all_keys {
        let key_str = String::from_utf8_lossy(key);

        // Check for metadata keys and parse their height
        let metadata_height = if let Some(stripped) = key_str.strip_prefix("block_hash_") {
            stripped.parse::<u32>().ok()
        } else if let Some(stripped) = key_str.strip_prefix("state_root_") {
            stripped.parse::<u32>().ok()
        } else if let Some(stripped) = key_str.strip_prefix("smt:root:") {
            stripped.parse::<u32>().ok()
        } else {
            None
        };

        if let Some(h) = metadata_height {
            if h > rollback_height {
                storage.delete_key(key)?;
                metadata_keys_deleted += 1;
                debug!("Deleted metadata key for height {}", h);
            }
        }
    }
    info!("Deleted {} metadata keys", metadata_keys_deleted);

    // --- Step 2: Roll back append-only SMT data structures ---
    let length_suffix = b"/length";
    let mut base_keys = HashSet::new();

    // Find all "base" keys by looking for keys ending in "/length"
    for key in &all_keys {
        if key.ends_with(length_suffix) {
            let base_key = &key[..key.len() - length_suffix.len()];
            base_keys.insert(base_key.to_vec());
        }
    }

    let mut smt_structures_rolled_back = 0;
    for base_key in base_keys {
        let mut length_key = base_key.clone();
        length_key.extend_from_slice(length_suffix);

        // Read the current length
        let old_length = if let Some(length_bytes) = storage.get_value(&length_key)? {
            String::from_utf8_lossy(&length_bytes).parse::<u32>().unwrap_or(0)
        } else {
            continue;
        };

        // Collect valid updates (height <= rollback_height)
        let mut valid_updates = Vec::new();
        for i in 0..old_length {
            let update_key_suffix = format!("/{}", i);
            let mut update_key = base_key.clone();
            update_key.extend_from_slice(update_key_suffix.as_bytes());

            if let Some(update_data) = storage.get_value(&update_key)? {
                if let Some(update_height) = parse_height_from_smt_value(&update_data) {
                    if update_height <= rollback_height {
                        valid_updates.push((i, update_data));
                    } else {
                        debug!("Removing SMT update at height {} (> {})", update_height, rollback_height);
                    }
                }
            }
        }

        // Remove all old entries
        for i in 0..old_length {
            let update_key_suffix = format!("/{}", i);
            let mut update_key = base_key.clone();
            update_key.extend_from_slice(update_key_suffix.as_bytes());
            storage.delete_key(&update_key)?;
        }

        // Re-insert valid entries with compacted indices
        for (new_index, (_, update_data)) in valid_updates.iter().enumerate() {
            let update_key_suffix = format!("/{}", new_index);
            let mut update_key = base_key.clone();
            update_key.extend_from_slice(update_key_suffix.as_bytes());
            storage.put_key(&update_key, update_data)?;
        }

        // Update or remove the length key
        let new_length = valid_updates.len() as u32;
        if new_length > 0 {
            storage.put_key(&length_key, new_length.to_string().as_bytes())?;
            debug!("SMT structure {} compacted from {} to {} entries", String::from_utf8_lossy(&base_key), old_length, new_length);
        } else {
            storage.delete_key(&length_key)?;
            debug!("SMT structure {} completely removed (no valid entries)", String::from_utf8_lossy(&base_key));
        }
        smt_structures_rolled_back += 1;
    }

    info!("Successfully rolled back {} SMT data structures to height {}", smt_structures_rolled_back, rollback_height);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockStorage {
        data: HashMap<Vec<u8>, Vec<u8>>,
    }

    impl SmtRollback for MockStorage {
        fn get_all_keys(&self) -> Result<Vec<Vec<u8>>> {
            Ok(self.data.keys().cloned().collect())
        }

        fn delete_key(&mut self, key: &[u8]) -> Result<()> {
            self.data.remove(key);
            Ok(())
        }

        fn put_key(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
            self.data.insert(key.to_vec(), value.to_vec());
            Ok(())
        }

        fn get_value(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
            Ok(self.data.get(key).cloned())
        }
    }

    #[test]
    fn test_rollback_metadata_keys() {
        let mut storage = MockStorage {
            data: HashMap::new(),
        };

        // Insert metadata for heights 3, 4, 5
        storage.data.insert(b"block_hash_3".to_vec(), b"hash3".to_vec());
        storage.data.insert(b"block_hash_4".to_vec(), b"hash4".to_vec());
        storage.data.insert(b"block_hash_5".to_vec(), b"hash5".to_vec());
        storage.data.insert(b"smt:root:3".to_vec(), b"root3".to_vec());
        storage.data.insert(b"smt:root:4".to_vec(), b"root4".to_vec());
        storage.data.insert(b"smt:root:5".to_vec(), b"root5".to_vec());

        // Rollback to height 3
        rollback_smt_data(&mut storage, 3, 5).unwrap();

        // Heights 4 and 5 should be deleted, height 3 should remain
        assert!(storage.data.contains_key(b"block_hash_3".as_ref()));
        assert!(!storage.data.contains_key(b"block_hash_4".as_ref()));
        assert!(!storage.data.contains_key(b"block_hash_5".as_ref()));
        assert!(storage.data.contains_key(b"smt:root:3".as_ref()));
        assert!(!storage.data.contains_key(b"smt:root:4".as_ref()));
        assert!(!storage.data.contains_key(b"smt:root:5".as_ref()));
    }

    #[test]
    fn test_parse_height_from_smt_value() {
        assert_eq!(parse_height_from_smt_value(b"123:data"), Some(123));
        assert_eq!(parse_height_from_smt_value(b"0:data"), Some(0));
        assert_eq!(parse_height_from_smt_value(b"noheight"), None);
        assert_eq!(parse_height_from_smt_value(b":data"), None);
    }
}

//! SMT Rollback Trait and Implementation
//!
//! This module provides a trait for properly rolling back Sparse Merkle Tree (SMT) data
//! during blockchain reorganizations. Both RocksDB and in-memory storage adapters must
//! implement this trait to ensure consistent reorg handling.

use anyhow::Result;
use log::{debug, info, warn};
use crate::smt::{deserialize_key_manifest, REORG_HEIGHT_KEY};

/// Trait for rolling back SMT data during blockchain reorganizations
pub trait SmtRollback {
    /// Iterate over all keys in storage (streaming, memory-efficient)
    fn iter_keys<F>(&self, callback: F) -> Result<()>
    where
        F: FnMut(&[u8]) -> Result<()>;

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

    // --- Step 1: Collect metadata keys to delete (streaming) ---
    let mut metadata_keys_to_delete = Vec::new();
    storage.iter_keys(|key| {
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
                metadata_keys_to_delete.push(key.to_vec());
            }
        }
        Ok(())
    })?;

    // Delete metadata keys
    for key in &metadata_keys_to_delete {
        storage.delete_key(key)?;
    }
    info!("Deleted {} metadata keys", metadata_keys_to_delete.len());

    // --- Step 2: Collect base keys for SMT structures (streaming) ---
    let length_suffix = b"/length";
    let mut base_keys = Vec::new(); // Use Vec instead of HashSet to save memory
    let max_keys_to_collect = 100_000; // Safety limit to prevent OOM

    info!("Scanning for SMT structures to roll back...");
    let mut total_length_keys_found = 0;
    storage.iter_keys(|key| {
        if key.ends_with(length_suffix) {
            total_length_keys_found += 1;
            let base_key = &key[..key.len() - length_suffix.len()];

            // Only collect up to max limit to prevent OOM
            if base_keys.len() < max_keys_to_collect {
                base_keys.push(base_key.to_vec());
            }

            // Log progress every 10000 keys
            if total_length_keys_found % 10000 == 0 {
                info!("Found {} SMT /length keys so far...", total_length_keys_found);
            }
        }
        Ok(())
    })?;

    if total_length_keys_found > max_keys_to_collect {
        warn!("WARNING: Found {} SMT structures, but can only process {} at a time due to memory limits.",
              total_length_keys_found, max_keys_to_collect);
        warn!("Consider implementing multi-pass rollback for very large databases.");
    }

    // Deduplicate and sort for consistent processing
    base_keys.sort_unstable();
    base_keys.dedup();

    info!("Collected {} unique SMT structures to process (found {} total /length keys)",
          base_keys.len(), total_length_keys_found);

    // Process structures in batches to limit memory usage
    let batch_size = 100; // Process 100 structures at a time
    let mut smt_structures_rolled_back = 0;

    for (batch_idx, batch) in base_keys.chunks(batch_size).enumerate() {
        info!("Processing batch {} ({} structures)...", batch_idx + 1, batch.len());

        for base_key in batch {
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

            if smt_structures_rolled_back % 1000 == 0 {
                info!("Rolled back {} SMT structures so far...", smt_structures_rolled_back);
            }
        }
    }

    info!("Successfully rolled back {} SMT data structures to height {}", smt_structures_rolled_back, rollback_height);
    Ok(())
}

/// Check if key manifests exist for all heights in the given range (exclusive of rollback_height).
/// Returns true if all manifests exist (meaning deferred rollback can be used).
pub fn manifests_exist_for_range<S: SmtRollback>(
    storage: &S,
    rollback_height: u32,
    current_height: u32,
) -> bool {
    for h in (rollback_height + 1)..=current_height {
        let manifest_key = format!("/__INTERNAL/keys-at-height/{}", h).into_bytes();
        match storage.get_value(&manifest_key) {
            Ok(Some(_)) => continue,
            _ => return false,
        }
    }
    true
}

/// Deferred (zero-mutation) rollback: only updates lightweight metadata.
///
/// Instead of scanning and mutating all k/v entries:
/// 1. Set `/__INTERNAL/reorg-height` = min(existing, rollback_height)
/// 2. Delete `smt:root:{h}` for h > rollback_height
/// 3. Delete manifest keys for h > rollback_height
/// 4. Update indexed height to rollback_height
///
/// Orphaned entries are handled lazily in the read path via blockhash validation.
pub fn rollback_deferred<S: SmtRollback>(
    storage: &mut S,
    rollback_height: u32,
    current_height: u32,
) -> Result<()> {
    info!(
        "Starting DEFERRED rollback from height {} to height {} (zero k/v mutations)",
        current_height, rollback_height
    );

    if rollback_height >= current_height {
        debug!(
            "Rollback height {} >= current height {}, nothing to do",
            rollback_height, current_height
        );
        return Ok(());
    }

    // 1. Set reorg-height = min(existing, rollback_height + 1)
    //    The +1 is because entries AT rollback_height are valid (they were committed on the canonical chain),
    //    but entries at rollback_height+1 and above may be orphaned.
    let new_reorg_height = rollback_height + 1;
    let reorg_key = REORG_HEIGHT_KEY.as_bytes();
    let existing_reorg_height = match storage.get_value(reorg_key)? {
        Some(bytes) if bytes.len() >= 4 => {
            Some(u32::from_le_bytes(bytes[..4].try_into().unwrap()))
        }
        _ => None,
    };

    let final_reorg_height = match existing_reorg_height {
        Some(existing) => std::cmp::min(existing, new_reorg_height),
        None => new_reorg_height,
    };

    storage.put_key(reorg_key, &final_reorg_height.to_le_bytes())?;
    info!(
        "Set reorg-height marker to {} (was {:?})",
        final_reorg_height, existing_reorg_height
    );

    // 2. Delete smt:root:{h} for h > rollback_height — O(reorg_depth)
    let mut roots_deleted = 0;
    for h in (rollback_height + 1)..=current_height {
        let root_key = format!("smt:root:{}", h).into_bytes();
        storage.delete_key(&root_key)?;
        roots_deleted += 1;
    }
    info!("Deleted {} orphaned SMT root keys", roots_deleted);

    // 3. Delete manifest keys for h > rollback_height (optional cleanup)
    for h in (rollback_height + 1)..=current_height {
        let manifest_key = format!("/__INTERNAL/keys-at-height/{}", h).into_bytes();
        storage.delete_key(&manifest_key)?;
    }

    // 4. Delete metadata keys (block_hash_*, state_root_*) for heights > rollback_height
    //    These are small and bounded by reorg depth.
    let mut metadata_keys_to_delete = Vec::new();
    storage.iter_keys(|key| {
        let key_str = String::from_utf8_lossy(key);
        let metadata_height = if let Some(stripped) = key_str.strip_prefix("block_hash_") {
            stripped.parse::<u32>().ok()
        } else if let Some(stripped) = key_str.strip_prefix("state_root_") {
            stripped.parse::<u32>().ok()
        } else {
            None
        };

        if let Some(h) = metadata_height {
            if h > rollback_height {
                metadata_keys_to_delete.push(key.to_vec());
            }
        }
        Ok(())
    })?;
    for key in &metadata_keys_to_delete {
        storage.delete_key(key)?;
    }
    info!("Deleted {} metadata keys", metadata_keys_to_delete.len());

    info!(
        "Deferred rollback complete. Orphaned entries will be filtered on read."
    );
    Ok(())
}

/// Targeted rollback using per-height key manifests.
///
/// Instead of scanning ALL keys, only processes keys listed in the manifests
/// for the heights being rolled back. This is O(keys_touched * reorg_depth)
/// instead of O(total_keys).
pub fn rollback_smt_data_targeted<S: SmtRollback>(
    storage: &mut S,
    rollback_height: u32,
    current_height: u32,
) -> Result<()> {
    info!(
        "Starting TARGETED rollback from height {} to height {} using manifests",
        current_height, rollback_height
    );

    if rollback_height >= current_height {
        return Ok(());
    }

    // Delete metadata keys for heights > rollback_height
    let mut metadata_keys_to_delete = Vec::new();
    storage.iter_keys(|key| {
        let key_str = String::from_utf8_lossy(key);
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
                metadata_keys_to_delete.push(key.to_vec());
            }
        }
        Ok(())
    })?;
    for key in &metadata_keys_to_delete {
        storage.delete_key(key)?;
    }
    info!("Deleted {} metadata keys", metadata_keys_to_delete.len());

    // Collect all unique keys from manifests for heights > rollback_height
    let mut keys_to_rollback = std::collections::HashSet::new();
    for h in (rollback_height + 1)..=current_height {
        let manifest_key = format!("/__INTERNAL/keys-at-height/{}", h).into_bytes();
        if let Some(manifest_data) = storage.get_value(&manifest_key)? {
            let keys = deserialize_key_manifest(&manifest_data)?;
            for key in keys {
                keys_to_rollback.insert(key);
            }
        }
        // Delete the manifest itself
        storage.delete_key(&manifest_key)?;
    }

    info!(
        "Found {} unique keys to roll back from manifests",
        keys_to_rollback.len()
    );

    // Roll back each key using the same logic as rollback_smt_data
    let length_suffix = b"/length";
    for base_key in &keys_to_rollback {
        let mut length_key = base_key.clone();
        length_key.extend_from_slice(length_suffix);

        let old_length = if let Some(length_bytes) = storage.get_value(&length_key)? {
            String::from_utf8_lossy(&length_bytes)
                .parse::<u32>()
                .unwrap_or(0)
        } else {
            continue;
        };

        let mut valid_updates = Vec::new();
        for i in 0..old_length {
            let update_key_suffix = format!("/{}", i);
            let mut update_key = base_key.clone();
            update_key.extend_from_slice(update_key_suffix.as_bytes());

            if let Some(update_data) = storage.get_value(&update_key)? {
                if let Some(update_height) = parse_height_from_smt_value(&update_data) {
                    if update_height <= rollback_height {
                        valid_updates.push((i, update_data));
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

        let new_length = valid_updates.len() as u32;
        if new_length > 0 {
            storage.put_key(&length_key, new_length.to_string().as_bytes())?;
        } else {
            storage.delete_key(&length_key)?;
        }
    }

    info!(
        "Targeted rollback complete. Processed {} keys.",
        keys_to_rollback.len()
    );
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
        fn iter_keys<F>(&self, mut callback: F) -> Result<()>
        where
            F: FnMut(&[u8]) -> Result<()>,
        {
            for key in self.data.keys() {
                callback(key)?;
            }
            Ok(())
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

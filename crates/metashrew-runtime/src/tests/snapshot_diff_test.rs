//! Test to verify snapshot diff creation works correctly with append-only approach
//!
//! This test validates that the snapshot system creates proper diff files
//! when using the new append-only storage format.

use crate::smt::SMTHelper;
use crate::tests::incremental_smt_test::MemoryStore;
use crate::traits::KeyValueStoreLike;
use anyhow::Result;
use std::collections::HashMap;
use tempfile::TempDir;

// Constants for the new append-only approach
const LENGTH_SUFFIX: &str = "/length";

/// Test that snapshot diff creation works with optimized BST format
#[test]
fn test_snapshot_diff_creation() -> Result<()> {
    println!("🔍 Testing snapshot diff creation with BST tip optimization");

    let storage = MemoryStore::new();
    let mut smt = SMTHelper::new(storage.clone());

    // Simulate indexing operations that would happen during block processing
    println!("📝 Simulating block processing with BST operations...");

    // Block 1: Initial state
    smt.put(b"key1", b"value1", 1)?;
    smt.put(b"key2", b"value2", 1)?;

    // Block 2: Update existing key and add new key
    smt.put(b"key1", b"value1_updated", 2)?;
    smt.put(b"key3", b"value3", 2)?;

    // Block 3: More updates
    smt.put(b"key2", b"value2_updated", 3)?;
    smt.put(b"key4", b"value4", 3)?;

    println!("✅ Simulated 3 blocks of processing");

    // Verify that BST operations are working by checking current values
    let current_key1 = smt.get_current(b"key1")?;
    let current_key2 = smt.get_current(b"key2")?;
    let current_key3 = smt.get_current(b"key3")?;
    let current_key4 = smt.get_current(b"key4")?;

    assert_eq!(current_key1, Some(b"value1_updated".to_vec()));
    assert_eq!(current_key2, Some(b"value2_updated".to_vec()));
    assert_eq!(current_key3, Some(b"value3".to_vec()));
    assert_eq!(current_key4, Some(b"value4".to_vec()));

    println!("✅ BST operations working correctly");

    // Verify historical queries work
    let historical_key1_at_1 = smt.get_at_height(b"key1", 1)?;
    let historical_key1_at_2 = smt.get_at_height(b"key1", 2)?;
    
    assert_eq!(historical_key1_at_1, Some(b"value1".to_vec()));
    assert_eq!(historical_key1_at_2, Some(b"value1_updated".to_vec()));

    println!("✅ Historical queries working correctly");

    // Debug: Let's see what keys are actually in storage
    println!("\n🔍 Debug: Checking what keys are in storage...");
    
    // Try to scan all keys to see what's actually stored in the SMTHelper's storage
    let all_keys_scan = smt.storage.scan_prefix(b"")?; // Empty prefix should return all keys
    println!("📋 Found {} total keys in SMTHelper storage:", all_keys_scan.len());
    for (i, (key, _)) in all_keys_scan.iter().enumerate().take(10) {
        println!("  {}: {:?}", i, String::from_utf8_lossy(key));
    }
    if all_keys_scan.len() > 10 {
        println!("  ... and {} more", all_keys_scan.len() - 10);
    }

    // Now test the snapshot diff creation functionality
    println!("\n📸 Testing snapshot diff creation...");

    // Create a temporary directory for snapshot files
    let temp_dir = TempDir::new()?;
    let snapshot_dir = temp_dir.path().join("snapshots");
    std::fs::create_dir_all(&snapshot_dir)?;

    // Simulate the track_db_changes function from snapshot.rs
    // This is the core function that was updated to work with optimized BST
    let changes = track_db_changes_simulation(&smt.storage, 1, 3)?;
    
    println!("📋 Tracked {} database changes", changes.len());
    assert!(!changes.is_empty(), "Should have tracked database changes");

    // Verify the changes contain the expected append-only keys
    let has_length_keys = changes.iter().any(|(k, _)| {
        let key_str = String::from_utf8_lossy(k);
        key_str.ends_with(LENGTH_SUFFIX)
    });
    let has_update_keys = changes.iter().any(|(k, _)| {
        let key_str = String::from_utf8_lossy(k);
        key_str.contains('/') && !key_str.ends_with(LENGTH_SUFFIX)
    });

    println!("🔍 Debug: Change analysis:");
    println!("  - Has length keys: {}", has_length_keys);
    println!("  - Has update keys: {}", has_update_keys);
    
    // Print first few changes to see what we actually have
    println!("  - First few changes:");
    for (i, (key, _)) in changes.iter().enumerate().take(5) {
        println!("    {}: {:?}", i, String::from_utf8_lossy(key));
    }

    assert!(has_length_keys, "Changes should include length tracking keys");
    assert!(has_update_keys, "Changes should include update keys");

    // Create a compressed diff file (simulating what snapshot.rs does)
    let diff_path = snapshot_dir.join("diff_1_to_3.bin.zst");
    create_compressed_diff(&changes, &diff_path)?;

    // Verify the diff file was created and is not empty
    assert!(diff_path.exists(), "Diff file should be created");
    let file_size = std::fs::metadata(&diff_path)?.len();
    assert!(file_size > 0, "Diff file should not be empty, got {} bytes", file_size);

    println!("✅ Created non-empty diff file: {} bytes", file_size);

    // Test reading the diff file back
    let restored_changes = read_compressed_diff(&diff_path)?;
    assert_eq!(changes.len(), restored_changes.len(), "Should restore same number of changes");

    println!("✅ Successfully read back {} changes from diff file", restored_changes.len());

    // Verify the content matches
    for (original_key, original_value) in &changes {
        let restored_value = restored_changes.get(original_key)
            .expect(&format!("Key should be present: {:?}", original_key));
        assert_eq!(original_value, restored_value, "Values should match for key: {:?}", original_key);
    }

    println!("✅ All change data verified correctly");
    println!("🎉 Snapshot diff creation test passed!");

    Ok(())
}

/// Helper function to get all keys from storage using scan_prefix
fn get_all_storage_keys(storage: &MemoryStore) -> Result<Vec<Vec<u8>>> {
    let mut keys = Vec::new();
    
    // Scan for all keys (empty prefix returns everything)
    for (key, _) in storage.scan_prefix(b"")? {
        let key_str = String::from_utf8_lossy(&key);
        // Only include append-only keys (those with "/" in them)
        if key_str.contains('/') {
            keys.push(key);
        }
    }
    
    Ok(keys)
}

/// Simulate the track_db_changes function from snapshot.rs
/// This mimics the logic that was updated to work with append-only format
fn track_db_changes_simulation(
    storage: &MemoryStore,
    start_height: u32,
    end_height: u32,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut changes = Vec::new();
    
    // Get all keys from storage
    let all_keys = get_all_storage_keys(storage)?;
    
    // Filter for keys that were modified in the height range
    for key in all_keys {
        if let Some(value) = storage.get_immutable(&key)? {
            let key_str = String::from_utf8_lossy(&key);
            
            if key_str.ends_with(LENGTH_SUFFIX) {
                // Always include length tracking keys
                changes.push((key, value));
            } else if key_str.contains('/') && !key_str.ends_with(LENGTH_SUFFIX) {
                // This is an update key, check if it's in our height range
                if let Some(height) = extract_height_from_update_value(&value) {
                    if height >= start_height && height <= end_height {
                        changes.push((key, value));
                    }
                }
            }
        }
    }
    
    Ok(changes)
}

/// Extract height from an update value in the new append-only format
fn extract_height_from_update_value(value: &[u8]) -> Option<u32> {
    let value_str = String::from_utf8_lossy(value);
    
    // The format is "height:value"
    if let Some(colon_pos) = value_str.find(':') {
        let height_str = &value_str[..colon_pos];
        height_str.parse::<u32>().ok()
    } else {
        None
    }
}

/// Create a compressed diff file (simulating zstd compression)
fn create_compressed_diff(
    changes: &[(Vec<u8>, Vec<u8>)],
    path: &std::path::Path,
) -> Result<()> {
    use std::io::Write;
    
    // For this test, we'll create a simple binary format
    // In the real implementation, this would use zstd compression
    let mut data = Vec::new();
    
    // Write number of changes
    data.extend_from_slice(&(changes.len() as u32).to_le_bytes());
    
    // Write each change
    for (key, value) in changes {
        // Write key length and key
        data.extend_from_slice(&(key.len() as u32).to_le_bytes());
        data.extend_from_slice(key);
        
        // Write value length and value
        data.extend_from_slice(&(value.len() as u32).to_le_bytes());
        data.extend_from_slice(value);
    }
    
    // Write to file
    let mut file = std::fs::File::create(path)?;
    file.write_all(&data)?;
    
    Ok(())
}

/// Read a compressed diff file
fn read_compressed_diff(path: &std::path::Path) -> Result<HashMap<Vec<u8>, Vec<u8>>> {
    use std::io::Read;
    
    let mut file = std::fs::File::open(path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    
    let mut changes = HashMap::new();
    let mut offset = 0;
    
    // Read number of changes
    if data.len() < 4 {
        return Ok(changes);
    }
    let num_changes = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    offset += 4;
    
    // Read each change
    for _ in 0..num_changes {
        // Read key length
        if offset + 4 > data.len() {
            break;
        }
        let key_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;
        
        // Read key
        if offset + key_len > data.len() {
            break;
        }
        let key = data[offset..offset + key_len].to_vec();
        offset += key_len;
        
        // Read value length
        if offset + 4 > data.len() {
            break;
        }
        let value_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;
        
        // Read value
        if offset + value_len > data.len() {
            break;
        }
        let value = data[offset..offset + value_len].to_vec();
        offset += value_len;
        
        changes.insert(key, value);
    }
    
    Ok(changes)
}

/// Test that verifies the append-only format is compatible with snapshot operations
#[test]
fn test_append_only_snapshot_compatibility() -> Result<()> {
    println!("🔄 Testing append-only snapshot compatibility");

    let storage = MemoryStore::new();
    let mut smt = SMTHelper::new(storage.clone());

    // Create some test data
    smt.put(b"test_key", b"test_value", 1)?;

    // Verify current value is accessible
    let current_value = smt.get_current(b"test_key")?;
    assert_eq!(current_value, Some(b"test_value".to_vec()));

    // Verify historical value is accessible
    let historical_value = smt.get_at_height(b"test_key", 1)?;
    assert_eq!(historical_value, Some(b"test_value".to_vec()));

    // Check that the storage format is correct
    let all_keys = get_all_storage_keys(&smt.storage)?;
    
    // Should have length key
    let length_key = b"test_key/length";
    let has_length = smt.storage.get_immutable(length_key)?.is_some();
    assert!(has_length, "Should have length tracking key");
    
    // Should have update key
    let update_key = b"test_key/0";
    let has_update = smt.storage.get_immutable(update_key)?.is_some();
    assert!(has_update, "Should have update key");
    
    // Verify the update value format
    if let Some(update_value) = smt.storage.get_immutable(update_key)? {
        let update_str = String::from_utf8_lossy(&update_value);
        assert!(update_str.starts_with("1:"), "Update value should start with height");
        assert!(update_str.ends_with("test_value"), "Update value should end with the actual value");
    }

    println!("✅ Append-only format is compatible with snapshot operations");
    Ok(())
}
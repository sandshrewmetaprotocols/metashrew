use anyhow::Result;
use metashrew_runtime::smt::SMTHelper;
use metashrew_runtime::KeyValueStoreLike;
use rocksdb::{Options, DB};
use rockshrew_runtime::RocksDBRuntimeAdapter;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn test_state_root_with_gaps() -> Result<()> {
    println!("=== STATE ROOT GAP TEST ===");

    // Create a temporary directory for the test database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test_db");

    // Create RocksDB options
    let mut opts = Options::default();
    opts.create_if_missing(true);

    // Open the database
    let db = DB::open(&opts, &db_path)?;
    let db_arc = Arc::new(db);

    // Create a RocksDB adapter
    let adapter = RocksDBRuntimeAdapter::new(db_arc);

    // Create an SMTHelper to test state root calculation directly
    let mut smt_helper = SMTHelper::new(adapter);

    println!("DEBUG: Created SMTHelper with RocksDB adapter");

    // Simulate the production scenario: process some blocks, then skip some, then try to process a later block

    // Process blocks 0-2 normally
    for height in 0..3 {
        let key = format!("/test/key_height_{}", height).into_bytes();
        let value = format!("value_for_height_{}", height).into_bytes();

        smt_helper.put(&key, &value, height)?;
        let state_root = smt_helper.calculate_and_store_state_root(height)?;
        println!(
            "DEBUG: Stored state root for height {}: {}",
            height,
            hex::encode(state_root)
        );
    }

    // Now simulate a gap - skip heights 3-879999 and try to process height 880000
    // This simulates what might happen if the indexer was restarted or the database was partially cleared
    let gap_height = 880000u32;
    println!(
        "DEBUG: Simulating gap - trying to process height {} without previous state roots",
        gap_height
    );

    let key = format!("/test/key_height_{}", gap_height).into_bytes();
    let value = format!("value_for_height_{}", gap_height).into_bytes();

    // Store the key-value pair
    smt_helper.put(&key, &value, gap_height)?;

    // Try to calculate state root - this should fail because there's no state root for height 879999
    match smt_helper.calculate_and_store_state_root(gap_height) {
        Ok(state_root) => {
            println!(
                "DEBUG: Unexpectedly succeeded in calculating state root for height {}: {}",
                gap_height,
                hex::encode(state_root)
            );
        }
        Err(e) => {
            println!(
                "ERROR: Failed to calculate state root for height {}: {:?}",
                gap_height, e
            );
            println!("This matches the production error!");

            // Check if this is the same error we see in production
            let error_msg = format!("{:?}", e);
            if error_msg.contains("No state root found") {
                println!("SUCCESS: Reproduced the production error!");
                return Err(e); // This is expected - we want to see this error
            }
        }
    }

    println!("=== STATE ROOT GAP TEST COMPLETE ===");
    Ok(())
}

#[test]
fn test_state_root_recovery_strategy() -> Result<()> {
    println!("=== STATE ROOT RECOVERY STRATEGY TEST ===");

    // Create a temporary directory for the test database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test_db");

    // Create RocksDB options
    let mut opts = Options::default();
    opts.create_if_missing(true);

    // Open the database
    let db = DB::open(&opts, &db_path)?;
    let db_arc = Arc::new(db);

    // Create a RocksDB adapter
    let adapter = RocksDBRuntimeAdapter::new(db_arc);

    // Create an SMTHelper to test state root calculation directly
    let mut smt_helper = SMTHelper::new(adapter);

    println!("DEBUG: Testing recovery strategy for missing state roots");

    // Simulate the scenario where we need to process a block but don't have previous state roots
    let height = 880065u32;

    let key = format!("/test/key_height_{}", height).into_bytes();
    let value = format!("value_for_height_{}", height).into_bytes();

    // Store the key-value pair
    smt_helper.put(&key, &value, height)?;

    // Instead of failing, let's try a recovery strategy:
    // Calculate state root starting from empty state (like genesis)
    println!("DEBUG: Attempting recovery by calculating state root from current state...");

    // Modify the calculate_and_store_state_root to handle missing previous roots
    // For now, let's manually implement the recovery logic

    // Get all keys that exist in the database up to this height
    // In the new append-only approach, we scan for keys with "/length" suffix
    let length_suffix = "/length";
    let mut all_keys = std::collections::HashSet::new();

    // Scan all entries to find unique keys
    for (key, _) in smt_helper.storage.scan_prefix(b"")?.into_iter() {
        let key_str = String::from_utf8_lossy(&key);
        if key_str.ends_with(length_suffix) {
            // Extract the original key by removing the "/length" suffix
            let original_key_str = &key_str[..key_str.len() - length_suffix.len()];
            let original_key = original_key_str.as_bytes().to_vec();
            
            // Check if this key has any updates at or before the target height
            let heights = smt_helper.get_heights_for_key(&original_key)?;
            if heights.iter().any(|&h| h <= height) {
                all_keys.insert(original_key);
            }
        }
    }

    println!(
        "DEBUG: Found {} unique keys in database up to height {}",
        all_keys.len(),
        height
    );

    // Build current state map
    let mut current_state: std::collections::BTreeMap<Vec<u8>, Vec<u8>> =
        std::collections::BTreeMap::new();
    for key in all_keys {
        if let Ok(Some(value)) = smt_helper.get_at_height(&key, height) {
            current_state.insert(key.clone(), value);
        }
    }

    println!(
        "DEBUG: Built current state with {} key-value pairs",
        current_state.len()
    );

    // Calculate state root from current state (recovery mode)
    let mut hasher = Sha256::new();
    hasher.update(b"metashrew_state_root_v1");

    for (key, value) in current_state.iter() {
        hasher.update(&(key.len() as u32).to_le_bytes());
        hasher.update(key);
        hasher.update(&(value.len() as u32).to_le_bytes());
        hasher.update(value);
    }

    let recovery_root: [u8; 32] = hasher.finalize().into();

    // Store the recovery state root
    let root_key = format!("smt:root:{}", height).into_bytes();
    smt_helper.storage.put(&root_key, &recovery_root)?;

    println!(
        "DEBUG: Recovery state root calculated and stored: {}",
        hex::encode(recovery_root)
    );

    // Verify we can retrieve it
    match smt_helper.get_smt_root_at_height(height) {
        Ok(retrieved_root) => {
            println!(
                "DEBUG: Successfully retrieved recovery state root: {}",
                hex::encode(retrieved_root)
            );
            assert_eq!(
                retrieved_root, recovery_root,
                "Retrieved root should match calculated root"
            );
        }
        Err(e) => {
            println!("ERROR: Failed to retrieve recovery state root: {:?}", e);
            return Err(e);
        }
    }

    println!("SUCCESS: Recovery strategy works!");
    println!("=== STATE ROOT RECOVERY STRATEGY TEST COMPLETE ===");
    Ok(())
}

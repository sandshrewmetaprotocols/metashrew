use anyhow::Result;
use metashrew_runtime::smt::SMTHelper;
use rocksdb::{Options, DB};
use rockshrew_runtime::RocksDBRuntimeAdapter;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn test_state_root_with_rocksdb_adapter() -> Result<()> {
    println!("=== STATE ROOT PRODUCTION TEST (RocksDB) ===");

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

    // Add some test key-value pairs
    let test_data = vec![
        (b"/test/key1".to_vec(), b"value1".to_vec()),
        (b"/test/key2".to_vec(), b"value2".to_vec()),
        (b"/test/key3".to_vec(), b"value3".to_vec()),
    ];

    let height = 0u32;

    println!(
        "DEBUG: Adding {} test key-value pairs at height {}",
        test_data.len(),
        height
    );

    // Store the key-value pairs in BST
    for (key, value) in &test_data {
        match smt_helper.bst_put(key, value, height) {
            Ok(_) => println!(
                "DEBUG: Successfully stored key: {:?}",
                String::from_utf8_lossy(key)
            ),
            Err(e) => {
                println!(
                    "DEBUG: Failed to store key: {:?}, error: {:?}",
                    String::from_utf8_lossy(key),
                    e
                );
                return Err(e);
            }
        }
    }

    println!("DEBUG: All key-value pairs stored, now calculating state root...");

    // Calculate and store the state root
    match smt_helper.calculate_and_store_state_root(height) {
        Ok(state_root) => {
            println!(
                "DEBUG: State root calculation succeeded: {}",
                hex::encode(state_root)
            );

            // Verify it's not all zeros
            let zero_root = [0u8; 32];
            if state_root == zero_root {
                println!("ERROR: State root is all zeros!");
                return Err(anyhow::anyhow!("State root should not be all zeros"));
            } else {
                println!(
                    "SUCCESS: State root is non-zero: {}",
                    hex::encode(state_root)
                );
            }
        }
        Err(e) => {
            println!("ERROR: State root calculation failed: {:?}", e);
            return Err(e);
        }
    }

    // Test retrieving the state root
    match smt_helper.get_smt_root_at_height(height) {
        Ok(retrieved_root) => {
            println!(
                "DEBUG: Retrieved state root: {}",
                hex::encode(retrieved_root)
            );
        }
        Err(e) => {
            println!("ERROR: Failed to retrieve state root: {:?}", e);
            return Err(e);
        }
    }

    // Now test the problematic scenario: try to get state root for height 1 when only height 0 exists
    println!("DEBUG: Testing problematic scenario - getting state root for height 1...");
    match smt_helper.get_smt_root_at_height(1) {
        Ok(retrieved_root) => {
            println!(
                "DEBUG: Retrieved state root for height 1: {}",
                hex::encode(retrieved_root)
            );
            println!("SUCCESS: Should have found state root from height 0");
        }
        Err(e) => {
            println!("ERROR: Failed to retrieve state root for height 1: {:?}", e);
            println!("This is the same error we see in production!");
            return Err(e);
        }
    }

    println!("=== STATE ROOT PRODUCTION TEST COMPLETE ===");
    Ok(())
}

#[test]
fn test_state_root_sequential_heights() -> Result<()> {
    println!("=== STATE ROOT SEQUENTIAL HEIGHTS TEST ===");

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

    println!("DEBUG: Testing sequential height processing...");

    // Process multiple heights sequentially like in production
    for height in 0..3 {
        println!("DEBUG: Processing height {}", height);

        // Add some test data for this height
        let key = format!("/test/key_height_{}", height).into_bytes();
        let value = format!("value_for_height_{}", height).into_bytes();

        // Store the key-value pair
        smt_helper.bst_put(&key, &value, height)?;
        println!("DEBUG: Stored key-value for height {}", height);

        // Calculate state root for this height
        match smt_helper.calculate_and_store_state_root(height) {
            Ok(state_root) => {
                println!(
                    "DEBUG: State root for height {}: {}",
                    height,
                    hex::encode(state_root)
                );
            }
            Err(e) => {
                println!(
                    "ERROR: Failed to calculate state root for height {}: {:?}",
                    height, e
                );
                return Err(e);
            }
        }
    }

    // Now test retrieving state roots for all heights
    for height in 0..3 {
        match smt_helper.get_smt_root_at_height(height) {
            Ok(state_root) => {
                println!(
                    "DEBUG: Retrieved state root for height {}: {}",
                    height,
                    hex::encode(state_root)
                );
            }
            Err(e) => {
                println!(
                    "ERROR: Failed to retrieve state root for height {}: {:?}",
                    height, e
                );
                return Err(e);
            }
        }
    }

    println!("=== STATE ROOT SEQUENTIAL HEIGHTS TEST COMPLETE ===");
    Ok(())
}

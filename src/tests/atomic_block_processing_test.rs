use anyhow::Result;
use metashrew_runtime::smt::SMTHelper;
use metashrew_runtime::KeyValueStoreLike;
use rocksdb::{MultiThreaded, Options, TransactionDB, TransactionDBOptions, DB};
use rockshrew_runtime::RocksDBRuntimeAdapter;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn test_atomic_block_processing_requirement() -> Result<()> {
    println!("=== ATOMIC BLOCK PROCESSING TEST ===");

    // This test demonstrates the atomicity problem and shows how to fix it

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

    println!("DEBUG: Testing atomic block processing requirements");

    // Simulate a block processing scenario that should be atomic
    let height = 1u32;
    let test_data = vec![
        (b"/test/key1".to_vec(), b"value1".to_vec()),
        (b"/test/key2".to_vec(), b"value2".to_vec()),
    ];

    // Step 1: Store key-value pairs (this represents WASM runtime processing)
    for (key, value) in &test_data {
        smt_helper.bst_put(key, value, height)?;
        println!(
            "DEBUG: Stored key-value pair: {:?} = {:?}",
            String::from_utf8_lossy(key),
            String::from_utf8_lossy(value)
        );
    }

    // Step 2: Initialize state root for height 0 (our fix for start block issue)
    println!("DEBUG: Initializing state root for height 0 (start block fix)");
    let empty_state_root = [0u8; 32];
    let root_key = format!("smt:root:{}", 0).into_bytes();
    smt_helper.storage.put(&root_key, &empty_state_root)?;

    // Step 3: Calculate state root (this should now work)
    let _state_root = match smt_helper.calculate_and_store_state_root(height) {
        Ok(root) => {
            println!(
                "DEBUG: State root calculated successfully: {}",
                hex::encode(root)
            );
            root
        }
        Err(e) => {
            println!("ERROR: State root calculation still failed: {:?}", e);
            return Err(e);
        }
    };

    // Step 4: Store indexed height (this can also fail)
    // In the current implementation, this is done separately and can fail
    // leaving the database inconsistent

    println!("SUCCESS: All steps completed, but this should be atomic!");
    println!("SOLUTION: All database writes for a single block must be in one transaction");

    println!("=== ATOMIC BLOCK PROCESSING TEST COMPLETE ===");
    Ok(())
}

#[test]
fn test_start_block_state_root_initialization() -> Result<()> {
    println!("=== START BLOCK STATE ROOT INITIALIZATION TEST ===");

    // This test demonstrates the start block problem

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

    // Create an SMTHelper
    let mut smt_helper = SMTHelper::new(adapter);

    println!("DEBUG: Testing start block state root initialization");

    // Simulate starting at a high block number (like --start-block 880000)
    let start_height = 880000u32;

    println!("DEBUG: Simulating start at height {}", start_height);

    // Try to process the first block at this height
    let key = b"/test/first_key".to_vec();
    let value = b"first_value".to_vec();

    smt_helper.bst_put(&key, &value, start_height)?;

    // This should fail because there's no state root for height 879999
    match smt_helper.calculate_and_store_state_root(start_height) {
        Ok(state_root) => {
            println!("DEBUG: Unexpectedly succeeded: {}", hex::encode(state_root));
            println!("This means the implementation already handles missing previous state roots");
        }
        Err(e) => {
            println!("ERROR: Failed as expected: {:?}", e);
            println!("PROBLEM: When starting at a high block number, we need to initialize");
            println!("the state root for the start block without requiring previous state roots");

            // SOLUTION: Initialize state root for start block
            println!("SOLUTION: Initialize empty state root for start block");

            // For start blocks, we should initialize with an empty state root
            // This represents the state at the start block height
            let empty_state_root = [0u8; 32]; // Or calculate from current state
            let root_key = format!("smt:root:{}", start_height - 1).into_bytes();
            smt_helper.storage.put(&root_key, &empty_state_root)?;

            // Now try again
            match smt_helper.calculate_and_store_state_root(start_height) {
                Ok(state_root) => {
                    println!(
                        "SUCCESS: State root calculated after initialization: {}",
                        hex::encode(state_root)
                    );
                }
                Err(e) => {
                    println!("ERROR: Still failed after initialization: {:?}", e);
                    return Err(e);
                }
            }
        }
    }

    println!("=== START BLOCK STATE ROOT INITIALIZATION TEST COMPLETE ===");
    Ok(())
}

#[test]
fn test_proposed_atomic_solution() -> Result<()> {
    println!("=== PROPOSED ATOMIC SOLUTION TEST ===");

    // This test shows how to implement atomic block processing

    // Create a temporary directory for the test database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test_db");

    // Create TransactionDB options for atomic operations
    let mut opts = Options::default();
    opts.create_if_missing(true);

    let txn_db_opts = TransactionDBOptions::default();
    let txn_db: TransactionDB<MultiThreaded> = TransactionDB::open(&opts, &txn_db_opts, &db_path)?;

    println!("DEBUG: Testing atomic block processing with TransactionDB");

    let height = 1u32;
    let test_data = vec![
        (b"/test/key1".to_vec(), b"value1".to_vec()),
        (b"/test/key2".to_vec(), b"value2".to_vec()),
    ];

    // Start a transaction for atomic block processing
    let txn = txn_db.transaction();

    // All operations should be done within this transaction
    println!("DEBUG: Starting atomic transaction for block {}", height);

    // Step 1: Store key-value pairs
    for (key, value) in &test_data {
        let storage_key = format!("bst:height:{}:{}", hex::encode(key), height);
        txn.put(storage_key.as_bytes(), value)?;
        println!(
            "DEBUG: Added to transaction: {:?} = {:?}",
            String::from_utf8_lossy(key),
            String::from_utf8_lossy(value)
        );
    }

    // Step 2: Calculate and store state root
    // (This would need to be modified to work with the transaction)
    let state_root = [1u8; 32]; // Simplified for demo
    let root_key = format!("smt:root:{}", height);
    txn.put(root_key.as_bytes(), &state_root)?;
    println!(
        "DEBUG: Added state root to transaction: {}",
        hex::encode(state_root)
    );

    // Step 3: Store indexed height
    let height_key = b"__INTERNAL/height";
    txn.put(height_key, &height.to_le_bytes())?;
    println!("DEBUG: Added indexed height to transaction: {}", height);

    // Step 4: Store block hash
    let block_hash = [2u8; 32]; // Simplified for demo
    let hash_key = format!("/__INTERNAL/height-to-hash/{}", height);
    txn.put(hash_key.as_bytes(), &block_hash)?;
    println!(
        "DEBUG: Added block hash to transaction: {}",
        hex::encode(block_hash)
    );

    // Commit the transaction atomically
    match txn.commit() {
        Ok(_) => {
            println!("SUCCESS: All block data committed atomically!");
            println!("If any step had failed, the entire transaction would be rolled back");
        }
        Err(e) => {
            println!("ERROR: Transaction failed and was rolled back: {:?}", e);
            return Err(anyhow::anyhow!("Transaction failed: {:?}", e));
        }
    }

    println!("=== PROPOSED ATOMIC SOLUTION TEST COMPLETE ===");
    Ok(())
}

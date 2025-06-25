use anyhow::Result;
use memshrew_runtime::{KeyValueStoreLike, MemStoreAdapter};

#[test]
fn test_state_root_calculation_debug() -> Result<()> {
    println!("=== STATE ROOT DEBUG TEST ===");

    // Create a memory store adapter
    let adapter = MemStoreAdapter::new();

    // Create an SMTHelper to test state root calculation directly
    let mut smt_helper = metashrew_runtime::smt::SMTHelper::new(adapter.clone());

    println!("DEBUG: Created SMTHelper");

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
        match smt_helper.put(key, value, height) {
            Ok(_) => println!(
                "DEBUG: Successfully stored key: {:?}",
                String::from_utf8_lossy(key)
            ),
            Err(e) => println!(
                "DEBUG: Failed to store key: {:?}, error: {:?}",
                String::from_utf8_lossy(key),
                e
            ),
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
        }
    }

    // Test getting keys at height
    match smt_helper.get_keys_at_height(height) {
        Ok(keys) => {
            println!("DEBUG: Found {} keys at height {}", keys.len(), height);
            for key in &keys {
                println!(
                    "DEBUG: Key at height {}: {:?}",
                    height,
                    String::from_utf8_lossy(key)
                );
            }
        }
        Err(e) => {
            println!("ERROR: Failed to get keys at height: {:?}", e);
        }
    }

    println!("=== STATE ROOT DEBUG TEST COMPLETE ===");
    Ok(())
}

#[test]
fn test_bst_scanning_logic() -> Result<()> {
    println!("=== BST SCANNING LOGIC TEST ===");

    // Create a memory store adapter
    let adapter = MemStoreAdapter::new();

    // Create an SMTHelper to test BST scanning
    let mut smt_helper = metashrew_runtime::smt::SMTHelper::new(adapter.clone());

    // Add some test key-value pairs
    let test_data = vec![
        (b"/test/key1".to_vec(), b"value1".to_vec()),
        (b"/test/key2".to_vec(), b"value2".to_vec()),
        (b"/test/key3".to_vec(), b"value3".to_vec()),
    ];

    let height = 0u32;

    println!("DEBUG: Adding test data...");

    // Store the key-value pairs in BST
    for (key, value) in &test_data {
        smt_helper.put(key, value, height)?;
        println!("DEBUG: Stored key: {:?}", String::from_utf8_lossy(key));
    }

    // Test the BST scanning logic directly
    println!("DEBUG: Testing BST prefix scanning...");

    // In the new append-only approach, we scan for keys with "/length" suffix
    let length_suffix = "/length";
    println!("DEBUG: Scanning all database entries for append-only structure");

    match adapter.scan_prefix(b"") {
        Ok(results) => {
            println!("DEBUG: Found {} entries in database", results.len());
            for (key, value) in &results {
                let key_str = String::from_utf8_lossy(key);
                println!(
                    "DEBUG: Database entry: key={}, value_len={}",
                    key_str,
                    value.len()
                );

                // Look for append-only entries (key/0, key/1, etc.)
                if key_str.contains('/') && !key_str.ends_with(length_suffix) {
                    let parts: Vec<&str> = key_str.split('/').collect();
                    if parts.len() == 2 {
                        if let Ok(_index) = parts[1].parse::<u32>() {
                            let value_str = String::from_utf8_lossy(value);
                            if let Some(colon_pos) = value_str.find(':') {
                                let height_str = &value_str[..colon_pos];
                                let data_str = &value_str[colon_pos + 1..];
                                println!(
                                    "DEBUG: Append-only entry - key: {}, height: {}, data: {}",
                                    parts[0], height_str, data_str
                                );
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            println!("ERROR: Failed to scan BST prefix: {:?}", e);
            return Err(anyhow::anyhow!("BST scanning failed: {:?}", e));
        }
    }

    println!("=== BST SCANNING LOGIC TEST COMPLETE ===");
    Ok(())
}

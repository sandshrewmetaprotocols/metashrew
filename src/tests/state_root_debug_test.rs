use anyhow::Result;
use memshrew_runtime::{MemStoreAdapter, KeyValueStoreLike};

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
    
    let height = 100u32;
    
    println!("DEBUG: Adding {} test key-value pairs at height {}", test_data.len(), height);
    
    // Store the key-value pairs in BST
    for (key, value) in &test_data {
        match smt_helper.bst_put(key, value, height) {
            Ok(_) => println!("DEBUG: Successfully stored key: {:?}", String::from_utf8_lossy(key)),
            Err(e) => println!("DEBUG: Failed to store key: {:?}, error: {:?}", String::from_utf8_lossy(key), e),
        }
    }
    
    println!("DEBUG: All key-value pairs stored, now calculating state root...");
    
    // Calculate and store the state root
    match smt_helper.calculate_and_store_state_root(height) {
        Ok(state_root) => {
            println!("DEBUG: State root calculation succeeded: {}", hex::encode(state_root));
            
            // Verify it's not all zeros
            let zero_root = [0u8; 32];
            if state_root == zero_root {
                println!("ERROR: State root is all zeros!");
                return Err(anyhow::anyhow!("State root should not be all zeros"));
            } else {
                println!("SUCCESS: State root is non-zero: {}", hex::encode(state_root));
            }
        },
        Err(e) => {
            println!("ERROR: State root calculation failed: {:?}", e);
            return Err(e);
        }
    }
    
    // Test retrieving the state root
    match smt_helper.get_smt_root_at_height(height) {
        Ok(retrieved_root) => {
            println!("DEBUG: Retrieved state root: {}", hex::encode(retrieved_root));
        },
        Err(e) => {
            println!("ERROR: Failed to retrieve state root: {:?}", e);
        }
    }
    
    // Test getting keys at height
    match smt_helper.get_keys_at_height(height) {
        Ok(keys) => {
            println!("DEBUG: Found {} keys at height {}", keys.len(), height);
            for key in &keys {
                println!("DEBUG: Key at height {}: {:?}", height, String::from_utf8_lossy(key));
            }
        },
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
    
    let height = 100u32;
    
    println!("DEBUG: Adding test data...");
    
    // Store the key-value pairs in BST
    for (key, value) in &test_data {
        smt_helper.bst_put(key, value, height)?;
        println!("DEBUG: Stored key: {:?}", String::from_utf8_lossy(key));
    }
    
    // Test the BST scanning logic directly
    println!("DEBUG: Testing BST prefix scanning...");
    
    let prefix = metashrew_runtime::smt::BST_HEIGHT_PREFIX.as_bytes();
    println!("DEBUG: Scanning with prefix: {:?}", String::from_utf8_lossy(prefix));
    
    match adapter.scan_prefix(prefix) {
        Ok(results) => {
            println!("DEBUG: Found {} entries with BST prefix", results.len());
            for (key, value) in &results {
                let key_str = String::from_utf8_lossy(key);
                println!("DEBUG: BST entry: key={}, value_len={}", key_str, value.len());
                
                // Parse the key to extract original key
                if let Some(rest) = key_str.strip_prefix(metashrew_runtime::smt::BST_HEIGHT_PREFIX) {
                    if let Some(colon_pos) = rest.find(':') {
                        let hex_key = &rest[..colon_pos];
                        let height_str = &rest[colon_pos + 1..];
                        println!("DEBUG: Parsed - hex_key: {}, height: {}", hex_key, height_str);
                        
                        if let Ok(original_key) = hex::decode(hex_key) {
                            println!("DEBUG: Original key: {:?}", String::from_utf8_lossy(&original_key));
                        }
                    }
                }
            }
        },
        Err(e) => {
            println!("ERROR: Failed to scan BST prefix: {:?}", e);
            return Err(anyhow::anyhow!("BST scanning failed: {:?}", e));
        }
    }
    
    println!("=== BST SCANNING LOGIC TEST COMPLETE ===");
    Ok(())
}
//! Debug test to investigate BST storage issues

use memshrew_runtime::{MemStoreAdapter, MemStoreRuntime};
use metashrew_runtime::smt::SMTHelper;
use metashrew_support::utils;
use std::path::PathBuf;
use super::block_builder::create_test_block;
use bitcoin::{BlockHash, hashes::Hash};

#[tokio::test]
async fn test_bst_storage_debug() {
    println!("=== BST Storage Debug Test ===");
    
    // Create runtime with in-memory storage
    let adapter = MemStoreAdapter::new();
    let mut runtime = MemStoreRuntime::load(
        PathBuf::from("./target/wasm32-unknown-unknown/release/metashrew_minimal.wasm"),
        adapter.clone(),
    ).expect("Failed to load runtime");

    // Process one block
    let block = create_test_block(0, BlockHash::all_zeros(), &[42]);
    let encoded_block = utils::consensus_encode(&block).expect("Failed to encode block");
    
    println!("Processing block 0...");
    runtime.context.lock().unwrap().height = 0;
    runtime.context.lock().unwrap().block = encoded_block;
    runtime.run().expect("Failed to process block 0");
    runtime.refresh_memory().expect("Failed to refresh memory");
    
    // Check what's actually stored in the database
    let all_data = adapter.get_all_data();
    println!("Total keys in database: {}", all_data.len());
    
    for (key, value) in &all_data {
        let key_str = String::from_utf8_lossy(key);
        println!("  Key: {} (len: {})", key_str, key.len());
        println!("    Value: {:?} (len: {})", value, value.len());
        
        // Check if this is a BST key
        if key_str.contains("bst:height:") {
            println!("    *** BST HEIGHT KEY FOUND! ***");
        }
        if key_str.contains("bst:") {
            println!("    *** BST KEY FOUND! ***");
        }
    }
    
    // Now test direct BST operations
    println!("\n--- Testing Direct BST Operations ---");
    let db = runtime.context.lock().unwrap().db.clone();
    let mut smt_helper = SMTHelper::new(db.clone());
    
    // Try to manually store something in BST
    let test_key = b"/test";
    let test_value = b"test_value";
    let test_height = 0u32;
    
    println!("Manually storing in BST: key={:?}, value={:?}, height={}", test_key, test_value, test_height);
    match smt_helper.bst_put(test_key, test_value, test_height) {
        Ok(_) => println!("  BST put succeeded"),
        Err(e) => println!("  BST put failed: {:?}", e),
    }
    
    // Check if it was stored
    match smt_helper.bst_get_heights_for_key(test_key) {
        Ok(heights) => {
            println!("  Heights for test key: {:?}", heights);
        }
        Err(e) => {
            println!("  Error getting heights: {:?}", e);
        }
    }
    
    // Check database again
    let all_data_after = adapter.get_all_data();
    println!("\nTotal keys in database after manual BST put: {}", all_data_after.len());
    
    for (key, value) in &all_data_after {
        let key_str = String::from_utf8_lossy(key);
        if key_str.contains("bst:") {
            println!("  BST Key: {} -> {:?}", key_str, value);
        }
    }
}
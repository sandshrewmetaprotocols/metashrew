//! Example demonstrating the append-only BST database functionality
//! 
//! This example shows how to:
//! - Store key-value pairs with height indexing
//! - Query values at specific block heights
//! - Iterate backwards through key history
//! - Perform binary search for historical values
//! - Handle chain reorganizations with rollback

use anyhow::Result;
use rockshrew_runtime::{BSTHelper, RocksDBRuntimeAdapter};
use rocksdb::Options;
use std::sync::Arc;
use tempfile::TempDir;

fn main() -> Result<()> {
    env_logger::init();
    
    println!("BST Database Example");
    println!("===================");
    
    // Create a temporary database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test_db");
    
    let mut opts = Options::default();
    opts.create_if_missing(true);
    
    let adapter = RocksDBRuntimeAdapter::open(db_path.to_string_lossy().to_string(), opts)?;
    let helper = BSTHelper::new(adapter.db.clone());
    
    // Example 1: Store values at different heights
    println!("\n1. Storing values at different heights");
    let key = b"example_key";
    
    // Store value at height 100
    helper.smt_helper.bst_put(key, b"value_at_100", 100)?;
    println!("Stored 'value_at_100' at height 100");
    
    // Store updated value at height 200
    helper.smt_helper.bst_put(key, b"value_at_200", 200)?;
    println!("Stored 'value_at_200' at height 200");
    
    // Store another update at height 300
    helper.smt_helper.bst_put(key, b"value_at_300", 300)?;
    println!("Stored 'value_at_300' at height 300");
    
    // Example 2: Query values at specific heights
    println!("\n2. Querying values at specific heights");
    
    if let Some(value) = helper.get_value_at_height(key, 150)? {
        println!("Value at height 150: {}", String::from_utf8_lossy(&value));
    }
    
    if let Some(value) = helper.get_value_at_height(key, 250)? {
        println!("Value at height 250: {}", String::from_utf8_lossy(&value));
    }
    
    if let Some(value) = helper.get_value_at_height(key, 350)? {
        println!("Value at height 350: {}", String::from_utf8_lossy(&value));
    }
    
    // Example 3: Iterate backwards through history
    println!("\n3. Iterating backwards through key history");
    let history = helper.iterate_backwards(key, Some(350))?;
    
    for (height, value) in history {
        println!("Height {}: {}", height, String::from_utf8_lossy(&value));
    }
    
    // Example 4: Get all update heights for a key
    println!("\n4. All update heights for the key");
    let heights = helper.get_key_update_heights(key)?;
    println!("Key was updated at heights: {:?}", heights);
    
    // Example 5: Store multiple keys at the same height
    println!("\n5. Storing multiple keys at height 400");
    helper.smt_helper.bst_put(b"key1", b"value1_at_400", 400)?;
    helper.smt_helper.bst_put(b"key2", b"value2_at_400", 400)?;
    helper.smt_helper.bst_put(b"key3", b"value3_at_400", 400)?;
    
    let keys_at_400 = helper.get_keys_touched_at_height(400)?;
    println!("Keys touched at height 400: {} keys", keys_at_400.len());
    for key in &keys_at_400 {
        println!("  - {}", String::from_utf8_lossy(key));
    }
    
    // Example 6: Calculate state root
    println!("\n6. State root calculation");
    helper.smt_helper.calculate_and_store_state_root(400)?;
    let state_root = helper.get_state_root_at_height(400)?;
    println!("State root at height 400: {}", hex::encode(state_root));
    
    // Example 7: Demonstrate rollback (reorg handling)
    println!("\n7. Demonstrating rollback (reorg handling)");
    println!("Before rollback:");
    if let Some(value) = helper.get_value_at_height(key, 350)? {
        println!("  Value at height 350: {}", String::from_utf8_lossy(&value));
    }
    
    // Rollback to height 150
    helper.rollback_to_height(150)?;
    println!("Rolled back to height 150");
    
    println!("After rollback:");
    if let Some(value) = helper.get_value_at_height(key, 350)? {
        println!("  Value at height 350: {}", String::from_utf8_lossy(&value));
    } else {
        println!("  No value found at height 350 (correctly rolled back)");
    }
    
    if let Some(value) = helper.get_value_at_height(key, 150)? {
        println!("  Value at height 150: {}", String::from_utf8_lossy(&value));
    }
    
    // Example 8: Show statistics
    println!("\n8. BST Statistics");
    let stats = helper.get_statistics()?;
    println!("{}", stats);
    
    // Example 9: Verify integrity
    println!("\n9. Integrity verification");
    let is_valid = helper.verify_integrity()?;
    println!("BST integrity check: {}", if is_valid { "PASSED" } else { "FAILED" });
    
    println!("\nExample completed successfully!");
    
    Ok(())
}
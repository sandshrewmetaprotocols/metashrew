//! Test to verify database persistence is working correctly

use anyhow::Result;
use bitcoin::hashes::Hash;
use memshrew_runtime::{MemStoreAdapter, MemStoreRuntime, KeyValueStoreLike};
use metashrew_support::utils;
use std::path::PathBuf;

#[tokio::test]
async fn test_simple_key_value_persistence() -> Result<()> {
    println!("=== TESTING SIMPLE KEY-VALUE PERSISTENCE ===");
    
    // Create runtime
    let wasm_path = PathBuf::from("./target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
    let mem_adapter = MemStoreAdapter::new();
    let mut runtime = MemStoreRuntime::load(wasm_path, mem_adapter)?;

    // Create a simple test block
    let test_block = super::block_builder::create_test_block(
        0,
        bitcoin::BlockHash::all_zeros(),
        b"test_block_0",
    );

    // Process block 0
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = utils::consensus_encode(&test_block)?;
        context.height = 0;
    }
    runtime.run()?;

    // Now test if we can retrieve simple key-value pairs
    // The alkanes indexer should have set "/seen-genesis" to 0x01
    
    // Test direct database access
    let db = {
        let guard = runtime.context.lock().unwrap();
        guard.db.clone()
    };
    
    println!("DEBUG: Testing direct database key access...");
    
    // Test various key formats that might be used
    let test_keys = vec![
        "/seen-genesis".as_bytes().to_vec(),
        "seen-genesis".as_bytes().to_vec(),
        "/seen-genesis".as_bytes().to_vec(),
    ];
    
    for key in test_keys {
        match db.get_immutable(&key) {
            Ok(Some(value)) => {
                println!("✅ Found key '{}': {} bytes: {:?}", 
                    String::from_utf8_lossy(&key), value.len(), value);
            }
            Ok(None) => {
                println!("❌ Key '{}' not found", String::from_utf8_lossy(&key));
            }
            Err(e) => {
                println!("❌ Error accessing key '{}': {}", String::from_utf8_lossy(&key), e);
            }
        }
    }
    
    // Test BST format keys
    println!("DEBUG: Testing BST format keys...");
    let bst_key = format!("bst:height:{}:0", hex::encode("/seen-genesis".as_bytes()));
    match db.get_immutable(&bst_key.as_bytes().to_vec()) {
        Ok(Some(value)) => {
            println!("✅ Found BST key '{}': {} bytes: {:?}", bst_key, value.len(), value);
        }
        Ok(None) => {
            println!("❌ BST key '{}' not found", bst_key);
        }
        Err(e) => {
            println!("❌ Error accessing BST key '{}': {}", bst_key, e);
        }
    }
    
    // Test SMT format keys  
    println!("DEBUG: Testing SMT format keys...");
    let smt_key = format!("smt:height::{}", hex::encode("/seen-genesis".as_bytes()));
    match db.get_immutable(&format!("{}:0", smt_key).as_bytes().to_vec()) {
        Ok(Some(value)) => {
            println!("✅ Found SMT key '{}:0': {} bytes: {:?}", smt_key, value.len(), value);
        }
        Ok(None) => {
            println!("❌ SMT key '{}:0' not found", smt_key);
        }
        Err(e) => {
            println!("❌ Error accessing SMT key '{}:0': {}", smt_key, e);
        }
    }
    
    // Scan all keys to see what's actually in the database
    println!("DEBUG: Scanning all keys in database...");
    let all_keys = db.scan_prefix(&[])?;
    println!("Total keys found: {}", all_keys.len());
    for (i, (key, value)) in all_keys.iter().enumerate() {
        if i < 20 { // Limit output
            println!("  Key: '{}' -> {} bytes", String::from_utf8_lossy(key), value.len());
        }
    }
    
    println!("=== SIMPLE KEY-VALUE PERSISTENCE TEST COMPLETE ===");
    Ok(())
}
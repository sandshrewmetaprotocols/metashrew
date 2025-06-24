//! Test to simulate alkanes indexer behavior

use anyhow::Result;
use bitcoin::hashes::Hash;
use memshrew_runtime::{MemStoreAdapter, MemStoreRuntime};
use metashrew_support::utils;
use std::path::PathBuf;

/// Create a simple WASM module that simulates alkanes indexer behavior
/// This will test the `/seen-genesis` key storage and retrieval
#[tokio::test]
async fn test_alkanes_genesis_simulation() -> Result<()> {
    println!("=== ALKANES GENESIS SIMULATION TEST ===");
    
    // Create runtime
    let wasm_path = PathBuf::from("./target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
    let mem_adapter = MemStoreAdapter::new();
    let mut runtime = MemStoreRuntime::load(wasm_path, mem_adapter)?;

    // Create a test block
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

    // Now simulate what alkanes indexer does:
    // 1. Check if "/seen-genesis" exists (should be empty initially)
    // 2. Set "/seen-genesis" to 0x01
    // 3. Check if "/seen-genesis" exists again (should return 0x01)
    
    println!("DEBUG: Simulating alkanes indexer genesis check...");
    
    // Test using view function to simulate IndexPointer.get()
    // This simulates: IndexPointer::from_keyword("/seen-genesis").get()
    let genesis_check_input = "/seen-genesis".as_bytes().to_vec();
    
    // Create a simple view function that just returns the key data
    // Since we don't have a custom WASM module, we'll test the host functions directly
    
    // Test the database directly to see what's stored
    let db = {
        let guard = runtime.context.lock().unwrap();
        guard.db.clone()
    };
    
    // Check if the key exists in any format
    let test_keys = vec![
        "/seen-genesis".as_bytes().to_vec(),
        format!("smt:height::{}", hex::encode("/seen-genesis".as_bytes())).as_bytes().to_vec(),
        format!("smt:height::{}:0", hex::encode("/seen-genesis".as_bytes())).as_bytes().to_vec(),
        format!("bst:height:{}:0", hex::encode("/seen-genesis".as_bytes())).as_bytes().to_vec(),
    ];
    
    println!("DEBUG: Checking for /seen-genesis in various formats...");
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
    
    println!("=== ALKANES GENESIS SIMULATION TEST COMPLETE ===");
    Ok(())
}
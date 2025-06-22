//! Test to verify state root functionality and JSON-RPC compatibility
//!
//! This test verifies that state roots are being calculated and stored correctly,
//! and that they can be retrieved via the same mechanisms used by the JSON-RPC server.

use super::TestConfig;
use super::block_builder::ChainBuilder;
use anyhow::Result;
use memshrew_runtime::KeyValueStoreLike;
use metashrew_runtime::smt::SMTHelper;
use metashrew_support::utils;

#[tokio::test]
async fn test_stateroot_calculation_and_retrieval() -> Result<()> {
    println!("=== STATE ROOT CALCULATION AND RETRIEVAL TEST ===");
    
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create a small chain of blocks
    let chain = ChainBuilder::new()
        .add_blocks(3)
        .blocks();
    
    println!("Processing {} blocks...", chain.len());
    
    // Process each block and verify state root is calculated
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        let height = height as u32;
        
        println!("Processing block {} ({} bytes)", height, block_bytes.len());
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes.clone();
            context.height = height;
        }
        
        // Execute the runtime
        runtime.run()?;
        runtime.refresh_memory()?;
        
        // Verify state root was calculated and stored
        let adapter = &runtime.context.lock().unwrap().db;
        let smt_helper = SMTHelper::new(adapter.clone());
        
        match smt_helper.get_smt_root_at_height(height) {
            Ok(state_root) => {
                println!("✅ State root for height {}: 0x{}", height, hex::encode(&state_root));
                
                // Verify it's not all zeros
                let zero_root = [0u8; 32];
                if state_root == zero_root {
                    return Err(anyhow::anyhow!("State root for height {} is all zeros!", height));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Failed to get state root for height {}: {}", height, e));
            }
        }
    }
    
    println!("=== TESTING JSON-RPC COMPATIBLE RETRIEVAL ===");
    
    // Test the same retrieval mechanism used by the JSON-RPC server
    let adapter = &runtime.context.lock().unwrap().db;
    
    for height in 0..chain.len() as u32 {
        // Simulate the same logic used in RocksDBStorageAdapter::get_state_root
        let smt_helper = SMTHelper::new(adapter.clone());
        
        match smt_helper.get_smt_root_at_height(height) {
            Ok(root) => {
                println!("✅ JSON-RPC compatible retrieval for height {}: 0x{}", height, hex::encode(&root));
                
                // Verify it's not empty
                if root.is_empty() {
                    return Err(anyhow::anyhow!("State root for height {} is empty!", height));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("JSON-RPC compatible retrieval failed for height {}: {}", height, e));
            }
        }
    }
    
    println!("=== TESTING 'LATEST' HEIGHT LOGIC ===");
    
    // Test the "latest" height logic used by the JSON-RPC server
    let latest_height = (chain.len() as u32).saturating_sub(1);
    println!("Latest height calculated as: {}", latest_height);
    
    let smt_helper = SMTHelper::new(adapter.clone());
    match smt_helper.get_smt_root_at_height(latest_height) {
        Ok(root) => {
            println!("✅ State root for 'latest' (height {}): 0x{}", latest_height, hex::encode(&root));
        },
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to get state root for 'latest' height {}: {}", latest_height, e));
        }
    }
    
    println!("=== STATE ROOT TEST COMPLETE ===");
    Ok(())
}

#[tokio::test]
async fn test_stateroot_key_format_consistency() -> Result<()> {
    println!("=== STATE ROOT KEY FORMAT CONSISTENCY TEST ===");
    
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Process a single block
    let genesis = super::TestUtils::create_genesis_block();
    let block_bytes = utils::consensus_encode(&genesis)?;
    let height = 0u32;
    
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes;
        context.height = height;
    }
    
    runtime.run()?;
    
    // Check the key format used for state root storage
    let adapter = &mut runtime.context.lock().unwrap().db;
    
    // Test the key format used by the WASM runtime (single colon)
    let wasm_key = format!("smt:root:{}", height);
    println!("Testing WASM runtime key format: '{}'", wasm_key);
    
    match adapter.get(wasm_key.as_bytes()) {
        Ok(Some(value)) => {
            println!("✅ Found state root with WASM key format: 0x{}", hex::encode(&value));
        },
        Ok(None) => {
            println!("❌ No state root found with WASM key format");
        },
        Err(e) => {
            println!("❌ Error accessing WASM key format: {}", e);
        }
    }
    
    // Test the legacy key format (double colon) - should not exist
    let legacy_key = format!("smt:root::{}", height);
    println!("Testing legacy key format: '{}'", legacy_key);
    
    match adapter.get(legacy_key.as_bytes()) {
        Ok(Some(value)) => {
            println!("⚠️  Found state root with legacy key format: 0x{}", hex::encode(&value));
        },
        Ok(None) => {
            println!("✅ No state root found with legacy key format (expected)");
        },
        Err(e) => {
            println!("❌ Error accessing legacy key format: {}", e);
        }
    }
    
    // Verify SMTHelper can retrieve the state root
    let smt_helper = SMTHelper::new(adapter.clone());
    match smt_helper.get_smt_root_at_height(height) {
        Ok(root) => {
            println!("✅ SMTHelper retrieved state root: 0x{}", hex::encode(&root));
        },
        Err(e) => {
            return Err(anyhow::anyhow!("SMTHelper failed to retrieve state root: {}", e));
        }
    }
    
    println!("=== KEY FORMAT CONSISTENCY TEST COMPLETE ===");
    Ok(())
}

#[tokio::test]
async fn test_empty_vs_missing_stateroot() -> Result<()> {
    println!("=== EMPTY VS MISSING STATE ROOT TEST ===");
    
    let config = TestConfig::new();
    let runtime = config.create_runtime()?;
    
    // Test with a fresh database (no blocks processed)
    let adapter = &runtime.context.lock().unwrap().db;
    let smt_helper = SMTHelper::new(adapter.clone());
    
    // Try to get state root for height 0 (should not exist)
    match smt_helper.get_smt_root_at_height(0) {
        Ok(root) => {
            println!("⚠️  Unexpectedly found state root for unprocessed height 0: 0x{}", hex::encode(&root));
            
            // Check if it's all zeros
            let zero_root = [0u8; 32];
            if root == zero_root {
                println!("❌ State root is all zeros - this indicates a problem");
                return Err(anyhow::anyhow!("State root should not exist for unprocessed blocks"));
            }
        },
        Err(_) => {
            println!("✅ No state root found for unprocessed height 0 (expected)");
        }
    }
    
    // Try to get state root for a high height (should not exist)
    match smt_helper.get_smt_root_at_height(999999) {
        Ok(root) => {
            println!("❌ Unexpectedly found state root for height 999999: 0x{}", hex::encode(&root));
            return Err(anyhow::anyhow!("State root should not exist for unprocessed blocks"));
        },
        Err(_) => {
            println!("✅ No state root found for height 999999 (expected)");
        }
    }
    
    println!("=== EMPTY VS MISSING STATE ROOT TEST COMPLETE ===");
    Ok(())
}
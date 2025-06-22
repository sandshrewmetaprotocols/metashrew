//! Debug test to investigate BST historical query issues

use memshrew_runtime::{MemStoreAdapter, MemStoreRuntime};
use metashrew_runtime::smt::SMTHelper;
use metashrew_support::utils;
use std::path::PathBuf;
use super::block_builder::create_test_block;
use bitcoin::{BlockHash, hashes::Hash};

#[tokio::test]
async fn test_bst_historical_debug() {
    println!("=== BST Historical Debug Test ===");
    
    // Create runtime with in-memory storage
    let adapter = MemStoreAdapter::new();
    let mut runtime = MemStoreRuntime::load(
        PathBuf::from("./target/wasm32-unknown-unknown/release/metashrew_minimal.wasm"),
        adapter.clone(),
    ).expect("Failed to load runtime");

    // Process multiple blocks to create historical data
    let mut prev_hash = BlockHash::all_zeros();
    let mut blocks = Vec::new();
    
    for height in 0..3 {
        let block = create_test_block(height, prev_hash, &[height as u8]);
        prev_hash = block.block_hash();
        let encoded_block = utils::consensus_encode(&block).expect("Failed to encode block");
        blocks.push(encoded_block);
    }

    // Process each block and inspect BST state after each
    for (height, block) in blocks.iter().enumerate() {
        println!("\n--- Processing Block {} ---", height);
        
        runtime.context.lock().unwrap().height = height as u32;
        runtime.context.lock().unwrap().block = block.clone();
        runtime.run().expect(&format!("Failed to process block {}", height));
        runtime.refresh_memory().expect("Failed to refresh memory");
        
        // After processing, inspect the BST directly
        let db = runtime.context.lock().unwrap().db.clone();
        let smt_helper = SMTHelper::new(db.clone());
        
        println!("Block {} processed. Inspecting BST state:", height);
        
        // Check what heights exist for the blocktracker key
        let key = b"/blocktracker";
        match smt_helper.bst_get_heights_for_key(key) {
            Ok(heights) => {
                println!("  Heights found for /blocktracker: {:?}", heights);
                
                // Get value at each height
                for h in &heights {
                    match smt_helper.bst_get_at_height(key, *h) {
                        Ok(Some(value)) => {
                            println!("    Height {}: {} bytes: {:?}", h, value.len(), value);
                        }
                        Ok(None) => {
                            println!("    Height {}: No value found", h);
                        }
                        Err(e) => {
                            println!("    Height {}: Error: {:?}", h, e);
                        }
                    }
                }
                
                // Test current value
                match smt_helper.bst_get_current(key) {
                    Ok(Some(value)) => {
                        println!("  Current value: {} bytes: {:?}", value.len(), value);
                    }
                    Ok(None) => {
                        println!("  Current value: No value found");
                    }
                    Err(e) => {
                        println!("  Current value: Error: {:?}", e);
                    }
                }
            }
            Err(e) => {
                println!("  Error getting heights: {:?}", e);
            }
        }
        
        // Test view function at current height
        let view_result = runtime.view("blocktracker".to_string(), &vec![], height as u32).await
            .expect(&format!("Failed to query view at height {}", height));
        println!("  View function at height {}: {} bytes: {:?}", height, view_result.len(), view_result);
    }
    
    println!("\n--- Testing Historical Queries ---");
    
    // Now test historical queries at each height
    for query_height in 0..3 {
        println!("\nQuerying at height {}:", query_height);
        
        // Direct BST query
        let db = runtime.context.lock().unwrap().db.clone();
        let smt_helper = SMTHelper::new(db.clone());
        let key = b"/blocktracker";
        
        match smt_helper.bst_get_at_height(key, query_height) {
            Ok(Some(value)) => {
                println!("  Direct BST query: {} bytes: {:?}", value.len(), value);
            }
            Ok(None) => {
                println!("  Direct BST query: No value found");
            }
            Err(e) => {
                println!("  Direct BST query: Error: {:?}", e);
            }
        }
        
        // View function query
        let view_result = runtime.view("blocktracker".to_string(), &vec![], query_height).await
            .expect(&format!("Failed to query view at height {}", query_height));
        println!("  View function: {} bytes: {:?}", view_result.len(), view_result);
        
        // Expected: height 0 should have 1 byte, height 1 should have 2 bytes, height 2 should have 3 bytes
        let expected_length = (query_height + 1) as usize;
        if view_result.len() != expected_length {
            println!("  ❌ MISMATCH: Expected {} bytes, got {} bytes", expected_length, view_result.len());
        } else {
            println!("  ✅ CORRECT: Got expected {} bytes", expected_length);
        }
    }
}
//! Comprehensive end-to-end tests for BST functionality
//! 
//! This test suite validates:
//! - Real memshrew-runtime adapter with metashrew-minimal WASM
//! - BST structures and historical queries
//! - View function correctness at historical points

use anyhow::Result;
use memshrew_runtime::{MemStoreAdapter, MemStoreRuntime};
use metashrew_support::utils;
use std::path::PathBuf;
use bitcoin::BlockHash;
use bitcoin::hashes::Hash;
use super::block_builder::create_test_block;

/// Simple comprehensive test that validates BST functionality
#[tokio::test]
async fn test_comprehensive_bst_functionality() -> Result<()> {
    // Create runtime with metashrew-minimal WASM
    let wasm_path = PathBuf::from("./target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
    let mem_adapter = MemStoreAdapter::new();
    let mut runtime = MemStoreRuntime::load(wasm_path, mem_adapter)?;
    
    // Process 5 blocks
    let mut blocks = Vec::new();
    let mut prev_hash = BlockHash::all_zeros();
    
    for height in 0..5 {
        let block = create_test_block(
            height,
            prev_hash,
            format!("test_block_{}", height).as_bytes(),
        );
        prev_hash = block.block_hash();
        blocks.push(block);
    }
    
    // Process each block
    for (height, block) in blocks.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
    }
    
    // Test historical queries at each height
    for height in 0..5 {
        let view_input = vec![];
        let blocktracker_data = runtime.view("blocktracker".to_string(), &view_input, height).await?;
        
        // At each height, blocktracker should have (height + 1) bytes
        let expected_length = (height + 1) as usize;
        assert_eq!(blocktracker_data.len(), expected_length,
                  "Blocktracker should have {} bytes at height {}", expected_length, height);
        
        println!("✓ Height {}: {} bytes", height, blocktracker_data.len());
    }
    
    // Test getblock view function
    for height in 0..5 {
        let height_input = (height as u32).to_le_bytes().to_vec();
        let block_data = runtime.view("getblock".to_string(), &height_input, height).await?;
        
        assert!(!block_data.is_empty(), "Block data should exist at height {}", height);
        println!("✓ Block at height {}: {} bytes", height, block_data.len());
    }
    
    println!("✅ Comprehensive BST functionality test passed!");
    
    Ok(())
}

/// Test BST historical consistency
#[tokio::test]
async fn test_bst_historical_consistency() -> Result<()> {
    let wasm_path = PathBuf::from("./target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
    let mem_adapter = MemStoreAdapter::new();
    let mut runtime = MemStoreRuntime::load(wasm_path, mem_adapter)?;
    
    // Process 3 blocks
    let mut prev_hash = BlockHash::all_zeros();
    
    for height in 0..3 {
        let block = create_test_block(
            height,
            prev_hash,
            format!("consistency_test_{}", height).as_bytes(),
        );
        prev_hash = block.block_hash();
        
        let block_bytes = utils::consensus_encode(&block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
    }
    
    // Test that historical queries are consistent
    let view_input = vec![];
    
    // Query at height 0 multiple times - should always return same result
    let result1 = runtime.view("blocktracker".to_string(), &view_input, 0).await?;
    let result2 = runtime.view("blocktracker".to_string(), &view_input, 0).await?;
    assert_eq!(result1, result2, "Historical queries should be consistent");
    
    // Query at height 1 multiple times - should always return same result
    let result1 = runtime.view("blocktracker".to_string(), &view_input, 1).await?;
    let result2 = runtime.view("blocktracker".to_string(), &view_input, 1).await?;
    assert_eq!(result1, result2, "Historical queries should be consistent");
    
    // Verify that different heights return different results
    let height0_result = runtime.view("blocktracker".to_string(), &view_input, 0).await?;
    let height1_result = runtime.view("blocktracker".to_string(), &view_input, 1).await?;
    let height2_result = runtime.view("blocktracker".to_string(), &view_input, 2).await?;
    
    assert_eq!(height0_result.len(), 1);
    assert_eq!(height1_result.len(), 2);
    assert_eq!(height2_result.len(), 3);
    
    // Verify that height0_result is a prefix of height1_result
    assert_eq!(&height1_result[0..1], &height0_result[..]);
    
    // Verify that height1_result is a prefix of height2_result
    assert_eq!(&height2_result[0..2], &height1_result[..]);
    
    println!("✅ BST historical consistency test passed!");
    
    Ok(())
}
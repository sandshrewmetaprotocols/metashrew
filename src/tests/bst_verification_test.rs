//! Test to verify BST storage is working correctly

use anyhow::Result;
use memshrew_runtime::{MemStoreAdapter, MemStoreRuntime};
use metashrew_support::utils;
use std::path::PathBuf;
use bitcoin::hashes::Hash;

#[tokio::test]
async fn test_bst_storage_verification() -> Result<()> {
    // Create runtime
    let wasm_path = PathBuf::from("./target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
    let mem_adapter = MemStoreAdapter::new();
    let mut runtime = MemStoreRuntime::load(wasm_path, mem_adapter)?;
    
    // Create test blocks
    let test_block_0 = super::block_builder::create_test_block(
        0,
        bitcoin::BlockHash::all_zeros(),
        b"test_block_0",
    );
    let test_block_1 = super::block_builder::create_test_block(
        1,
        test_block_0.block_hash(),
        b"test_block_1",
    );
    
    // Process block 0
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = utils::consensus_encode(&test_block_0)?;
        context.height = 0;
    }
    runtime.run()?;
    
    // Process block 1
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = utils::consensus_encode(&test_block_1)?;
        context.height = 1;
    }
    runtime.run()?;
    
    // Test view function at different heights
    let view_input = vec![];
    
    // Check blocktracker at height 0 (should be 1 byte)
    let blocktracker_h0 = runtime.view("blocktracker".to_string(), &view_input, 0).await?;
    println!("Blocktracker at height 0: {} bytes: {:?}", blocktracker_h0.len(), blocktracker_h0);
    assert_eq!(blocktracker_h0.len(), 1, "Blocktracker at height 0 should be 1 byte");
    
    // Check blocktracker at height 1 (should be 2 bytes)
    let blocktracker_h1 = runtime.view("blocktracker".to_string(), &view_input, 1).await?;
    println!("Blocktracker at height 1: {} bytes: {:?}", blocktracker_h1.len(), blocktracker_h1);
    assert_eq!(blocktracker_h1.len(), 2, "Blocktracker at height 1 should be 2 bytes");
    
    // Check that historical queries work - blocktracker at height 0 should still return 1 byte
    let blocktracker_h0_again = runtime.view("blocktracker".to_string(), &view_input, 0).await?;
    assert_eq!(blocktracker_h0_again, blocktracker_h0, "Historical queries should be consistent");
    
    // Check block data at height 0 using getblock view function
    let height_0_input = vec![0, 0, 0, 0]; // Height 0 as little-endian bytes
    let block_h0 = runtime.view("getblock".to_string(), &height_0_input, 0).await?;
    println!("Block at height 0: {} bytes", block_h0.len());
    assert!(block_h0.len() > 0, "Block data should exist at height 0");
    
    // Check block data at height 1 using getblock view function
    let height_1_input = vec![1, 0, 0, 0]; // Height 1 as little-endian bytes
    let block_h1 = runtime.view("getblock".to_string(), &height_1_input, 1).await?;
    println!("Block at height 1: {} bytes", block_h1.len());
    assert!(block_h1.len() > 0, "Block data should exist at height 1");
    
    // Verify blocks are different
    assert_ne!(block_h0, block_h1, "Blocks at different heights should be different");
    
    println!("✅ BST storage verification successful!");
    println!("✅ Historical queries working correctly!");
    println!("✅ Height-based indexing working correctly!");
    
    Ok(())
}
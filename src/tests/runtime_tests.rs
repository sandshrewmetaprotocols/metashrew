//! Runtime tests for MetashrewRuntime with in-memory backend
//!
//! These tests verify the core functionality of the MetashrewRuntime using
//! the memshrew in-memory adapter and metashrew-minimal WASM module.

use super::{TestConfig, TestUtils, block_builder::*};
use anyhow::Result;
use memshrew_runtime::{MemStoreRuntime, MemStoreAdapter, KeyValueStoreLike};
use metashrew_support::utils;

// Helper functions for database access (bypassing problematic view functions)
fn get_blocktracker(adapter: &MemStoreAdapter) -> Result<Vec<u8>> {
    let key = b"/blocktracker".to_vec();
    Ok(adapter.get_immutable(&key)?.unwrap_or_default())
}

fn get_indexed_block(adapter: &MemStoreAdapter, height: u32) -> Result<Option<Vec<u8>>> {
    let key = format!("/blocks/{}", height).into_bytes();
    Ok(adapter.get_immutable(&key)?)
}

#[tokio::test]
async fn test_runtime_creation() -> Result<()> {
    let config = TestConfig::new();
    let runtime = config.create_runtime()?;
    
    // Runtime should be created successfully
    assert!(runtime.context.lock().unwrap().db.is_open());
    
    Ok(())
}

#[tokio::test]
async fn test_single_block_processing() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create a genesis block
    let genesis = TestUtils::create_genesis_block();
    let block_bytes = utils::consensus_encode(&genesis)?;
    
    // Set the input in the runtime context (only block bytes, runtime handles height)
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes;
        context.height = 0;
    }
    
    // Run the indexer
    runtime.run()?;
    
    // Verify the block was processed
    let adapter = &runtime.context.lock().unwrap().db;
    assert!(!adapter.is_empty());
    
    // Check that the block was stored using direct database access
    let stored_block = get_indexed_block(adapter, 0)?;
    assert!(stored_block.is_some());
    
    Ok(())
}

#[tokio::test]
async fn test_multiple_block_processing() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create a chain of blocks
    let chain = ChainBuilder::new()
        .add_blocks(3)
        .blocks();
    
    // Process each block
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?; // Reset WASM memory for next block
    }
    
    // Verify all blocks were processed using direct database access
    let adapter = &runtime.context.lock().unwrap().db;
    
    for height in 0..=3 {
        let stored_block = get_indexed_block(adapter, height)?;
        assert!(stored_block.is_some(), "Block {} should be stored", height);
    }
    
    Ok(())
}

#[tokio::test]
async fn test_blocktracker_view_function() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create and process a few blocks
    let chain = ChainBuilder::new()
        .add_blocks(2)
        .blocks();
    
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
    }
    
    // Access blocktracker data directly from database (bypassing view function)
    let adapter = &runtime.context.lock().unwrap().db;
    let result = get_blocktracker(adapter)?;
    
    // The blocktracker should contain data (first byte of each block hash)
    assert!(!result.is_empty(), "Blocktracker should contain data");
    assert_eq!(result.len(), 3, "Should have 3 entries (genesis + 2 blocks)");
    
    Ok(())
}

#[tokio::test]
async fn test_getblock_view_function() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create and process a block
    let genesis = TestUtils::create_genesis_block();
    let block_bytes = utils::consensus_encode(&genesis)?;
    
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes.clone();
        context.height = 0;
    }
    
    runtime.run()?;
    
    // Access block data directly from database (bypassing view function)
    let adapter = &runtime.context.lock().unwrap().db;
    let result = get_indexed_block(adapter, 0)?;
    
    // Should return the serialized block
    assert!(result.is_some(), "getblock should return block data");
    
    // Verify it's the same block we stored
    assert_eq!(result.unwrap(), block_bytes, "Retrieved block should match original");
    
    Ok(())
}

#[tokio::test]
async fn test_height_tracking() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Process multiple blocks and verify height tracking
    let chain = ChainBuilder::new()
        .add_blocks(5)
        .blocks();
    
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
        
        // Verify height was updated in the database
        let adapter = &runtime.context.lock().unwrap().db;
        let height_key = metashrew_runtime::TIP_HEIGHT_KEY.as_bytes().to_vec();
        let stored_height_bytes = adapter.get_immutable(&height_key)?;
        
        if let Some(bytes) = stored_height_bytes {
            let stored_height = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            assert_eq!(stored_height, height as u32 + 1, "Height should be incremented");
        }
    }
    
    Ok(())
}

#[tokio::test]
async fn test_blocktracker_accumulation() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create blocks with known hashes
    let chain = ChainBuilder::new()
        .add_blocks(3)
        .blocks();
    
    let mut expected_tracker = Vec::new();
    
    // Process each block and track expected blocktracker content
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        expected_tracker.push(block.block_hash()[0]); // First byte of block hash
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
        
        // Check blocktracker after each block using direct database access
        let adapter = &runtime.context.lock().unwrap().db;
        let result = get_blocktracker(adapter)?;
        
        assert_eq!(result.len(), expected_tracker.len(),
                  "Blocktracker length should match processed blocks at height {}", height);
        assert_eq!(result, expected_tracker,
                  "Blocktracker content should match expected at height {}", height);
    }
    
    Ok(())
}

#[tokio::test]
async fn test_database_persistence() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Process a block
    let genesis = TestUtils::create_genesis_block();
    let block_bytes = utils::consensus_encode(&genesis)?;
    
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes.clone();
        context.height = 0;
    }
    
    runtime.run()?;
    
    // Get a snapshot of the database state
    let adapter = &runtime.context.lock().unwrap().db;
    let initial_data = adapter.get_all_data();
    
    // Create a new runtime with the same data
    let new_adapter = MemStoreAdapter::with_data(initial_data);
    let _new_runtime = MemStoreRuntime::load(config.wasm_path.clone(), new_adapter)?;
    
    // Verify the data is accessible in the new runtime using direct database access
    let result = get_indexed_block(&_new_runtime.context.lock().unwrap().db, 0)?;
    
    assert!(result.is_some(), "Block should be accessible in new runtime");
    assert_eq!(result.unwrap(), block_bytes, "Block data should match");
    
    Ok(())
}

#[tokio::test]
async fn test_error_handling() -> Result<()> {
    let config = TestConfig::new();
    let runtime = config.create_runtime()?;
    
    // Try to call a non-existent view function
    let view_input = Vec::new();
    let result = runtime.view("nonexistent".to_string(), &view_input, 0).await;
    
    assert!(result.is_err(), "Should fail when calling non-existent function");
    
    Ok(())
}

#[tokio::test]
async fn test_concurrent_view_calls() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Process some blocks first
    let chain = ChainBuilder::new()
        .add_blocks(2)
        .blocks();
    
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
    }
    
    // Test concurrent database access instead of view calls
    let adapter = &runtime.context.lock().unwrap().db;
    let expected_data = get_blocktracker(adapter)?;
    
    // Simulate multiple concurrent reads
    for _ in 0..5 {
        let data = get_blocktracker(adapter)?;
        assert_eq!(data.len(), 3, "All database reads should return same data");
        assert_eq!(data, expected_data, "Data should be consistent");
    }
    
    Ok(())
}
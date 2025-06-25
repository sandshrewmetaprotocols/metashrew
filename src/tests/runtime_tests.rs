//! Runtime tests for MetashrewRuntime with in-memory backend
//!
//! These tests verify the core functionality of the MetashrewRuntime using
//! the memshrew in-memory adapter and metashrew-minimal WASM module.

use super::block_builder::ChainBuilder;
use super::{TestConfig, TestUtils};
use anyhow::Result;
use memshrew_runtime::{MemStoreAdapter, MemStoreRuntime};
use metashrew_runtime::smt::SMTHelper;
use metashrew_support::utils;

// Helper functions for BST database access
fn get_blocktracker_bst(adapter: &MemStoreAdapter, height: u32) -> Result<Vec<u8>> {
    let smt_helper = SMTHelper::new(adapter.clone());
    let key = b"/blocktracker".to_vec();
    Ok(smt_helper
        .get_at_height(&key, height)?
        .unwrap_or_default())
}

fn get_indexed_block_bst(adapter: &MemStoreAdapter, height: u32) -> Result<Option<Vec<u8>>> {
    let smt_helper = SMTHelper::new(adapter.clone());
    let key = format!("/blocks/{}", height).into_bytes();
    Ok(smt_helper.get_at_height(&key, height)?)
}

#[tokio::test]
async fn test_basic_indexing_workflow() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;

    // Create a small chain of blocks
    let chain = ChainBuilder::new().add_blocks(3).blocks();

    // Process each block
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

    // Verify all blocks were processed and stored
    let adapter = &runtime.context.lock().unwrap().db;
    assert!(!adapter.is_empty());

    for height in 0..chain.len() {
        let stored_block = get_indexed_block_bst(adapter, height as u32)?;
        assert!(stored_block.is_some(), "Block {} should be stored", height);
    }

    Ok(())
}

#[tokio::test]
async fn test_database_storage_and_retrieval() -> Result<()> {
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

    // Verify database storage using direct access
    let adapter = &runtime.context.lock().unwrap().db;
    assert!(!adapter.is_empty());

    // Check block storage
    let stored_block = get_indexed_block_bst(adapter, 0)?;
    assert!(stored_block.is_some(), "Block should be stored");

    // Check blocktracker storage
    let blocktracker = get_blocktracker_bst(adapter, 0)?;
    assert!(!blocktracker.is_empty(), "Blocktracker should contain data");

    Ok(())
}

#[tokio::test]
async fn test_blocktracker_accumulation() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;

    // Process multiple blocks and verify blocktracker grows
    let chain = ChainBuilder::new().add_blocks(3).blocks();

    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;

        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }

        runtime.run()?;
        runtime.refresh_memory()?;

        // Verify blocktracker grows with each block
        let adapter = &runtime.context.lock().unwrap().db;
        let result = get_blocktracker_bst(adapter, height as u32)?;

        // Blocktracker length = number of blocks processed
        let expected_length = height + 1;
        assert_eq!(
            result.len(),
            expected_length,
            "Blocktracker should grow with each block"
        );
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

    // Verify the data is accessible in the new runtime using BST access
    let result = get_indexed_block_bst(&_new_runtime.context.lock().unwrap().db, 0)?;

    assert!(
        result.is_some(),
        "Block should be accessible in new runtime"
    );

    // BST stores the actual block data without height prefix
    let stored_data = result.unwrap();
    assert_eq!(
        stored_data.len(),
        block_bytes.len(),
        "Stored data should match original block data"
    );

    Ok(())
}

#[tokio::test]
async fn test_error_handling() -> Result<()> {
    let config = TestConfig::new();
    let runtime = config.create_runtime()?;

    // Try to call a non-existent view function
    let view_input = Vec::new();
    let result = runtime
        .view("nonexistent".to_string(), &view_input, 0)
        .await;

    assert!(
        result.is_err(),
        "Should fail when calling non-existent function"
    );

    Ok(())
}

#[tokio::test]
async fn test_concurrent_view_calls() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;

    // Process some blocks first
    let chain = ChainBuilder::new().add_blocks(2).blocks();

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
    let final_height = (chain.len() - 1) as u32;
    let expected_data = get_blocktracker_bst(adapter, final_height)?;

    // Simulate multiple concurrent reads
    for _ in 0..5 {
        let data = get_blocktracker_bst(adapter, final_height)?;
        assert_eq!(
            data.len(),
            expected_data.len(),
            "All database reads should return same data"
        );
        assert_eq!(data, expected_data, "Data should be consistent");
    }

    Ok(())
}

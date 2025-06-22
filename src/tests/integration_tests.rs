//! Integration tests for the complete Metashrew indexing workflow
//!
//! These tests verify end-to-end functionality including block processing,
//! view function execution, and data consistency across multiple operations.

use super::block_builder::ChainBuilder;
use super::TestConfig;
use anyhow::Result;
use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::smt::SMTHelper;
use metashrew_support::utils;
use std::collections::HashMap;

// Helper functions for BST database access
fn get_blocktracker_bst(adapter: &MemStoreAdapter, height: u32) -> Result<Vec<u8>> {
    let smt_helper = SMTHelper::new(adapter.clone());
    let key = b"/blocktracker".to_vec();
    Ok(smt_helper
        .bst_get_at_height(&key, height)?
        .unwrap_or_default())
}

fn get_indexed_block_bst(adapter: &MemStoreAdapter, height: u32) -> Result<Option<Vec<u8>>> {
    let smt_helper = SMTHelper::new(adapter.clone());
    let key = format!("/blocks/{}", height).into_bytes();
    Ok(smt_helper.bst_get_at_height(&key, height)?)
}

/// Test complete indexing workflow - comprehensive E2E test
#[tokio::test]
async fn test_complete_indexing_workflow() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;

    // Create a realistic chain of blocks
    let chain = ChainBuilder::new().add_blocks(10).blocks();

    // Process all blocks in sequence
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

    // Verify final state using direct database access
    let adapter = &runtime.context.lock().unwrap().db;

    // Check that all blocks are stored using BST access
    for height in 0..chain.len() {
        let stored_block = get_indexed_block_bst(adapter, height as u32)?;
        assert!(stored_block.is_some(), "Block {} should be stored", height);
    }

    // Check blocktracker has correct length (should be number of blocks processed)
    let final_height = (chain.len() - 1) as u32;
    let blocktracker_result = get_blocktracker_bst(adapter, final_height)?;
    assert_eq!(
        blocktracker_result.len(),
        chain.len(),
        "Blocktracker should track all blocks"
    );

    Ok(())
}

/// Test database state consistency during indexing
#[tokio::test]
async fn test_database_state_consistency() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;

    // Process some blocks
    let chain = ChainBuilder::new().add_blocks(3).blocks();

    let mut snapshots: Vec<HashMap<Vec<u8>, Vec<u8>>> = Vec::new();

    // Take snapshots after each block
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;

        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }

        runtime.run()?;
        runtime.refresh_memory()?;

        // Take a snapshot of the database state
        let adapter = &runtime.context.lock().unwrap().db;
        snapshots.push(adapter.get_all_data());
    }

    // Verify that each snapshot contains the previous data plus new data
    for i in 1..snapshots.len() {
        let prev_snapshot = &snapshots[i - 1];
        let curr_snapshot = &snapshots[i];

        // Current snapshot should have more or equal keys
        assert!(
            curr_snapshot.len() >= prev_snapshot.len(),
            "Database should only grow, not shrink"
        );
    }

    Ok(())
}

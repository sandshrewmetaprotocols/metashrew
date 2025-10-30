
//! Test for historical view function execution.
//!
//! This test verifies that `metashrew_view` can correctly render a result
//! from the historical state of the database, even after the state has been
//! overwritten in a later block.

use crate::test_utils::{TestConfig, TestUtils};
use memshrew_runtime::MemStoreAdapter;
use metashrew_sync::{MetashrewRuntimeAdapter, SyncConfig, SyncEngine, ViewCall, BitcoinNodeAdapter, RuntimeAdapter, StorageAdapter};
use anyhow::Result;
use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use crate::in_memory_adapters::InMemoryBitcoinNode;

#[tokio::test]
async fn test_historical_view() -> Result<()> {
    // 1. Setup
    let test_config = TestConfig::new();
    let storage = MemStoreAdapter::new();
    let runtime = test_config.create_runtime_from_adapter(storage.clone())?;
    let adapter = MetashrewRuntimeAdapter::new(runtime);

    let genesis_block = TestUtils::create_test_block(0, BlockHash::all_zeros());
    let mut node = InMemoryBitcoinNode::new(genesis_block.clone());
    node.add_block(genesis_block, 0);

    let block1 = TestUtils::create_test_block(1, BlockHash::from_slice(&node.get_block_hash(0).await?)?);
    node.add_block(block1.clone(), 1);

    let block2 = TestUtils::create_test_block(2, BlockHash::from_slice(&node.get_block_hash(1).await?)?);
    node.add_block(block2.clone(), 2);

    // 2. Index block 1
    let sync_config = SyncConfig {
        start_block: 1,
        exit_at: Some(2),
        ..Default::default()
    };
    let mut syncer = metashrew_sync::sync::MetashrewSync::new(
        node.clone(),
        storage.clone(),
        adapter.clone(),
        sync_config,
    );
    syncer.start().await?;

    // 3. Index block 2
    let sync_config_2 = SyncConfig {
        start_block: 2,
        exit_at: Some(3),
        ..Default::default()
    };
    let mut syncer_2 = metashrew_sync::sync::MetashrewSync::new(
        node.clone(),
        storage.clone(),
        adapter.clone(),
        sync_config_2,
    );
    syncer_2.start().await?;

    // 4. View latest state (height 2)
    let view_call_latest = ViewCall {
        function_name: "getblock".to_string(),
        input_data: 2u32.to_le_bytes().to_vec(),
        height: 2,
    };
    let result_latest = adapter.execute_view(view_call_latest).await?;
    assert_eq!(result_latest.data, metashrew_support::utils::consensus_encode(&block2).unwrap(), "Latest value should be from block 2");

    // 5. View historical state (height 1)
    let view_call_historical = ViewCall {
        function_name: "getblock".to_string(),
        input_data: 1u32.to_le_bytes().to_vec(),
        height: 1,
    };
    let result_historical = adapter.execute_view(view_call_historical).await?;
    assert_eq!(result_historical.data, metashrew_support::utils::consensus_encode(&block1).unwrap(), "Historical value should be from block 1");

    Ok(())
}

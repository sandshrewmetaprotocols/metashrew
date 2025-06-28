//! Comprehensive end-to-end tests for the full sync framework
use super::block_builder::ChainBuilder;
use super::in_memory_adapters::InMemoryBitcoinNode;
use super::TestConfig;
use anyhow::Result;
use memshrew_runtime::MemStoreAdapter;
use rockshrew_sync::{MetashrewRuntimeAdapter, MetashrewSync, SyncConfig, StorageAdapter};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::test]
async fn test_in_memory_sync_framework() -> Result<()> {
    // 1. Setup the test environment
    let config = TestConfig::new();
    let chain = ChainBuilder::new().add_blocks(5);
    let blocks = chain.blocks();
    let node = InMemoryBitcoinNode::new(blocks[0].clone());
    for i in 1..blocks.len() {
        node.add_block(blocks[i].clone(), i as u32);
    }

    // 2. Create in-memory adapters
    let storage_adapter = MemStoreAdapter::new();
    let runtime = Arc::new(Mutex::new(
        config.create_runtime_from_adapter(storage_adapter.clone())?,
    ));
    let runtime_adapter = MetashrewRuntimeAdapter::from_arc(runtime.clone());

    // 3. Configure and create the sync engine
    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(5),
        ..Default::default()
    };
    let mut sync_engine = MetashrewSync::new(node, storage_adapter.clone(), runtime_adapter, sync_config);

    // 4. Run the synchronization
    sync_engine.run().await?;

    // 5. Verify the results
    let final_height = sync_engine.storage().read().await.get_indexed_height().await?;
    assert_eq!(final_height, 4, "Should be indexed up to block 4");

    // 6. Test historical view calls
    for height in 0..5 {
        let view_input = vec![];
        let blocktracker_data = runtime
            .lock()
            .await
            .view("blocktracker".to_string(), &view_input, height)
            .await?;
        let expected_length = (height + 1) as usize;
        assert_eq!(
            blocktracker_data.len(),
            expected_length,
            "Blocktracker should have {} bytes at height {}",
            expected_length,
            height
        );
    }

    println!("✅ In-memory sync framework test passed!");
    Ok(())
}

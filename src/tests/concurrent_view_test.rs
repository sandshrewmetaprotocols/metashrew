use crate::in_memory_adapters::{InMemoryBitcoinNode, InMemoryRuntime};
use memshrew_runtime::MemStoreAdapter;
use metashrew_sync::{JsonRpcProvider, MetashrewSync, SyncConfig, SyncEngine};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use bitcoin::hashes::Hash;

#[tokio::test]
async fn test_concurrent_view_while_indexing() {
    // 1. Set up the test
    let wasm_bytes = include_bytes!("../../target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
    let runtime = InMemoryRuntime::new(wasm_bytes);
    let storage = MemStoreAdapter::new();
    let genesis_block = crate::test_utils::TestUtils::create_test_block(0, bitcoin::BlockHash::all_zeros());
    let node = InMemoryBitcoinNode::new(genesis_block.clone());

    // Add a block to the mock node
    let block1 = crate::test_utils::TestUtils::create_test_block(1, genesis_block.block_hash());
    node.add_block(block1, 1);

    let (tx, mut rx) = mpsc::channel(1);

    let config = SyncConfig {
        start_block: 0,
        exit_at: Some(1),
        pipeline_size: Some(1),
        max_reorg_depth: 10,
        reorg_check_threshold: 6,
    };

    let mut sync = MetashrewSync::new(node, storage, runtime, config);
    sync.init().await;

    let sync_arc = Arc::new(tokio::sync::Mutex::new(sync));
    let sync_clone = sync_arc.clone();

    // 2. Start the indexer in a separate thread, but make it block before acquiring the write lock
    let indexer_handle = tokio::spawn(async move {
        let mut sync_guard = sync_clone.lock().await;
        // Signal that we are about to process a block
        tx.send(()).await.unwrap();
        // Simulate a delay before the actual processing starts
        tokio::time::sleep(Duration::from_secs(5)).await;
        sync_guard.process_single_block(1).await.unwrap();
    });

    // Wait for the indexer to signal that it's about to process a block
    rx.recv().await.unwrap();

    // 3. Call metashrew_view concurrently
    let view_result = sync_arc
        .lock()
        .await
        .metashrew_view(
            "getblock".to_string(),
            hex::encode(0u32.to_le_bytes()).to_string(),
            "0".to_string(),
        )
        .await;

    // 4. Assert the view call succeeds
    assert!(view_result.is_ok());

    // 5. Clean up
    indexer_handle.await.unwrap();
}

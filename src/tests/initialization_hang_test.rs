// Journal:
// - Added a new test case to simulate the initialization of the indexing process from a high block number (880000).
// - This test is designed to reproduce the hanging behavior reported by the user.
// - It sets up the sync engine to start from a non-zero block and then calls the `init` function.
// - The test then checks that the `current_height` is correctly set to the start block, and that the
//   state root for the previous block is initialized to an empty hash.

use bitcoin::block::{Block, Header, Version};
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, TxMerkleNode, CompactTarget};
use crate::in_memory_adapters::{InMemoryBitcoinNode};
use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::MetashrewRuntime;
use metashrew_sync::{StorageAdapter, SyncConfig, MetashrewSync};
use std::sync::Arc;
use tokio::sync::RwLock;

use rockshrew_mono::adapters::MetashrewRuntimeAdapter;

#[tokio::test]
async fn test_initialization_hang() {
    let node_adapter = InMemoryBitcoinNode::new(Block { header: Header { version: Version::from_consensus(1), prev_blockhash: BlockHash::all_zeros(), merkle_root: TxMerkleNode::all_zeros(), time: 0, bits: CompactTarget::from_consensus(0), nonce: 0 }, txdata: vec![] });
    let storage_adapter = MemStoreAdapter::new();
    let runtime = MetashrewRuntime::new(crate::test_utils::WASM, MemStoreAdapter::new()).unwrap();
    let runtime_adapter = MetashrewRuntimeAdapter::new(Arc::new(RwLock::new(runtime)));

    let start_block = 880000;
    let config = SyncConfig {
        start_block,
        exit_at: Some(start_block + 1),
        ..Default::default()
    };

    let sync = MetashrewSync::new(
        node_adapter.clone(),
        storage_adapter.clone(),
        runtime_adapter.clone(),
        config,
    );

    sync.init().await;

    // Assert that the current height is the start block
    assert_eq!(sync.current_height.load(std::sync::atomic::Ordering::SeqCst), start_block);

    // Manually trigger the reorg check to simulate the problematic behavior
    let _ = metashrew_sync::handle_reorg(
        start_block,
        Arc::new(node_adapter),
        Arc::new(RwLock::new(storage_adapter.clone())),
        Arc::new(RwLock::new(runtime_adapter)),
        &sync.config,
    )
    .await;

    let prev_state_root = storage_adapter.get_state_root(start_block - 1).await.unwrap();
    assert_eq!(prev_state_root, Some(vec![0; 32]));
}
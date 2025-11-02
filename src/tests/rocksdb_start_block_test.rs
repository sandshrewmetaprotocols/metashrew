// Journal:
// - Added a new test case to reproduce the initialization hang when using RocksDB and a non-zero start block.
// - This test uses a temporary RocksDB database to better simulate the production environment.

use crate::in_memory_adapters::InMemoryBitcoinNode;
use bitcoin::block::{Block, Header, Version};
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, CompactTarget, TxMerkleNode};
use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::MetashrewRuntime;
use metashrew_sync::{SyncConfig, MetashrewSync};
use rockshrew_mono::adapters::MetashrewRuntimeAdapter;
use rockshrew_runtime::RocksDBStorageAdapter;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_rocksdb_start_block_hang() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().to_str().unwrap().to_string();
    let db = rocksdb::DB::open_default(db_path).unwrap();
    let storage_adapter = RocksDBStorageAdapter::new(Arc::new(db));

    let node_adapter = InMemoryBitcoinNode::new(Block {
        header: Header {
            version: Version::from_consensus(1),
            prev_blockhash: BlockHash::all_zeros(),
            merkle_root: TxMerkleNode::all_zeros(),
            time: 0,
            bits: CompactTarget::from_consensus(0),
            nonce: 0,
        },
        txdata: vec![],
    });

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

    assert_eq!(
        sync.current_height
            .load(std::sync::atomic::Ordering::SeqCst),
        start_block
    );
}

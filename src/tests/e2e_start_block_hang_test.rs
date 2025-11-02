// Journal:
// - Added a new end-to-end test to reproduce the initialization hang when using RocksDB and a non-zero start block.
// - This test calls the `run` function in `rockshrew-mono/src/lib.rs` with a `RocksDBStorageAdapter` and a large `start_block` value.
// - It runs the `run` function in a separate task and uses a timeout to detect if it hangs.

use crate::in_memory_adapters::InMemoryBitcoinNode;
use bitcoin::block::{Block, Header, Version};
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, CompactTarget, TxMerkleNode};
use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::MetashrewRuntime;
use rockshrew_mono::adapters::MetashrewRuntimeAdapter;
use rockshrew_mono::{run, Args};
use rockshrew_runtime::RocksDBStorageAdapter;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn test_e2e_start_block_hang() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().to_str().unwrap().to_string();

    let args = Args {
        daemon_rpc_url: "http://localhost:8332".to_string(),
        indexer: PathBuf::from("../target/wasm32-unknown-unknown/release/metashrew_minimal.wasm"),
        db_path: PathBuf::from(db_path),
        fork: None,
        legacy_fork: false,
        start_block: Some(880000),
        auth: None,
        host: "127.0.0.1".to_string(),
        port: 8080,
        label: None,
        exit_at: Some(880001),
        pipeline_size: Some(1),
        cors: None,
        snapshot_directory: None,
        snapshot_interval: 1000,
        repo: None,
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    let db = rocksdb::DB::open_default(args.db_path.clone()).unwrap();
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

    let run_future = run(
        args,
        node_adapter,
        storage_adapter,
        runtime_adapter,
        None,
    );

    match timeout(Duration::from_secs(10), run_future).await {
        Ok(_) => (),
        Err(_) => panic!("The run function timed out, which indicates a hang."),
    }
}

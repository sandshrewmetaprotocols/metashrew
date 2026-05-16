//! Integration tests for the real `RocksDBStorageAdapter::commit_atomic`.
//!
//! These exercise the on-disk durability path that the mock-storage tests
//! in `metashrew-sync/tests/strict_determinism.rs` cannot reach. The key
//! invariants pinned here:
//!
//! 1. `commit_atomic` writes all three keys (indexed-height pointer,
//!    block-hash record, state-root record) into a single underlying
//!    RocksDB batch. After a successful call, all three are visible.
//!
//! 2. The strict-in-order rule (`height == tip + 1` after the first
//!    commit) is enforced even when the underlying RocksDB is fresh.
//!
//! 3. The write is durable: opening a fresh `DB` handle on the same path
//!    after the commit returns Ok produces the committed state. (We
//!    can't easily simulate a host-level crash inside a unit test, but
//!    we *can* verify that the batch is committed through a re-open,
//!    which is what `sync=true` guarantees us against process exit.)

use metashrew_sync::StorageAdapter;
use rockshrew_runtime::{RocksDBRuntimeAdapter, RocksDBStorageAdapter};
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn commit_atomic_writes_all_three_keys_atomically() {
    let dir = tempdir().unwrap();
    let adapter = RocksDBRuntimeAdapter::open_optimized(
        dir.path().to_str().unwrap().to_string(),
    )
    .unwrap();
    let db = adapter.db.clone();
    let mut storage = RocksDBStorageAdapter::new(db.clone());

    let h: u32 = 850_000;
    let block_hash = [0xaa_u8; 32];
    let state_root = [0xbb_u8; 32];

    storage
        .commit_atomic(h, &block_hash, &state_root)
        .await
        .expect("first commit on fresh DB must succeed");

    // All three keys are visible.
    assert_eq!(storage.get_indexed_height().await.unwrap(), h);
    assert_eq!(
        storage.get_block_hash(h).await.unwrap(),
        Some(block_hash.to_vec())
    );
    // get_state_root uses SMTHelper, so just verify the underlying raw key
    // is set instead.
    let root_key = format!("smt:root:{}", h).into_bytes();
    let raw = db.get(&root_key).unwrap();
    assert_eq!(raw, Some(state_root.to_vec()));
}

#[tokio::test]
async fn commit_atomic_rejects_out_of_order_on_rocksdb() {
    let dir = tempdir().unwrap();
    let adapter = RocksDBRuntimeAdapter::open_optimized(
        dir.path().to_str().unwrap().to_string(),
    )
    .unwrap();
    let mut storage = RocksDBStorageAdapter::new(adapter.db.clone());

    storage
        .commit_atomic(100, &[1u8; 32], &[2u8; 32])
        .await
        .expect("first commit succeeds");

    // Out-of-order attempts fail.
    assert!(storage
        .commit_atomic(102, &[3u8; 32], &[4u8; 32])
        .await
        .is_err());
    assert!(storage
        .commit_atomic(100, &[3u8; 32], &[4u8; 32])
        .await
        .is_err());
    assert!(storage
        .commit_atomic(50, &[3u8; 32], &[4u8; 32])
        .await
        .is_err());

    // Tip unchanged.
    assert_eq!(storage.get_indexed_height().await.unwrap(), 100);

    // In-order commit succeeds.
    storage
        .commit_atomic(101, &[5u8; 32], &[6u8; 32])
        .await
        .expect("in-order commit at tip+1 succeeds");
    assert_eq!(storage.get_indexed_height().await.unwrap(), 101);
}

#[tokio::test]
async fn commit_atomic_survives_reopen() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap().to_string();

    // Commit a block, then drop the adapter and re-open.
    {
        let adapter = RocksDBRuntimeAdapter::open_optimized(path.clone()).unwrap();
        let mut storage = RocksDBStorageAdapter::new(adapter.db.clone());
        storage
            .commit_atomic(123_456, &[0x42u8; 32], &[0x43u8; 32])
            .await
            .unwrap();
        // Drop adapter — releases the DB handle.
    }

    // Re-open. With sync=true, the batch is in the WAL and replays on open
    // (or has already been flushed). Either way the state must be visible.
    let adapter2 = RocksDBRuntimeAdapter::open_optimized(path.clone()).unwrap();
    let storage2 = RocksDBStorageAdapter::new(adapter2.db.clone());
    assert_eq!(storage2.get_indexed_height().await.unwrap(), 123_456);
    assert_eq!(
        storage2.get_block_hash(123_456).await.unwrap(),
        Some(vec![0x42u8; 32])
    );
}

// Helper to silence the unused-Arc warning on `db` in the first test if we
// stop using it directly in a future refactor.
#[allow(dead_code)]
fn _ensure_arc_db_in_scope(_: Arc<rocksdb::DB>) {}

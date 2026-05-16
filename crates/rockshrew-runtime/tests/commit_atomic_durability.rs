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

/// Regression test for the v9.0.5-rc.4 fix: the WASM-side __flush
/// (`BatchedSMTHelper::calculate_and_store_state_root_batched`) MUST NOT
/// write the `__INTERNAL/height` indexed-height pointer. If it does, the
/// strict-in-order check inside `commit_atomic` will self-reject — it
/// will read its own batch's height write back and refuse to advance to
/// the same height, even though that's the legitimate next block.
///
/// This was observed on `meta` during snapshot verification at h≈947735:
/// rc.2 crashed after 5 bounded retries; rc.3's infinite-retry loop would
/// have hung instead. The fix moves ownership of `__INTERNAL/height` from
/// the WASM batch to `commit_atomic` exclusively.
///
/// Properties pinned by this test:
///   1. After `calculate_and_store_state_root_batched` for block N runs,
///      `__INTERNAL/height` is STILL at N-1. The WASM batch did not
///      advance it.
///   2. `commit_atomic` for height N then succeeds (tip + 1 rule holds).
///   3. Re-calling `commit_atomic` for height N is a *correct* rejection
///      (legitimate out-of-order), not a self-rejection. This is the
///      signal that distinguishes "the fix is working" from "the bug is
///      gone but for the wrong reason".
#[tokio::test]
async fn wasm_flush_does_not_advance_indexed_height_key() {
    use metashrew_runtime::smt::BatchedSMTHelper;

    let dir = tempdir().unwrap();
    let adapter = RocksDBRuntimeAdapter::open_optimized(
        dir.path().to_str().unwrap().to_string(),
    )
    .unwrap();
    let db = adapter.db.clone();
    let mut storage = RocksDBStorageAdapter::new(db.clone());

    // Establish a tip at N-1 = 100 via a clean commit_atomic call.
    const N: u32 = 101;
    storage
        .commit_atomic(N - 1, &[0x11; 32], &[0x22; 32])
        .await
        .expect("seed commit at N-1 must succeed");
    assert_eq!(storage.get_indexed_height().await.unwrap(), N - 1);

    // Now invoke the WASM-side flush batch directly for height N. This is
    // what `process_block_atomic` does internally via the indexer's
    // __flush host function. We pass a small synthetic k/v set so the
    // append-only entries exercise the same code paths as production.
    let key_values: Vec<(Vec<u8>, Vec<u8>)> = vec![
        (b"k1".to_vec(), b"v1".to_vec()),
        (b"k2".to_vec(), b"v2".to_vec()),
    ];
    let block_hash = [0x33_u8; 32];
    let mut batched_smt = BatchedSMTHelper::new(adapter.clone());
    batched_smt
        .calculate_and_store_state_root_batched(N, &key_values, &block_hash)
        .expect("WASM-side flush batch for height N must commit");

    // Property 1: the indexed-height pointer is STILL at N-1. If this
    // assertion fails, the WASM batch is still writing `__INTERNAL/height`
    // and the bug is back.
    assert_eq!(
        storage.get_indexed_height().await.unwrap(),
        N - 1,
        "WASM-side __flush must NOT advance __INTERNAL/height — that key \
         is owned exclusively by commit_atomic. If this fails, the rc.4 \
         fix has regressed."
    );

    // Property 1b: but the per-height block-hash record IS written by
    // __flush (we want view/preview lookups to resolve immediately).
    assert_eq!(
        storage.get_block_hash(N).await.unwrap(),
        Some(block_hash.to_vec()),
        "WASM-side __flush should still write the per-height block-hash \
         record so view/preview lookups resolve."
    );

    // Property 2: commit_atomic for height N succeeds (tip + 1 = N).
    let state_root = [0x44_u8; 32];
    storage
        .commit_atomic(N, &block_hash, &state_root)
        .await
        .expect(
            "commit_atomic for height N after WASM-side flush MUST succeed \
             — this is the self-rejection scenario the rc.4 fix targets",
        );
    assert_eq!(storage.get_indexed_height().await.unwrap(), N);

    // Property 3: a SECOND commit_atomic for height N is correctly
    // rejected with the strict-in-order error. This proves the
    // strict-in-order check still works for legitimate out-of-order
    // cases — the rc.4 fix didn't weaken it, it just removed the
    // self-rejection trap.
    let err = storage
        .commit_atomic(N, &block_hash, &state_root)
        .await
        .expect_err(
            "second commit_atomic at the same height must be rejected — \
             legitimate out-of-order, distinct from the self-rejection \
             scenario",
        );
    let msg = format!("{}", err);
    assert!(
        msg.contains("out-of-order commit rejected"),
        "rejection should be the strict-in-order error, got: {}",
        msg
    );
}

// Helper to silence the unused-Arc warning on `db` in the first test if we
// stop using it directly in a future refactor.
#[allow(dead_code)]
fn _ensure_arc_db_in_scope(_: Arc<rocksdb::DB>) {}

//! End-to-end tests for the v10 indexer-side `ConflictRead` /
//! `ConflictWrite` opcodes (Block-STM instrumented I/O).
//!
//! Exercises the host-side handler functions directly (no wasm), with
//! a real `MemStoreAdapter` underneath + `BlockStmCtx` on top. This
//! pins the semantics the wasmtime binding will deliver to alkanes-v3
//! tx-handlers in the next commit.

use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::block_stm::{BlockStmCtx, Version};
use metashrew_runtime::chain_entries::append_value_to_batch;
use metashrew_runtime::indexer_syscall::{handle_conflict_read, handle_conflict_write};
use metashrew_runtime::KeyValueStoreLike;
use std::sync::Arc;

/// Stage a v10 chain-entry write against `db` at `height`.
fn put_disk(db: &mut MemStoreAdapter, key: &[u8], value: &[u8], height: u32) {
    let mut batch = db.create_batch();
    append_value_to_batch(db, &mut batch, key, value, height).expect("append");
    db.write(batch).expect("commit");
}

#[test]
fn conflict_read_falls_through_to_disk_when_no_in_block_write() {
    let mut db = MemStoreAdapter::new();
    put_disk(&mut db, b"k", b"disk-value", 100);

    let ctx = Arc::new(BlockStmCtx::new());
    let value = handle_conflict_read(&db, 200, &ctx, 5, b"k").expect("read");
    assert_eq!(value, b"disk-value");

    // Tracker should have recorded the read as Storage-version.
    let snap = ctx.tracker_for(5).reads_snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].0, b"k");
    assert_eq!(snap[0].1.observed, Version::Storage);
}

#[test]
fn conflict_read_returns_empty_on_disk_miss_and_records_storage() {
    let db = MemStoreAdapter::new();
    let ctx = Arc::new(BlockStmCtx::new());

    let value = handle_conflict_read(&db, 200, &ctx, 5, b"never").expect("read");
    assert!(value.is_empty());

    let snap = ctx.tracker_for(5).reads_snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].1.observed, Version::Storage);
}

#[test]
fn conflict_write_stages_in_mvmemory_at_tx_seq() {
    let db = MemStoreAdapter::new();
    let ctx = Arc::new(BlockStmCtx::new());

    handle_conflict_write(&ctx, 5, b"k".to_vec(), b"v5".to_vec());

    // Direct MvMemory check: tx 10 reading k should see tx 5's write.
    let (version, value) = ctx.mv.read(10, b"k");
    assert_eq!(version, Version::Tx(5));
    assert_eq!(value, Some(b"v5".to_vec()));
}

#[test]
fn conflict_read_prefers_in_block_write_over_disk() {
    let mut db = MemStoreAdapter::new();
    put_disk(&mut db, b"k", b"disk-value", 100);

    let ctx = Arc::new(BlockStmCtx::new());
    // An earlier-seq tx wrote a newer value to k.
    handle_conflict_write(&ctx, 3, b"k".to_vec(), b"tx3-value".to_vec());

    // Tx 10's read should see tx3's write, not the disk value.
    let value = handle_conflict_read(&db, 200, &ctx, 10, b"k").expect("read");
    assert_eq!(value, b"tx3-value");

    let snap = ctx.tracker_for(10).reads_snapshot();
    assert_eq!(snap[0].1.observed, Version::Tx(3));
}

#[test]
fn conflict_read_only_sees_strictly_earlier_writes() {
    let db = MemStoreAdapter::new();
    let ctx = Arc::new(BlockStmCtx::new());

    handle_conflict_write(&ctx, 5, b"k".to_vec(), b"v5".to_vec());

    // Tx 5 reads its own key — should NOT see its own write
    // (strict-less invariant of MvMemory). Falls through to disk
    // (which has nothing), returns empty.
    let value = handle_conflict_read(&db, 200, &ctx, 5, b"k").expect("read");
    assert!(value.is_empty());

    let snap = ctx.tracker_for(5).reads_snapshot();
    assert_eq!(snap[0].1.observed, Version::Storage);
}

#[test]
fn conflict_read_picks_highest_earlier_writer() {
    let db = MemStoreAdapter::new();
    let ctx = Arc::new(BlockStmCtx::new());

    handle_conflict_write(&ctx, 1, b"k".to_vec(), b"v1".to_vec());
    handle_conflict_write(&ctx, 3, b"k".to_vec(), b"v3".to_vec());
    handle_conflict_write(&ctx, 7, b"k".to_vec(), b"v7".to_vec());

    let value = handle_conflict_read(&db, 200, &ctx, 5, b"k").expect("read");
    assert_eq!(value, b"v3", "tx 5 sees tx 3 (highest < 5)");

    let value = handle_conflict_read(&db, 200, &ctx, 10, b"k").expect("read");
    assert_eq!(value, b"v7", "tx 10 sees tx 7 (highest < 10)");
}

#[test]
fn full_block_stm_round_trip_two_independent_txs() {
    // Two txs touch disjoint keys. The merge produces what both txs
    // wrote, in canonical key-sorted order.
    let ctx = Arc::new(BlockStmCtx::new());

    // Tx 0 writes a.
    handle_conflict_write(&ctx, 0, b"a".to_vec(), b"from-tx0".to_vec());
    // Tx 1 writes b.
    handle_conflict_write(&ctx, 1, b"b".to_vec(), b"from-tx1".to_vec());

    // Neither tx reads the other's key — no validation needed.
    let merged = ctx.mv.merge();
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0], (b"a".to_vec(), b"from-tx0".to_vec()));
    assert_eq!(merged[1], (b"b".to_vec(), b"from-tx1".to_vec()));
}

#[test]
fn deterministic_for_same_inputs_and_tx_seqs() {
    let mut db = MemStoreAdapter::new();
    put_disk(&mut db, b"baseline", b"initial".to_vec().as_slice(), 50);

    let ctx_a = Arc::new(BlockStmCtx::new());
    handle_conflict_write(&ctx_a, 2, b"k".to_vec(), b"v2".to_vec());
    let read_a = handle_conflict_read(&db, 100, &ctx_a, 5, b"baseline").unwrap();
    let read_a_k = handle_conflict_read(&db, 100, &ctx_a, 5, b"k").unwrap();
    let merge_a = ctx_a.mv.merge();

    let ctx_b = Arc::new(BlockStmCtx::new());
    handle_conflict_write(&ctx_b, 2, b"k".to_vec(), b"v2".to_vec());
    let read_b = handle_conflict_read(&db, 100, &ctx_b, 5, b"baseline").unwrap();
    let read_b_k = handle_conflict_read(&db, 100, &ctx_b, 5, b"k").unwrap();
    let merge_b = ctx_b.mv.merge();

    assert_eq!(read_a, read_b);
    assert_eq!(read_a_k, read_b_k);
    assert_eq!(merge_a, merge_b);
}

#[test]
fn read_set_records_one_entry_per_unique_key_first_read_wins() {
    let mut db = MemStoreAdapter::new();
    put_disk(&mut db, b"k", b"initial", 100);

    let ctx = Arc::new(BlockStmCtx::new());

    // Tx 5 reads k twice. First read sees Storage. Then an earlier-
    // seq tx writes k. Tx 5's second read would see Tx(3), but the
    // tracker records FIRST observation only.
    let v1 = handle_conflict_read(&db, 200, &ctx, 5, b"k").unwrap();
    assert_eq!(v1, b"initial");

    handle_conflict_write(&ctx, 3, b"k".to_vec(), b"new-from-tx3".to_vec());

    let v2 = handle_conflict_read(&db, 200, &ctx, 5, b"k").unwrap();
    assert_eq!(v2, b"new-from-tx3", "read returns current MvMemory state");

    // But the tracker still records the FIRST read (Storage).
    let snap = ctx.tracker_for(5).reads_snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(
        snap[0].1.observed,
        Version::Storage,
        "first-read-wins for the recorded dependency"
    );

    // Validation would now catch this: current version for tx 5 reading
    // k is Tx(3), but tracker says Storage — mismatch → re-execute.
    assert!(!metashrew_runtime::block_stm::validate_tx(
        &ctx.mv,
        &ctx.tracker_for(5)
    ));
}

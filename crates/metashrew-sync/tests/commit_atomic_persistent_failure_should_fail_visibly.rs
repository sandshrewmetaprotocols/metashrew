//! Repro: `commit_atomic` retry loop has no max-attempts bound.
//!
//! ### Production evidence
//!
//! From the FROST Batallion 6 chat (2026-05-21, mork1e's node, v10
//! metashrew):
//!
//! ```text
//! [23:15:20Z INFO  metashrew_runtime::runtime] processed block 892936 atomically
//! [23:15:20Z ERROR metashrew_sync::sync]      snapshot-path atomic commit STILL failing
//!                                             for height 892936 (attempt 590):
//!                                             Storage error: commit_atomic write failed
//!                                             at height 892936: IO error: While open a
//!                                             file for random read: /data/.metashrew/
//!                                             v10-v3/006117.sst: Too many open files —
//!                                             block 892936 atomic apply has been
//!                                             retrying for 590 attempts
//! ...
//! [23:20:25Z ERROR metashrew_sync::sync]      ... (attempt 600) ... retrying for 600 attempts
//! ```
//!
//! The pod was wedged for hours retrying the same block against an
//! exhausted file-descriptor table (`ulimit -n` default ≈ 1024, RocksDB
//! needs ~1 FD per active SST file, the v10 DB has thousands of SSTs).
//! No progress, no escape, no visible error to the operator — just
//! infinite retry with exponential backoff capped at 30 s.
//!
//! ### Root cause
//!
//! `SnapshotMetashrewSync::process_block` runs an unbounded `loop` with
//! only two exit conditions:
//!   1. Commit succeeds → return Ok.
//!   2. `current_height` already moved past `height` → return Ok.
//!
//! Persistent commit failure has no third exit. Backoff is capped at
//! 30 s; at that rate 600 attempts is 5 hours of CPU-idle spinning
//! while the operator may not even know.
//!
//! ### What this test asserts
//!
//! When `commit_atomic` fails persistently (here simulated by
//! `MockStorage::set_available(false)`), `process_block` must return
//! `Err` within a bounded time (we pick 60 seconds — enough to ride out
//! transient hiccups via exponential backoff, but not "forever"). The
//! Err must surface the underlying commit error so operators can
//! diagnose (FD exhaustion, ENOSPC, EIO, etc).
//!
//! ### Current behavior
//!
//! Without a max-attempts bound this test HANGS in `process_block` and
//! the `tokio::time::timeout` fires after 60 s, asserting failure. With
//! a bounded retry policy the test returns `Err` within ~15-30 s and
//! the assertion holds.

use metashrew_sync::snapshot::SyncMode;
use metashrew_sync::snapshot_sync::SnapshotMetashrewSync;
use metashrew_sync::{MockBitcoinNode, MockRuntime, MockStorage, SnapshotSyncEngine, SyncConfig};
use std::time::Duration;
use tokio::time::timeout;

/// Bound — even on a slow runner, an honest retry policy that respects
/// the 30 s backoff cap should be DONE inside this window. With the
/// bug present the loop spins forever and the timeout fires.
const PROCESS_BLOCK_DEADLINE: Duration = Duration::from_secs(60);

fn test_config() -> SyncConfig {
    SyncConfig {
        start_block: 0,
        exit_at: None,
        pipeline_size: None,
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
        enable_startup_heal: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commit_atomic_persistent_failure_must_return_err_within_deadline() {
    let runtime = MockRuntime::new();
    let storage = MockStorage::new();

    // Seed bitcoind so the SPV `validate_block_connects` lookups have
    // something to compare against — the bug being tested is downstream
    // of that gate.
    let block_hash = vec![0xabu8; 32];
    let block_data = vec![0u8; 80];
    let node = MockBitcoinNode::new();
    node.add_block(1, block_hash.clone(), block_data.clone());

    let mut sync = SnapshotMetashrewSync::new(
        node,
        storage.clone(),
        runtime,
        test_config(),
        SyncMode::Normal,
    );
    sync.init().await;

    // Persistent commit failure: every `commit_atomic` call returns
    // `Err(SyncError::Storage("Storage not available"))`. This is the
    // "Too many open files" class — the indexer can build the batch
    // but the storage layer rejects every write attempt.
    storage.set_available(false);

    // Use `process_block_with_snapshots` to bypass the SPV check
    // (validate_block_connects requires a real serialized bitcoin
    // block, and the failure mode under test is downstream of that
    // gate — see line 546 in snapshot_sync.rs which explicitly says
    // "same infinite-retry-with-exponential-backoff policy as
    // process_block"). Both code paths have the same bug.
    let outcome = timeout(
        PROCESS_BLOCK_DEADLINE,
        sync.process_block_with_snapshots(1, &block_data),
    )
    .await;
    let _ = block_hash;

    match outcome {
        Ok(Err(e)) => {
            // Expected path once the fix lands: a bounded retry policy
            // returns Err after N attempts, surfacing the commit error
            // to the operator. The error message must mention the
            // commit failure mode so monitoring can alert on the right
            // signal.
            let msg = format!("{e:#}");
            assert!(
                msg.contains("commit") || msg.contains("Storage") || msg.contains("available"),
                "expected commit-failure error surfaced to caller, got: {msg}"
            );
        }
        Ok(Ok(())) => panic!(
            "process_block returned Ok despite storage being unavailable — \
             the retry loop must NOT swallow persistent commit failures"
        ),
        Err(_elapsed) => panic!(
            "process_block did not return within {:?} — the retry loop \
             has no max-attempts bound and spun forever. This is the \
             production bug: mork1e's node at block 892936 retried 600+ \
             attempts on `Too many open files` with no operator-visible \
             give-up. The fix: cap retries at N attempts (suggest 20-30, \
             which at the 30 s backoff cap is 10-15 min before failing \
             the block visibly).",
            PROCESS_BLOCK_DEADLINE
        ),
    }
}

//! v9.0.5-rc.9: snapshot-path stale-height retry-loop guard.
//!
//! The retry loops in `SnapshotMetashrewSync::process_block` and
//! `SnapshotMetashrewSync::process_block_with_snapshots` had no
//! staleness check: if either was driven with a `height` that had
//! already been committed by some other code path (the runtime tip
//! had advanced past `height`), the loop would call
//! `commit_atomic(height, ...)`, get rejected with the rc.5
//! "out-of-order commit rejected" error, log it, sleep, and retry
//! — forever.
//!
//! Production evidence on the meta verify pod (block 948955, image
//! `mainnet-v905rc8-v220rc3`, pod `rockshrew-v904-alkanes-0`):
//!
//!   01:20:05 ERROR snapshot-path atomic commit failed for height 948955
//!                  (attempt 6, out-of-order commit rejected … current
//!                   tip is 948960) — block 948955 atomic apply has been
//!                   retrying for 6 attempts
//!   ...
//!   02:00:34 ERROR (attempt 30, ... retrying for 30 attempts)
//!
//! The pod's `current_height` was at 948960; the loop was still trying
//! to commit 948955 ~45 min later, never advancing. CPU idle, no useful
//! work, indexer wedged. mork had reverted to v9.0.4-rc.1 after hitting
//! the same shape on his node.
//!
//! Root cause introduced over the rc.1 → rc.7 sequence:
//!   - rc.5 (`02a769d`): single-batch atomic commit — introduced the
//!     strict in-order rejection in `commit_atomic`.
//!   - rc.3 (`150abef`): never exit on atomic-write failure — turned
//!     the rejection into an infinite retry loop instead of a process
//!     exit / fatal error.
//!   - rc.6 (`d582ada`) / rc.7 (`dd409eb`): added a fetcher-side dedup
//!     guard in `rockshrew-mono/src/lib.rs`, but never touched
//!     `snapshot_sync.rs`. The fetcher dedup catches "fetcher sent the
//!     same height twice to the processor channel" but does NOT cover
//!     "snapshot-sync retry loop was called with a height that has
//!     already been committed by another code path" (e.g. a snapshot
//!     consumer + the normal fetcher both feeding the runtime, or a
//!     stale future firing after the normal path advanced the tip).
//!
//! The invariant we pin here:
//!   When `current_height` (the engine's atomic) is already strictly
//!   greater than the `height` argument, `process_block` /
//!   `process_block_with_snapshots` must return `Ok(())` quickly
//!   ("block already applied, skipping stale retry"), NOT spin in the
//!   retry loop forever.
//!
//! Without the fix this test hangs and the `tokio::time::timeout`
//! fires; with the fix it returns within a few ms.

use metashrew_sync::snapshot::SyncMode;
use metashrew_sync::snapshot_sync::SnapshotMetashrewSync;
use metashrew_sync::{
    MockBitcoinNode, MockRuntime, MockStorage, SnapshotSyncEngine, StorageAdapter, SyncConfig,
};
use std::time::Duration;
use tokio::time::timeout;

/// Drive storage forward to a known tip so subsequent
/// `commit_atomic(stale_height, ...)` calls get rejected by the
/// strict-in-order rule.
async fn warm_storage_to_tip(storage: &mut MockStorage, tip: u32) {
    for h in 1..=tip {
        let bh = vec![h as u8; 32];
        let sr = vec![h as u8; 32];
        storage.commit_atomic(h, &bh, &sr, &[]).await.unwrap();
    }
}

fn test_config() -> SyncConfig {
    SyncConfig {
        start_block: 0,
        exit_at: None,
        pipeline_size: None,
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
        // Skip startup-heal: we're hand-warming storage, no need to validate
        // pointers against bitcoind (which would require a richer mock).
        enable_startup_heal: false,
    }
}

/// Pre-fix: `process_block_with_snapshots(stale_height)` spins forever
/// because every iteration tries to commit `stale_height` against a
/// storage tip that's already past it.
///
/// Post-fix: returns Ok within a few ms with attempts=1 (loop exits
/// without ever touching `process_block_atomic`, because the staleness
/// guard fires before that).
#[tokio::test]
async fn snapshot_loop_does_not_spin_on_already_applied_height() {
    // ---- setup ----
    // Storage already has blocks 1..=100 committed; tip is at 100.
    let mut storage = MockStorage::new();
    warm_storage_to_tip(&mut storage, 100).await;

    // Node knows the stale height we'll request (so get_block_hash succeeds
    // when the inner retry loop tries to look it up). Tip on the node side
    // doesn't matter for this test.
    let node = MockBitcoinNode::new();
    node.add_block(50, vec![50u8; 32], vec![0u8; 80]);

    let runtime = MockRuntime::new();

    let mut sync =
        SnapshotMetashrewSync::new(node, storage, runtime, test_config(), SyncMode::Normal);

    // Seed the engine's current_height to 101 (one past the storage tip).
    // `init()` reads storage's indexed_height (= 100) and stores
    // `start_height = indexed_height + 1 = 101` into the atomic.
    sync.init().await;
    assert_eq!(
        sync.current_height(),
        101,
        "test setup invariant: post-init current_height should reflect storage tip + 1"
    );

    // ---- act ----
    // Drive a stale block (50) through the snapshot-path inner retry loop.
    // Pre-fix this hangs (commit_atomic returns out-of-order on every
    // attempt). The 2-second timeout is generous: with the fix the call
    // returns essentially instantly; without the fix even after the first
    // few backoff rounds (~100 + 200 + 400 + 800ms ≈ 1.5s) we'd cross 2s
    // and timeout fires.
    let block_data = vec![0u8; 80];
    let res = timeout(
        Duration::from_secs(2),
        sync.process_block_with_snapshots(50, &block_data),
    )
    .await;

    // ---- assert ----
    assert!(
        res.is_ok(),
        "snapshot-path retry loop did not return for an already-applied height \
         (current_height=101, requested height=50) within 2s — the staleness \
         guard is missing"
    );
    let inner = res.unwrap();
    assert!(
        inner.is_ok(),
        "snapshot-path returned an error for an already-applied height \
         (current_height=101, requested height=50): {:?} — the guard should \
         treat this as Ok(()), not propagate the commit-rejection error",
        inner
    );
}

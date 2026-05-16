//! Tests that pin down the strict block-processing determinism invariants
//! introduced in `feat/strict-block-determinism`.
//!
//! These tests are *not* about exercising the WASM runtime end-to-end —
//! they're about pinning the storage-side commit semantics:
//!
//! 1. `commit_atomic` enforces strict in-order progression
//!    (`height == tip + 1`), refusing to commit out of sequence even when
//!    the caller supplies a valid `(block_hash, state_root)` tuple.
//!
//! 2. The "retry the atomic commit until it succeeds" pattern that the
//!    sync engines now use is idempotent: a `commit_atomic` that fails
//!    transiently leaves storage in the previous-tip state, and a
//!    subsequent successful retry produces the same on-disk state as a
//!    single successful call. This is the invariant that previously
//!    failed in production — the non-atomic fallback path on the
//!    mainnet pods was producing partial writes (state root + height
//!    advanced, dieselTotalSupply k/v lost) because the runtime's
//!    direct-write `process_block` path and the storage adapter's
//!    three-call `(set_indexed_height, store_block_hash, store_state_root)`
//!    sequence were not atomic with respect to each other.
//!
//! The fault-injection driver in this file faithfully simulates the retry
//! loop now used in `MetashrewSync::process_block`,
//! `ProcessingClone::process_block`, and
//! `SnapshotMetashrewSync::process_block`. If those production loops
//! ever drift away from the retry-and-exit pattern, this test will keep
//! exercising the storage-side invariants independently.

use async_trait::async_trait;
use metashrew_sync::{MockStorage, StorageAdapter, StorageStats, SyncError, SyncResult};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

/// A storage adapter that wraps `MockStorage` and forces its `commit_atomic`
/// to fail the first `faults_per_block` times for *each* (height, attempt)
/// pair before succeeding. All other methods pass straight through.
///
/// We track faults per-height so a multi-block test gets the same fault
/// budget on every block (otherwise a global counter would burn down on
/// the first block and let the rest commit clean).
#[derive(Clone)]
struct FaultyStorage {
    inner: MockStorage,
    faults_per_block: u32,
    /// Map from height to remaining faults at that height.
    remaining: Arc<TokioMutex<std::collections::HashMap<u32, u32>>>,
    total_commit_attempts: Arc<TokioMutex<u32>>,
}

impl FaultyStorage {
    fn new(faults_per_block: u32) -> Self {
        Self {
            inner: MockStorage::new(),
            faults_per_block,
            remaining: Arc::new(TokioMutex::new(std::collections::HashMap::new())),
            total_commit_attempts: Arc::new(TokioMutex::new(0)),
        }
    }

    async fn attempts(&self) -> u32 {
        *self.total_commit_attempts.lock().await
    }
}

#[async_trait]
impl StorageAdapter for FaultyStorage {
    async fn get_indexed_height(&self) -> SyncResult<u32> {
        self.inner.get_indexed_height().await
    }
    async fn set_indexed_height(&mut self, height: u32) -> SyncResult<()> {
        self.inner.set_indexed_height(height).await
    }
    async fn store_block_hash(&mut self, height: u32, hash: &[u8]) -> SyncResult<()> {
        self.inner.store_block_hash(height, hash).await
    }
    async fn get_block_hash(&self, height: u32) -> SyncResult<Option<Vec<u8>>> {
        self.inner.get_block_hash(height).await
    }
    async fn store_state_root(&mut self, height: u32, root: &[u8]) -> SyncResult<()> {
        self.inner.store_state_root(height, root).await
    }
    async fn get_state_root(&self, height: u32) -> SyncResult<Option<Vec<u8>>> {
        self.inner.get_state_root(height).await
    }
    async fn rollback_to_height(&mut self, height: u32) -> SyncResult<()> {
        self.inner.rollback_to_height(height).await
    }
    async fn is_available(&self) -> bool {
        self.inner.is_available().await
    }
    async fn get_stats(&self) -> SyncResult<StorageStats> {
        self.inner.get_stats().await
    }

    async fn commit_atomic(
        &mut self,
        height: u32,
        block_hash: &[u8],
        state_root: &[u8],
    ) -> SyncResult<()> {
        {
            let mut attempts = self.total_commit_attempts.lock().await;
            *attempts += 1;
        }
        {
            let mut remaining = self.remaining.lock().await;
            let slot = remaining.entry(height).or_insert(self.faults_per_block);
            if *slot > 0 {
                *slot -= 1;
                return Err(SyncError::Storage(format!(
                    "injected transient fault at height {}",
                    height
                )));
            }
        }
        // Delegate to the in-memory atomic commit — same strict-in-order
        // semantics the production RocksDB adapter has.
        self.inner.commit_atomic(height, block_hash, state_root).await
    }
}

/// Simulates the retry loop used by `MetashrewSync::process_block` and
/// friends. Returns Ok with the number of attempts it took, or Err if all
/// retries are exhausted.
async fn retry_commit<S: StorageAdapter + ?Sized>(
    storage: &mut S,
    height: u32,
    block_hash: &[u8],
    state_root: &[u8],
    max_retries: u32,
) -> Result<u32, SyncError> {
    let mut last_err: Option<SyncError> = None;
    for attempt in 1..=max_retries {
        match storage.commit_atomic(height, block_hash, state_root).await {
            Ok(()) => return Ok(attempt),
            Err(e) => {
                last_err = Some(e);
                // Mirror the production backoff so the test surfaces the
                // same timing behavior. Short enough to keep the test fast.
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| SyncError::Storage("unknown".into())))
}

/// `commit_atomic` MUST reject any height that is not `tip + 1`, except on
/// a fresh DB (no block at height 0 yet), where any first height is
/// allowed. This pins the "no skips, no gaps, no out-of-order writes"
/// invariant at the storage layer.
#[tokio::test]
async fn commit_atomic_rejects_out_of_order() {
    let mut storage = MockStorage::new();

    // Fresh DB — first commit at height 100 is allowed (matches the
    // configured-start-block scenario in production).
    storage
        .commit_atomic(100, &[0xaa; 32], &[0xbb; 32])
        .await
        .expect("first commit on fresh DB must succeed");
    assert_eq!(storage.get_indexed_height().await.unwrap(), 100);

    // Next commit MUST be exactly 101.
    assert!(
        storage
            .commit_atomic(102, &[0xcc; 32], &[0xdd; 32])
            .await
            .is_err(),
        "commit_atomic must reject height 102 when tip is 100"
    );
    assert!(
        storage
            .commit_atomic(100, &[0xcc; 32], &[0xdd; 32])
            .await
            .is_err(),
        "commit_atomic must reject re-writing the current tip height"
    );
    assert!(
        storage
            .commit_atomic(50, &[0xcc; 32], &[0xdd; 32])
            .await
            .is_err(),
        "commit_atomic must reject committing below the current tip"
    );

    // After rejected calls, storage must be unchanged.
    assert_eq!(storage.get_indexed_height().await.unwrap(), 100);
    assert_eq!(
        storage.get_block_hash(100).await.unwrap(),
        Some(vec![0xaa; 32])
    );
    assert!(
        storage.get_block_hash(102).await.unwrap().is_none(),
        "rejected commit must not leak a partial block-hash write"
    );

    // The correct next height succeeds.
    storage
        .commit_atomic(101, &[0xee; 32], &[0xff; 32])
        .await
        .expect("in-order commit at tip+1 must succeed");
    assert_eq!(storage.get_indexed_height().await.unwrap(), 101);
}

/// The retry-on-failure loop is bit-for-bit idempotent against a no-failure
/// baseline.
///
/// Setup:
///   - Build a sequence of 10 blocks starting at height 100. Each block has
///     a deterministic block_hash and state_root derived from its height.
///   - Run the sequence through `BaselineStorage` (a vanilla `MockStorage`
///     with no faults) using `retry_commit`. Capture the final state.
///   - Run the same sequence through `FaultyStorage(fail_count=2)` using
///     the same `retry_commit` driver. Capture the final state.
///   - Assert: the two states (indexed height, block-hash record at every
///     height, state-root record at every height) are bit-for-bit identical.
///   - Assert: the faulty driver issued exactly 2 extra commit attempts
///     (the two injected faults each consumed one retry).
#[tokio::test]
async fn retry_on_commit_failure_is_bit_for_bit_idempotent() {
    const START_HEIGHT: u32 = 100;
    const BLOCK_COUNT: u32 = 10;
    const RETRIES: u32 = 5;

    fn deterministic_block_hash(height: u32) -> Vec<u8> {
        let mut h = vec![0u8; 32];
        let bytes = height.to_le_bytes();
        for i in 0..32 {
            h[i] = bytes[i % 4] ^ (i as u8);
        }
        h
    }
    fn deterministic_state_root(height: u32) -> Vec<u8> {
        let mut r = vec![0u8; 32];
        let bytes = height.to_le_bytes();
        for i in 0..32 {
            r[i] = bytes[i % 4].wrapping_add(i as u8);
        }
        r
    }

    // 1. Baseline run — no faults.
    let mut baseline = MockStorage::new();
    for h in START_HEIGHT..START_HEIGHT + BLOCK_COUNT {
        let attempts = retry_commit(
            &mut baseline,
            h,
            &deterministic_block_hash(h),
            &deterministic_state_root(h),
            RETRIES,
        )
        .await
        .expect("baseline commit must succeed");
        assert_eq!(attempts, 1, "baseline must succeed on first attempt");
    }

    // 2. Faulty run — every block fails its commit twice before succeeding.
    let mut faulty = FaultyStorage::new(2); // 2 faults per block
    for h in START_HEIGHT..START_HEIGHT + BLOCK_COUNT {
        let attempts = retry_commit(
            &mut faulty,
            h,
            &deterministic_block_hash(h),
            &deterministic_state_root(h),
            RETRIES,
        )
        .await
        .expect("faulty commit must eventually succeed within retry budget");
        assert_eq!(
            attempts, 3,
            "faulty commit must take exactly 3 attempts (2 injected faults + 1 success) at height {}",
            h
        );
    }

    // 3. Bit-for-bit comparison of the two final states.
    assert_eq!(
        baseline.get_indexed_height().await.unwrap(),
        faulty.get_indexed_height().await.unwrap(),
        "indexed-height must match between baseline and faulty runs"
    );

    for h in START_HEIGHT..START_HEIGHT + BLOCK_COUNT {
        let b_hash = baseline.get_block_hash(h).await.unwrap();
        let f_hash = faulty.get_block_hash(h).await.unwrap();
        assert_eq!(b_hash, f_hash, "block hash mismatch at height {}", h);

        let b_root = baseline.get_state_root(h).await.unwrap();
        let f_root = faulty.get_state_root(h).await.unwrap();
        assert_eq!(b_root, f_root, "state root mismatch at height {}", h);
    }

    // 4. Sanity-check the fault-injection accounting: 3 attempts per block
    // (2 faults + 1 success) × BLOCK_COUNT blocks.
    assert_eq!(faulty.attempts().await, 3 * BLOCK_COUNT);
}

/// A transient failure that exhausts the retry budget MUST leave storage at
/// the *previous* tip — i.e. the failed block was not partially committed.
///
/// This is the invariant that was lost in production: the fallback path
/// would advance `tip_height` while losing some of the runtime's k/v
/// writes, producing the observed `dieselTotalSupply` drift. With the new
/// atomic-only commit semantics, an exhausted retry budget must leave the
/// DB in a clean "block N not committed" state, so restart-recovery can
/// re-run block N from scratch and either succeed or fail identically.
#[tokio::test]
async fn exhausted_retry_leaves_storage_at_previous_tip() {
    let mut storage = FaultyStorage::new(1000); // Always fails.
    let h = 100;
    let res = retry_commit(
        &mut storage,
        h,
        &[0xaa; 32],
        &[0xbb; 32],
        3, // small budget so we exhaust quickly
    )
    .await;
    assert!(res.is_err(), "exhausted retry budget must return an error");

    // Storage is at the fresh-DB state: no block-hash record, no state-root
    // record, height still 0. The user's invariant: "either the block is
    // committed or it isn't."
    assert_eq!(storage.get_indexed_height().await.unwrap(), 0);
    assert!(storage.get_block_hash(h).await.unwrap().is_none());
    assert!(storage.get_state_root(h).await.unwrap().is_none());

    // We attempted exactly `budget` times.
    assert_eq!(storage.attempts().await, 3);
}

/// `commit_atomic` is observably all-or-nothing under contention: a caller
/// that issues an out-of-order commit cannot leave behind a partial
/// block-hash or state-root record that a subsequent in-order caller would
/// then trip over.
#[tokio::test]
async fn rejected_commit_leaves_no_partial_writes() {
    let mut storage = MockStorage::new();
    // Establish tip at 100.
    storage
        .commit_atomic(100, &[1u8; 32], &[2u8; 32])
        .await
        .unwrap();

    // Try a series of bogus out-of-order commits.
    for bad_h in [99, 100, 102, 105, 200] {
        let _ = storage
            .commit_atomic(bad_h, &[0xff; 32], &[0xff; 32])
            .await;
    }

    // The legit in-order next commit succeeds, and crucially the storage
    // never contains any of the bogus values.
    storage
        .commit_atomic(101, &[3u8; 32], &[4u8; 32])
        .await
        .unwrap();

    assert_eq!(storage.get_indexed_height().await.unwrap(), 101);
    assert_eq!(storage.get_block_hash(100).await.unwrap(), Some(vec![1u8; 32]));
    assert_eq!(storage.get_block_hash(101).await.unwrap(), Some(vec![3u8; 32]));
    for bad_h in [99u32, 102, 105, 200] {
        assert!(
            storage.get_block_hash(bad_h).await.unwrap().is_none(),
            "out-of-order commit at {} must not have left a block-hash record",
            bad_h
        );
    }
}

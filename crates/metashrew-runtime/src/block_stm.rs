//! v10 Block-STM data structures for per-tx parallel indexer execution.
//!
//! ## Why
//!
//! The alkanes-v3 indexer is wasm-bound on one CPU core while 14
//! cores sit idle on the meta-pod box. Most txs in a block touch
//! disjoint state (different outpoints / addresses), so they could
//! execute in parallel. Block-STM (Aptos's optimistic-concurrency
//! algorithm) gives us that: schedule txs in canonical order,
//! execute speculatively in parallel, validate read-sets against
//! later-discovered earlier-seq writes, re-execute any losers.
//!
//! ## What this module provides
//!
//! Pure data structures + a validation predicate. The wasmtime
//! plumbing (ThreadSpawn / ConflictRead / scheduler) lives in
//! follow-up commits — this one is the foundation.
//!
//! - [`Version`]: opaque tag for "where did this value come from"
//!   (disk baseline or in-block tx).
//! - [`MvMemory`]: multi-version memory mapping `key -> BTreeMap<tx_seq, value>`.
//!   Reads return the strictly-earlier tx's write or fall back to
//!   storage. Writes index by writer tx_seq. Re-execution removes
//!   the prior incarnation's writes before the new one starts.
//! - [`ConflictTracker`]: per-tx read-set, populated by the
//!   instrumented `ConflictRead` host op.
//! - [`validate_tx`]: returns false if any of this tx's recorded
//!   reads is now stale (i.e. an earlier-seq tx wrote that key after
//!   we read it).
//!
//! ## Determinism contract
//!
//! Block-STM only parallelizes; the OUTPUT must be byte-identical to
//! sequential execution. The contract:
//!
//! 1. Every read by tx N sees either the disk baseline OR a write by
//!    some earlier tx M (M < N). Never tx N's own writes, never a
//!    later tx's writes.
//! 2. Validation re-checks each read against the current MvMemory
//!    state. If an earlier tx M committed a write after tx N's read
//!    resolved, tx N is invalidated and must re-execute.
//! 3. Re-execution clears the tx's prior write-set before running
//!    (`MvMemory::clear_writes_of`), so the next incarnation can't
//!    accidentally read stale own-writes via the merge path.
//! 4. The final merge takes the highest-tx_seq write per key, which is
//!    the canonical "last writer wins" rule sequential execution
//!    would produce.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

/// Where a value visible to a reader came from.
///
/// `Storage` = the disk-backed state at the start of the block,
/// before any tx ran. `Tx(seq)` = an in-block write by the tx with
/// this sequence number.
///
/// Tagging reads with a `Version` is what makes validation work: on
/// re-validation we recompute the version each read WOULD see now,
/// and compare against the version it actually saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Version {
    Storage,
    Tx(u32),
}

/// What a tx observed about a single key during one execution
/// incarnation. The host's `ConflictRead` populates this after each
/// read resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadDep {
    /// The version the read returned. If the current MvMemory state
    /// would now return a different version for the same `(tx_seq,
    /// key)` query, this dep is stale and the tx must re-execute.
    pub observed: Version,
}

/// Multi-version memory: per-key, all in-block writes indexed by
/// writer tx_seq. Disk-backed state is the implicit baseline (NOT
/// stored here); only in-block writes appear.
///
/// Read semantics: `mv.read(tx_seq=N, key)` returns the write by the
/// highest `M < N` if any exists, else `(Storage, None)`. Tx N
/// CANNOT see its own writes (the `..tx_seq` range is strict-less).
/// This pins the "no self-read of staged writes" invariant
/// structurally.
///
/// Concurrency: all methods take `&self`. Internal locking is a
/// per-MvMemory `RwLock<HashMap<...>>`. The lock is held only for
/// the duration of one map op; long-running worker threads release
/// between reads.
#[derive(Debug, Default)]
pub struct MvMemory {
    inner: RwLock<HashMap<Vec<u8>, BTreeMap<u32, Vec<u8>>>>,
}

impl MvMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve a read by tx `tx_seq` for `key`. Returns
    /// `(version, Some(value))` if a strictly-earlier in-block tx
    /// wrote the key; `(Storage, None)` otherwise. Caller fetches
    /// from disk on `Storage`.
    ///
    /// `value` is cloned out of the map so the caller doesn't hold
    /// the RwLock across host-boundary work.
    pub fn read(&self, tx_seq: u32, key: &[u8]) -> (Version, Option<Vec<u8>>) {
        let guard = self.inner.read().unwrap();
        if let Some(versions) = guard.get(key) {
            if let Some((&writer_seq, value)) = versions.range(..tx_seq).next_back() {
                return (Version::Tx(writer_seq), Some(value.clone()));
            }
        }
        (Version::Storage, None)
    }

    /// Compute the version `tx_seq` would see for `key` right now,
    /// without returning the value. Used by [`validate_tx`].
    pub fn current_version_for(&self, tx_seq: u32, key: &[u8]) -> Version {
        let guard = self.inner.read().unwrap();
        match guard.get(key).and_then(|v| v.range(..tx_seq).next_back()) {
            Some((&writer_seq, _)) => Version::Tx(writer_seq),
            None => Version::Storage,
        }
    }

    /// Stage a write by `tx_seq` to `key`. If `tx_seq` already wrote
    /// to this key in this incarnation, the value is replaced (last
    /// write in the incarnation wins).
    pub fn write(&self, tx_seq: u32, key: Vec<u8>, value: Vec<u8>) {
        let mut guard = self.inner.write().unwrap();
        guard.entry(key).or_default().insert(tx_seq, value);
    }

    /// Remove every write by `tx_seq`. Called before a re-execution
    /// of `tx_seq` begins; the new incarnation then writes fresh
    /// entries. Keeps the map bounded by removing emptied entries.
    pub fn clear_writes_of(&self, tx_seq: u32) {
        let mut guard = self.inner.write().unwrap();
        for entries in guard.values_mut() {
            entries.remove(&tx_seq);
        }
        guard.retain(|_, v| !v.is_empty());
    }

    /// Final merge: for each key with at least one in-block write,
    /// return the highest-tx_seq value. Caller folds these into the
    /// block's WriteBatch. Output is sorted by key for determinism.
    pub fn merge(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let guard = self.inner.read().unwrap();
        let mut out: Vec<(Vec<u8>, Vec<u8>)> = guard
            .iter()
            .filter_map(|(key, versions)| {
                versions
                    .iter()
                    .next_back()
                    .map(|(_, value)| (key.clone(), value.clone()))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Total number of distinct keys with at least one in-block
    /// write. Useful for capacity-hinting the WriteBatch.
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Per-tx instrumented read tracker. Lives on the tx's worker thread
/// for the duration of one incarnation. Wasm-side `ConflictRead`
/// dispatches here to record the version each read observed.
///
/// `clear()` wipes the read-set when re-execution begins.
#[derive(Debug)]
pub struct ConflictTracker {
    pub tx_seq: u32,
    reads: RwLock<HashMap<Vec<u8>, ReadDep>>,
}

impl ConflictTracker {
    pub fn new(tx_seq: u32) -> Self {
        Self {
            tx_seq,
            reads: RwLock::new(HashMap::new()),
        }
    }

    /// Record that this tx observed `key` at `version`. First-read
    /// wins: a subsequent read of the same key in the same
    /// incarnation does NOT overwrite the recorded dependency,
    /// because it'd be reading the same value (deterministic by
    /// MvMemory contract) so the recorded version is unchanged.
    pub fn record_read(&self, key: Vec<u8>, version: Version) {
        let mut guard = self.reads.write().unwrap();
        guard.entry(key).or_insert(ReadDep { observed: version });
    }

    /// Snapshot of the read-set. Returns owned `Vec` so the lock
    /// can be released. Used by the validator + tests.
    pub fn reads_snapshot(&self) -> Vec<(Vec<u8>, ReadDep)> {
        let guard = self.reads.read().unwrap();
        guard.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }

    /// Wipe the read-set. Called before re-execution: the new
    /// incarnation builds a fresh set.
    pub fn clear(&self) {
        let mut guard = self.reads.write().unwrap();
        guard.clear();
    }

    pub fn len(&self) -> usize {
        self.reads.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Per-block context shared between the main indexer Store and every
/// spawned tx-handler Store. Holds the MvMemory plus a registry of
/// per-tx ConflictTrackers.
///
/// Lifecycle: one `BlockStmCtx` per block being indexed in parallel
/// mode. Set on the main indexer Store via `State::with_block_stm`,
/// cloned (via the Arc on the outer wrapper) into each spawned tx-
/// handler Store. Dropped at end of block.
///
/// Wrapped in `Arc` by the State holders so opcodes can grab a clone
/// without holding the wasmtime caller across host-boundary work.
#[derive(Debug, Default)]
pub struct BlockStmCtx {
    pub mv: MvMemory,
    /// Per-tx trackers, indexed by tx_seq. Trackers are created lazily
    /// on first read by `tracker_for`. The validator iterates this
    /// map after each pass to decide which txs must re-execute.
    trackers: RwLock<HashMap<u32, Arc<ConflictTracker>>>,
}

impl BlockStmCtx {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the tracker for `tx_seq`, creating it on first access.
    /// Cheap when warm (single RwLock read); slightly more on first
    /// touch (write lock to insert).
    pub fn tracker_for(&self, tx_seq: u32) -> Arc<ConflictTracker> {
        if let Some(t) = self.trackers.read().unwrap().get(&tx_seq) {
            return t.clone();
        }
        let mut guard = self.trackers.write().unwrap();
        guard
            .entry(tx_seq)
            .or_insert_with(|| Arc::new(ConflictTracker::new(tx_seq)))
            .clone()
    }

    /// Snapshot of all tx_seq -> tracker pairs. Used by the scheduler
    /// (next commit) to iterate validation in order.
    pub fn all_trackers(&self) -> Vec<(u32, Arc<ConflictTracker>)> {
        let guard = self.trackers.read().unwrap();
        let mut out: Vec<_> = guard.iter().map(|(k, v)| (*k, v.clone())).collect();
        out.sort_by_key(|x| x.0);
        out
    }

    /// Number of tx_seqs that have at least one recorded read.
    /// Used as a lower-bound count of parallel txs in a block.
    pub fn tx_count(&self) -> usize {
        self.trackers.read().unwrap().len()
    }
}

/// Validate that every read recorded in `tracker` still resolves to
/// the same version `mv` would return now. Returns false on the
/// first mismatch — the caller re-executes the tx.
///
/// Determinism contract: if `validate_tx` returns true for tx N
/// after tx N has finished executing, AND every tx M<N has likewise
/// validated, then tx N's outputs are byte-identical to what
/// sequential execution would produce. (The scheduler must ensure
/// validation order; that's the next commit.)
pub fn validate_tx(mv: &MvMemory, tracker: &ConflictTracker) -> bool {
    for (key, dep) in tracker.reads_snapshot() {
        if mv.current_version_for(tracker.tx_seq, &key) != dep.observed {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- MvMemory ----

    #[test]
    fn read_with_no_writes_returns_storage_fallback() {
        let mv = MvMemory::new();
        assert_eq!(mv.read(5, b"absent"), (Version::Storage, None));
    }

    #[test]
    fn read_after_in_block_write_returns_tx_version() {
        let mv = MvMemory::new();
        mv.write(3, b"k".to_vec(), b"v3".to_vec());
        // Tx 5 reading k: should see tx 3's write.
        assert_eq!(
            mv.read(5, b"k"),
            (Version::Tx(3), Some(b"v3".to_vec()))
        );
    }

    #[test]
    fn read_only_sees_strictly_earlier_writers() {
        let mv = MvMemory::new();
        mv.write(3, b"k".to_vec(), b"v3".to_vec());
        // Tx 3 reading k: should NOT see its own write (strict <).
        assert_eq!(mv.read(3, b"k"), (Version::Storage, None));
        // Tx 2 reading k: also should not (writer 3 > reader 2).
        assert_eq!(mv.read(2, b"k"), (Version::Storage, None));
    }

    #[test]
    fn read_picks_highest_earlier_writer() {
        let mv = MvMemory::new();
        mv.write(1, b"k".to_vec(), b"v1".to_vec());
        mv.write(5, b"k".to_vec(), b"v5".to_vec());
        mv.write(3, b"k".to_vec(), b"v3".to_vec());

        // Tx 10 reads k: should see tx 5 (highest < 10).
        assert_eq!(mv.read(10, b"k"), (Version::Tx(5), Some(b"v5".to_vec())));
        // Tx 4 reads k: tx 3 is highest < 4.
        assert_eq!(mv.read(4, b"k"), (Version::Tx(3), Some(b"v3".to_vec())));
        // Tx 2 reads k: only tx 1 < 2.
        assert_eq!(mv.read(2, b"k"), (Version::Tx(1), Some(b"v1".to_vec())));
    }

    #[test]
    fn clear_writes_of_removes_only_target_tx() {
        let mv = MvMemory::new();
        mv.write(1, b"k".to_vec(), b"v1".to_vec());
        mv.write(2, b"k".to_vec(), b"v2".to_vec());
        mv.write(3, b"k".to_vec(), b"v3".to_vec());

        mv.clear_writes_of(2);

        // Tx 4 reads k: tx 3 is highest remaining.
        assert_eq!(mv.read(4, b"k"), (Version::Tx(3), Some(b"v3".to_vec())));
        // Tx 3 reads k: tx 1 is highest < 3 (tx 2 was cleared).
        assert_eq!(mv.read(3, b"k"), (Version::Tx(1), Some(b"v1".to_vec())));
    }

    #[test]
    fn clear_writes_of_drops_emptied_entries() {
        let mv = MvMemory::new();
        mv.write(1, b"k".to_vec(), b"v1".to_vec());
        assert_eq!(mv.len(), 1);
        mv.clear_writes_of(1);
        assert_eq!(mv.len(), 0, "emptied key entries must be retained-out");
    }

    #[test]
    fn re_execution_replaces_value_at_same_tx_seq() {
        let mv = MvMemory::new();
        mv.write(7, b"k".to_vec(), b"first-incarnation".to_vec());
        // Re-exec without clear (should not be done in practice, but
        // confirms semantics): same tx_seq overwrites.
        mv.write(7, b"k".to_vec(), b"second-incarnation".to_vec());
        assert_eq!(
            mv.read(10, b"k"),
            (Version::Tx(7), Some(b"second-incarnation".to_vec()))
        );
    }

    #[test]
    fn merge_returns_highest_seq_per_key_sorted() {
        let mv = MvMemory::new();
        // Out-of-order writes to test that merge picks the right one.
        mv.write(5, b"alpha".to_vec(), b"alpha-from-5".to_vec());
        mv.write(2, b"alpha".to_vec(), b"alpha-from-2".to_vec());
        mv.write(3, b"beta".to_vec(), b"beta-from-3".to_vec());
        mv.write(1, b"gamma".to_vec(), b"gamma-from-1".to_vec());

        let merged = mv.merge();
        assert_eq!(merged.len(), 3);
        // Sorted by key.
        assert_eq!(merged[0].0, b"alpha");
        assert_eq!(merged[0].1, b"alpha-from-5", "highest writer wins");
        assert_eq!(merged[1].0, b"beta");
        assert_eq!(merged[1].1, b"beta-from-3");
        assert_eq!(merged[2].0, b"gamma");
        assert_eq!(merged[2].1, b"gamma-from-1");
    }

    #[test]
    fn current_version_for_matches_read_result_version() {
        let mv = MvMemory::new();
        mv.write(2, b"k".to_vec(), b"v".to_vec());
        let (v, _) = mv.read(10, b"k");
        assert_eq!(mv.current_version_for(10, b"k"), v);
    }

    // ---- ConflictTracker ----

    #[test]
    fn tracker_starts_empty() {
        let t = ConflictTracker::new(5);
        assert_eq!(t.tx_seq, 5);
        assert!(t.is_empty());
    }

    #[test]
    fn record_read_first_wins_for_same_key() {
        let t = ConflictTracker::new(5);
        t.record_read(b"k".to_vec(), Version::Tx(3));
        // Second read of the same key (shouldn't happen under
        // determinism contract, but if it does we keep the first).
        t.record_read(b"k".to_vec(), Version::Tx(4));
        let reads = t.reads_snapshot();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].1.observed, Version::Tx(3));
    }

    #[test]
    fn clear_wipes_tracker() {
        let t = ConflictTracker::new(5);
        t.record_read(b"a".to_vec(), Version::Storage);
        t.record_read(b"b".to_vec(), Version::Tx(2));
        assert_eq!(t.len(), 2);
        t.clear();
        assert!(t.is_empty());
    }

    // ---- validate_tx ----

    #[test]
    fn validate_passes_when_nothing_changed() {
        let mv = MvMemory::new();
        let t = ConflictTracker::new(10);
        // Tx 10 read 'k' and saw Storage. MvMemory has no writes to
        // 'k'. Validation should still pass.
        t.record_read(b"k".to_vec(), Version::Storage);
        assert!(validate_tx(&mv, &t));
    }

    #[test]
    fn validate_fails_when_earlier_tx_wrote_after_our_read() {
        let mv = MvMemory::new();
        let t = ConflictTracker::new(10);
        t.record_read(b"k".to_vec(), Version::Storage);

        // Now an earlier-seq tx writes the same key. Tx 10's read
        // is invalidated — it would now resolve to Tx(5) instead of
        // Storage.
        mv.write(5, b"k".to_vec(), b"v5".to_vec());

        assert!(!validate_tx(&mv, &t));
    }

    #[test]
    fn validate_ignores_later_seq_writes() {
        let mv = MvMemory::new();
        let t = ConflictTracker::new(5);
        t.record_read(b"k".to_vec(), Version::Storage);
        // A later-seq tx writing the key doesn't invalidate tx 5;
        // tx 5 wouldn't see it anyway under strict-less-than reads.
        mv.write(10, b"k".to_vec(), b"v10".to_vec());
        assert!(validate_tx(&mv, &t));
    }

    #[test]
    fn validate_fails_when_observed_version_no_longer_highest() {
        let mv = MvMemory::new();
        mv.write(2, b"k".to_vec(), b"v2".to_vec());

        let t = ConflictTracker::new(10);
        t.record_read(b"k".to_vec(), Version::Tx(2));
        // Now tx 7 writes k. Tx 10's read should now see Tx(7), not
        // Tx(2), so the dep is stale.
        mv.write(7, b"k".to_vec(), b"v7".to_vec());
        assert!(!validate_tx(&mv, &t));
    }

    #[test]
    fn validate_passes_after_clear_writes_restores_observed_version() {
        let mv = MvMemory::new();
        mv.write(2, b"k".to_vec(), b"v2".to_vec());
        mv.write(7, b"k".to_vec(), b"v7".to_vec());

        let t = ConflictTracker::new(10);
        // Tx 10 read after tx 7's write — observed Tx(7).
        t.record_read(b"k".to_vec(), Version::Tx(7));

        // Tx 7 gets re-executed and clears its writes.
        mv.clear_writes_of(7);

        // Now tx 10's read would see Tx(2), not Tx(7) — invalidated.
        assert!(!validate_tx(&mv, &t));
    }

    #[test]
    fn validate_passes_on_multi_key_read_set_all_unchanged() {
        let mv = MvMemory::new();
        mv.write(1, b"a".to_vec(), b"a".to_vec());
        mv.write(2, b"b".to_vec(), b"b".to_vec());

        let t = ConflictTracker::new(10);
        t.record_read(b"a".to_vec(), Version::Tx(1));
        t.record_read(b"b".to_vec(), Version::Tx(2));
        t.record_read(b"c".to_vec(), Version::Storage);

        assert!(validate_tx(&mv, &t));
    }

    // ---- BlockStmCtx ----

    #[test]
    fn block_stm_ctx_tracker_is_created_on_first_access() {
        let ctx = BlockStmCtx::new();
        assert_eq!(ctx.tx_count(), 0);
        let t = ctx.tracker_for(7);
        assert_eq!(t.tx_seq, 7);
        assert_eq!(ctx.tx_count(), 1);
    }

    #[test]
    fn block_stm_ctx_tracker_is_shared_across_accesses() {
        let ctx = BlockStmCtx::new();
        let a = ctx.tracker_for(3);
        let b = ctx.tracker_for(3);
        // Same Arc → record on `a` is visible on `b`.
        a.record_read(b"k".to_vec(), Version::Storage);
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn all_trackers_returns_sorted_by_tx_seq() {
        let ctx = BlockStmCtx::new();
        let _ = ctx.tracker_for(5);
        let _ = ctx.tracker_for(2);
        let _ = ctx.tracker_for(8);
        let snap = ctx.all_trackers();
        let seqs: Vec<u32> = snap.iter().map(|x| x.0).collect();
        assert_eq!(seqs, vec![2, 5, 8]);
    }

    #[test]
    fn validate_fails_on_any_single_invalidated_read() {
        let mv = MvMemory::new();
        mv.write(1, b"a".to_vec(), b"a".to_vec());
        mv.write(2, b"b".to_vec(), b"b".to_vec());

        let t = ConflictTracker::new(10);
        t.record_read(b"a".to_vec(), Version::Tx(1));
        t.record_read(b"b".to_vec(), Version::Tx(2));
        t.record_read(b"c".to_vec(), Version::Storage);

        // Earlier-seq write to 'c' between our read and now.
        mv.write(5, b"c".to_vec(), b"c5".to_vec());
        assert!(!validate_tx(&mv, &t));
    }
}

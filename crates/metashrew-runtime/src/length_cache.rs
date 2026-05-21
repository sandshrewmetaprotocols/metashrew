//! Cross-block length cache for v10 versioned-chain entries.
//!
//! Every block's [`crate::chain_entries::build_block_write_batch`] needs
//! the current chain length (count of historical entries) for every
//! touched key, so it can stage the new entry at index `length` and
//! increment `length` to `length+1`. Without a cache, that's one RocksDB
//! point lookup per unique key per block — at the mass-mint window's
//! ~10k unique keys/block this is the dominant I/O cost on the indexer
//! hot path.
//!
//! This cache stores `key → length` in process memory. Lookups hit the
//! cache; misses fall through to a single batched `multi_get` for all
//! keys that aren't cached (rare after warmup). The cache is populated
//! ONLY after a successful `commit_atomic`, by walking the
//! just-written `WriteBatch` and extracting every `{key}/length` put.
//! That keeps the cache durably-consistent with disk: if a commit
//! fails, the cache is untouched and the retry's `build_block_write_batch`
//! sees the pre-commit state — same chain indices assigned the second
//! time, no corruption.
//!
//! # Concurrency
//!
//! `RwLock<HashMap>` — block-apply takes a write-lock briefly per
//! commit; view RPCs and reads take read-locks. Contention is low
//! because writes happen once per block (≈4-second cadence at mainnet)
//! and reads are mostly in-batch (one bulk_insert per block).
//!
//! # Memory cost
//!
//! ~64-byte avg key + 4-byte length + HashMap overhead ≈ 100 bytes per
//! cached entry. At 1M unique keys (typical after a long sync) that's
//! ~100 MB — fits in the pod's 16 GiB request.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Process-wide cache of `{key} → current chain length` mappings.
///
/// Shared across the storage adapter clones via `Arc`; every clone of a
/// `RocksDBRuntimeAdapter` points at the same cache instance. The cache
/// is durable as long as the process is alive; cold-start re-warms via
/// cache-miss disk reads.
#[derive(Clone, Debug, Default)]
pub struct LengthCache {
    inner: Arc<RwLock<HashMap<Vec<u8>, u32>>>,
}

impl LengthCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up the cached length for `key`. Returns `None` on cache
    /// miss; the caller falls back to a disk read and SHOULD NOT
    /// populate the cache itself (that happens after `commit_atomic`
    /// success via [`Self::bulk_insert`]).
    pub fn get(&self, key: &[u8]) -> Option<u32> {
        self.inner.read().unwrap().get(key).copied()
    }

    /// Bulk-insert post-commit length values. Called from
    /// `commit_atomic` after `db.write_opt` succeeds, with the
    /// `{key}/length` updates that were just durably written. Acquires
    /// a write-lock briefly.
    pub fn bulk_insert<I>(&self, updates: I)
    where
        I: IntoIterator<Item = (Vec<u8>, u32)>,
    {
        let mut w = self.inner.write().unwrap();
        for (k, v) in updates {
            w.insert(k, v);
        }
    }

    /// Number of cached entries. Diagnostic only.
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.read().unwrap().is_empty()
    }

    /// Drop every cached entry. Used by reorg / rollback paths to
    /// force re-warm from disk so the cache can't carry stale post-fork
    /// lengths through a rollback boundary.
    pub fn clear(&self) {
        self.inner.write().unwrap().clear();
    }
}

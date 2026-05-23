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
//! # Bounded memory (since v10-length-cache-lru)
//!
//! Backed by [`lru_mem::LruCache`], capped to
//! [`DEFAULT_LENGTH_CACHE_CAPACITY_BYTES`] (512 MiB) by default.
//! Insertions past the cap evict the least-recently-used entry.
//! Cap is overridable at process start via the
//! `METASHREW_LENGTH_CACHE_BYTES` env var.
//!
//! The original v10-perf implementation used an unbounded
//! `Arc<RwLock<HashMap<Vec<u8>, u32>>>`. Production wedged at ~35 GiB
//! RSS on meta and OOM-killed a separate 192 GiB box once the mass-mint
//! workload's unique-key set grew past ~hundreds of millions. The
//! eviction here trades a small re-warm cost on cold keys for hard
//! upper bound on cache memory — well-tuned to the indexer's actual
//! locality, since `build_block_write_batch` repeatedly touches the
//! same handful of "hot" pool / supply / address keys.
//!
//! # Concurrency
//!
//! `RwLock<LruCache>` — even reads need a write-lock because
//! `lru_mem::LruCache::get` bumps the entry to MRU position. Contention
//! is low because writes happen once per block (≈4-second cadence at
//! mainnet) and reads are mostly in-batch (one bulk_insert per block).
//!
//! # Memory accounting
//!
//! `lru_mem::LruCache::new(cap_bytes)` charges each entry by
//! `MemSize(K) + MemSize(V)` — for `(Vec<u8>, u32)` that's `key.len() +
//! 24 (Vec header) + 4 (u32)` per entry. With ~64-byte keys typical of
//! the protorune storage layout, the per-entry charge is ~92 bytes
//! plus internal overhead the LruCache adds on top.

use lru_mem::LruCache;
use std::sync::{Arc, RwLock};

/// Default cap on length-cache memory. 512 MiB — empirically enough to
/// hold the working-set hot keys for several days of mainnet indexing
/// without OOM. Override at process start via
/// `METASHREW_LENGTH_CACHE_BYTES`.
pub const DEFAULT_LENGTH_CACHE_CAPACITY_BYTES: usize = 512 * 1024 * 1024;

/// Process-wide cache of `{key} → current chain length` mappings.
///
/// Shared across the storage adapter clones via `Arc`; every clone of a
/// `RocksDBRuntimeAdapter` points at the same cache instance. The cache
/// is durable as long as the process is alive; cold-start re-warms via
/// cache-miss disk reads.
#[derive(Clone, Debug)]
pub struct LengthCache {
    inner: Arc<RwLock<LruCache<Vec<u8>, u32>>>,
}

impl Default for LengthCache {
    fn default() -> Self {
        let cap = std::env::var("METASHREW_LENGTH_CACHE_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_LENGTH_CACHE_CAPACITY_BYTES);
        Self::with_capacity_bytes(cap)
    }
}

impl LengthCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a cache with an explicit byte cap. Tests use this to
    /// pin a tiny size and exercise the eviction path; production
    /// pulls the cap from the env via `default()`.
    pub fn with_capacity_bytes(cap_bytes: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(LruCache::new(cap_bytes.max(1)))),
        }
    }

    /// Look up the cached length for `key`. Returns `None` on cache
    /// miss; the caller falls back to a disk read and SHOULD NOT
    /// populate the cache itself (that happens after `commit_atomic`
    /// success via [`Self::bulk_insert`]).
    ///
    /// Requires a write-lock because `lru_mem::LruCache::get` mutates
    /// the LRU bookkeeping.
    pub fn get(&self, key: &[u8]) -> Option<u32> {
        self.inner.write().unwrap().get(key).copied()
    }

    /// Bulk-insert post-commit length values. Called from
    /// `commit_atomic` after `db.write_opt` succeeds, with the
    /// `{key}/length` updates that were just durably written. Acquires
    /// a write-lock briefly.
    ///
    /// `insert` returns `Err` if a single entry's size exceeds the
    /// cache's total cap — we silently drop in that case (extreme
    /// pathological key, shouldn't happen in practice).
    pub fn bulk_insert<I>(&self, updates: I)
    where
        I: IntoIterator<Item = (Vec<u8>, u32)>,
    {
        let mut w = self.inner.write().unwrap();
        for (k, v) in updates {
            let _ = w.insert(k, v);
        }
    }

    /// Number of cached entries. Diagnostic only.
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.read().unwrap().is_empty()
    }

    /// Current resident-memory cost in bytes. Sum over all entries of
    /// `MemSize(K) + MemSize(V)`. Diagnostic / observability.
    pub fn current_size_bytes(&self) -> usize {
        self.inner.read().unwrap().current_size()
    }

    /// Drop every cached entry. Used by reorg / rollback paths to
    /// force re-warm from disk so the cache can't carry stale post-fork
    /// lengths through a rollback boundary.
    pub fn clear(&self) {
        self.inner.write().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_eviction_bounds_memory() {
        // Tiny 4 KiB cap so we can fill it.
        let c = LengthCache::with_capacity_bytes(4 * 1024);
        // Each entry is roughly 60-100 bytes; ~50 entries fit in 4 KiB.
        for i in 0..1000_u32 {
            let mut key = vec![0u8; 32];
            key[..4].copy_from_slice(&i.to_le_bytes());
            c.bulk_insert(std::iter::once((key, i)));
        }
        // After 1000 inserts, cache must NOT hold all 1000.
        assert!(c.len() < 1000, "expected LRU to evict; len={}", c.len());
        // Most recent entry should still be present.
        let mut recent_key = vec![0u8; 32];
        recent_key[..4].copy_from_slice(&999_u32.to_le_bytes());
        assert_eq!(c.get(&recent_key), Some(999));
        // First entry should have been evicted.
        let first_key = vec![0u8; 32];
        assert_eq!(c.get(&first_key), None);
    }

    #[test]
    fn clear_drops_everything() {
        let c = LengthCache::with_capacity_bytes(1024 * 1024);
        c.bulk_insert((0_u32..100).map(|i| {
            let mut k = vec![0u8; 16];
            k[..4].copy_from_slice(&i.to_le_bytes());
            (k, i)
        }));
        assert_eq!(c.len(), 100);
        c.clear();
        assert_eq!(c.len(), 0);
        assert!(c.is_empty());
    }
}

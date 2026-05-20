//! Host-side LRU cache backing the v10 view-syscall `CacheGet` /
//! `CachePut` ops.
//!
//! Sits in front of the wasm view runtime as a memoization layer:
//! the wasm computes some derived value once, calls
//! `metashrew_core::view::cache_put(key, value)`, and every subsequent
//! view RPC at the same height that asks for the same key gets a hit.
//!
//! # Determinism contract
//!
//! The cache is host-shared across requests but the cache MUST NEVER
//! change the *value* the view returns — only the cost. The wasm
//! author is responsible for keying entries by inputs that don't drift
//! (height, block hash, outpoint id, content-addressed digest, etc.).
//!
//! The host applies a small belt-and-suspenders layer of its own:
//! every cache key is prefixed by the indexed-tip height the host
//! observed when the request arrived, so a key like `runesByOutpoint:0xabcd`
//! gets stored as `<height_le:4 bytes><key bytes>` — cache entries
//! never bleed across height transitions even if the wasm author
//! forgets to include the height. This is purely defensive; well-written
//! view code should still include any other height-sensitive inputs.
//!
//! # Memory model
//!
//! Single process-global `lru_mem::LruCache` behind a `RwLock`. The
//! default 256MB cap is plenty for the JSON-blob-sized view outputs
//! we expect; configurable at runtime via `set_capacity`.

use lru_mem::LruCache;
use std::sync::{Arc, LazyLock, RwLock};

/// Default cap on cache memory. Per `lru_mem::LruCache::new` semantics,
/// this counts the heap-resident size of stored keys + values. The
/// indexer process is configured for many tens of GB of RSS on
/// production pods, so 256MB for the view cache is a small fraction.
pub const DEFAULT_VIEW_CACHE_CAPACITY_BYTES: usize = 256 * 1024 * 1024;

/// Process-global view cache. Lazily initialized on first access so
/// the cost lands the first time anything tries to read/write it
/// (typically on the first view RPC of the indexer's lifetime).
static VIEW_CACHE: LazyLock<Arc<RwLock<LruCache<Vec<u8>, Vec<u8>>>>> =
    LazyLock::new(|| {
        Arc::new(RwLock::new(LruCache::new(DEFAULT_VIEW_CACHE_CAPACITY_BYTES)))
    });

/// Build the canonical lookup key: `[height_le | wasm_key]`. The
/// height prefix is the load-bearing determinism guard — see the
/// module-level doc comment.
fn prefixed_key(height: u32, wasm_key: &[u8]) -> Vec<u8> {
    let mut k = Vec::with_capacity(4 + wasm_key.len());
    k.extend_from_slice(&height.to_le_bytes());
    k.extend_from_slice(wasm_key);
    k
}

/// Read a value from the view cache at the given height. Returns `None`
/// on miss. Acquires only a write lock under the hood (the LRU has to
/// move the entry to the most-recently-used position).
pub fn get(height: u32, wasm_key: &[u8]) -> Option<Vec<u8>> {
    let key = prefixed_key(height, wasm_key);
    let mut cache = VIEW_CACHE.write().ok()?;
    cache.get(&key).cloned()
}

/// Insert a value into the view cache at the given height. Silently
/// drops the entry if it exceeds the cache's per-entry cap.
///
/// The `_ttl_seconds` parameter is wired through the proto for future
/// expansion; the current implementation uses pure LRU eviction.
pub fn put(height: u32, wasm_key: &[u8], value: Vec<u8>, _ttl_seconds: u32) {
    let key = prefixed_key(height, wasm_key);
    if let Ok(mut cache) = VIEW_CACHE.write() {
        // `try_insert` would return Err if the entry is bigger than the
        // total cap; in that case we silently drop. `insert` returns the
        // evicted entries via the LRU policy, which we ignore.
        let _ = cache.insert(key, value);
    }
}

/// Drop all entries from the view cache. Exposed for tests + for
/// operators who need to invalidate after a manual data manipulation
/// (e.g., reorg replay).
pub fn clear() {
    if let Ok(mut cache) = VIEW_CACHE.write() {
        cache.clear();
    }
}

/// Override the cache capacity at runtime. Useful for tests and for
/// operators who want to size the cache against their pod's memory
/// budget. Existing entries beyond the new cap are LRU-evicted on
/// next insert.
pub fn set_capacity(bytes: usize) {
    if let Ok(mut cache) = VIEW_CACHE.write() {
        cache.set_max_size(bytes);
    }
}

/// Return `(current_size_bytes, max_size_bytes, entry_count)` — for
/// observability (`/metrics` endpoint, debug logging).
pub fn stats() -> (usize, usize, usize) {
    if let Ok(cache) = VIEW_CACHE.read() {
        (cache.current_size(), cache.max_size(), cache.len())
    } else {
        (0, 0, 0)
    }
}

/// Shared mutex for tests that touch the global cache. `cargo test` runs
/// tests in parallel by default, and the cache is a process-global. Any
/// test that depends on cache state (clear + put + assert) must hold
/// this lock for its duration so it doesn't race against the dispatcher
/// tests in `view_syscall`. Exposed `pub(crate)` so `view_syscall::tests`
/// can use the same mutex.
#[cfg(test)]
pub(crate) static TEST_CACHE_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_miss_returns_none() {
        let _g = TEST_CACHE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        // Use an unlikely-to-collide height so other tests can't have
        // pre-populated this key.
        assert!(get(0xff_ff_00_01, b"unknown-key").is_none());
    }

    #[test]
    fn put_then_get_roundtrips() {
        let _g = TEST_CACHE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        put(0xff_ff_00_02, b"my-key", b"my-value".to_vec(), 0);
        assert_eq!(
            get(0xff_ff_00_02, b"my-key").as_deref(),
            Some(b"my-value".as_slice()),
        );
    }

    #[test]
    fn height_prefix_isolates_entries() {
        let _g = TEST_CACHE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        put(0xff_ff_01_00, b"k", b"v-at-100".to_vec(), 0);
        put(0xff_ff_01_01, b"k", b"v-at-101".to_vec(), 0);
        assert_eq!(get(0xff_ff_01_00, b"k").as_deref(), Some(b"v-at-100".as_slice()));
        assert_eq!(get(0xff_ff_01_01, b"k").as_deref(), Some(b"v-at-101".as_slice()));
    }

    #[test]
    fn clear_drops_everything() {
        let _g = TEST_CACHE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        put(0xff_ff_02_00, b"k1", b"v1".to_vec(), 0);
        put(0xff_ff_02_00, b"k2", b"v2".to_vec(), 0);
        clear();
        assert!(get(0xff_ff_02_00, b"k1").is_none());
        assert!(get(0xff_ff_02_00, b"k2").is_none());
    }

    #[test]
    fn stats_reports_current_size() {
        let _g = TEST_CACHE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        let (sz0, _, n0) = stats();
        assert_eq!(sz0, 0);
        assert_eq!(n0, 0);
        put(0xff_ff_03_00, b"key", b"value".to_vec(), 0);
        let (sz1, _, n1) = stats();
        assert!(sz1 > 0);
        assert_eq!(n1, 1);
    }
}

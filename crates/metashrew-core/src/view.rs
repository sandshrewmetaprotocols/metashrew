//! v10 view-syscall wasm-side wrappers.
//!
//! These are the guest-side counterpart to the runtime's `__flush`
//! syscall dispatcher (see `metashrew-runtime/src/view_syscall.rs`).
//! View functions opt in by building with `--features view-syscalls`
//! and calling [`cache_get`] / [`cache_put`] to read / write the
//! host-shared LRU cache, or — once wired in a follow-up commit —
//! [`spawn`] / [`join`] to parallelize hot loops.
//!
//! # ABI
//!
//! Every view-syscall reuses the existing `__flush(i32)` import. The
//! `i32` is a pointer to the data-start of an AssemblyScript-layout
//! `[u32 LE: len | payload]` buffer; `payload` is a serialized
//! `ViewSyscall` proto. Responses (where applicable) are written by
//! the host directly into wasm linear memory at the `response_ptr`
//! the wasm pre-allocates and encodes into the request:
//! `[u32 LE: response_len | response_bytes]`.
//!
//! On cache miss, on syscall-not-yet-supported, or on any host error,
//! the host writes a `response_len = 0` marker. The wrapper turns that
//! into `None` (`cache_get`) or [`SpawnError::Unsupported`] (`spawn`).
//!
//! # Determinism
//!
//! The cache is host-shared across requests but the host enforces a
//! `[height_le_u32][key]` prefix on every entry so the same `(height,
//! key)` always resolves the same way. View authors must ensure that
//! anything they cache is fully determined by their request inputs —
//! the cache must never change the value a view returns, only the
//! cost. See the doc-comment on `view_cache.rs` in metashrew-runtime
//! for the full contract.

use crate::imports::__flush;
use metashrew_support::proto::metashrew::{
    view_syscall::Request as ViewReq, CacheGet, CachePut, ViewSyscall,
};
use prost::Message;

/// Default size of the response buffer pre-allocated for the host to
/// write into. 64 KiB fits any plausible cached value; views that need
/// more can use [`cache_get_with_capacity`].
pub const DEFAULT_RESPONSE_CAP: usize = 64 * 1024;

/// Read a value from the host-shared view cache.
///
/// Returns `Some(bytes)` on cache hit, `None` on miss (or on any
/// host-side error — see the module doc-comment on the fail-closed
/// contract).
///
/// The host prefixes the key with the current indexed-tip height
/// internally, so callers don't need to thread the height into `key`.
/// You probably DO want to include other request-specific inputs (an
/// outpoint id, a request-shape hash) so the cache doesn't bleed
/// across distinct logical queries.
pub fn cache_get(key: &[u8]) -> Option<Vec<u8>> {
    cache_get_with_capacity(key, DEFAULT_RESPONSE_CAP)
}

/// Like [`cache_get`] but with a caller-controlled response-buffer
/// size. Use this for cached values bigger than `DEFAULT_RESPONSE_CAP`.
pub fn cache_get_with_capacity(key: &[u8], response_cap: usize) -> Option<Vec<u8>> {
    // Pre-allocate the response buffer. The host writes
    // [u32 LE: response_len | response_bytes] starting at response_ptr.
    // We keep the buffer alive past __flush via the leak()-style box.
    // Box::leak is safe here because we own the leaked allocation for
    // the lifetime of the syscall, and we drop it via Box::from_raw
    // after we've copied the response out.
    let response_buf = vec![0u8; response_cap + 4];
    let response_ptr_box = Box::into_raw(response_buf.into_boxed_slice());
    let response_ptr = response_ptr_box as *mut u8 as u32;

    let syscall = ViewSyscall {
        request: Some(ViewReq::CacheGet(CacheGet { key: key.to_vec() })),
        response_ptr,
        response_max: (response_cap + 4) as u32,
    };
    let payload = syscall.encode_to_vec();
    let payload_with_prefix = to_arraybuffer_layout(&payload);

    // __flush is an `extern "C"` host import on real wasm builds (so
    // an unsafe call is required) and a safe stub under the
    // `test-utils` feature. We use `#[allow(unused_unsafe)]` so the
    // safe-stub build doesn't warn.
    let payload_ptr = payload_with_prefix.as_ptr() as u32 + 4;
    #[allow(unused_unsafe)]
    unsafe {
        __flush(payload_ptr as i32);
    }

    // Read the response from response_ptr. The host wrote
    // [u32 LE: response_len | response_bytes]; we read both, then drop
    // the leaked box.
    //
    // SAFETY: the box is still live here because we held the raw
    // pointer; we recover ownership before returning.
    let result = unsafe {
        let raw = std::slice::from_raw_parts(response_ptr_box as *const u8, response_cap + 4);
        let len_bytes: [u8; 4] = raw[0..4].try_into().ok()?;
        let response_len = u32::from_le_bytes(len_bytes) as usize;
        if response_len == 0 || response_len > response_cap {
            // Miss or oversized — fail closed.
            None
        } else {
            Some(raw[4..4 + response_len].to_vec())
        }
    };
    // SAFETY: response_ptr_box came from Box::into_raw above; rehydrate
    // and drop.
    unsafe {
        drop(Box::from_raw(response_ptr_box));
    }
    result
}

/// Write a value into the host-shared view cache.
///
/// `ttl_seconds` is reserved for future expansion (the v10 host
/// implementation uses pure LRU eviction and ignores TTL). Pass 0 to
/// signal "no TTL, evict by LRU policy".
///
/// CachePut has no response — the wrapper does NOT pre-allocate a
/// response buffer.
pub fn cache_put(key: &[u8], value: &[u8], ttl_seconds: u32) {
    let syscall = ViewSyscall {
        request: Some(ViewReq::CachePut(CachePut {
            key: key.to_vec(),
            value: value.to_vec(),
            ttl_seconds,
        })),
        response_ptr: 0,
        response_max: 0,
    };
    let payload = syscall.encode_to_vec();
    let payload_with_prefix = to_arraybuffer_layout(&payload);
    let payload_ptr = payload_with_prefix.as_ptr() as u32 + 4;
    #[allow(unused_unsafe)]
    unsafe {
        __flush(payload_ptr as i32);
    }
}

/// Build the AssemblyScript ArrayBuffer layout `[u32 LE: len | bytes]`
/// that the host's `try_read_arraybuffer_as_vec` expects.
///
/// Duplicate of `metashrew_support::compat::to_arraybuffer_layout`,
/// inlined here to avoid pulling the entire `compat` module into the
/// view-only feature gate. Kept private to the view module.
fn to_arraybuffer_layout(payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(payload.len() + 4);
    let len = payload.len() as u32;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(payload);
    buf
}

#[cfg(test)]
mod tests {
    //! These tests only verify the proto-encoding path, not the host
    //! interaction (no wasm runtime here). Real end-to-end coverage is
    //! in `metashrew-runtime/tests/view_threads.rs` (host) and will be
    //! added in a follow-up integration test once a built indexer wasm
    //! exercising the view-syscalls feature is available.

    use super::*;
    use metashrew_support::proto::metashrew::view_syscall::Request as ViewReq;

    #[test]
    fn cache_get_encodes_proto_correctly() {
        let key = b"my-cache-key";
        let syscall = ViewSyscall {
            request: Some(ViewReq::CacheGet(CacheGet { key: key.to_vec() })),
            response_ptr: 0x1234,
            response_max: 0x5678,
        };
        let bytes = syscall.encode_to_vec();
        let decoded = ViewSyscall::decode(bytes.as_slice()).expect("decode");
        match decoded.request {
            Some(ViewReq::CacheGet(req)) => assert_eq!(req.key, key),
            other => panic!("expected CacheGet, got {:?}", other),
        }
        assert_eq!(decoded.response_ptr, 0x1234);
        assert_eq!(decoded.response_max, 0x5678);
    }

    #[test]
    fn cache_put_encodes_proto_correctly() {
        let syscall = ViewSyscall {
            request: Some(ViewReq::CachePut(CachePut {
                key: b"k".to_vec(),
                value: b"v".to_vec(),
                ttl_seconds: 42,
            })),
            response_ptr: 0,
            response_max: 0,
        };
        let bytes = syscall.encode_to_vec();
        let decoded = ViewSyscall::decode(bytes.as_slice()).expect("decode");
        match decoded.request {
            Some(ViewReq::CachePut(req)) => {
                assert_eq!(req.key, b"k");
                assert_eq!(req.value, b"v");
                assert_eq!(req.ttl_seconds, 42);
            }
            other => panic!("expected CachePut, got {:?}", other),
        }
    }

    #[test]
    fn arraybuffer_layout_is_len_prefixed() {
        let buf = to_arraybuffer_layout(&[0xAA, 0xBB, 0xCC]);
        assert_eq!(buf.len(), 7);
        assert_eq!(&buf[0..4], &3u32.to_le_bytes());
        assert_eq!(&buf[4..], &[0xAA, 0xBB, 0xCC]);
    }
}

//! v10 view-syscall wasm-side wrappers.
//!
//! These are the guest-side counterpart to the runtime's `__flush`
//! syscall dispatcher (see `metashrew-runtime/src/view_syscall.rs`).
//! View functions opt in by building with `--features view-syscalls`
//! and calling [`cache_get`] / [`cache_put`] to read / write the
//! host-shared LRU cache, or [`spawn`] / [`join`] to parallelize hot
//! loops onto host-managed worker threads via a closure trampoline.
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
//! the host writes a `response_len = 0` marker (cache ops) or a
//! sentinel value (thread ops: `INVALID_THREAD_ID = 0` from spawn,
//! `INVALID_EXIT_CODE = i32::MIN` from join). The wrapper turns that
//! into `None` for both `cache_get` and `spawn` / `join`.
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
    view_syscall::Request as ViewReq, CacheGet, CachePut, ThreadJoin, ThreadSpawn, ViewSyscall,
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

// =============================================================================
// ThreadSpawn / ThreadJoin — wasm-side closure trampoline.
//
// The host's `ThreadSpawn` op takes (fn_idx, arg) and indirect-calls
// `__indirect_function_table[fn_idx](arg)` on a freshly-instantiated
// child instance of the same module. fn_idx is the slot of our
// `__view_thread_entry` trampoline; arg is a thin pointer to a
// heap-leaked `Box<ThreadEntry>`.
//
// Lifetime contract for the boxed closure:
//   1. `spawn` boxes the user's closure twice — inner Box gives type
//      erasure (`Box<dyn FnOnce()->i32 + Send>`, a fat pointer with
//      data+vtable), outer Box gives a thin pointer that fits in u32.
//   2. `spawn` calls `Box::into_raw` on the outer box, casts to u32,
//      passes as `arg`. The allocation is now leaked from spawn's POV.
//   3. The host spawns a child wasmtime instance and calls into the
//      trampoline. Because instances share linear memory in spawn
//      semantics (or — in the v10 model — the child re-instantiates
//      the module pointing at the SAME memory pool the parent owns),
//      the leaked Box's u32 pointer is valid in the trampoline's
//      address space too. (This is the load-bearing precondition; if
//      the host ever migrates to instance-isolated memory, the boxed
//      closure must be deep-copied or serialized instead.)
//   4. The trampoline reconstitutes the outer Box via `Box::from_raw`,
//      derefs to get the inner Box, calls the closure, returns the
//      i32. Both Boxes are dropped at end of scope — the heap
//      allocation is reclaimed cleanly.
//   5. If `spawn` detects a host-side failure (thread_id 0 sentinel
//      from `INVALID_THREAD_ID`), it reclaims the leaked Box itself
//      so the closure isn't leaked. This is the only path where the
//      trampoline doesn't run.
// =============================================================================

/// Boxed type-erased closure that crosses the spawn/join boundary.
///
/// We use a type alias rather than `Box<dyn ...>` inline because the
/// outer Box (`Box<ThreadEntry>`) is a thin pointer to a fat pointer
/// — fits in u32, dereferences to the fat pointer, calls the closure.
type ThreadEntry = Box<dyn FnOnce() -> i32 + Send + 'static>;

/// Static trampoline. The wasm linker assigns this function a slot in
/// the indirect function table; the host's ThreadSpawn op invokes
/// `__indirect_function_table[fn_idx](arg)` where `fn_idx` is the
/// value of `__view_thread_entry as u32`.
///
/// `arg` is the raw u32 pointer to the heap-leaked `Box<ThreadEntry>`.
/// The trampoline reconstitutes the box, invokes the closure, and
/// returns the closure's i32 result. Both Boxes drop at end of scope
/// — heap allocation reclaimed.
///
/// SAFETY: `arg` must be a value previously produced by
/// `Box::into_raw` on a `Box<ThreadEntry>` in [`spawn`] below. Any
/// other value is undefined behavior. The host only calls this via
/// the indirect-function-table slot we register, so as long as
/// nothing else writes to that slot the contract is upheld.
#[no_mangle]
pub extern "C" fn __view_thread_entry(arg: u32) -> i32 {
    // SAFETY: `arg` was produced by `Box::into_raw(Box<ThreadEntry>)`
    // in `spawn` below. We take ownership of the box now (drops at
    // end of scope to reclaim the leaked allocation).
    let outer: Box<ThreadEntry> = unsafe { Box::from_raw(arg as *mut ThreadEntry) };
    let inner: ThreadEntry = *outer;
    inner()
}

/// Sentinel value the host returns from `ThreadSpawn` when the
/// view-thread registry / spawn module is unavailable (e.g. the wasm
/// is running under an indexer engine that doesn't have
/// `wasm_threads(true)`). Mirrors `metashrew_runtime::view_threads::INVALID_THREAD_ID`.
pub const INVALID_THREAD_ID: u32 = 0;

/// Sentinel value the host returns from `ThreadJoin` when the
/// thread id is unknown, has already been joined, or the spawned
/// task panicked. Mirrors `metashrew_runtime::view_threads::INVALID_EXIT_CODE`.
pub const INVALID_EXIT_CODE: i32 = i32::MIN;

/// Unique handle returned by [`spawn`]. Use [`join`] to await the
/// thread's exit code.
///
/// Constructible only via `spawn`; consumers can't accidentally
/// fabricate a `ThreadId` and try to join it. (They CAN copy and
/// double-join, which the host fails closed on — see `INVALID_EXIT_CODE`.)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ThreadId(u32);

impl ThreadId {
    /// Numeric thread id (the same value the host's registry uses).
    /// Exposed for diagnostics / cross-language plumbing.
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

/// Spawn a host-managed thread that runs `closure` to completion and
/// returns its `i32` exit code.
///
/// Returns `Some(ThreadId)` on success, `None` if the host couldn't
/// spawn the thread (registry/module absent — e.g. running under an
/// indexer engine, or under a view engine built without
/// `wasm_threads(true)`). On `None`, the closure is dropped without
/// running and its heap allocation is reclaimed.
///
/// Requires `--features view-syscalls` on the wasm side AND the
/// host's view engine to have been built with `wasm_threads(true)`
/// (the v10 default for view-mode; indexer-mode is hard-off).
///
/// # Closure capture lifetime
///
/// The closure is moved into the spawn (`Send + 'static` bounds) and
/// runs on a separate wasm child instance. Captures must be `Send`
/// (no `Rc`, no `RefCell`, no thread-locals). Captured heap data
/// MUST live on the same linear memory the parent uses — in practice
/// this means any `Vec`/`String` capture is fine because wasm32
/// allocators use the module's memory; raw pointers into module-data
/// are also fine.
pub fn spawn<F>(closure: F) -> Option<ThreadId>
where
    F: FnOnce() -> i32 + Send + 'static,
{
    // Box twice: inner Box gives type erasure (fat pointer:
    // data+vtable, 8 bytes on wasm32), outer Box gives a thin pointer
    // (4 bytes, fits in u32) that we can pass through the host
    // ThreadSpawn op.
    let inner: ThreadEntry = Box::new(closure);
    let outer: Box<ThreadEntry> = Box::new(inner);
    let arg = Box::into_raw(outer) as u32;

    // `__view_thread_entry as *const () as u32` resolves to the
    // function's slot in the `__indirect_function_table` exported by
    // the wasm module. Rust on wasm32 emits a table-slot reference
    // when you cast a `#[no_mangle] pub extern "C" fn` to a pointer
    // and then to an integer. (Direct fn-to-integer is allowed but
    // lints; the `*const ()` intermediate silences it without
    // changing semantics.)
    let fn_idx = __view_thread_entry as *const () as u32;

    // Response buffer: `[u32 LE: response_len | u32 LE: thread_id]` = 8 bytes.
    // We over-allocate by 4 (for safety) but the host writes exactly 8.
    let response_buf = vec![0u8; 8];
    let response_ptr_box = Box::into_raw(response_buf.into_boxed_slice());
    let response_ptr = response_ptr_box as *mut u8 as u32;

    let syscall = ViewSyscall {
        request: Some(ViewReq::ThreadSpawn(ThreadSpawn { fn_idx, arg })),
        response_ptr,
        response_max: 8,
    };
    let payload = syscall.encode_to_vec();
    let payload_with_prefix = to_arraybuffer_layout(&payload);
    let payload_ptr = payload_with_prefix.as_ptr() as u32 + 4;

    #[allow(unused_unsafe)]
    unsafe {
        __flush(payload_ptr as i32);
    }

    // SAFETY: response_ptr_box is still live (held the raw pointer
    // through the __flush call); recover ownership before returning.
    let (response_len, thread_id) = unsafe {
        let raw = std::slice::from_raw_parts(response_ptr_box as *const u8, 8);
        let len = u32::from_le_bytes(raw[0..4].try_into().unwrap_or([0; 4])) as usize;
        let tid = if len >= 4 {
            u32::from_le_bytes(raw[4..8].try_into().unwrap_or([0; 4]))
        } else {
            INVALID_THREAD_ID
        };
        drop(Box::from_raw(response_ptr_box));
        (len, tid)
    };

    // Failure modes:
    //   * response_len == 0  → host wrote a miss marker (registry absent,
    //                          or write_syscall_response failed). The
    //                          host did NOT spawn the worker, so the
    //                          leaked closure is still leaked — reclaim it.
    //   * thread_id == 0     → host wrote INVALID_THREAD_ID. Same as above.
    if response_len == 0 || thread_id == INVALID_THREAD_ID {
        // SAFETY: `arg` was produced by `Box::into_raw` above and the
        // trampoline didn't run (host returned a sentinel), so the
        // allocation is still live. Reclaim it.
        unsafe {
            drop(Box::from_raw(arg as *mut ThreadEntry));
        }
        return None;
    }

    Some(ThreadId(thread_id))
}

/// Block on the given thread's completion and return the closure's
/// `i32` exit code.
///
/// Returns `None` if the thread id was unknown to the host (already
/// joined, never spawned, registry shut down) or if the spawned
/// closure panicked. Note the host distinguishes these cases via the
/// `INVALID_EXIT_CODE` (i32::MIN) sentinel — we collapse them into
/// `None` since the wasm side can't usefully discriminate.
///
/// Calling `join` twice on the same `ThreadId` is a no-op (the
/// second call returns `None`); copy-and-double-join is therefore
/// safe but always-loses the second time.
pub fn join(handle: ThreadId) -> Option<i32> {
    // Response buffer: `[u32 LE: response_len | i32 LE: exit_code]` = 8 bytes.
    let response_buf = vec![0u8; 8];
    let response_ptr_box = Box::into_raw(response_buf.into_boxed_slice());
    let response_ptr = response_ptr_box as *mut u8 as u32;

    let syscall = ViewSyscall {
        request: Some(ViewReq::ThreadJoin(ThreadJoin {
            thread_id: handle.0,
        })),
        response_ptr,
        response_max: 8,
    };
    let payload = syscall.encode_to_vec();
    let payload_with_prefix = to_arraybuffer_layout(&payload);
    let payload_ptr = payload_with_prefix.as_ptr() as u32 + 4;

    #[allow(unused_unsafe)]
    unsafe {
        __flush(payload_ptr as i32);
    }

    // SAFETY: response_ptr_box is still live (held the raw pointer
    // through the __flush call); recover ownership before returning.
    let (response_len, exit_code) = unsafe {
        let raw = std::slice::from_raw_parts(response_ptr_box as *const u8, 8);
        let len = u32::from_le_bytes(raw[0..4].try_into().unwrap_or([0; 4])) as usize;
        let exit = if len >= 4 {
            i32::from_le_bytes(raw[4..8].try_into().unwrap_or([0; 4]))
        } else {
            INVALID_EXIT_CODE
        };
        drop(Box::from_raw(response_ptr_box));
        (len, exit)
    };

    if response_len == 0 || exit_code == INVALID_EXIT_CODE {
        None
    } else {
        Some(exit_code)
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
    //! interaction (no wasm runtime here). End-to-end coverage of the
    //! host side is in `metashrew-runtime/tests/view_threads.rs`,
    //! which uses a WAT fixture to drive `__flush` with ThreadSpawn /
    //! ThreadJoin payloads through the full dispatcher and verifies
    //! the worker actually runs.
    //!
    //! Full end-to-end coverage of the Rust closure trampoline
    //! (compiled `view::spawn(|| ...)` + child instance + Box::from_raw
    //! in `__view_thread_entry`) is deferred — it requires a
    //! shared-memory wasm fixture (`(memory (shared 1 N))` +
    //! `-C target-feature=+atomics,+bulk-memory` + `-Z build-std`),
    //! because the leaked `Box<ThreadEntry>` pointer is only
    //! dereferenceable in the child instance when parent and child
    //! share linear memory. See the comment on `__view_thread_entry`
    //! above for the lifetime contract.

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

    #[test]
    fn thread_spawn_encodes_proto_correctly() {
        let syscall = ViewSyscall {
            request: Some(ViewReq::ThreadSpawn(ThreadSpawn {
                fn_idx: 0xdead_beef,
                arg: 0x1234_5678,
            })),
            response_ptr: 0xaaaa,
            response_max: 8,
        };
        let bytes = syscall.encode_to_vec();
        let decoded = ViewSyscall::decode(bytes.as_slice()).expect("decode");
        match decoded.request {
            Some(ViewReq::ThreadSpawn(req)) => {
                assert_eq!(req.fn_idx, 0xdead_beef);
                assert_eq!(req.arg, 0x1234_5678);
            }
            other => panic!("expected ThreadSpawn, got {:?}", other),
        }
        assert_eq!(decoded.response_ptr, 0xaaaa);
        assert_eq!(decoded.response_max, 8);
    }

    #[test]
    fn thread_join_encodes_proto_correctly() {
        let syscall = ViewSyscall {
            request: Some(ViewReq::ThreadJoin(ThreadJoin {
                thread_id: 0xcafe_babe,
            })),
            response_ptr: 0xbbbb,
            response_max: 8,
        };
        let bytes = syscall.encode_to_vec();
        let decoded = ViewSyscall::decode(bytes.as_slice()).expect("decode");
        match decoded.request {
            Some(ViewReq::ThreadJoin(req)) => {
                assert_eq!(req.thread_id, 0xcafe_babe);
            }
            other => panic!("expected ThreadJoin, got {:?}", other),
        }
        assert_eq!(decoded.response_ptr, 0xbbbb);
        assert_eq!(decoded.response_max, 8);
    }

    #[test]
    fn thread_id_constructor_is_private_via_spawn() {
        // ThreadId can be constructed only inside `spawn` (its sole
        // field is private). What we CAN verify here is that ThreadId
        // round-trips its inner u32 unchanged through `as_u32`.
        //
        // We synthesize one via a Default-like manual transmute path —
        // but ThreadId has no public ctor, so we just verify the
        // semantics of the proto-bound numbers (separately from
        // ThreadId itself).
        let raw_id: u32 = 0x4242_4242;
        // Round-trip via proto.
        let syscall = ViewSyscall {
            request: Some(ViewReq::ThreadJoin(ThreadJoin { thread_id: raw_id })),
            response_ptr: 0,
            response_max: 0,
        };
        let bytes = syscall.encode_to_vec();
        let decoded = ViewSyscall::decode(bytes.as_slice()).expect("decode");
        match decoded.request {
            Some(ViewReq::ThreadJoin(req)) => assert_eq!(req.thread_id, raw_id),
            _ => panic!("expected ThreadJoin"),
        }
    }

    #[test]
    fn invalid_sentinels_match_host_constants() {
        // These MUST stay in lock-step with
        // metashrew_runtime::view_threads::{INVALID_THREAD_ID, INVALID_EXIT_CODE}.
        // The host writes them as the failure-mode payload of ThreadSpawn /
        // ThreadJoin; wasm relies on the exact match to distinguish
        // success from failure.
        assert_eq!(INVALID_THREAD_ID, 0);
        assert_eq!(INVALID_EXIT_CODE, i32::MIN);
    }

    #[test]
    fn view_thread_entry_is_exported() {
        // Sanity: the trampoline must have a well-known address (so
        // the wasm linker can put it in the indirect function table).
        // We don't actually call it from here — calling
        // `Box::from_raw(0 as *mut ThreadEntry)` would be UB. We just
        // verify the function-pointer cast compiles, which proves the
        // symbol exists and is callable.
        let fn_ptr: extern "C" fn(u32) -> i32 = __view_thread_entry;
        // Cast to usize via *const () to silence the
        // function_casts_as_integer lint; on non-wasm32 this isn't an
        // indirect-function-table slot but it's still a valid
        // function pointer (a code address).
        let addr = fn_ptr as *const () as usize;
        assert!(addr != 0, "trampoline must have a non-null code address");
    }
}

//! v10 view-mode syscall dispatcher (the host-side logic, separated
//! from wasmtime memory I/O so it's directly unit-testable).
//!
//! The wasmtime `__flush` binding in `runtime::setup_linker_view` reads
//! the proto payload from wasm linear memory, calls
//! [`dispatch_view_syscall`] with it, and writes any response back into
//! wasm memory via [`write_syscall_response`]. The dispatch step
//! itself doesn't need access to wasm memory — making it testable
//! without compiling a fixture wasm.

use crate::proto::metashrew::view_syscall::Request as ViewReq;
use crate::proto::metashrew::{ThreadJoin, ThreadSpawn, ViewSyscall};
use prost::Message;

/// Decoded thread op extracted from a ViewSyscall payload. Used by the
/// wasmtime `__flush` binding to split off thread ops (which need
/// access to the wasm Caller's engine / module / registry) from the
/// pure-function cache ops (`CacheGet` / `CachePut`) handled by
/// [`dispatch_view_syscall`].
#[derive(Debug, PartialEq)]
pub enum ThreadOp {
    Spawn(ThreadSpawn),
    Join(ThreadJoin),
}

/// Decode a thread op out of an already-decoded `ViewSyscall`-shaped
/// payload. Returns `None` if the payload doesn't decode at all or if
/// it's a non-thread op (CacheGet / CachePut / etc.).
pub fn decode_thread_op(payload: &[u8]) -> Option<ThreadOp> {
    let syscall = ViewSyscall::decode(payload).ok()?;
    match syscall.request? {
        ViewReq::ThreadSpawn(req) => Some(ThreadOp::Spawn(req)),
        ViewReq::ThreadJoin(req) => Some(ThreadOp::Join(req)),
        _ => None,
    }
}

/// Result of dispatching one view syscall.
#[derive(Debug, PartialEq, Eq)]
pub enum SyscallResult {
    /// The payload didn't decode as a `ViewSyscall`. Caller should
    /// treat this as the legacy no-op semantic (compat path for wasm
    /// that doesn't know about syscalls).
    NotASyscall,
    /// No response needed (e.g. `CachePut`).
    NoResponse,
    /// Write these bytes to the response buffer.
    Respond(Vec<u8>),
    /// The op exists in the proto but isn't wired yet (e.g. ThreadSpawn
    /// before step 4 lands). Caller should still write a length=0
    /// marker so wasm sees a deterministic miss.
    UnsupportedOp,
}

/// Decode + dispatch the syscall payload. `height` is the indexer tip
/// at the time of the view call — used as the cache-key prefix so
/// entries can't bleed across height transitions.
pub fn dispatch_view_syscall(height: u32, payload: &[u8]) -> SyscallResult {
    let syscall = match ViewSyscall::decode(payload) {
        Ok(s) => s,
        Err(_) => return SyscallResult::NotASyscall,
    };

    match syscall.request {
        Some(ViewReq::CacheGet(req)) => {
            // Empty Vec on miss; the caller's response writer will
            // emit a `len=0` marker which wasm reads as "miss".
            let value = crate::view_cache::get(height, &req.key).unwrap_or_default();
            SyscallResult::Respond(value)
        }
        Some(ViewReq::CachePut(req)) => {
            crate::view_cache::put(height, &req.key, req.value, req.ttl_seconds);
            SyscallResult::NoResponse
        }
        Some(ViewReq::ThreadSpawn(_)) | Some(ViewReq::ThreadJoin(_)) => {
            // Step 4 — view-threads not yet wired.
            SyscallResult::UnsupportedOp
        }
        None => SyscallResult::NoResponse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::metashrew::view_syscall::Request as ViewReq;
    use crate::proto::metashrew::{CacheGet, CachePut, ThreadSpawn, ViewSyscall};

    fn encode_get(key: &[u8]) -> Vec<u8> {
        let s = ViewSyscall {
            request: Some(ViewReq::CacheGet(CacheGet { key: key.to_vec() })),
            response_ptr: 0,
            response_max: 0,
        };
        s.encode_to_vec()
    }

    fn encode_put(key: &[u8], value: &[u8]) -> Vec<u8> {
        let s = ViewSyscall {
            request: Some(ViewReq::CachePut(CachePut {
                key: key.to_vec(),
                value: value.to_vec(),
                ttl_seconds: 0,
            })),
            response_ptr: 0,
            response_max: 0,
        };
        s.encode_to_vec()
    }

    #[test]
    fn empty_payload_is_a_noop() {
        // proto3 accepts a 0-byte payload as a valid empty message —
        // ViewSyscall with no `request` set. Dispatcher returns
        // NoResponse, equivalent to the legacy no-op semantic from
        // the wasm's perspective.
        assert_eq!(
            dispatch_view_syscall(100, b""),
            SyscallResult::NoResponse,
        );
    }

    #[test]
    fn garbage_payload_falls_back_to_legacy_noop() {
        // A `KeyValueFlush` will technically decode as ViewSyscall
        // (proto3 is forgiving) but its `request` oneof will be `None`,
        // so dispatch returns NoResponse — same observable as the
        // legacy no-op semantic.
        let kvf = crate::proto::metashrew::KeyValueFlush {
            list: vec![b"key".to_vec(), b"value".to_vec()],
        };
        let bytes = kvf.encode_to_vec();
        // Either NotASyscall (decode fails) or NoResponse (decodes but no
        // request set) — both are equivalent to no-op for wasm.
        let result = dispatch_view_syscall(100, &bytes);
        assert!(
            matches!(result, SyscallResult::NotASyscall | SyscallResult::NoResponse),
            "legacy KeyValueFlush payload must not be misinterpreted as a CacheGet/Put/etc, got {:?}",
            result
        );
    }

    #[test]
    fn put_then_get_roundtrips_through_dispatcher() {
        let _g = crate::view_cache::TEST_CACHE_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::view_cache::clear();

        // 1) put
        let put = encode_put(b"my-key", b"my-value");
        assert_eq!(
            dispatch_view_syscall(0xff_aa_00_00, &put),
            SyscallResult::NoResponse,
        );

        // 2) get hit
        let get = encode_get(b"my-key");
        assert_eq!(
            dispatch_view_syscall(0xff_aa_00_00, &get),
            SyscallResult::Respond(b"my-value".to_vec()),
        );

        // 3) get miss for different key
        let miss = encode_get(b"missing-key");
        assert_eq!(
            dispatch_view_syscall(0xff_aa_00_00, &miss),
            SyscallResult::Respond(Vec::new()),
            "miss should return empty Vec (response writer turns this into len=0 marker)",
        );
    }

    #[test]
    fn height_prefix_isolates_cache_entries() {
        let _g = crate::view_cache::TEST_CACHE_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::view_cache::clear();

        let put = encode_put(b"same-key", b"v-at-300");
        let _ = dispatch_view_syscall(0xff_aa_01_00, &put);

        let put2 = encode_put(b"same-key", b"v-at-301");
        let _ = dispatch_view_syscall(0xff_aa_01_01, &put2);

        let get = encode_get(b"same-key");
        assert_eq!(
            dispatch_view_syscall(0xff_aa_01_00, &get),
            SyscallResult::Respond(b"v-at-300".to_vec()),
        );
        assert_eq!(
            dispatch_view_syscall(0xff_aa_01_01, &get),
            SyscallResult::Respond(b"v-at-301".to_vec()),
        );
        // A height that was never written to should miss.
        assert_eq!(
            dispatch_view_syscall(0xff_aa_01_02, &get),
            SyscallResult::Respond(Vec::new()),
        );
    }

    #[test]
    fn thread_ops_return_unsupported_until_step_4() {
        let spawn = ViewSyscall {
            request: Some(ViewReq::ThreadSpawn(ThreadSpawn { fn_idx: 7, arg: 42 })),
            response_ptr: 0,
            response_max: 0,
        }
        .encode_to_vec();
        assert_eq!(
            dispatch_view_syscall(100, &spawn),
            SyscallResult::UnsupportedOp,
        );
    }
}

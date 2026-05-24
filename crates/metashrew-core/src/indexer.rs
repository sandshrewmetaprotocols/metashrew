//! v10 indexer-syscall wasm-side wrappers.
//!
//! Guest-side counterpart to the runtime's `IndexerSyscall` dispatcher
//! (see `metashrew-runtime/src/indexer_syscall.rs`). Indexer-mode
//! wasm opts in by building with `--features indexer-syscalls` and
//! using these helpers to:
//!
//!  * [`batch_get`] — single-syscall multi-key point-read (replaces N
//!    sequential `__get` round-trips).
//!  * [`spawn`] / [`join`] — Block-STM tx-handler spawn. Schedules a
//!    fresh wasmtime Store on a host worker thread to run an
//!    indirect-call into the wasm's function table.
//!  * [`conflict_read`] / [`conflict_write`] — instrumented variants
//!    of `__get` / `__set` used INSIDE a spawned tx-handler. Reads
//!    record a dependency in the per-tx ConflictTracker; writes
//!    stage into the per-block MvMemory at the spawned Store's
//!    tx_seq. The Block-STM scheduler validates the dependencies and
//!    re-executes losers; the final merge folds the highest-tx_seq
//!    write per key into the block's WriteBatch.
//!
//! # ABI
//!
//! Every indexer syscall reuses the existing `__flush(i32)` import.
//! Disambiguation from a legacy `KeyValueFlush` payload is by a
//! single magic byte (`0x01`) prepended to the encoded
//! `IndexerSyscall` proto. The pointer the wasm passes to `__flush`
//! is the data-start of an AssemblyScript-layout
//! `[u32 LE: len | payload]` buffer; `payload[0]` is the magic byte
//! and `payload[1..]` is the encoded `IndexerSyscall`.
//!
//! # Determinism
//!
//! Unlike `metashrew_core::view`, indexer syscalls are consensus-
//! critical. The HOST enforces determinism (Block-STM validate / re-
//! execute), but the wasm contract is that tx-handlers ONLY do I/O
//! via the conflict-tracked opcodes. Direct `__get` / `__set` from
//! inside a spawned tx-handler would bypass tracking and break
//! determinism — don't do it.
//!
//! # Closure trampoline (not used)
//!
//! Unlike the view-side `spawn` which uses a `__view_thread_entry`
//! closure trampoline, indexer-side spawn does NOT support arbitrary
//! Rust closures. Reason: indexer child Stores have ISOLATED linear
//! memory (no `wasm_threads(true)` — consensus determinism), so a
//! heap pointer from the parent isn't valid in the child. Tx-
//! handlers locate their work via `tx_seq` (the `arg` to spawn) +
//! storage lookups, not via in-process pointers.

use crate::imports::__flush;
use metashrew_support::proto::metashrew::{
    indexer_syscall::Request as IndexerReq, BatchGet, BatchGetResponse, ConflictRead,
    ConflictWrite, IndexerSyscall, IndexerThreadJoin, IndexerThreadSpawn,
};
use prost::Message;

/// Magic byte prefix that tells the host "this `__flush` payload is
/// an IndexerSyscall, not a legacy KeyValueFlush". Must equal
/// `metashrew_runtime::indexer_syscall::INDEXER_SYSCALL_MAGIC`. We
/// hard-code it rather than import from the runtime crate because
/// the wasm-core build doesn't pull metashrew-runtime.
pub const INDEXER_SYSCALL_MAGIC: u8 = 0x01;

/// Sentinel returned by [`spawn`] when the host's
/// `indexer_threads` registry or `spawn_module` is missing — e.g.
/// the wasm is running under a Store the runtime didn't enable
/// parallel execution on. Wasm should treat as "spawn failed, run
/// inline".
pub const INVALID_THREAD_ID: u32 = 0;

/// Sentinel returned by [`join`] when the thread id is unknown,
/// has already been joined, or the spawned tx-handler panicked /
/// trapped.
pub const INVALID_EXIT_CODE: i32 = i32::MIN;

/// Default response buffer size pre-allocated for opcodes that
/// return data. 64 KiB fits even large BatchGet responses for
/// typical alkanes-v3 workloads.
pub const DEFAULT_RESPONSE_CAP: usize = 64 * 1024;

/// Build the wire-format payload: `[magic_byte | encoded_syscall]`.
/// Returns the payload bytes (caller wraps in arraybuffer-layout
/// before passing to `__flush`).
fn build_payload(syscall: IndexerSyscall) -> Vec<u8> {
    let proto = syscall.encode_to_vec();
    let mut out = Vec::with_capacity(1 + proto.len());
    out.push(INDEXER_SYSCALL_MAGIC);
    out.extend(proto);
    out
}

/// Wrap `payload` in `[u32 LE: len | payload]`. Returns the buffer
/// (live until the returned `Vec` drops) — caller passes
/// `(buffer.as_ptr() as u32 + 4) as i32` to `__flush`.
fn to_arraybuffer_layout(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend((payload.len() as u32).to_le_bytes());
    out.extend(payload);
    out
}

/// Common dispatch: encode the syscall + magic byte, allocate a
/// response buffer, call `__flush`, read the response. Returns the
/// response bytes (empty if the host didn't write one) AND drops
/// the response buffer cleanly.
fn dispatch_with_response(
    request: IndexerReq,
    response_cap: usize,
) -> Vec<u8> {
    let response_buf = vec![0u8; response_cap + 4];
    let response_ptr_box = Box::into_raw(response_buf.into_boxed_slice());
    let response_ptr = response_ptr_box as *mut u8 as u32;

    let syscall = IndexerSyscall {
        request: Some(request),
        response_ptr,
        response_max: (response_cap + 4) as u32,
    };
    let payload = build_payload(syscall);
    let with_prefix = to_arraybuffer_layout(&payload);
    let payload_ptr = with_prefix.as_ptr() as u32 + 4;

    #[allow(unused_unsafe)]
    unsafe {
        __flush(payload_ptr as i32);
    }

    let result = unsafe {
        let raw =
            std::slice::from_raw_parts(response_ptr_box as *const u8, response_cap + 4);
        let len_bytes: [u8; 4] = match raw[0..4].try_into() {
            Ok(b) => b,
            Err(_) => return Vec::new(),
        };
        let response_len = u32::from_le_bytes(len_bytes) as usize;
        if response_len == 0 || response_len > response_cap {
            Vec::new()
        } else {
            raw[4..4 + response_len].to_vec()
        }
    };
    unsafe {
        drop(Box::from_raw(response_ptr_box));
    }
    result
}

/// Like [`dispatch_with_response`] but for fire-and-forget opcodes
/// that produce no response (ConflictWrite, etc.).
fn dispatch_fire_and_forget(request: IndexerReq) {
    let syscall = IndexerSyscall {
        request: Some(request),
        response_ptr: 0,
        response_max: 0,
    };
    let payload = build_payload(syscall);
    let with_prefix = to_arraybuffer_layout(&payload);
    let payload_ptr = with_prefix.as_ptr() as u32 + 4;
    #[allow(unused_unsafe)]
    unsafe {
        __flush(payload_ptr as i32);
    }
}

// =============================================================================
// BatchGet
// =============================================================================

/// Batch point-read at the indexer read height. Returns one entry
/// per request key, in input order. `None` = key was missing at
/// read height. `Some(bytes)` = stored value (may itself be empty).
///
/// Win over N sequential `__get` calls: one wasm↔host transition
/// instead of N. Doesn't yet pipeline RocksDB reads — that lands
/// later via a `multi_get_at_height` trait method on the runtime
/// side.
pub fn batch_get(keys: &[Vec<u8>]) -> Vec<Option<Vec<u8>>> {
    batch_get_with_capacity(keys, DEFAULT_RESPONSE_CAP)
}

/// Like [`batch_get`] but with a caller-controlled response buffer
/// size. Use for batches whose total value bytes might exceed
/// `DEFAULT_RESPONSE_CAP`.
pub fn batch_get_with_capacity(keys: &[Vec<u8>], response_cap: usize) -> Vec<Option<Vec<u8>>> {
    let n = keys.len();
    let req = BatchGet { keys: keys.to_vec() };
    let response = dispatch_with_response(IndexerReq::BatchGet(req), response_cap);
    if response.is_empty() {
        // Host wrote 0-length response — fail closed, surface all
        // misses. Caller can compare against `keys.len()` to detect
        // this vs a legit batch of misses.
        return vec![None; n];
    }
    match BatchGetResponse::decode(response.as_slice()) {
        Ok(r) => r
            .entries
            .into_iter()
            .map(|e| if e.present { Some(e.value) } else { None })
            .collect(),
        Err(_) => vec![None; n],
    }
}

// =============================================================================
// ConflictRead / ConflictWrite — used INSIDE spawned tx-handlers
// =============================================================================

/// Instrumented read for use INSIDE a spawned tx-handler. The host
/// looks up the spawned Store's `block_stm` + `tx_seq`, resolves
/// the value via MvMemory + storage fallback, and records a
/// dependency in the ConflictTracker so the scheduler can later
/// validate.
///
/// Returns the value bytes. Empty Vec means either the key is
/// missing OR the host failed-closed (no block_stm context on this
/// Store — wasm is calling from outside a tx-handler). The wasm
/// can't distinguish those cases from the response alone; if it
/// matters, use a `present`/`absent` envelope at the application
/// layer (alkanes-v3 already wraps stored values with their own
/// presence flag via the protorune balance-sheet protobuf).
pub fn conflict_read(key: &[u8]) -> Vec<u8> {
    conflict_read_with_capacity(key, DEFAULT_RESPONSE_CAP)
}

pub fn conflict_read_with_capacity(key: &[u8], response_cap: usize) -> Vec<u8> {
    dispatch_with_response(
        IndexerReq::ConflictRead(ConflictRead { key: key.to_vec() }),
        response_cap,
    )
}

/// Instrumented write for use INSIDE a spawned tx-handler. Stages
/// the value at this Store's `tx_seq` in the per-block MvMemory.
/// The scheduler's merge step later picks the highest-tx_seq value
/// per key into the block's WriteBatch.
///
/// No response — fire-and-forget.
pub fn conflict_write(key: &[u8], value: &[u8]) {
    dispatch_fire_and_forget(IndexerReq::ConflictWrite(ConflictWrite {
        key: key.to_vec(),
        value: value.to_vec(),
    }));
}

// =============================================================================
// ThreadSpawn / ThreadJoin
// =============================================================================

/// Unique handle returned by [`spawn`]. Pass to [`join`] to await
/// the tx-handler's exit code. `Copy` so wasm can stash + join
/// later without ownership friction.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ThreadId(u32);

impl ThreadId {
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

/// Spawn a tx-handler on the host's indexer worker pool.
///
/// `fn_idx` is a slot in `__indirect_function_table` — the host
/// instantiates a fresh child Store, looks up that slot, and calls
/// `slot(arg)`. The child Store is configured with the same
/// `block_stm` as the parent + `tx_seq = arg`, so the tx-handler's
/// `conflict_read` / `conflict_write` calls route through the per-
/// block MvMemory at the right tx_seq.
///
/// Returns `Some(ThreadId)` on success, `None` if the host couldn't
/// spawn (parent Store lacks `indexer_threads` registry OR
/// `spawn_module` OR `block_stm`). On `None`, the caller should
/// fall back to running the tx-handler inline.
///
/// Convention: `arg == tx_seq`. The host hardcodes this — the
/// child Store's `tx_seq` field is set to `arg`, regardless of
/// what's passed.
pub fn spawn(fn_idx: u32, tx_seq: u32) -> Option<ThreadId> {
    let response = dispatch_with_response(
        IndexerReq::ThreadSpawn(IndexerThreadSpawn { fn_idx, arg: tx_seq }),
        4,
    );
    if response.len() < 4 {
        return None;
    }
    let tid_bytes: [u8; 4] = response[0..4].try_into().ok()?;
    let tid = u32::from_le_bytes(tid_bytes);
    if tid == INVALID_THREAD_ID {
        None
    } else {
        Some(ThreadId(tid))
    }
}

/// Block until the named tx-handler finishes, return its `i32`
/// exit code. `None` indicates a host-side failure: unknown thread
/// id, already joined, or the tx-handler panicked / trapped.
pub fn join(tid: ThreadId) -> Option<i32> {
    let response = dispatch_with_response(
        IndexerReq::ThreadJoin(IndexerThreadJoin { thread_id: tid.0 }),
        4,
    );
    if response.len() < 4 {
        return None;
    }
    let exit_bytes: [u8; 4] = response[0..4].try_into().ok()?;
    let exit = i32::from_le_bytes(exit_bytes);
    if exit == INVALID_EXIT_CODE {
        None
    } else {
        Some(exit)
    }
}

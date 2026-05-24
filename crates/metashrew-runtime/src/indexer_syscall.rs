//! v10 indexer-mode syscall dispatcher (host-side logic, separated
//! from wasmtime memory I/O so it's directly unit-testable).
//!
//! Unlike `view_syscall.rs` (perf-only on the view path), the indexer
//! syscalls are **consensus-critical**: opcodes that touch state must
//! produce deterministic results across every replica indexing the
//! same block. The dispatcher itself is opcode-agnostic; per-opcode
//! determinism guarantees live in their respective handlers.
//!
//! Dispatch is a two-stage process:
//!
//!   1. `classify()` peeks `buf[0]` to disambiguate a legacy
//!      `KeyValueFlush` payload from an `IndexerSyscall` payload.
//!      Cost: one comparison, zero allocation, zero decode.
//!
//!   2. For syscall payloads, `dispatch_indexer_syscall()` decodes the
//!      `IndexerSyscall` proto once and returns a structured result
//!      that the wasmtime binding can pattern-match on. Storage-needing
//!      opcodes (e.g. `BatchGet`) are surfaced as typed variants so
//!      the binding can pass the runtime's storage handle.

use crate::block_stm::BlockStmCtx;
use crate::proto::metashrew::indexer_syscall::Request as IndexerReq;
use crate::proto::metashrew::{
    BatchGet, BatchGetEntry, BatchGetResponse, ConflictRead, ConflictWrite, IndexerSyscall,
    IndexerThreadJoin, IndexerThreadSpawn,
};
use crate::traits::KeyValueStoreLike;
use prost::Message;

/// Magic byte at buffer[0] indicating the rest is an `IndexerSyscall`
/// protobuf. Chosen from the protobuf-impossible range [0x00..0x08) so
/// it can never collide with a valid encoded protobuf message head.
pub const INDEXER_SYSCALL_MAGIC: u8 = 0x01;

/// What kind of payload `__flush` received. Determined by the first
/// byte alone — no decode needed.
#[derive(Debug, PartialEq, Eq)]
pub enum FlushKind {
    /// Legacy `KeyValueFlush`. Caller should decode `buf` (whole
    /// buffer) and apply via the chain-entry write path.
    Legacy,
    /// `IndexerSyscall` payload. The opcode protobuf begins at
    /// `buf[1..]` (the magic byte must be stripped before decoding).
    Syscall,
}

/// Peek the first byte to determine which decode path applies. No
/// allocation, no decode — just one comparison.
pub fn classify(buf: &[u8]) -> FlushKind {
    match buf.first() {
        Some(&INDEXER_SYSCALL_MAGIC) => FlushKind::Syscall,
        _ => FlushKind::Legacy,
    }
}

/// What the wasmtime binding should do with this syscall.
#[derive(Debug, PartialEq)]
pub enum SyscallResult {
    /// No response payload needed. Binding does not write to the
    /// wasm response buffer.
    NoResponse,
    /// Write these bytes to the wasm response buffer.
    Respond(Vec<u8>),
    /// Op exists in the proto but isn't wired in this build. Binding
    /// should write a `len=0` marker so wasm sees a deterministic
    /// not-supported response.
    UnsupportedOp,
    /// Storage-needing opcode: binding must call
    /// [`handle_batch_get`] with its `&T` storage handle plus the
    /// indexer's read height, then write the resulting bytes via
    /// the response writer.
    NeedsBatchGet(BatchGet),
    /// Block-STM read: binding must look up the spawned Store's
    /// `tx_seq` + `block_stm` context, call [`handle_conflict_read`]
    /// with storage handle + read height, then write the value.
    NeedsConflictRead(ConflictRead),
    /// Block-STM write: binding must look up the spawned Store's
    /// `tx_seq` + `block_stm` context, call [`handle_conflict_write`].
    /// No response needed (binding may pass `response_ptr=0`).
    NeedsConflictWrite(ConflictWrite),
    /// Spawn a wasm tx-handler on the indexer worker pool. Binding
    /// must look up `indexer_threads` registry on the caller's Store
    /// and clone module/engine to instantiate a fresh per-thread
    /// Store. Wired in the follow-up commit; for now the binding
    /// writes INVALID_THREAD_ID (0) as a deterministic miss marker.
    NeedsIndexerThreadSpawn(IndexerThreadSpawn),
    /// Block on a previously-spawned tx-handler. Same registry as
    /// above. Wired in the follow-up commit; for now the binding
    /// writes INVALID_EXIT_CODE (i32::MIN).
    NeedsIndexerThreadJoin(IndexerThreadJoin),
}

/// What the dispatcher extracted from the raw syscall payload.
#[derive(Debug, PartialEq)]
pub struct DispatchedSyscall {
    /// Wasm-allocated response buffer offset (0 = no response wanted).
    pub response_ptr: u32,
    /// Wasm-allocated response buffer capacity (must be >= 4 to fit
    /// the length prefix; smaller is treated as "no response wanted").
    pub response_max: u32,
    /// What the binding should do next.
    pub result: SyscallResult,
}

/// Caller-side error signal when an opcode payload fails to decode.
/// Wraps as `Err` so the binding can short-circuit to `had_failure`
/// without entangling decode errors with the routed result.
#[derive(Debug, PartialEq, Eq)]
pub struct SyscallDecodeError;

/// Decode the `IndexerSyscall` payload + route to a `SyscallResult`.
/// `payload` is the bytes AFTER the magic byte — caller has classified
/// and stripped already.
///
/// Single decode of the whole `IndexerSyscall` message: the binding
/// pulls `response_ptr` / `response_max` from the returned struct
/// without needing to re-decode.
pub fn dispatch_indexer_syscall(
    payload: &[u8],
) -> Result<DispatchedSyscall, SyscallDecodeError> {
    let syscall = IndexerSyscall::decode(payload).map_err(|_| SyscallDecodeError)?;
    let result = match syscall.request {
        Some(IndexerReq::Noop(_)) => SyscallResult::NoResponse,
        Some(IndexerReq::BatchGet(req)) => SyscallResult::NeedsBatchGet(req),
        Some(IndexerReq::ConflictRead(req)) => SyscallResult::NeedsConflictRead(req),
        Some(IndexerReq::ConflictWrite(req)) => SyscallResult::NeedsConflictWrite(req),
        Some(IndexerReq::ThreadSpawn(req)) => SyscallResult::NeedsIndexerThreadSpawn(req),
        Some(IndexerReq::ThreadJoin(req)) => SyscallResult::NeedsIndexerThreadJoin(req),
        None => SyscallResult::NoResponse,
    };
    Ok(DispatchedSyscall {
        response_ptr: syscall.response_ptr,
        response_max: syscall.response_max,
        result,
    })
}

/// Handle a `BatchGet` opcode against the runtime's storage handle.
/// Reads each requested key at `target_height` (binding passes
/// `height - 1` to match `__get`'s per-block semantic), packs the
/// results into a `BatchGetResponse`, and returns the encoded bytes.
///
/// Storage errors are surfaced as `Err` rather than coalesced to
/// "all keys missing" — a transient backend failure during block
/// apply must not silently produce a successful-looking empty
/// response (would diverge the indexer from a healthy replica).
///
/// Determinism: same `(db_state, target_height, keys)` -> same bytes
/// out, byte-for-byte. The version-chain walk in `get_at_height` is
/// deterministic (binary search on monotonic versions).
pub fn handle_batch_get<T: KeyValueStoreLike>(
    db: &T,
    target_height: u32,
    req: &BatchGet,
) -> anyhow::Result<Vec<u8>> {
    let mut entries = Vec::with_capacity(req.keys.len());
    for k in &req.keys {
        let entry = match crate::chain_entries::get_at_height(db, k, target_height)? {
            Some(v) => BatchGetEntry { present: true, value: v },
            None => BatchGetEntry { present: false, value: Vec::new() },
        };
        entries.push(entry);
    }
    Ok(BatchGetResponse { entries }.encode_to_vec())
}

/// Block-STM instrumented read on behalf of `tx_seq` for `key`.
///
/// Resolution order:
///   1. `BlockStmCtx.mv.read(tx_seq, key)` — returns the strictly-
///      earlier in-block writer's value if any.
///   2. Fallback to disk via `get_at_height(db, key, target_height)`
///      when the MvMemory returned `Storage` (no in-block write).
///
/// In both cases, records `(key, Version)` in this tx's ConflictTracker
/// so the scheduler can later detect invalidation when an earlier-seq
/// tx writes the key.
///
/// Determinism: pure function of (block_stm state, db state,
/// target_height, tx_seq, key). Byte-identical across replicas.
pub fn handle_conflict_read<T: KeyValueStoreLike>(
    db: &T,
    target_height: u32,
    block_stm: &BlockStmCtx,
    tx_seq: u32,
    key: &[u8],
) -> anyhow::Result<Vec<u8>> {
    let (version, mv_value) = block_stm.mv.read(tx_seq, key);
    let value = match mv_value {
        Some(v) => v,
        None => crate::chain_entries::get_at_height(db, key, target_height)?
            .unwrap_or_default(),
    };
    block_stm
        .tracker_for(tx_seq)
        .record_read(key.to_vec(), version);
    Ok(value)
}

/// Block-STM instrumented write on behalf of `tx_seq`. Stages the
/// write at `tx_seq` in MvMemory; the scheduler's merge step later
/// reads the highest-seq value per key into the block's WriteBatch.
///
/// No read-set side effect (writes aren't validated; only reads are).
pub fn handle_conflict_write(
    block_stm: &BlockStmCtx,
    tx_seq: u32,
    key: Vec<u8>,
    value: Vec<u8>,
) {
    block_stm.mv.write(tx_seq, key, value);
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::metashrew::indexer_syscall::Request as IndexerReq;
    use crate::proto::metashrew::{
        BatchGet, ConflictRead, ConflictWrite, IndexerSyscall, IndexerThreadJoin,
        IndexerThreadSpawn, KeyValueFlush, Noop,
    };

    fn encode_syscall(req: IndexerReq, response_ptr: u32, response_max: u32) -> Vec<u8> {
        IndexerSyscall {
            request: Some(req),
            response_ptr,
            response_max,
        }
        .encode_to_vec()
    }

    #[test]
    fn proto_tag_safety_invariant() {
        // Foundational invariant: protobuf field number 0 is forbidden,
        // so the smallest valid encoded protobuf first byte is 0x08.
        // Reserving 0x01 as the magic byte is therefore unambiguous.
        assert!(INDEXER_SYSCALL_MAGIC < 0x08);
    }

    #[test]
    fn legacy_kvflush_classifies_as_legacy() {
        let kvf = KeyValueFlush {
            list: vec![b"k".to_vec(), b"v".to_vec()],
        };
        let bytes = kvf.encode_to_vec();
        assert_eq!(bytes[0], 0x0a); // field 1, wire type 2 (LEN)
        assert_eq!(classify(&bytes), FlushKind::Legacy);
    }

    #[test]
    fn empty_kvflush_classifies_as_legacy() {
        let kvf = KeyValueFlush { list: vec![] };
        let bytes = kvf.encode_to_vec();
        assert!(bytes.is_empty());
        assert_eq!(classify(&bytes), FlushKind::Legacy);
    }

    #[test]
    fn magic_byte_alone_classifies_as_syscall() {
        assert_eq!(classify(&[INDEXER_SYSCALL_MAGIC]), FlushKind::Syscall);
    }

    #[test]
    fn magic_byte_then_proto_classifies_as_syscall() {
        let mut buf = vec![INDEXER_SYSCALL_MAGIC];
        buf.extend(encode_syscall(IndexerReq::Noop(Noop {}), 0, 0));
        assert_eq!(classify(&buf), FlushKind::Syscall);
    }

    #[test]
    fn reserved_dispatch_tags_route_to_legacy_for_now() {
        // 0x00, 0x02..0x07 are reserved. Until assigned, they classify
        // as Legacy and will fail KeyValueFlush::decode — failing loudly
        // is safer than silent accept of an unknown dispatch tag.
        for tag in [0x00u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07] {
            assert_eq!(classify(&[tag]), FlushKind::Legacy);
        }
    }

    #[test]
    fn noop_dispatches_to_no_response() {
        let payload = encode_syscall(IndexerReq::Noop(Noop {}), 0, 0);
        let d = dispatch_indexer_syscall(&payload).unwrap();
        assert_eq!(d.result, SyscallResult::NoResponse);
        assert_eq!(d.response_ptr, 0);
        assert_eq!(d.response_max, 0);
    }

    #[test]
    fn empty_syscall_proto_dispatches_to_no_response() {
        // 0-byte payload decodes as IndexerSyscall with request: None.
        let d = dispatch_indexer_syscall(&[]).unwrap();
        assert_eq!(d.result, SyscallResult::NoResponse);
    }

    #[test]
    fn garbage_payload_is_decode_error() {
        // 0xff bytes use field 31 with wire type 7 (invalid).
        let garbage = vec![0xff, 0xff, 0xff];
        assert_eq!(dispatch_indexer_syscall(&garbage), Err(SyscallDecodeError));
    }

    #[test]
    fn batch_get_routes_to_needs_storage_variant() {
        let req = BatchGet {
            keys: vec![b"key1".to_vec(), b"key2".to_vec()],
        };
        let payload = encode_syscall(
            IndexerReq::BatchGet(req.clone()),
            0xdeadbeef,
            1024,
        );
        let d = dispatch_indexer_syscall(&payload).unwrap();
        assert_eq!(d.response_ptr, 0xdeadbeef);
        assert_eq!(d.response_max, 1024);
        match d.result {
            SyscallResult::NeedsBatchGet(got) => {
                assert_eq!(got.keys, req.keys);
            }
            other => panic!("expected NeedsBatchGet, got {:?}", other),
        }
    }

    #[test]
    fn conflict_read_routes_to_needs_conflict_read() {
        let req = ConflictRead { key: b"some-key".to_vec() };
        let payload = encode_syscall(IndexerReq::ConflictRead(req.clone()), 1024, 4096);
        let d = dispatch_indexer_syscall(&payload).unwrap();
        assert_eq!(d.response_ptr, 1024);
        match d.result {
            SyscallResult::NeedsConflictRead(got) => assert_eq!(got.key, req.key),
            other => panic!("expected NeedsConflictRead, got {:?}", other),
        }
    }

    #[test]
    fn conflict_write_routes_to_needs_conflict_write() {
        let req = ConflictWrite {
            key: b"k".to_vec(),
            value: b"v".to_vec(),
        };
        let payload = encode_syscall(IndexerReq::ConflictWrite(req.clone()), 0, 0);
        let d = dispatch_indexer_syscall(&payload).unwrap();
        match d.result {
            SyscallResult::NeedsConflictWrite(got) => {
                assert_eq!(got.key, req.key);
                assert_eq!(got.value, req.value);
            }
            other => panic!("expected NeedsConflictWrite, got {:?}", other),
        }
    }

    #[test]
    fn thread_spawn_routes_to_needs_indexer_thread_spawn() {
        let req = IndexerThreadSpawn { fn_idx: 7, arg: 42 };
        let payload = encode_syscall(IndexerReq::ThreadSpawn(req), 0, 4);
        let d = dispatch_indexer_syscall(&payload).unwrap();
        match d.result {
            SyscallResult::NeedsIndexerThreadSpawn(got) => {
                assert_eq!(got.fn_idx, 7);
                assert_eq!(got.arg, 42);
            }
            other => panic!("expected NeedsIndexerThreadSpawn, got {:?}", other),
        }
    }

    #[test]
    fn thread_join_routes_to_needs_indexer_thread_join() {
        let req = IndexerThreadJoin { thread_id: 999 };
        let payload = encode_syscall(IndexerReq::ThreadJoin(req), 0, 4);
        let d = dispatch_indexer_syscall(&payload).unwrap();
        match d.result {
            SyscallResult::NeedsIndexerThreadJoin(got) => {
                assert_eq!(got.thread_id, 999);
            }
            other => panic!("expected NeedsIndexerThreadJoin, got {:?}", other),
        }
    }

    #[test]
    fn batch_get_with_no_keys_round_trips() {
        let req = BatchGet { keys: vec![] };
        let payload = encode_syscall(IndexerReq::BatchGet(req), 100, 100);
        let d = dispatch_indexer_syscall(&payload).unwrap();
        match d.result {
            SyscallResult::NeedsBatchGet(got) => assert!(got.keys.is_empty()),
            other => panic!("expected NeedsBatchGet, got {:?}", other),
        }
    }
}

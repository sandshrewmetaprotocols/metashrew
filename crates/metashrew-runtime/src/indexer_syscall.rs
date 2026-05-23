//! v10 indexer-mode syscall dispatcher (host-side logic, separated
//! from wasmtime memory I/O so it's directly unit-testable).
//!
//! Unlike `view_syscall.rs` (perf-only on the view path), the indexer
//! syscalls are **consensus-critical**: opcodes that touch state must
//! produce deterministic results across every replica indexing the
//! same block. The dispatcher itself is opcode-agnostic; per-opcode
//! determinism guarantees live in their respective handlers (added in
//! subsequent commits — see the proto for the opcode-vs-commit map).
//!
//! Dispatch: byte-peek on buffer[0]. See `metashrew.proto`'s
//! IndexerSyscall doc-comment for the rationale.

use crate::proto::metashrew::indexer_syscall::Request as IndexerReq;
use crate::proto::metashrew::IndexerSyscall;
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

/// Result of dispatching one indexer syscall.
#[derive(Debug, PartialEq, Eq)]
pub enum SyscallResult {
    /// The payload didn't decode as a valid `IndexerSyscall`. Indicates
    /// either a wasm bug (set the magic byte but emitted garbage) or
    /// a wire-format incompatibility (host older than the wasm).
    /// Caller should set `had_failure` and abort the block.
    DecodeFailure,
    /// No response payload needed. Caller does not write to the wasm
    /// response buffer.
    NoResponse,
    /// Write these bytes to the wasm response buffer at
    /// `response_ptr` as `[u32 LE: len | bytes]`.
    Respond(Vec<u8>),
    /// Op exists in the proto but isn't wired in this build. Caller
    /// should write a `len=0` marker so wasm sees a deterministic
    /// not-supported response.
    UnsupportedOp,
}

/// Decode + dispatch the syscall payload. `payload` is the bytes
/// AFTER the magic byte (caller has already classified and stripped).
///
/// Opcodes are added in subsequent commits; for now only `Noop` is
/// wired so the dispatch path is end-to-end testable.
pub fn dispatch_indexer_syscall(payload: &[u8]) -> SyscallResult {
    let syscall = match IndexerSyscall::decode(payload) {
        Ok(s) => s,
        Err(_) => return SyscallResult::DecodeFailure,
    };

    match syscall.request {
        Some(IndexerReq::Noop(_)) => SyscallResult::NoResponse,
        None => SyscallResult::NoResponse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::metashrew::indexer_syscall::Request as IndexerReq;
    use crate::proto::metashrew::{IndexerSyscall, KeyValueFlush, Noop};

    #[test]
    fn proto_tag_safety_invariant() {
        // Foundational invariant: protobuf field number 0 is forbidden,
        // so the smallest valid encoded protobuf first byte is 0x08
        // (field 1, wire type 0). Reserving 0x01 as the magic byte is
        // therefore unambiguous — no valid protobuf can start with it.
        assert!(INDEXER_SYSCALL_MAGIC < 0x08);
    }

    #[test]
    fn legacy_kvflush_classifies_as_legacy() {
        let kvf = KeyValueFlush {
            list: vec![b"k".to_vec(), b"v".to_vec()],
        };
        let bytes = kvf.encode_to_vec();
        // First byte should be 0x0a (field 1, wire type 2 = LEN).
        assert_eq!(bytes[0], 0x0a);
        assert_eq!(classify(&bytes), FlushKind::Legacy);
    }

    #[test]
    fn empty_kvflush_classifies_as_legacy() {
        // An empty KeyValueFlush encodes to 0 bytes. Empty buffer must
        // route to the legacy path so wasm that emits a no-op flush
        // sees no change.
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
        let s = IndexerSyscall {
            request: Some(IndexerReq::Noop(Noop {})),
            response_ptr: 0,
            response_max: 0,
        };
        let mut buf = vec![INDEXER_SYSCALL_MAGIC];
        buf.extend(s.encode_to_vec());
        assert_eq!(classify(&buf), FlushKind::Syscall);
    }

    #[test]
    fn reserved_dispatch_tags_route_to_legacy_for_now() {
        // 0x00, 0x02..0x07 are reserved but not yet assigned. Until
        // they are, the classifier routes them to Legacy (which will
        // then fail KeyValueFlush::decode and set had_failure). That's
        // the safe default — better to fail loudly than to silently
        // accept an unknown dispatch tag.
        for tag in [0x00u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07] {
            assert_eq!(classify(&[tag]), FlushKind::Legacy);
        }
    }

    #[test]
    fn noop_dispatches_to_no_response() {
        let s = IndexerSyscall {
            request: Some(IndexerReq::Noop(Noop {})),
            response_ptr: 0,
            response_max: 0,
        };
        assert_eq!(
            dispatch_indexer_syscall(&s.encode_to_vec()),
            SyscallResult::NoResponse,
        );
    }

    #[test]
    fn empty_syscall_proto_dispatches_to_no_response() {
        // 0-byte payload decodes as IndexerSyscall with request: None.
        assert_eq!(
            dispatch_indexer_syscall(&[]),
            SyscallResult::NoResponse,
        );
    }

    #[test]
    fn garbage_payload_is_decode_failure() {
        // 0xff bytes use field number 31 with wire type 7 (invalid).
        let garbage = vec![0xff, 0xff, 0xff];
        assert_eq!(
            dispatch_indexer_syscall(&garbage),
            SyscallResult::DecodeFailure,
        );
    }

    #[test]
    fn kvflush_payload_misrouted_to_syscall_dispatcher_is_decode_failure_or_norequest() {
        // Belt-and-suspenders: even if a KeyValueFlush were somehow
        // routed here, the dispatcher should not silently accept it.
        // Proto3 forgives unknown fields, so it likely decodes to an
        // IndexerSyscall with request: None — which we return as
        // NoResponse. That's still safe (no state change), and the
        // classifier prevents this from happening in practice.
        let kvf = KeyValueFlush {
            list: vec![b"k".to_vec(), b"v".to_vec()],
        };
        let result = dispatch_indexer_syscall(&kvf.encode_to_vec());
        assert!(
            matches!(result, SyscallResult::NoResponse | SyscallResult::DecodeFailure),
            "misrouted KVFlush must not be silently accepted as a real opcode; got {:?}",
            result
        );
    }
}

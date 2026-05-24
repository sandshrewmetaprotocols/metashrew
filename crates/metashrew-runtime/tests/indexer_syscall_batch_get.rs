//! End-to-end test for the v10 indexer-side `BatchGet` syscall opcode.
//!
//! Pins the contract that `handle_batch_get` returns the same values
//! a sequence of N `__get` calls would return, in the same order,
//! with a `present` flag that distinguishes "missing key" from
//! "present key with empty value".

use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::chain_entries::append_value_to_batch;
use metashrew_runtime::indexer_syscall::handle_batch_get;
use metashrew_runtime::proto::metashrew::{BatchGet, BatchGetResponse};
use metashrew_runtime::KeyValueStoreLike;
use prost::Message;

fn put(db: &mut MemStoreAdapter, key: &[u8], value: &[u8], height: u32) {
    let mut batch = db.create_batch();
    append_value_to_batch(db, &mut batch, key, value, height).expect("append_value_to_batch");
    db.write(batch).expect("write batch");
}

fn decode_response(bytes: &[u8]) -> BatchGetResponse {
    BatchGetResponse::decode(bytes).expect("BatchGetResponse decodes")
}

#[test]
fn empty_keys_returns_empty_entries() {
    let db = MemStoreAdapter::new();
    let req = BatchGet { keys: vec![] };
    let bytes = handle_batch_get(&db, 100, &req).expect("handle_batch_get");
    let resp = decode_response(&bytes);
    assert!(resp.entries.is_empty());
}

#[test]
fn single_key_present_returns_value_with_present_flag() {
    let mut db = MemStoreAdapter::new();
    put(&mut db, b"/alkanes/totalsupply/2-1", b"\xff\xff\xff\xff", 100);

    let req = BatchGet {
        keys: vec![b"/alkanes/totalsupply/2-1".to_vec()],
    };
    let bytes = handle_batch_get(&db, 100, &req).expect("handle_batch_get");
    let resp = decode_response(&bytes);

    assert_eq!(resp.entries.len(), 1);
    assert!(resp.entries[0].present, "key was written, must be present");
    assert_eq!(resp.entries[0].value, b"\xff\xff\xff\xff");
}

#[test]
fn missing_key_returns_present_false_with_empty_value() {
    let db = MemStoreAdapter::new();
    let req = BatchGet {
        keys: vec![b"never-written".to_vec()],
    };
    let bytes = handle_batch_get(&db, 100, &req).expect("handle_batch_get");
    let resp = decode_response(&bytes);

    assert_eq!(resp.entries.len(), 1);
    assert!(!resp.entries[0].present, "key was never written, must be miss");
    assert!(resp.entries[0].value.is_empty());
}

#[test]
fn mixed_present_and_missing_keys_preserve_request_order() {
    let mut db = MemStoreAdapter::new();
    put(&mut db, b"key-a", b"value-a", 50);
    put(&mut db, b"key-c", b"value-c", 60);
    // key-b intentionally never written.

    let req = BatchGet {
        keys: vec![
            b"key-a".to_vec(),
            b"key-b".to_vec(),
            b"key-c".to_vec(),
        ],
    };
    let bytes = handle_batch_get(&db, 100, &req).expect("handle_batch_get");
    let resp = decode_response(&bytes);

    assert_eq!(resp.entries.len(), 3);
    assert!(resp.entries[0].present);
    assert_eq!(resp.entries[0].value, b"value-a");
    assert!(!resp.entries[1].present);
    assert!(resp.entries[1].value.is_empty());
    assert!(resp.entries[2].present);
    assert_eq!(resp.entries[2].value, b"value-c");
}

#[test]
fn read_at_target_height_respects_version_chain() {
    let mut db = MemStoreAdapter::new();
    put(&mut db, b"k", b"v1", 100);
    put(&mut db, b"k", b"v2", 200);

    // Read at h=150 (between writes) sees v1.
    let req = BatchGet { keys: vec![b"k".to_vec()] };
    let bytes = handle_batch_get(&db, 150, &req).expect("handle_batch_get");
    let resp = decode_response(&bytes);
    assert!(resp.entries[0].present);
    assert_eq!(resp.entries[0].value, b"v1", "h=150 must see v1");

    // Read at h=250 (after both writes) sees v2.
    let bytes = handle_batch_get(&db, 250, &req).expect("handle_batch_get");
    let resp = decode_response(&bytes);
    assert!(resp.entries[0].present);
    assert_eq!(resp.entries[0].value, b"v2", "h=250 must see v2");

    // Read at h=50 (before first write) is a miss.
    let bytes = handle_batch_get(&db, 50, &req).expect("handle_batch_get");
    let resp = decode_response(&bytes);
    assert!(!resp.entries[0].present, "h=50 is before any write");
}

#[test]
fn present_empty_value_distinguishes_from_missing() {
    // Storage values can legitimately be empty bytes. The `present`
    // flag must distinguish that from a key-not-found result.
    let mut db = MemStoreAdapter::new();
    put(&mut db, b"empty-key", b"", 100);
    put(&mut db, b"nonempty-key", b"hello", 100);

    let req = BatchGet {
        keys: vec![
            b"empty-key".to_vec(),
            b"nonempty-key".to_vec(),
            b"missing-key".to_vec(),
        ],
    };
    let bytes = handle_batch_get(&db, 100, &req).expect("handle_batch_get");
    let resp = decode_response(&bytes);

    assert!(resp.entries[0].present, "empty-but-present must be present=true");
    assert!(resp.entries[0].value.is_empty());

    assert!(resp.entries[1].present);
    assert_eq!(resp.entries[1].value, b"hello");

    assert!(!resp.entries[2].present);
    assert!(resp.entries[2].value.is_empty());
}

#[test]
fn duplicate_keys_each_get_their_own_entry() {
    // Wasm can pre-flight the same key twice; each request slot gets
    // its own response slot (parallel ordering invariant).
    let mut db = MemStoreAdapter::new();
    put(&mut db, b"dupe-key", b"v", 100);

    let req = BatchGet {
        keys: vec![
            b"dupe-key".to_vec(),
            b"dupe-key".to_vec(),
            b"dupe-key".to_vec(),
        ],
    };
    let bytes = handle_batch_get(&db, 100, &req).expect("handle_batch_get");
    let resp = decode_response(&bytes);

    assert_eq!(resp.entries.len(), 3);
    for entry in &resp.entries {
        assert!(entry.present);
        assert_eq!(entry.value, b"v");
    }
}

#[test]
fn deterministic_for_same_inputs() {
    // Two calls with the same (db_state, height, keys) must produce
    // byte-identical responses — this is the consensus contract.
    let mut db = MemStoreAdapter::new();
    put(&mut db, b"a", b"1", 10);
    put(&mut db, b"b", b"22", 20);
    put(&mut db, b"c", b"333", 30);

    let req = BatchGet {
        keys: vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()],
    };

    let first = handle_batch_get(&db, 100, &req).expect("first");
    let second = handle_batch_get(&db, 100, &req).expect("second");
    assert_eq!(first, second, "BatchGet must be deterministic");
}

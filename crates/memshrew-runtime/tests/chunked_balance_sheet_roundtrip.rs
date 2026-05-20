//! v10 chain-entry roundtrip tests for chunked payloads.
//!
//! Pins the contract that the v10 append-only chain format
//! (`{key}/length` + `{key}/{i}` with `[u32 LE height | value]` payloads)
//! roundtrips arbitrary binary chunks through the in-memory adapter,
//! including the historical-isolation semantics that view RPCs depend on.
//!
//! Things this test pins:
//! 1. **Roundtrip at the write height** — a put at H followed by a get at H
//!    returns the same bytes.
//! 2. **Historical isolation** — reads at heights between two writes return
//!    the older value; reads past the second write return the newer value.
//! 3. **Raw on-disk format** — the `{key}/{i}` value decodes via
//!    [`decode_value_entry`] (i.e. v10 binary `[u32 LE height | value]`,
//!    not v9 `"height:hex"` ASCII). A regression here would silently break
//!    consensus on a fresh sync.
//! 4. **Length key format** — the length key is u32 LE, not ASCII decimal.

use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::chain_entries::{
    append_value_to_batch, decode_chain_length, decode_value_entry, get_at_height,
};
use metashrew_runtime::{to_labeled_key, KeyValueStoreLike};

/// Stage a put against `db` using the v10 append-only primitive and commit
/// the resulting batch.
fn put(db: &mut MemStoreAdapter, key: &[u8], value: &[u8], height: u32) {
    let mut batch = db.create_batch();
    append_value_to_batch(db, &mut batch, key, value, height).expect("append_value_to_batch");
    db.write(batch).expect("write batch");
}

#[test]
fn chunk_roundtrip_at_write_height() {
    let mut db = MemStoreAdapter::new();
    let key = b"/runes/byoutpoint/abcd:0";
    let chunk = b"chunked-balance-sheet-protobuf-payload";

    put(&mut db, key, chunk, 100);

    let read = get_at_height(&db, key, 100)
        .expect("get at h=100")
        .expect("entry exists at h=100");
    assert_eq!(read.as_slice(), chunk, "roundtrip at write height");
}

#[test]
fn chunk_historical_isolation() {
    let mut db = MemStoreAdapter::new();
    let key = b"/runes/byoutpoint/cafe:1";
    let chunk_v1 = b"chunk-version-1";
    let chunk_v2 = b"chunk-version-2-with-additional-balances";

    put(&mut db, key, chunk_v1, 100);
    put(&mut db, key, chunk_v2, 110);

    // Read before any write → None.
    assert!(
        get_at_height(&db, key, 99).expect("get at h=99").is_none(),
        "no chunk before h=100"
    );

    // Read at write height → exact match.
    assert_eq!(
        get_at_height(&db, key, 100)
            .expect("get at h=100")
            .expect("v1 entry"),
        chunk_v1
    );

    // Read between writes → still sees v1.
    assert_eq!(
        get_at_height(&db, key, 105)
            .expect("get at h=105")
            .expect("v1 entry between writes"),
        chunk_v1
    );

    // Read at exact second write → sees v2.
    assert_eq!(
        get_at_height(&db, key, 110)
            .expect("get at h=110")
            .expect("v2 entry"),
        chunk_v2
    );

    // Read past second write → still v2.
    assert_eq!(
        get_at_height(&db, key, 1000)
            .expect("get at h=1000")
            .expect("v2 entry future"),
        chunk_v2
    );
}

#[test]
fn raw_storage_is_v10_binary_format() {
    let mut db = MemStoreAdapter::new();
    let key = b"/scan/v10-format";
    let chunk = b"raw-binary-payload-with-\xff\x00-non-utf8-bytes";

    put(&mut db, key, chunk, 4242);

    // Pull the raw entry at index 0 directly out of the store and assert it
    // decodes as [u32 LE height | value]. If a v9 string format ever crept
    // back in, this would fail.
    let raw_entry = db
        .get(&to_labeled_key(&[&b"/scan/v10-format/0"[..]].concat()))
        .expect("storage get")
        .expect("entry at index 0 exists");
    let (decoded_height, decoded_value) = decode_value_entry(&raw_entry).expect("v10 decode");
    assert_eq!(decoded_height, 4242, "height header");
    assert_eq!(decoded_value, chunk, "value tail");

    // Length key is u32 LE.
    let raw_length = db
        .get(&to_labeled_key(&[&b"/scan/v10-format/length"[..]].concat()))
        .expect("storage get length")
        .expect("length key exists");
    assert_eq!(decode_chain_length(&raw_length), 1, "length is u32 LE");
    assert_eq!(raw_length.len(), 4, "length key is exactly 4 bytes");
}

#[test]
fn chunk_overwrite_grows_chain_length_monotonically() {
    let mut db = MemStoreAdapter::new();
    let key = b"/chain/length-grows";

    for h in [10u32, 20, 30, 40, 50] {
        let v = format!("payload-{}", h);
        put(&mut db, key, v.as_bytes(), h);
    }

    // The length counter should advance to 5.
    let length_key = {
        let mut k = key.to_vec();
        k.extend_from_slice(b"/length");
        to_labeled_key(&k)
    };
    let raw_length = db
        .get(&length_key)
        .expect("storage get length")
        .expect("length key exists");
    assert_eq!(decode_chain_length(&raw_length), 5);

    // Each historical read should match the value written at that height.
    for h in [10u32, 20, 30, 40, 50] {
        let v = get_at_height(&db, key, h)
            .expect("historical read")
            .expect("entry exists");
        assert_eq!(v, format!("payload-{}", h).as_bytes());
    }
}

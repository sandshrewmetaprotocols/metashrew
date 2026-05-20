//! v10 chunk API + binary-entry-format roundtrip tests.
//!
//! These exercise the underlying [`SMTHelper`] storage layer that backs the
//! new `KeyValuePointer::set_chunk` / `get_chunk` ergonomic API.
//! `set_chunk` is storage-semantically identical to `set` — it writes one
//! versioned entry per call — so what we verify here is the **chain
//! semantics** the chunk API relies on:
//!
//! 1. **Roundtrip at write height** — write a chunk at H, read it back at H.
//! 2. **Historical isolation** — re-write at H+10, read at H+5 gets the old
//!    chunk, read at H+15 gets the new chunk. (This is the property that
//!    makes collapsing N scalar writes into one chunk write actually safe:
//!    the version chain still gives us reorg-safe historical reads.)
//! 3. **Binary entry format** — assert the raw bytes stored at
//!    `{key}/{i}` decode via [`decode_value_entry`], i.e. the v10 format is
//!    actually `[u32 LE height | value]` and not the v9 `"height:hex"`
//!    string. A regression here would silently break consensus on a fresh
//!    sync (the indexer reads its own writes, so a bad format would
//!    *roundtrip* correctly but not byte-equal another v10 indexer).
//! 4. **Length key format** — the length key is u32 LE, not ASCII decimal.

use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::smt::{decode_chain_length, decode_value_entry, SMTHelper};
use metashrew_runtime::{to_labeled_key, KeyValueStoreLike};

/// Build a fresh SMT helper backed by an empty in-memory store.
fn fresh_helper() -> SMTHelper<MemStoreAdapter> {
    SMTHelper::new(MemStoreAdapter::new())
}

#[test]
fn chunk_roundtrip_at_write_height() {
    let mut smt = fresh_helper();
    let key = b"/runes/byoutpoint/abcd:0";
    let chunk = b"chunked-balance-sheet-protobuf-payload";

    smt.put(key, chunk, 100).expect("put at h=100");

    let read = smt
        .get_at_height(key, 100)
        .expect("get at h=100")
        .expect("entry exists at h=100");
    assert_eq!(read.as_slice(), chunk, "roundtrip at write height");
}

#[test]
fn chunk_historical_isolation() {
    let mut smt = fresh_helper();
    let key = b"/runes/byoutpoint/cafe:1";
    let chunk_v1 = b"chunk-version-1";
    let chunk_v2 = b"chunk-version-2-with-additional-balances";

    smt.put(key, chunk_v1, 100).expect("put v1 at h=100");
    smt.put(key, chunk_v2, 110).expect("put v2 at h=110");

    // Read before any write → None.
    assert!(
        smt.get_at_height(key, 99).expect("get at h=99").is_none(),
        "no chunk before h=100"
    );

    // Read at write height → exact match.
    assert_eq!(
        smt.get_at_height(key, 100)
            .expect("get at h=100")
            .expect("v1 entry"),
        chunk_v1
    );

    // Read between writes → still sees v1.
    assert_eq!(
        smt.get_at_height(key, 105)
            .expect("get at h=105")
            .expect("v1 entry between writes"),
        chunk_v1
    );

    // Read at exact second write → sees v2.
    assert_eq!(
        smt.get_at_height(key, 110)
            .expect("get at h=110")
            .expect("v2 entry"),
        chunk_v2
    );

    // Read past second write → still v2.
    assert_eq!(
        smt.get_at_height(key, 1000)
            .expect("get at h=1000")
            .expect("v2 entry future"),
        chunk_v2
    );

    // get_current always returns the most-recent.
    assert_eq!(
        smt.get_current(key)
            .expect("get_current")
            .expect("v2 current"),
        chunk_v2
    );
}

#[test]
fn raw_storage_is_v10_binary_format() {
    let mut smt = fresh_helper();
    let key = b"/scan/v10-format";
    let chunk = b"raw-binary-payload-with-\xff\x00-non-utf8-bytes";

    smt.put(key, chunk, 4242).expect("put chunk");

    // Pull the raw entry at index 0 directly out of the store and assert it
    // decodes as [u32 LE height | value]. If a v9 string format ever crept
    // back in, this would fail.
    let raw_entry = smt
        .storage
        .get(&to_labeled_key(&[&b"/scan/v10-format/0"[..]].concat()))
        .expect("storage get")
        .expect("entry at index 0 exists");
    let (decoded_height, decoded_value) = decode_value_entry(&raw_entry).expect("v10 decode");
    assert_eq!(decoded_height, 4242, "height header");
    assert_eq!(decoded_value, chunk, "value tail");

    // Length key is u32 LE.
    let raw_length = smt
        .storage
        .get(&to_labeled_key(&[&b"/scan/v10-format/length"[..]].concat()))
        .expect("storage get length")
        .expect("length key exists");
    assert_eq!(decode_chain_length(&raw_length), 1, "length is u32 LE");
    assert_eq!(raw_length.len(), 4, "length key is exactly 4 bytes");

}

#[test]
fn chunk_overwrite_grows_chain_length_monotonically() {
    let mut smt = fresh_helper();
    let key = b"/chain/length-grows";

    for h in [10u32, 20, 30, 40, 50] {
        let v = format!("payload-{}", h);
        smt.put(key, v.as_bytes(), h).expect("put");
    }

    // The chain should have 5 entries.
    let heights = smt.get_heights_for_key(key).expect("get heights");
    assert_eq!(heights, vec![10, 20, 30, 40, 50]);

    // Each historical read should match the value written at that height.
    for h in [10u32, 20, 30, 40, 50] {
        let v = smt
            .get_at_height(key, h)
            .expect("historical read")
            .expect("entry exists");
        assert_eq!(v, format!("payload-{}", h).as_bytes());
    }
}

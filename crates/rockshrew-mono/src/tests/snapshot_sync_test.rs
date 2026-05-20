// crates/rockshrew-mono/src/tests/snapshot_sync_test.rs
//
// Integration test for snapshot-style raw RocksDB copy synchronization.
//
// Two RocksDB instances:
//   1. A "snapshot" instance, populated with v10 chain entries via the
//      append-only primitives in `metashrew_runtime::chain_entries`.
//   2. A "repo" instance, hydrated by directly copying every key/value
//      from the snapshot DB.
//
// After the copy, the chain-level reader (`get_at_height`) must return
// identical values from both DBs — that's the contract a snapshot sync
// needs to honor.

use metashrew_runtime::{
    chain_entries::{append_value_to_batch, get_at_height},
    traits::KeyValueStoreLike,
};
use rockshrew_runtime::adapter::RocksDBRuntimeAdapter;
use tempfile::tempdir;

#[tokio::test]
async fn test_snapshot_sync_basic() {
    // 1. Setup snapshot instance and populate it with v10 chain entries.
    let snapshot_dir = tempdir().unwrap();
    let mut snapshot_db = RocksDBRuntimeAdapter::open_optimized(
        snapshot_dir.path().to_str().unwrap().to_string(),
    )
    .unwrap();

    for i in 0..10u32 {
        let mut batch = snapshot_db.create_batch();
        append_value_to_batch(
            &snapshot_db,
            &mut batch,
            &i.to_be_bytes(),
            &[i as u8; 32],
            i,
        )
        .unwrap();
        snapshot_db.write(batch).unwrap();
    }

    // 2. Create a "snapshot" by copying all raw key-value pairs from the DB.
    let iter = snapshot_db.db.iterator(rocksdb::IteratorMode::Start);
    let snapshot_data: Vec<(Box<[u8]>, Box<[u8]>)> =
        iter.map(|item| item.unwrap()).collect();

    // 3. Setup a new, empty repo instance.
    let repo_dir = tempdir().unwrap();
    let mut repo_db = RocksDBRuntimeAdapter::open_optimized(
        repo_dir.path().to_str().unwrap().to_string(),
    )
    .unwrap();

    // 4. "Sync" the repo instance by applying the snapshot data.
    for (key, value) in snapshot_data {
        repo_db.put(&key, &value).unwrap();
    }

    // 5. Verify the repo instance's chain entries match the original.
    for i in 0..10u32 {
        let snap_val = get_at_height(&snapshot_db, &i.to_be_bytes(), i).unwrap();
        let repo_val = get_at_height(&repo_db, &i.to_be_bytes(), i).unwrap();
        assert_eq!(snap_val, repo_val, "chain value mismatch at height {}", i);
        assert_eq!(snap_val, Some(vec![i as u8; 32]));
    }
}

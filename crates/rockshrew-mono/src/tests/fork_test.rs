use crate::Args;
use metashrew_runtime::{KeyValueStoreLike, MetashrewRuntime};
use rockshrew_runtime::RocksDBRuntimeAdapter;
use std::path::PathBuf;
use tempfile::tempdir;

#[tokio::test]
async fn test_forking_logic() {
    let temp_dir = tempdir().unwrap();
    let primary_db_path = temp_dir.path().join("primary");
    let fork_db_path = temp_dir.path().join("fork");

    // 1. Create and populate the primary database
    {
        let mut primary_db =
            RocksDBRuntimeAdapter::open_optimized(primary_db_path.to_str().unwrap().to_string())
                .unwrap();
        primary_db.put(b"key1", b"value1").unwrap();
        primary_db.put(b"key2", b"value2").unwrap();
    }

    // 2. Create the forked database
    let args = Args {
        daemon_rpc_url: "".to_string(),
        indexer: PathBuf::from("./src/tests/empty.wasm"),
        db_path: fork_db_path.clone(),
        fork: Some(primary_db_path.clone()),
        start_block: None,
        auth: None,
        host: "".to_string(),
        port: 0,
        label: None,
        exit_at: None,
        pipeline_size: None,
        cors: None,
        snapshot_directory: None,
        snapshot_interval: 0,
        repo: None,
        max_reorg_depth: 0,
        reorg_check_threshold: 0,
    };

    let runtime = if let Some(fork_path) = args.fork.clone() {
        let db_path = args.db_path.to_string_lossy().to_string();
        let fork_path = fork_path.to_string_lossy().to_string();
        let opts = RocksDBRuntimeAdapter::get_optimized_options();
        let adapter = RocksDBRuntimeAdapter::open_fork(db_path, fork_path, opts).unwrap();
        MetashrewRuntime::load(args.indexer.clone(), adapter).unwrap()
    } else {
        MetashrewRuntime::load(
            args.indexer.clone(),
            RocksDBRuntimeAdapter::open_optimized(args.db_path.to_string_lossy().to_string())
                .unwrap(),
        )
        .unwrap()
    };

    let mut adapter = runtime.context.lock().unwrap().db.clone();

    // 3. Read data from both databases
    assert_eq!(adapter.get(b"key1").unwrap(), Some(b"value1".to_vec()));
    assert_eq!(adapter.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    assert_eq!(adapter.get(b"key3").unwrap(), None);

    // 4. Write new data to the forked database
    adapter.put(b"key3", b"value3").unwrap();
    adapter.put(b"key1", b"new_value1").unwrap();

    // 5. Assert data is read correctly
    assert_eq!(adapter.get(b"key1").unwrap(), Some(b"new_value1".to_vec()));
    assert_eq!(adapter.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    assert_eq!(adapter.get(b"key3").unwrap(), Some(b"value3".to_vec()));
}
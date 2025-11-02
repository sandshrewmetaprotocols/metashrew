use tempfile::tempdir;
use metashrew_sync::{SnapshotMetashrewSync, SyncConfig, SyncMode, MockBitcoinNode, MockRuntime, StorageAdapter};
use rockshrew_runtime::RocksDBStorageAdapter;
use std::sync::Arc;
use rocksdb::DB;

#[tokio::test]
async fn test_start_block_logic() {
    let db_path = tempdir().unwrap();
    let db = DB::open_default(db_path.path()).unwrap();
    let mut storage_adapter = RocksDBStorageAdapter::new(Arc::new(db));

    // 1. Test with existing database
    {
        storage_adapter.set_indexed_height(100).await.unwrap();

        // a. No start_block provided
        let config = SyncConfig { start_block: 0, ..Default::default() };
        let sync_engine = SnapshotMetashrewSync::new(
            MockBitcoinNode::new(),
            storage_adapter.clone(),
            MockRuntime::new(),
            config,
            SyncMode::Normal,
        );
        sync_engine.init().await;
        assert_eq!(sync_engine.current_height(), 101);

        // b. start_block provided, but lower than indexed_height
        let config = SyncConfig { start_block: 50, ..Default::default() };
        let sync_engine = SnapshotMetashrewSync::new(
            MockBitcoinNode::new(),
            storage_adapter.clone(),
            MockRuntime::new(),
            config,
            SyncMode::Normal,
        );
        sync_engine.init().await;
        assert_eq!(sync_engine.current_height(), 101);

        // c. start_block provided, and higher than indexed_height
        let config = SyncConfig { start_block: 200, ..Default::default() };
        let sync_engine = SnapshotMetashrewSync::new(
            MockBitcoinNode::new(),
            storage_adapter.clone(),
            MockRuntime::new(),
            config,
            SyncMode::Normal,
        );
        sync_engine.init().await;
        assert_eq!(sync_engine.current_height(), 200);
    }

    // 2. Test with fresh database
    let db_path_2 = tempdir().unwrap();
    let db2 = DB::open_default(db_path_2.path()).unwrap();
    let storage_adapter_2 = RocksDBStorageAdapter::new(Arc::new(db2));
    {
        // a. No start_block provided
        let config = SyncConfig { start_block: 0, ..Default::default() };
        let sync_engine = SnapshotMetashrewSync::new(
            MockBitcoinNode::new(),
            storage_adapter_2.clone(),
            MockRuntime::new(),
            config,
            SyncMode::Normal,
        );
        sync_engine.init().await;
        assert_eq!(sync_engine.current_height(), 0);

        // b. start_block provided
        let config = SyncConfig { start_block: 300, ..Default::default() };
        let sync_engine = SnapshotMetashrewSync::new(
            MockBitcoinNode::new(),
            storage_adapter_2.clone(),
            MockRuntime::new(),
            config,
            SyncMode::Normal,
        );
        sync_engine.init().await;
        assert_eq!(sync_engine.current_height(), 300);
    }
}

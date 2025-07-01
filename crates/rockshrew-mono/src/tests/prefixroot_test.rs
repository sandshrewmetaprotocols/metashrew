// crates/rockshrew-mono/src/tests/prefixroot_test.rs

/*
 * Chadson v69.0.0
 *
 * This file contains the in-memory tests for the --prefixroot functionality.
 *
 * Purpose:
 * - To test the `metashrew_prefixroot` JSON-RPC method in a mocked environment.
 */

use metashrew_sync::{
    mock::{MockBitcoinNode, MockRuntime, MockStorage},
    JsonRpcProvider, SnapshotMetashrewSync, SyncConfig, SyncMode,
};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn test_prefixroot_in_memory() {
    // 1. Setup mocked components
    let node = MockBitcoinNode::new();
    let storage = MockStorage::new();
    let runtime = MockRuntime::new();

    // 2. Pre-populate the mock runtime with a known prefix root
    let prefix_name = "test_prefix".to_string();
    let expected_root = [1; 32];
    runtime
        .prefix_roots
        .lock()
        .await
        .insert(prefix_name.clone(), expected_root.clone());

    // 3. Create the sync engine instance with the mocks
    let sync_engine = SnapshotMetashrewSync::new(
        node,
        storage,
        runtime,
        SyncConfig::default(),
        SyncMode::Normal,
    );

    // Set a mock height for the query
    sync_engine.current_height.store(1, Ordering::SeqCst);

    // 4. Directly call the `metashrew_prefixroot` method
    let result = sync_engine
        .metashrew_prefixroot(prefix_name, "0".to_string())
        .await
        .unwrap();

    // 5. Assert that the result matches the expected root
    assert_eq!(result, format!("0x{}", hex::encode(expected_root)));
}
//! Tests for atomic block processing and start block initialization

use anyhow::Result;
use rockshrew_sync::{
    MetashrewSync, MockBitcoinNode, MockRuntime, MockStorage, StorageAdapter, SyncConfig,
    SyncEngine,
};

#[tokio::test]
async fn test_atomic_block_processing() -> Result<()> {
    // Create mock components
    let node = MockBitcoinNode::new();
    let storage = MockStorage::new();
    let runtime = MockRuntime::new();

    // Add some test blocks
    node.add_block(0, vec![0; 32], vec![1, 2, 3, 4]);
    node.add_block(1, vec![1; 32], vec![5, 6, 7, 8]);
    node.add_block(2, vec![2; 32], vec![9, 10, 11, 12]);

    // Create sync config
    let config = SyncConfig {
        start_block: 0,
        exit_at: Some(2),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    // Create sync engine
    let mut sync_engine = MetashrewSync::new(node, storage, runtime, config);

    // Test single block processing (should use atomic processing)
    sync_engine.process_single_block(0).await?;

    // Verify the block was processed
    let storage = sync_engine.storage().read().await;
    assert_eq!(storage.get_indexed_height().await?, 0);
    assert!(storage.get_block_hash(0).await?.is_some());
    assert!(storage.get_state_root(0).await?.is_some());

    println!("✓ Atomic block processing test passed");
    Ok(())
}

#[tokio::test]
async fn test_mock_runtime_atomic_processing() -> Result<()> {
    // Create mock components
    let node = MockBitcoinNode::new();
    let storage = MockStorage::new();
    let runtime = MockRuntime::new();

    // Add test blocks starting from height 100
    for i in 100..105 {
        node.add_block(i, vec![i as u8; 32], vec![i as u8; 4]);
    }

    // Create sync config with non-zero start block
    let config = SyncConfig {
        start_block: 100,
        exit_at: Some(102),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    // Create sync engine
    let mut sync_engine = MetashrewSync::new(node, storage, runtime, config);

    // Test that we can process a single block
    sync_engine.process_single_block(100).await?;

    // Verify the block was processed
    let storage = sync_engine.storage().read().await;
    assert_eq!(storage.get_indexed_height().await?, 100);
    assert!(storage.get_block_hash(100).await?.is_some());
    assert!(storage.get_state_root(100).await?.is_some());

    println!("✓ Mock runtime atomic processing test passed");
    Ok(())
}

#[tokio::test]
async fn test_atomic_processing_fallback() -> Result<()> {
    // Create mock components
    let node = MockBitcoinNode::new();
    let storage = MockStorage::new();
    let runtime = MockRuntime::new();

    // Add test blocks
    node.add_block(0, vec![0; 32], vec![1, 2, 3, 4]);

    // Make runtime not ready to force atomic processing to fail
    runtime.set_ready(false);

    // Create sync config
    let config = SyncConfig {
        start_block: 0,
        exit_at: Some(1),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    // Create sync engine
    let mut sync_engine = MetashrewSync::new(node, storage, runtime, config);

    // This should fail because runtime is not ready
    let result = sync_engine.process_single_block(0).await;
    assert!(
        result.is_err(),
        "Processing should fail when runtime is not ready"
    );

    // Make runtime ready again
    let runtime = sync_engine.runtime().read().await;
    runtime.set_ready(true);
    drop(runtime);

    // Now processing should work (with fallback to non-atomic)
    sync_engine.process_single_block(0).await?;

    // Verify the block was processed
    let storage = sync_engine.storage().read().await;
    assert_eq!(storage.get_indexed_height().await?, 0);

    println!("✓ Atomic processing fallback test passed");
    Ok(())
}

#[tokio::test]
async fn test_state_root_storage() -> Result<()> {
    // Create mock components
    let node = MockBitcoinNode::new();
    let storage = MockStorage::new();
    let runtime = MockRuntime::new();

    // Add test blocks starting from a high height to simulate the production issue
    for i in 880060..880070 {
        node.add_block(i, vec![(i % 256) as u8; 32], vec![(i % 256) as u8; 4]);
    }

    // Create sync config with start block similar to production scenario
    let config = SyncConfig {
        start_block: 880064,
        exit_at: Some(880066),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    // Create sync engine
    let mut sync_engine = MetashrewSync::new(node, storage, runtime, config);

    // Process the start block - this should not fail with "No state root found"
    sync_engine.process_single_block(880064).await?;

    // Verify the block was processed successfully
    let storage = sync_engine.storage().read().await;
    assert_eq!(storage.get_indexed_height().await?, 880064);
    assert!(storage.get_state_root(880064).await?.is_some());

    println!("✓ State root storage test passed");
    Ok(())
}

#[tokio::test]
async fn test_no_duplicate_state_root_initialization() -> Result<()> {
    // Create mock components
    let node = MockBitcoinNode::new();
    let storage = MockStorage::new();
    let runtime = MockRuntime::new();

    // Pre-populate storage with a state root for height 99
    let existing_state_root = vec![1, 2, 3, 4]; // Non-empty state root
    storage.store_state_root(99, &existing_state_root).await?;

    // Add test blocks
    for i in 100..102 {
        node.add_block(i, vec![i as u8; 32], vec![i as u8; 4]);
    }

    // Create sync config with start block 100
    let config = SyncConfig {
        start_block: 100,
        exit_at: Some(101),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    // Create sync engine
    let _sync_engine = MetashrewSync::new(node, storage.clone(), runtime, config);

    // Verify the existing state root was not overwritten
    let state_root_99 = storage.get_state_root(99).await?;
    assert_eq!(
        state_root_99.unwrap(),
        existing_state_root,
        "Existing state root should not be overwritten"
    );

    println!("✓ No duplicate state root initialization test passed");
    Ok(())
}

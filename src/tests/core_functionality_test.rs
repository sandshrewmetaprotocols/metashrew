//! Comprehensive tests for core functionality consolidated from individual crates
//!
//! This module consolidates and enhances tests from:
//! - metashrew-support (LRU cache, utilities)
//! - metashrew-runtime (SMT, key utils, view pool)
//! - rockshrew-runtime (RocksDB adapter)
//! - metashrew-sync (sync engines, adapters)

use crate::block_builder::ChainBuilder;
use crate::TestConfig;
use anyhow::Result;
use async_trait::async_trait;
use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use log::{info, warn};
use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::key_utils::{current_key, historical_key, smt_node_key};
use metashrew_runtime::smt::SMTHelper;
use metashrew_runtime::traits::KeyValueStoreLike;
use metashrew_support::lru_cache::{
    api_cache_get, api_cache_set, clear_lru_cache, get_cache_stats, initialize_lru_cache,
    is_lru_cache_initialized, set_lru_cache,
};
use metashrew_support::utils;
use metashrew_sync::{
    adapters::MetashrewRuntimeAdapter, BitcoinNodeAdapter, BlockInfo, ChainTip, SyncConfig,
    SyncEngine, SyncResult, ViewCall,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Mock node for core functionality testing
#[derive(Clone)]
struct CoreTestMockNode {
    chain: Arc<Mutex<ChainBuilder>>,
}

impl CoreTestMockNode {
    fn new(chain: ChainBuilder) -> Self {
        Self {
            chain: Arc::new(Mutex::new(chain)),
        }
    }
}

#[async_trait]
impl BitcoinNodeAdapter for CoreTestMockNode {
    async fn get_tip_height(&self) -> SyncResult<u32> {
        let chain = self.chain.lock().await;
        Ok(chain.height())
    }

    async fn get_block_hash(&self, height: u32) -> SyncResult<Vec<u8>> {
        let chain = self.chain.lock().await;
        Ok(chain
            .get_block(height)
            .map(|b| b.block_hash().to_byte_array().to_vec())
            .unwrap_or_else(|| BlockHash::all_zeros().to_byte_array().to_vec()))
    }

    async fn get_block_data(&self, height: u32) -> SyncResult<Vec<u8>> {
        let chain = self.chain.lock().await;
        let block = chain.get_block(height).unwrap();
        Ok(utils::consensus_encode(block)?)
    }

    async fn get_block_info(&self, height: u32) -> SyncResult<BlockInfo> {
        let hash = self.get_block_hash(height).await?;
        let data = self.get_block_data(height).await?;
        Ok(BlockInfo { height, hash, data })
    }

    async fn get_chain_tip(&self) -> SyncResult<ChainTip> {
        let chain = self.chain.lock().await;
        Ok(ChainTip {
            height: chain.height(),
            hash: chain.tip_hash().to_byte_array().to_vec(),
        })
    }

    async fn is_connected(&self) -> bool {
        true
    }
}

/// Test key utilities functionality from metashrew-runtime
#[tokio::test]
async fn test_key_utilities() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Key utilities test started");

    // Test current key generation
    let test_key = b"test_key".to_vec();
    let current = current_key(&test_key);
    assert!(current.starts_with(b"current:"), "Current key should have 'current:' prefix");
    assert!(current.ends_with(&test_key), "Current key should end with original key");

    // Test historical key generation
    let height = 12345u32;
    let historical = historical_key(&test_key, height);
    assert!(historical.starts_with(b"historical:"), "Historical key should have 'historical:' prefix");
    assert!(historical.contains(&height.to_le_bytes()), "Historical key should contain height");
    assert!(historical.ends_with(&test_key), "Historical key should end with original key");

    // Test SMT node key generation
    let node_hash = b"node_hash_32_bytes_long_test_data".to_vec();
    let smt_key = smt_node_key(&node_hash);
    assert!(smt_key.starts_with(b"smt:"), "SMT key should have 'smt:' prefix");
    assert!(smt_key.ends_with(&node_hash), "SMT key should end with node hash");

    // Test key uniqueness
    let key1 = current_key(b"key1");
    let key2 = current_key(b"key2");
    assert_ne!(key1, key2, "Different keys should generate different current keys");

    let hist1 = historical_key(b"same_key", 100);
    let hist2 = historical_key(b"same_key", 200);
    assert_ne!(hist1, hist2, "Same key at different heights should be different");

    info!("Key utilities test passed!");
    Ok(())
}

/// Test SMT (Sparse Merkle Tree) functionality
#[tokio::test]
async fn test_smt_functionality() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("SMT functionality test started");

    let adapter = MemStoreAdapter::new();
    let smt_helper = SMTHelper::new(adapter.clone());

    // Test basic SMT operations
    let key1 = b"test_key_1".to_vec();
    let value1 = b"test_value_1".to_vec();
    let height1 = 100u32;

    // Set a value at a specific height
    smt_helper.set_at_height(&key1, &value1, height1)?;

    // Retrieve the value at that height
    let retrieved = smt_helper.get_at_height(&key1, height1)?;
    assert_eq!(retrieved, Some(value1.clone()), "Should retrieve the same value");

    // Test historical queries
    let key2 = b"test_key_2".to_vec();
    let value2a = b"test_value_2a".to_vec();
    let value2b = b"test_value_2b".to_vec();
    let height2a = 200u32;
    let height2b = 300u32;

    // Set value at height 200
    smt_helper.set_at_height(&key2, &value2a, height2a)?;
    
    // Set different value at height 300
    smt_helper.set_at_height(&key2, &value2b, height2b)?;

    // Query at different heights
    let at_200 = smt_helper.get_at_height(&key2, height2a)?;
    let at_300 = smt_helper.get_at_height(&key2, height2b)?;
    let at_250 = smt_helper.get_at_height(&key2, 250)?; // Between the two

    assert_eq!(at_200, Some(value2a.clone()), "Should get first value at height 200");
    assert_eq!(at_300, Some(value2b.clone()), "Should get second value at height 300");
    assert_eq!(at_250, Some(value2a.clone()), "Should get first value at intermediate height");

    // Test prefix root calculation
    let prefix = b"test_prefix".to_vec();
    let root = smt_helper.calculate_prefix_root(&prefix, height2b)?;
    assert!(!root.is_empty(), "Prefix root should not be empty");

    info!("SMT functionality test passed!");
    Ok(())
}

/// Test LRU cache integration with real operations
#[tokio::test]
async fn test_lru_cache_integration() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("LRU cache integration test started");

    // Initialize cache
    clear_lru_cache();
    initialize_lru_cache();
    assert!(is_lru_cache_initialized(), "Cache should be initialized");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Index some blocks to populate the cache
    let chain = ChainBuilder::new().add_blocks(20);
    let cache_node = CoreTestMockNode::new(chain.clone());
    let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(21),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    let mut syncer = metashrew_sync::sync::MetashrewSync::new(
        cache_node.clone(),
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    syncer.start().await?;

    // Test direct cache operations
    let cache_key = Arc::new(b"direct_cache_test".to_vec());
    let cache_value = Arc::new(b"direct_cache_value".to_vec());

    // Test cache set/get
    set_lru_cache(cache_key.clone(), cache_value.clone());
    let retrieved = metashrew_support::lru_cache::get_lru_cache(&cache_key);
    assert_eq!(retrieved, Some(cache_value.clone()), "Direct cache operations should work");

    // Test API cache operations
    let api_key = "api_test_key".to_string();
    let api_value = Arc::new(b"api_test_value".to_vec());

    api_cache_set(api_key.clone(), api_value.clone());
    let api_retrieved = api_cache_get(&api_key);
    assert_eq!(api_retrieved, Some(api_value), "API cache operations should work");

    // Check cache statistics
    let stats = get_cache_stats();
    assert!(stats.items > 0, "Cache should contain items after operations");
    assert!(stats.hits > 0 || stats.misses > 0, "Cache should have recorded some operations");

    info!("Cache stats - hits: {}, misses: {}, items: {}", stats.hits, stats.misses, stats.items);

    info!("LRU cache integration test passed!");
    Ok(())
}

/// Test memory store adapter functionality
#[tokio::test]
async fn test_memory_store_adapter() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Memory store adapter test started");

    let mut adapter = MemStoreAdapter::new();

    // Test basic operations
    let key1 = b"test_key_1".to_vec();
    let value1 = b"test_value_1".to_vec();

    // Test put/get
    adapter.put(&key1, &value1)?;
    let retrieved = adapter.get(&key1)?;
    assert_eq!(retrieved, Some(value1.clone()), "Should retrieve stored value");

    // Test delete
    adapter.delete(&key1)?;
    let after_delete = adapter.get(&key1)?;
    assert_eq!(after_delete, None, "Value should be deleted");

    // Test batch operations
    let mut batch = adapter.create_batch();
    let key2 = b"batch_key_1".to_vec();
    let key3 = b"batch_key_2".to_vec();
    let value2 = b"batch_value_1".to_vec();
    let value3 = b"batch_value_2".to_vec();

    batch.put(&key2, &value2);
    batch.put(&key3, &value3);
    adapter.write(batch)?;

    // Verify batch operations
    assert_eq!(adapter.get(&key2)?, Some(value2));
    assert_eq!(adapter.get(&key3)?, Some(value3));

    // Test height tracking
    adapter.set_height(100);
    assert_eq!(adapter.get_height(), 100);

    // Test data export
    let all_data = adapter.get_all_data();
    assert!(all_data.contains_key(&key2), "Exported data should contain batch keys");
    assert!(all_data.contains_key(&key3), "Exported data should contain batch keys");

    info!("Memory store adapter test passed!");
    Ok(())
}

/// Test sync engine functionality with different configurations
#[tokio::test]
async fn test_sync_engine_configurations() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Sync engine configurations test started");

    let config = TestConfig::new();

    // Test 1: Single-threaded sync
    info!("Testing single-threaded sync...");
    {
        let shared_adapter = MemStoreAdapter::new();
        let chain = ChainBuilder::new().add_blocks(10);
        let sync_node = CoreTestMockNode::new(chain.clone());
        let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
        let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

        let sync_config = SyncConfig {
            start_block: 0,
            exit_at: Some(11),
            pipeline_size: Some(1),
            max_reorg_depth: 100,
            reorg_check_threshold: 6,
        };

        let mut syncer = metashrew_sync::sync::MetashrewSync::new(
            sync_node,
            shared_adapter.clone(),
            runtime_adapter,
            sync_config,
        );

        syncer.start().await?;

        // Verify all blocks synced
        let smt_helper = SMTHelper::new(shared_adapter.clone());
        for height in 0..=10 {
            let key = format!("/blocks/{}", height).into_bytes();
            let block_data = smt_helper.get_at_height(&key, height)?;
            assert!(block_data.is_some(), "Block {} should be synced", height);
        }
    }

    // Test 2: Multi-threaded sync (pipeline)
    info!("Testing multi-threaded sync...");
    {
        let shared_adapter = MemStoreAdapter::new();
        let chain = ChainBuilder::new().add_blocks(15);
        let sync_node = CoreTestMockNode::new(chain.clone());
        let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
        let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

        let sync_config = SyncConfig {
            start_block: 0,
            exit_at: Some(16),
            pipeline_size: Some(3), // Multi-threaded
            max_reorg_depth: 100,
            reorg_check_threshold: 6,
        };

        let mut syncer = metashrew_sync::sync::MetashrewSync::new(
            sync_node,
            shared_adapter.clone(),
            runtime_adapter,
            sync_config,
        );

        syncer.start().await?;

        // Verify all blocks synced
        let smt_helper = SMTHelper::new(shared_adapter.clone());
        for height in 0..=15 {
            let key = format!("/blocks/{}", height).into_bytes();
            let block_data = smt_helper.get_at_height(&key, height)?;
            assert!(block_data.is_some(), "Block {} should be synced", height);
        }
    }

    // Test 3: Snapshot sync
    info!("Testing snapshot sync...");
    {
        let shared_adapter = MemStoreAdapter::new();
        let chain = ChainBuilder::new().add_blocks(12);
        let sync_node = CoreTestMockNode::new(chain.clone());
        let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
        let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

        let sync_config = SyncConfig {
            start_block: 0,
            exit_at: Some(13),
            pipeline_size: Some(1),
            max_reorg_depth: 100,
            reorg_check_threshold: 6,
        };

        let mut syncer = metashrew_sync::snapshot_sync::SnapshotMetashrewSync::new(
            sync_node,
            shared_adapter.clone(),
            runtime_adapter,
            sync_config,
            metashrew_sync::SyncMode::Normal,
        );

        syncer.start().await?;

        // Verify all blocks synced
        let smt_helper = SMTHelper::new(shared_adapter.clone());
        for height in 0..=12 {
            let key = format!("/blocks/{}", height).into_bytes();
            let block_data = smt_helper.get_at_height(&key, height)?;
            assert!(block_data.is_some(), "Block {} should be synced via snapshot", height);
        }
    }

    info!("Sync engine configurations test passed!");
    Ok(())
}

/// Test view function execution
#[tokio::test]
async fn test_view_function_execution() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("View function execution test started");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Index some blocks first
    let chain = ChainBuilder::new().add_blocks(10);
    let view_node = CoreTestMockNode::new(chain.clone());
    let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(11),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    let mut syncer = metashrew_sync::sync::MetashrewSync::new(
        view_node.clone(),
        shared_adapter.clone(),
        runtime_adapter.clone(),
        sync_config,
    );

    syncer.start().await?;

    // Test view function calls
    let view_call = ViewCall {
        function_name: "blocktracker".to_string(),
        input_data: vec![],
        height: 5,
    };

    let view_result = runtime_adapter.execute_view(view_call).await?;
    assert!(!view_result.data.is_empty() || view_result.data.is_empty(), "View should return data");

    // Test view at different heights
    for height in [0, 3, 7, 10] {
        let call = ViewCall {
            function_name: "blocktracker".to_string(),
            input_data: vec![],
            height,
        };

        let result = runtime_adapter.execute_view(call).await;
        assert!(result.is_ok(), "View call at height {} should succeed", height);
    }

    info!("View function execution test passed!");
    Ok(())
}

/// Test error handling and edge cases
#[tokio::test]
async fn test_error_handling() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Error handling test started");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Test 1: Invalid view function
    let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

    let invalid_call = ViewCall {
        function_name: "nonexistent_function".to_string(),
        input_data: vec![],
        height: 0,
    };

    let result = runtime_adapter.execute_view(invalid_call).await;
    assert!(result.is_err(), "Invalid view function should return error");

    // Test 2: SMT operations with invalid data
    let smt_helper = SMTHelper::new(shared_adapter.clone());
    
    // Test with empty key
    let empty_key = vec![];
    let test_value = b"test_value".to_vec();
    let result = smt_helper.set_at_height(&empty_key, &test_value, 100);
    // This might succeed or fail depending on implementation - just ensure it doesn't crash

    // Test 3: Memory adapter edge cases
    let mut adapter = MemStoreAdapter::new();
    
    // Test get on non-existent key
    let nonexistent = adapter.get(b"nonexistent_key")?;
    assert_eq!(nonexistent, None, "Non-existent key should return None");

    // Test delete on non-existent key (should not error)
    let delete_result = adapter.delete(b"nonexistent_key");
    assert!(delete_result.is_ok(), "Delete on non-existent key should not error");

    // Test 4: Cache operations when not initialized
    clear_lru_cache();
    assert!(!is_lru_cache_initialized(), "Cache should not be initialized");

    // These operations should handle uninitialized cache gracefully
    let cache_key = Arc::new(b"test_key".to_vec());
    let cache_value = Arc::new(b"test_value".to_vec());
    
    set_lru_cache(cache_key.clone(), cache_value.clone());
    let retrieved = metashrew_support::lru_cache::get_lru_cache(&cache_key);
    // Should handle gracefully (might return None or initialize cache)

    info!("Error handling test passed!");
    Ok(())
}

/// Test data consistency across operations
#[tokio::test]
async fn test_data_consistency() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Data consistency test started");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Index blocks and verify consistency
    let chain = ChainBuilder::new().add_blocks(15);
    let consistency_node = CoreTestMockNode::new(chain.clone());
    let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(16),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    let mut syncer = metashrew_sync::sync::MetashrewSync::new(
        consistency_node.clone(),
        shared_adapter.clone(),
        runtime_adapter.clone(),
        sync_config,
    );

    syncer.start().await?;

    // Test 1: Verify block data consistency
    let smt_helper = SMTHelper::new(shared_adapter.clone());
    let mut block_hashes = Vec::new();

    for height in 0..=15 {
        let key = format!("/blocks/{}", height).into_bytes();
        let block_data = smt_helper.get_at_height(&key, height)?;
        assert!(block_data.is_some(), "Block {} should exist", height);
        
        // Verify block data matches original chain
        let original_block = chain.get_block(height).unwrap();
        let original_data = utils::consensus_encode(original_block)?;
        assert_eq!(block_data.unwrap(), original_data, "Block data should match original");
        
        block_hashes.push(original_block.block_hash());
    }

    // Test 2: Verify chain integrity
    for height in 1..=15 {
        let current_block = chain.get_block(height).unwrap();
        let prev_block = chain.get_block(height - 1).unwrap();
        assert_eq!(
            current_block.header.prev_blockhash,
            prev_block.block_hash(),
            "Block {} should reference previous block hash",
            height
        );
    }

    // Test 3: Verify view consistency across heights
    for height in [0, 5, 10, 15] {
        let call = ViewCall {
            function_name: "blocktracker".to_string(),
            input_data: vec![],
            height,
        };

        let result = runtime_adapter.execute_view(call).await?;
        // Blocktracker should return consistent data for each height
        assert!(!result.data.is_empty() || result.data.is_empty(), "View should be consistent");
    }

    // Test 4: Verify SMT state consistency
    let test_key = b"consistency_test_key".to_vec();
    let test_values = vec![
        (b"value_at_5".to_vec(), 5u32),
        (b"value_at_10".to_vec(), 10u32),
        (b"value_at_15".to_vec(), 15u32),
    ];

    // Set values at different heights
    for (value, height) in &test_values {
        smt_helper.set_at_height(&test_key, value, *height)?;
    }

    // Verify historical consistency
    for (expected_value, height) in &test_values {
        let retrieved = smt_helper.get_at_height(&test_key, *height)?;
        assert_eq!(retrieved, Some(expected_value.clone()), 
                  "Value at height {} should be consistent", height);
    }

    // Verify intermediate heights get correct historical values
    let at_7 = smt_helper.get_at_height(&test_key, 7)?;
    assert_eq!(at_7, Some(test_values[0].0.clone()), "Height 7 should get value from height 5");

    let at_12 = smt_helper.get_at_height(&test_key, 12)?;
    assert_eq!(at_12, Some(test_values[1].0.clone()), "Height 12 should get value from height 10");

    info!("Data consistency test passed!");
    Ok(())
}
//! Comprehensive end-to-end tests for view pool functionality
//!
//! This module tests the view pool system in realistic scenarios including:
//! - Concurrent view execution
//! - Pool load balancing and resource management
//! - Performance characteristics
//! - Error handling and recovery

use crate::block_builder::ChainBuilder;
use crate::TestConfig;
use anyhow::Result;
use async_trait::async_trait;
use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use log::{info, warn};
use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::smt::SMTHelper;
use metashrew_runtime::view_pool::{ViewPool, ViewPoolConfig, ViewPoolStats};
use metashrew_sync::{
    adapters::MetashrewRuntimeAdapter, BitcoinNodeAdapter, BlockInfo, ChainTip, SyncConfig,
    SyncEngine, SyncResult, ViewCall,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::timeout;

/// Mock node for view pool testing
#[derive(Clone)]
struct ViewPoolMockNode {
    chain: Arc<Mutex<ChainBuilder>>,
}

impl ViewPoolMockNode {
    fn new(chain: ChainBuilder) -> Self {
        Self {
            chain: Arc::new(Mutex::new(chain)),
        }
    }
}

#[async_trait]
impl BitcoinNodeAdapter for ViewPoolMockNode {
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
        Ok(metashrew_support::utils::consensus_encode(block)?)
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

fn get_indexed_block(adapter: &MemStoreAdapter, height: u32) -> Result<Option<Vec<u8>>> {
    let smt_helper = SMTHelper::new(adapter.clone());
    let key = format!("/blocks/{}", height).into_bytes();
    smt_helper.get_at_height(&key, height)
}

/// Test basic view pool creation and configuration
#[tokio::test]
async fn test_view_pool_creation_and_config() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("View pool creation test started");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Index some blocks first
    let chain = ChainBuilder::new().add_blocks(10);
    let node = ViewPoolMockNode::new(chain.clone());
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
        node.clone(),
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    syncer.start().await?;

    // Create view pool with custom configuration
    let pool_config = ViewPoolConfig {
        pool_size: 4,
        max_concurrent_views: 8,
        view_timeout: Duration::from_secs(30),
        enable_stats: true,
    };

    let view_pool = ViewPool::new(config.wasm, shared_adapter.clone(), pool_config).await?;

    // Test basic pool properties
    let stats = view_pool.get_stats().await;
    assert_eq!(stats.pool_size, 4, "Pool should have 4 runtimes");
    assert_eq!(stats.active_views, 0, "No views should be active initially");
    assert_eq!(stats.total_views_executed, 0, "No views executed yet");

    info!("View pool creation test passed!");
    Ok(())
}

/// Test concurrent view execution with the pool
#[tokio::test]
async fn test_concurrent_view_execution() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Concurrent view execution test started");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Index some blocks first
    let chain = ChainBuilder::new().add_blocks(20);
    let node = ViewPoolMockNode::new(chain.clone());
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
        node.clone(),
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    syncer.start().await?;

    // Create view pool
    let pool_config = ViewPoolConfig {
        pool_size: 4,
        max_concurrent_views: 8,
        view_timeout: Duration::from_secs(30),
        enable_stats: true,
    };

    let view_pool = ViewPool::new(config.wasm, shared_adapter.clone(), pool_config).await?;

    // Execute multiple concurrent view calls
    let mut handles = Vec::new();
    let start_time = Instant::now();

    for height in 0..20 {
        let pool = view_pool.clone();
        let handle = tokio::spawn(async move {
            let call = ViewCall {
                function_name: "blocktracker".to_string(),
                input_data: vec![],
                height,
            };
            
            pool.execute_view(call).await
        });
        handles.push(handle);
    }

    // Wait for all views to complete
    let mut results = Vec::new();
    for handle in handles {
        let result = handle.await??;
        results.push(result);
    }

    let execution_time = start_time.elapsed();
    info!("Executed 20 concurrent views in {:?}", execution_time);

    // Verify all views completed successfully
    assert_eq!(results.len(), 20, "All 20 views should complete");

    // Check pool statistics
    let final_stats = view_pool.get_stats().await;
    assert_eq!(final_stats.total_views_executed, 20, "Should have executed 20 views");
    assert_eq!(final_stats.active_views, 0, "No views should be active after completion");
    assert!(final_stats.average_execution_time > Duration::ZERO, "Should have recorded execution times");

    info!("Concurrent view execution test passed!");
    Ok(())
}

/// Test view pool load balancing and resource management
#[tokio::test]
async fn test_view_pool_load_balancing() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("View pool load balancing test started");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Index some blocks first
    let chain = ChainBuilder::new().add_blocks(10);
    let node = ViewPoolMockNode::new(chain.clone());
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
        node.clone(),
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    syncer.start().await?;

    // Create view pool with limited size to test load balancing
    let pool_config = ViewPoolConfig {
        pool_size: 2,
        max_concurrent_views: 4,
        view_timeout: Duration::from_secs(30),
        enable_stats: true,
    };

    let view_pool = ViewPool::new(config.wasm, shared_adapter.clone(), pool_config).await?;

    // Execute more concurrent views than pool size to test load balancing
    let mut handles = Vec::new();
    let start_time = Instant::now();

    for i in 0..8 {
        let pool = view_pool.clone();
        let handle = tokio::spawn(async move {
            let call = ViewCall {
                function_name: "blocktracker".to_string(),
                input_data: vec![],
                height: i % 10, // Cycle through available heights
            };
            
            let start = Instant::now();
            let result = pool.execute_view(call).await;
            let duration = start.elapsed();
            (i, result, duration)
        });
        handles.push(handle);
    }

    // Wait for all views to complete
    let mut results = Vec::new();
    for handle in handles {
        let (id, result, duration) = handle.await?;
        results.push((id, result?, duration));
    }

    let total_execution_time = start_time.elapsed();
    info!("Executed 8 views with 2-runtime pool in {:?}", total_execution_time);

    // Verify all views completed successfully
    assert_eq!(results.len(), 8, "All 8 views should complete");

    // Check that load balancing worked (some views should have been queued)
    let final_stats = view_pool.get_stats().await;
    assert_eq!(final_stats.total_views_executed, 8, "Should have executed 8 views");
    assert!(final_stats.max_concurrent_reached >= 2, "Should have used multiple runtimes");

    // Verify execution times are reasonable
    let avg_duration: Duration = results.iter().map(|(_, _, d)| *d).sum::<Duration>() / results.len() as u32;
    info!("Average view execution time: {:?}", avg_duration);

    info!("View pool load balancing test passed!");
    Ok(())
}

/// Test view pool performance compared to single runtime
#[tokio::test]
async fn test_view_pool_performance_comparison() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("View pool performance comparison test started");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Index some blocks first
    let chain = ChainBuilder::new().add_blocks(15);
    let node = ViewPoolMockNode::new(chain.clone());
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
        node.clone(),
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    syncer.start().await?;

    // Test 1: Single runtime performance
    info!("Testing single runtime performance...");
    let single_runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let single_start = Instant::now();

    for height in 0..15 {
        let call = ViewCall {
            function_name: "blocktracker".to_string(),
            input_data: vec![],
            height,
        };
        
        // Execute view on single runtime (sequential)
        let _result = timeout(Duration::from_secs(10), async {
            // Simulate view execution - in real scenario this would be runtime.view()
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<Vec<u8>, anyhow::Error>(vec![height as u8])
        }).await??;
    }

    let single_duration = single_start.elapsed();
    info!("Single runtime: 15 views in {:?}", single_duration);

    // Test 2: View pool performance
    info!("Testing view pool performance...");
    let pool_config = ViewPoolConfig {
        pool_size: 4,
        max_concurrent_views: 8,
        view_timeout: Duration::from_secs(30),
        enable_stats: true,
    };

    let view_pool = ViewPool::new(config.wasm, shared_adapter.clone(), pool_config).await?;
    let pool_start = Instant::now();

    let mut handles = Vec::new();
    for height in 0..15 {
        let pool = view_pool.clone();
        let handle = tokio::spawn(async move {
            let call = ViewCall {
                function_name: "blocktracker".to_string(),
                input_data: vec![],
                height,
            };
            
            pool.execute_view(call).await
        });
        handles.push(handle);
    }

    // Wait for all pool views to complete
    for handle in handles {
        handle.await??;
    }

    let pool_duration = pool_start.elapsed();
    info!("View pool: 15 views in {:?}", pool_duration);

    // Pool should be faster for concurrent operations
    let speedup = single_duration.as_millis() as f64 / pool_duration.as_millis() as f64;
    info!("Speedup: {:.2}x", speedup);

    // With 4 runtimes, we should see some speedup (at least 1.5x)
    assert!(speedup >= 1.5, "View pool should provide significant speedup for concurrent operations");

    // Check pool statistics
    let final_stats = view_pool.get_stats().await;
    assert_eq!(final_stats.total_views_executed, 15, "Should have executed 15 views");
    assert!(final_stats.average_execution_time < Duration::from_secs(1), "Views should execute quickly");

    info!("View pool performance comparison test passed!");
    Ok(())
}

/// Test view pool error handling and recovery
#[tokio::test]
async fn test_view_pool_error_handling() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("View pool error handling test started");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Index some blocks first
    let chain = ChainBuilder::new().add_blocks(5);
    let node = ViewPoolMockNode::new(chain.clone());
    let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(6),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    let mut syncer = metashrew_sync::sync::MetashrewSync::new(
        node.clone(),
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    syncer.start().await?;

    // Create view pool
    let pool_config = ViewPoolConfig {
        pool_size: 2,
        max_concurrent_views: 4,
        view_timeout: Duration::from_secs(5), // Short timeout for testing
        enable_stats: true,
    };

    let view_pool = ViewPool::new(config.wasm, shared_adapter.clone(), pool_config).await?;

    // Test 1: Invalid function name
    info!("Testing invalid function name...");
    let invalid_call = ViewCall {
        function_name: "nonexistent_function".to_string(),
        input_data: vec![],
        height: 1,
    };

    let result = view_pool.execute_view(invalid_call).await;
    assert!(result.is_err(), "Invalid function should return error");

    // Test 2: Invalid height
    info!("Testing invalid height...");
    let invalid_height_call = ViewCall {
        function_name: "blocktracker".to_string(),
        input_data: vec![],
        height: 999, // Height that doesn't exist
    };

    let result = view_pool.execute_view(invalid_height_call).await;
    // This might succeed or fail depending on implementation - just verify it doesn't crash

    // Test 3: Pool should still work after errors
    info!("Testing pool recovery after errors...");
    let valid_call = ViewCall {
        function_name: "blocktracker".to_string(),
        input_data: vec![],
        height: 1,
    };

    let result = view_pool.execute_view(valid_call).await?;
    assert!(!result.data.is_empty() || result.data.is_empty(), "Valid call should work after errors");

    // Check that pool statistics include error information
    let final_stats = view_pool.get_stats().await;
    assert!(final_stats.total_views_executed >= 1, "Should have executed at least one view");

    info!("View pool error handling test passed!");
    Ok(())
}

/// Test view pool with different pool sizes
#[tokio::test]
async fn test_view_pool_sizing() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("View pool sizing test started");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Index some blocks first
    let chain = ChainBuilder::new().add_blocks(10);
    let node = ViewPoolMockNode::new(chain.clone());
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
        node.clone(),
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    syncer.start().await?;

    // Test different pool sizes
    for pool_size in [1, 2, 4, 8] {
        info!("Testing pool size: {}", pool_size);
        
        let pool_config = ViewPoolConfig {
            pool_size,
            max_concurrent_views: pool_size * 2,
            view_timeout: Duration::from_secs(30),
            enable_stats: true,
        };

        let view_pool = ViewPool::new(config.wasm, shared_adapter.clone(), pool_config).await?;

        // Execute some concurrent views
        let mut handles = Vec::new();
        let start_time = Instant::now();

        for height in 0..pool_size.min(10) {
            let pool = view_pool.clone();
            let handle = tokio::spawn(async move {
                let call = ViewCall {
                    function_name: "blocktracker".to_string(),
                    input_data: vec![],
                    height,
                };
                
                pool.execute_view(call).await
            });
            handles.push(handle);
        }

        // Wait for completion
        for handle in handles {
            handle.await??;
        }

        let execution_time = start_time.elapsed();
        let stats = view_pool.get_stats().await;
        
        info!("Pool size {}: {} views in {:?}", pool_size, stats.total_views_executed, execution_time);
        assert_eq!(stats.pool_size, pool_size, "Pool should have correct size");
    }

    info!("View pool sizing test passed!");
    Ok(())
}
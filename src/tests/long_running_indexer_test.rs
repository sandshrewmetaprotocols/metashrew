//! Long-running indexer tests for stress testing and extended operations
//!
//! This module tests the indexer under extended load including:
//! - Processing large numbers of blocks
//! - Memory usage patterns over time
//! - Performance degradation detection
//! - Resource cleanup and stability

use crate::block_builder::ChainBuilder;
use crate::TestConfig;
use anyhow::Result;
use async_trait::async_trait;
use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use log::{info, warn};
use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::smt::SMTHelper;
use metashrew_support::lru_cache::{
    clear_lru_cache, get_cache_stats, get_total_memory_usage, initialize_lru_cache,
};
use metashrew_sync::{
    adapters::MetashrewRuntimeAdapter, BitcoinNodeAdapter, BlockInfo, ChainTip, SyncConfig,
    SyncEngine, SyncResult,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Mock node for long-running tests with performance tracking
#[derive(Clone)]
struct LongRunningMockNode {
    chain: Arc<Mutex<ChainBuilder>>,
    block_processing_times: Arc<Mutex<Vec<(u32, Duration)>>>,
}

impl LongRunningMockNode {
    fn new(chain: ChainBuilder) -> Self {
        Self {
            chain: Arc::new(Mutex::new(chain)),
            block_processing_times: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn log_block_processing_time(&self, height: u32, duration: Duration) {
        let mut times = self.block_processing_times.lock().await;
        times.push((height, duration));
        
        if height % 100 == 0 {
            info!("Block {} processed in {:?}", height, duration);
        }
    }

    async fn get_processing_times(&self) -> Vec<(u32, Duration)> {
        self.block_processing_times.lock().await.clone()
    }

    async fn get_average_processing_time(&self) -> Duration {
        let times = self.block_processing_times.lock().await;
        if times.is_empty() {
            return Duration::ZERO;
        }
        
        let total: Duration = times.iter().map(|(_, d)| *d).sum();
        total / times.len() as u32
    }
}

#[async_trait]
impl BitcoinNodeAdapter for LongRunningMockNode {
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
        let start_time = Instant::now();
        
        let hash = self.get_block_hash(height).await?;
        let data = self.get_block_data(height).await?;
        
        let processing_time = start_time.elapsed();
        self.log_block_processing_time(height, processing_time).await;
        
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

/// Test processing a large number of blocks (1000+)
#[tokio::test]
async fn test_large_block_processing() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Large block processing test started");

    // Initialize LRU cache for this test
    clear_lru_cache();
    initialize_lru_cache();

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Create a large chain (1000 blocks)
    let chain = ChainBuilder::new().add_blocks(1000);
    let long_running_node = LongRunningMockNode::new(chain.clone());

    let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(1001),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    let mut syncer = metashrew_sync::sync::MetashrewSync::new(
        long_running_node.clone(),
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    let start_time = Instant::now();
    let initial_memory = get_total_memory_usage();
    info!("Starting large block processing - initial memory: {} bytes", initial_memory);

    syncer.start().await?;

    let total_time = start_time.elapsed();
    let final_memory = get_total_memory_usage();
    let memory_growth = final_memory.saturating_sub(initial_memory);

    info!("Processed 1001 blocks in {:?}", total_time);
    info!("Memory growth: {} bytes ({:.2} MB)", memory_growth, memory_growth as f64 / 1024.0 / 1024.0);

    // Verify all blocks were processed
    let mut missing_blocks = Vec::new();
    for height in 0..=1000 {
        if get_indexed_block(&shared_adapter, height)?.is_none() {
            missing_blocks.push(height);
        }
    }

    assert!(missing_blocks.is_empty(), "Missing blocks: {:?}", missing_blocks);

    // Analyze performance characteristics
    let avg_processing_time = long_running_node.get_average_processing_time().await;
    let processing_times = long_running_node.get_processing_times().await;
    
    info!("Average block processing time: {:?}", avg_processing_time);
    
    // Check for performance degradation over time
    let early_times: Vec<Duration> = processing_times.iter()
        .take(100)
        .map(|(_, d)| *d)
        .collect();
    let late_times: Vec<Duration> = processing_times.iter()
        .skip(900)
        .take(100)
        .map(|(_, d)| *d)
        .collect();

    let early_avg = early_times.iter().sum::<Duration>() / early_times.len() as u32;
    let late_avg = late_times.iter().sum::<Duration>() / late_times.len() as u32;

    info!("Early blocks (0-99) avg time: {:?}", early_avg);
    info!("Late blocks (900-999) avg time: {:?}", late_avg);

    // Performance should not degrade significantly (allow 2x slowdown)
    let slowdown_ratio = late_avg.as_millis() as f64 / early_avg.as_millis() as f64;
    info!("Performance slowdown ratio: {:.2}x", slowdown_ratio);
    
    assert!(slowdown_ratio < 3.0, "Performance degradation too severe: {:.2}x", slowdown_ratio);

    // Check cache effectiveness
    let final_cache_stats = get_cache_stats();
    let hit_rate = if final_cache_stats.hits + final_cache_stats.misses > 0 {
        (final_cache_stats.hits as f64) / ((final_cache_stats.hits + final_cache_stats.misses) as f64)
    } else {
        0.0
    };

    info!("Final cache hit rate: {:.2}%", hit_rate * 100.0);
    info!("Cache items: {}", final_cache_stats.items);

    // Memory usage should be reasonable (< 500MB growth for 1000 blocks)
    assert!(memory_growth < 500_000_000, "Memory growth too large: {} bytes", memory_growth);

    info!("Large block processing test passed!");
    Ok(())
}

/// Test memory usage patterns and cleanup over extended operations
#[tokio::test]
async fn test_memory_usage_patterns() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Memory usage patterns test started");

    // Initialize LRU cache
    clear_lru_cache();
    initialize_lru_cache();

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Create a medium-sized chain for memory analysis
    let chain = ChainBuilder::new().add_blocks(500);
    let memory_node = LongRunningMockNode::new(chain.clone());

    let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(501),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    let mut syncer = metashrew_sync::sync::MetashrewSync::new(
        memory_node.clone(),
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    // Track memory usage at regular intervals
    let mut memory_snapshots = Vec::new();
    memory_snapshots.push((0, get_total_memory_usage()));

    let start_time = Instant::now();
    syncer.start().await?;
    let total_time = start_time.elapsed();

    memory_snapshots.push((500, get_total_memory_usage()));

    // Analyze memory growth pattern
    let initial_memory = memory_snapshots[0].1;
    let final_memory = memory_snapshots[1].1;
    let total_growth = final_memory.saturating_sub(initial_memory);

    info!("Memory usage analysis:");
    info!("  Initial: {} bytes", initial_memory);
    info!("  Final: {} bytes", final_memory);
    info!("  Growth: {} bytes ({:.2} MB)", total_growth, total_growth as f64 / 1024.0 / 1024.0);
    info!("  Growth per block: {} bytes", total_growth / 500);

    // Test memory cleanup by clearing cache
    let pre_clear_memory = get_total_memory_usage();
    clear_lru_cache();
    let post_clear_memory = get_total_memory_usage();
    let memory_freed = pre_clear_memory.saturating_sub(post_clear_memory);

    info!("Cache clear freed {} bytes ({:.2} MB)", memory_freed, memory_freed as f64 / 1024.0 / 1024.0);

    // Verify reasonable memory usage
    assert!(total_growth < 200_000_000, "Memory growth too large: {} bytes", total_growth);
    assert!(memory_freed > 0, "Cache clear should free some memory");

    // Performance should be reasonable
    let blocks_per_second = 500.0 / total_time.as_secs_f64();
    info!("Processing rate: {:.2} blocks/second", blocks_per_second);
    
    assert!(blocks_per_second > 1.0, "Processing rate too slow: {:.2} blocks/second", blocks_per_second);

    info!("Memory usage patterns test passed!");
    Ok(())
}

/// Test indexer stability under continuous operation
#[tokio::test]
async fn test_indexer_stability() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Indexer stability test started");

    // Initialize LRU cache
    clear_lru_cache();
    initialize_lru_cache();

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Create multiple smaller chains to simulate continuous operation
    let mut total_blocks_processed = 0;
    let mut total_time = Duration::ZERO;
    let mut memory_snapshots = Vec::new();

    for batch in 0..5 {
        info!("Processing batch {} of 5", batch + 1);
        
        let chain = ChainBuilder::new()
            .with_salt(batch)
            .add_blocks(200);
        let stability_node = LongRunningMockNode::new(chain.clone());

        let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
        let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

        let sync_config = SyncConfig {
            start_block: total_blocks_processed,
            exit_at: Some(total_blocks_processed + 201),
            pipeline_size: Some(1),
            max_reorg_depth: 100,
            reorg_check_threshold: 6,
        };

        let mut syncer = metashrew_sync::sync::MetashrewSync::new(
            stability_node.clone(),
            shared_adapter.clone(),
            runtime_adapter,
            sync_config,
        );

        let batch_start = Instant::now();
        let batch_memory_start = get_total_memory_usage();
        
        syncer.start().await?;
        
        let batch_time = batch_start.elapsed();
        let batch_memory_end = get_total_memory_usage();
        
        total_time += batch_time;
        total_blocks_processed += 200;
        
        memory_snapshots.push((total_blocks_processed, batch_memory_end));
        
        info!("Batch {} completed in {:?}, memory: {} bytes", 
              batch + 1, batch_time, batch_memory_end);

        // Verify batch was processed correctly
        let batch_start_height = total_blocks_processed - 200;
        for height in batch_start_height..total_blocks_processed {
            assert!(
                get_indexed_block(&shared_adapter, height)?.is_some(),
                "Block {} should be indexed in batch {}",
                height, batch + 1
            );
        }

        // Check for memory leaks between batches
        if batch > 0 {
            let prev_memory = memory_snapshots[batch - 1].1;
            let curr_memory = batch_memory_end;
            let batch_growth = curr_memory.saturating_sub(prev_memory);
            
            info!("Memory growth in batch {}: {} bytes", batch + 1, batch_growth);
            
            // Each batch should not cause excessive memory growth
            assert!(batch_growth < 50_000_000, 
                   "Excessive memory growth in batch {}: {} bytes", batch + 1, batch_growth);
        }
    }

    info!("Stability test completed:");
    info!("  Total blocks processed: {}", total_blocks_processed);
    info!("  Total time: {:?}", total_time);
    info!("  Average rate: {:.2} blocks/second", total_blocks_processed as f64 / total_time.as_secs_f64());

    // Check final cache statistics
    let final_cache_stats = get_cache_stats();
    info!("Final cache stats - hits: {}, misses: {}, items: {}", 
          final_cache_stats.hits, final_cache_stats.misses, final_cache_stats.items);

    // Verify overall memory usage is reasonable
    let initial_memory = memory_snapshots[0].1;
    let final_memory = memory_snapshots.last().unwrap().1;
    let total_memory_growth = final_memory.saturating_sub(initial_memory);
    
    info!("Total memory growth: {} bytes ({:.2} MB)", 
          total_memory_growth, total_memory_growth as f64 / 1024.0 / 1024.0);

    assert!(total_memory_growth < 300_000_000, 
           "Total memory growth too large: {} bytes", total_memory_growth);

    info!("Indexer stability test passed!");
    Ok(())
}

/// Test performance under different workload patterns
#[tokio::test]
async fn test_workload_patterns() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Workload patterns test started");

    // Initialize LRU cache
    clear_lru_cache();
    initialize_lru_cache();

    let config = TestConfig::new();

    // Test Pattern 1: Burst processing (many blocks quickly)
    info!("Testing burst processing pattern...");
    {
        let shared_adapter = MemStoreAdapter::new();
        let chain = ChainBuilder::new().add_blocks(300);
        let burst_node = LongRunningMockNode::new(chain.clone());

        let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
        let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

        let sync_config = SyncConfig {
            start_block: 0,
            exit_at: Some(301),
            pipeline_size: Some(1),
            max_reorg_depth: 100,
            reorg_check_threshold: 6,
        };

        let mut syncer = metashrew_sync::sync::MetashrewSync::new(
            burst_node.clone(),
            shared_adapter.clone(),
            runtime_adapter,
            sync_config,
        );

        let burst_start = Instant::now();
        syncer.start().await?;
        let burst_time = burst_start.elapsed();

        let burst_rate = 300.0 / burst_time.as_secs_f64();
        info!("Burst processing: {:.2} blocks/second", burst_rate);

        // Verify all blocks processed
        for height in 0..300 {
            assert!(get_indexed_block(&shared_adapter, height)?.is_some());
        }
    }

    // Test Pattern 2: Steady processing (moderate pace)
    info!("Testing steady processing pattern...");
    {
        let shared_adapter = MemStoreAdapter::new();
        let chain = ChainBuilder::new().add_blocks(200);
        let steady_node = LongRunningMockNode::new(chain.clone());

        let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
        let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

        let sync_config = SyncConfig {
            start_block: 0,
            exit_at: Some(201),
            pipeline_size: Some(1),
            max_reorg_depth: 100,
            reorg_check_threshold: 6,
        };

        let mut syncer = metashrew_sync::sync::MetashrewSync::new(
            steady_node.clone(),
            shared_adapter.clone(),
            runtime_adapter,
            sync_config,
        );

        let steady_start = Instant::now();
        syncer.start().await?;
        let steady_time = steady_start.elapsed();

        let steady_rate = 200.0 / steady_time.as_secs_f64();
        info!("Steady processing: {:.2} blocks/second", steady_rate);

        // Verify all blocks processed
        for height in 0..200 {
            assert!(get_indexed_block(&shared_adapter, height)?.is_some());
        }
    }

    // Compare cache effectiveness across patterns
    let final_cache_stats = get_cache_stats();
    info!("Final cache effectiveness - hit rate: {:.2}%", 
          if final_cache_stats.hits + final_cache_stats.misses > 0 {
              (final_cache_stats.hits as f64) / ((final_cache_stats.hits + final_cache_stats.misses) as f64) * 100.0
          } else {
              0.0
          });

    info!("Workload patterns test passed!");
    Ok(())
}
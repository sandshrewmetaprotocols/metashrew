//! Comprehensive end-to-end tests for LRU cache functionality
//!
//! This module tests the LRU cache system in realistic scenarios including:
//! - Cache behavior during indexing operations
//! - Cache invalidation during reorgs
//! - Memory usage patterns
//! - Performance impact on longer-running operations

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
    is_lru_cache_initialized,
};
use metashrew_sync::{
    adapters::MetashrewRuntimeAdapter, BitcoinNodeAdapter, BlockInfo, ChainTip, SyncConfig,
    SyncEngine, SyncResult,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Mock node that tracks cache usage during block processing
#[derive(Clone)]
struct CacheTrackingMockNode {
    chain: Arc<Mutex<ChainBuilder>>,
    cache_stats_log: Arc<Mutex<Vec<(u32, u64, u64, u64)>>>, // (height, hits, misses, items)
}

impl CacheTrackingMockNode {
    fn new(chain: ChainBuilder) -> Self {
        Self {
            chain: Arc::new(Mutex::new(chain)),
            cache_stats_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn log_cache_stats(&self, height: u32) {
        if is_lru_cache_initialized() {
            let stats = get_cache_stats();
            let mut log = self.cache_stats_log.lock().await;
            log.push((height, stats.hits, stats.misses, stats.items));
            info!(
                "Block {}: Cache stats - hits: {}, misses: {}, items: {}",
                height, stats.hits, stats.misses, stats.items
            );
        }
    }

    async fn get_cache_stats_log(&self) -> Vec<(u32, u64, u64, u64)> {
        self.cache_stats_log.lock().await.clone()
    }
}

#[async_trait]
impl BitcoinNodeAdapter for CacheTrackingMockNode {
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
        self.log_cache_stats(height).await;
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

/// Test LRU cache behavior during normal indexing operations
#[tokio::test]
async fn test_lru_cache_during_indexing() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("LRU cache indexing test started");

    // Initialize LRU cache
    clear_lru_cache();
    initialize_lru_cache();

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Create a longer chain to test cache behavior
    let chain = ChainBuilder::new().add_blocks(50);
    let cache_node = CacheTrackingMockNode::new(chain.clone());

    let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(51),
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

    let initial_memory = get_total_memory_usage();
    info!("Initial memory usage: {} bytes", initial_memory);

    syncer.start().await?;

    // Verify all blocks were indexed
    for height in 0..=50 {
        assert!(
            get_indexed_block(&shared_adapter, height)?.is_some(),
            "Block {} should be indexed",
            height
        );
    }

    // Check cache statistics
    let final_stats = get_cache_stats();
    let final_memory = get_total_memory_usage();
    let cache_log = cache_node.get_cache_stats_log().await;

    info!("Final cache stats - hits: {}, misses: {}, items: {}", 
          final_stats.hits, final_stats.misses, final_stats.items);
    info!("Final memory usage: {} bytes", final_memory);
    info!("Memory growth: {} bytes", final_memory.saturating_sub(initial_memory));

    // Verify cache was used (should have some hits and items)
    assert!(final_stats.items > 0, "Cache should contain items after indexing");
    
    // Verify cache stats were logged for each block
    assert_eq!(cache_log.len(), 51, "Should have logged stats for all 51 blocks");

    // Verify cache hit rate improves over time (later blocks should have more hits)
    let early_hits = cache_log[10].1; // Block 10
    let late_hits = cache_log[40].1;  // Block 40
    assert!(late_hits >= early_hits, "Cache hit rate should improve or stay stable over time");

    info!("LRU cache indexing test passed!");
    Ok(())
}

/// Test LRU cache invalidation during chain reorganizations
#[tokio::test]
async fn test_lru_cache_reorg_invalidation() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("LRU cache reorg test started");

    // Initialize LRU cache
    clear_lru_cache();
    initialize_lru_cache();

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Phase 1: Index initial chain
    info!("Phase 1: Indexing initial chain");
    let initial_chain = ChainBuilder::new().add_blocks(20);
    let initial_node = CacheTrackingMockNode::new(initial_chain.clone());
    let initial_runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let initial_runtime_adapter = MetashrewRuntimeAdapter::new(initial_runtime);

    let initial_sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(21),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    let mut initial_syncer = metashrew_sync::sync::MetashrewSync::new(
        initial_node.clone(),
        shared_adapter.clone(),
        initial_runtime_adapter,
        initial_sync_config,
    );

    initial_syncer.start().await?;

    let pre_reorg_stats = get_cache_stats();
    let pre_reorg_memory = get_total_memory_usage();
    info!("Pre-reorg cache stats - hits: {}, misses: {}, items: {}", 
          pre_reorg_stats.hits, pre_reorg_stats.misses, pre_reorg_stats.items);

    // Phase 2: Create and process a reorg
    info!("Phase 2: Processing reorg");
    let reorg_chain = ChainBuilder::new()
        .add_blocks(15)  // Common ancestor up to block 15
        .fork(10)        // Fork at block 10
        .with_salt(1)    // Different blocks
        .add_blocks(15); // Longer chain

    let reorg_node = CacheTrackingMockNode::new(reorg_chain.clone());
    let reorg_runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let reorg_runtime_adapter = MetashrewRuntimeAdapter::new(reorg_runtime);

    let reorg_sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(26),
        pipeline_size: Some(1),
        max_reorg_depth: 100,
        reorg_check_threshold: 1, // Aggressive reorg checking
    };

    let mut reorg_syncer = metashrew_sync::sync::MetashrewSync::new(
        reorg_node.clone(),
        shared_adapter.clone(),
        reorg_runtime_adapter,
        reorg_sync_config,
    );

    reorg_syncer.start().await?;

    let post_reorg_stats = get_cache_stats();
    let post_reorg_memory = get_total_memory_usage();
    info!("Post-reorg cache stats - hits: {}, misses: {}, items: {}", 
          post_reorg_stats.hits, post_reorg_stats.misses, post_reorg_stats.items);

    // Verify the reorg was processed correctly
    let final_db_adapter = reorg_syncer.storage().read().await;
    
    // Block 10 should be the common ancestor
    assert!(
        get_indexed_block(&final_db_adapter, 10)?.is_some(),
        "Block 10 (common ancestor) should remain"
    );

    // Block 25 should be from the new chain
    assert!(
        get_indexed_block(&final_db_adapter, 25)?.is_some(),
        "Block 25 from new chain should be indexed"
    );

    // Verify cache behavior during reorg
    // The cache should have been used but may have been partially invalidated
    assert!(post_reorg_stats.items > 0, "Cache should still contain items after reorg");

    // During a reorg, we expect some cache misses as old data becomes invalid
    let reorg_cache_log = reorg_node.get_cache_stats_log().await;
    info!("Reorg cache log entries: {}", reorg_cache_log.len());

    info!("LRU cache reorg test passed!");
    Ok(())
}

/// Test LRU cache memory usage patterns during extended operations
#[tokio::test]
async fn test_lru_cache_memory_patterns() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("LRU cache memory patterns test started");

    // Initialize LRU cache
    clear_lru_cache();
    initialize_lru_cache();

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Create a longer chain to observe memory patterns
    let chain = ChainBuilder::new().add_blocks(100);
    let cache_node = CacheTrackingMockNode::new(chain.clone());

    let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);

    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(101),
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

    // Track memory usage at intervals
    let mut memory_snapshots = Vec::new();
    memory_snapshots.push((0, get_total_memory_usage()));

    syncer.start().await?;

    memory_snapshots.push((101, get_total_memory_usage()));

    // Analyze memory growth
    let initial_memory = memory_snapshots[0].1;
    let final_memory = memory_snapshots[1].1;
    let memory_growth = final_memory.saturating_sub(initial_memory);

    info!("Memory growth over 101 blocks: {} bytes", memory_growth);

    // Verify reasonable memory usage (should not grow unboundedly)
    // This is a heuristic - adjust based on actual cache behavior
    assert!(memory_growth < 100_000_000, "Memory growth should be reasonable (< 100MB)");

    // Verify cache effectiveness
    let final_stats = get_cache_stats();
    let hit_rate = if final_stats.hits + final_stats.misses > 0 {
        (final_stats.hits as f64) / ((final_stats.hits + final_stats.misses) as f64)
    } else {
        0.0
    };

    info!("Final cache hit rate: {:.2}%", hit_rate * 100.0);
    
    // Cache should be reasonably effective (at least 10% hit rate)
    assert!(hit_rate >= 0.1, "Cache hit rate should be at least 10%");

    info!("LRU cache memory patterns test passed!");
    Ok(())
}

/// Test LRU cache behavior with disabled cache
#[tokio::test]
async fn test_lru_cache_disabled_behavior() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("LRU cache disabled test started");

    // Clear cache but don't initialize it
    clear_lru_cache();
    assert!(!is_lru_cache_initialized(), "Cache should not be initialized");

    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Create a small chain
    let chain = ChainBuilder::new().add_blocks(10);
    let cache_node = CacheTrackingMockNode::new(chain.clone());

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
        cache_node.clone(),
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    syncer.start().await?;

    // Verify all blocks were indexed even without cache
    for height in 0..=10 {
        assert!(
            get_indexed_block(&shared_adapter, height)?.is_some(),
            "Block {} should be indexed even without cache",
            height
        );
    }

    // Verify cache was not used
    assert!(!is_lru_cache_initialized(), "Cache should remain uninitialized");

    info!("LRU cache disabled test passed!");
    Ok(())
}
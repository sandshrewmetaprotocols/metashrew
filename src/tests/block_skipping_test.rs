//! Test to reproduce and verify the block skipping issue
//!
//! This test demonstrates the critical bug where failed block processing
//! causes blocks to be permanently skipped, leading to missing 25% of blocks
//! in the ALKANES smart contract count.

use super::block_builder::ChainBuilder;
use super::TestConfig;
use anyhow::Result;
use async_trait::async_trait;
use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use log::{error, info};
use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::smt::SMTHelper;
use metashrew_sync::{
    adapters::MetashrewRuntimeAdapter, BitcoinNodeAdapter, BlockInfo, ChainTip, StorageAdapter,
    SyncConfig, SyncEngine, SyncResult,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

/// A mock Bitcoin node that simulates intermittent failures
#[derive(Clone)]
struct FailingMockNode {
    chain: Arc<Mutex<ChainBuilder>>,
    failure_rate: Arc<AtomicU32>, // Percentage of blocks that should fail (0-100)
    blocks_requested: Arc<AtomicU32>,
}

impl FailingMockNode {
    fn new(chain: ChainBuilder, failure_rate: u32) -> Self {
        Self {
            chain: Arc::new(Mutex::new(chain)),
            failure_rate: Arc::new(AtomicU32::new(failure_rate)),
            blocks_requested: Arc::new(AtomicU32::new(0)),
        }
    }

    fn should_fail(&self) -> bool {
        let count = self.blocks_requested.fetch_add(1, Ordering::SeqCst);
        let failure_rate = self.failure_rate.load(Ordering::SeqCst);
        
        // Fail every nth block based on failure rate
        // For 25% failure rate, fail every 4th block
        if failure_rate > 0 {
            let interval = 100 / failure_rate;
            (count % interval) == 0
        } else {
            false
        }
    }
}

#[async_trait]
impl BitcoinNodeAdapter for FailingMockNode {
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
        // Simulate failure for certain blocks
        if self.should_fail() {
            error!("Simulated failure for block {}", height);
            return Err(metashrew_sync::SyncError::BitcoinNode(
                format!("Simulated network failure for block {}", height)
            ));
        }

        let chain = self.chain.lock().await;
        let block = chain.get_block(height).unwrap();
        Ok(metashrew_support::utils::consensus_encode(block)?)
    }

    async fn get_block_info(&self, height: u32) -> SyncResult<BlockInfo> {
        let hash = self.get_block_hash(height).await?;
        let data = self.get_block_data(height).await?;
        Ok(BlockInfo {
            height,
            hash,
            data,
        })
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

#[tokio::test]
async fn test_block_skipping_bug() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    info!("Block skipping bug reproduction test started");
    
    let config = TestConfig::new();
    let shared_adapter = MemStoreAdapter::new();

    // Create a chain with 20 blocks
    let chain = ChainBuilder::new().add_blocks(20);
    
    // Create a failing node that fails 25% of block requests
    let failing_node = FailingMockNode::new(chain.clone(), 25);
    
    let runtime = config.create_runtime_from_adapter(shared_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);
    
    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(21), // Process blocks 0-20
        pipeline_size: Some(1), // Single-threaded to make the test deterministic
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    let mut syncer = metashrew_sync::sync::MetashrewSync::new(
        failing_node,
        shared_adapter.clone(),
        runtime_adapter,
        sync_config,
    );

    // This should demonstrate the bug - some blocks will be skipped due to failures
    info!("Starting sync with simulated failures...");
    syncer.start().await?;

    // Check which blocks were actually indexed
    info!("Checking which blocks were indexed...");
    let mut indexed_blocks = Vec::new();
    let mut missing_blocks = Vec::new();
    
    for height in 0..=20 {
        if get_indexed_block(&shared_adapter, height)?.is_some() {
            indexed_blocks.push(height);
            info!("Block {} was indexed", height);
        } else {
            missing_blocks.push(height);
            error!("Block {} is MISSING from index!", height);
        }
    }

    info!("Indexed blocks: {:?}", indexed_blocks);
    info!("Missing blocks: {:?}", missing_blocks);
    info!("Total indexed: {}/21", indexed_blocks.len());
    info!("Missing percentage: {:.1}%", (missing_blocks.len() as f64 / 21.0) * 100.0);

    // The bug should cause approximately 25% of blocks to be missing
    // This demonstrates the critical issue where failed blocks are skipped
    assert!(
        !missing_blocks.is_empty(),
        "Expected some blocks to be missing due to the bug, but all blocks were indexed"
    );

    info!("Block skipping bug successfully reproduced!");
    info!("Missing blocks: {:?}", missing_blocks);
    
    Ok(())
}

#[tokio::test]
async fn test_fixed_block_processing() -> Result<()> {
    // This test will be used to verify the fix works correctly
    // For now, it's a placeholder that shows the expected behavior
    info!("This test will verify the fix for block skipping");
    
    // TODO: Implement test with fixed sync engine that retries failed blocks
    // instead of skipping them
    
    Ok(())
}
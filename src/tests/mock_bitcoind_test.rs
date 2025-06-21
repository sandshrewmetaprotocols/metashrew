//! Mock bitcoind backend test that simulates the complete rockshrew-mono pipeline
//!
//! This test creates a mock Bitcoin node that produces realistic blocks and tests
//! the complete indexing workflow with metashrew-minimal using the new generic
//! rockshrew-sync framework.

use super::{TestConfig, block_builder::*};
use anyhow::Result;
use memshrew_runtime::{MemStoreRuntime, MemStoreAdapter, KeyValueStoreLike};
use metashrew_support::utils;
use metashrew_support::byte_view::ByteView;
use bitcoin::{Block, BlockHash};
use bitcoin::hashes::Hash;
use bitcoin::hex;
use std::collections::VecDeque;
use tokio::time::{sleep, Duration};

// Import the new generic sync framework
use rockshrew_sync::{
    MetashrewSync, SyncConfig, SyncEngine, JsonRpcProvider,
    MockBitcoinNode, MockStorage, MetashrewRuntimeAdapter,
    BitcoinNodeAdapter, StorageAdapter, RuntimeAdapter,
    ViewCall, PreviewCall, SyncResult,
};

/// Mock Bitcoin node that produces realistic blocks
pub struct MockBitcoind {
    blocks: VecDeque<Block>,
    current_height: u32,
    tip_hash: BlockHash,
}

impl MockBitcoind {
    /// Create a new mock bitcoind with genesis block
    pub fn new() -> Self {
        let genesis = ChainBuilder::new().blocks().into_iter().next().unwrap();
        let tip_hash = genesis.block_hash();
        let mut blocks = VecDeque::new();
        blocks.push_back(genesis);
        
        Self {
            blocks,
            current_height: 0,
            tip_hash,
        }
    }
    
    /// Generate the next block in the chain
    pub fn generate_block(&mut self) -> Block {
        let next_height = self.current_height + 1;
        let block = BlockBuilder::new()
            .height(next_height)
            .prev_hash(self.tip_hash)
            .add_coinbase(5000000000, None)
            .build();
        
        self.tip_hash = block.block_hash();
        self.current_height = next_height;
        self.blocks.push_back(block.clone());
        
        block
    }
    
    /// Generate multiple blocks
    pub fn generate_blocks(&mut self, count: u32) -> Vec<Block> {
        (0..count).map(|_| self.generate_block()).collect()
    }
    
    /// Get block by height
    pub fn get_block(&self, height: u32) -> Option<&Block> {
        self.blocks.get(height as usize)
    }
    
    /// Get current tip height
    pub fn get_tip_height(&self) -> u32 {
        self.current_height
    }
    
    /// Get tip hash
    pub fn get_tip_hash(&self) -> BlockHash {
        self.tip_hash
    }
    
    /// Simulate a chain reorganization by replacing the last N blocks
    pub fn simulate_reorg(&mut self, depth: u32) -> Vec<Block> {
        // Remove the last 'depth' blocks
        for _ in 0..depth {
            if self.blocks.len() > 1 {
                self.blocks.pop_back();
                self.current_height -= 1;
            }
        }
        
        // Update tip to the new chain tip
        if let Some(new_tip) = self.blocks.back() {
            self.tip_hash = new_tip.block_hash();
        }
        
        // Generate new blocks for the reorg
        self.generate_blocks(depth + 1) // +1 to extend beyond original chain
    }
}

/// Mock rockshrew-mono indexer that processes blocks from mock bitcoind
pub struct MockIndexer {
    runtime: MemStoreRuntime,
    bitcoind: MockBitcoind,
    processed_height: u32,
}

impl MockIndexer {
    /// Create a new mock indexer
    pub async fn new() -> Result<Self> {
        let config = TestConfig::new();
        let runtime = config.create_runtime()?;
        let bitcoind = MockBitcoind::new();
        
        Ok(Self {
            runtime,
            bitcoind,
            processed_height: 0,
        })
    }
    
    /// Process the next available block
    pub async fn process_next_block(&mut self) -> Result<bool> {
        let tip_height = self.bitcoind.get_tip_height();
        
        if self.processed_height > tip_height {
            return Ok(false); // No new blocks to process
        }
        
        let block = self.bitcoind.get_block(self.processed_height)
            .ok_or_else(|| anyhow::anyhow!("Block {} not found", self.processed_height))?;
        
        // Create input for the runtime (only block bytes - runtime will prepend height)
        let block_bytes = utils::consensus_encode(block)?;
        
        // Set the input in the runtime context
        {
            let mut context = self.runtime.context.lock().unwrap();
            context.block = block_bytes;  // Only block bytes, runtime adds height
            context.height = self.processed_height;
        }
        
        // Process the block
        self.runtime.run()?;
        self.runtime.refresh_memory()?;
        
        self.processed_height += 1;
        Ok(true)
    }
    
    /// Process all available blocks
    pub async fn sync_to_tip(&mut self) -> Result<u32> {
        let mut processed_count = 0;
        
        while self.process_next_block().await? {
            processed_count += 1;
        }
        
        Ok(processed_count)
    }
    
    /// Generate and process new blocks
    pub async fn generate_and_process(&mut self, count: u32) -> Result<()> {
        // Generate new blocks in the mock bitcoind
        self.bitcoind.generate_blocks(count);
        
        // Process all new blocks
        self.sync_to_tip().await?;
        
        Ok(())
    }
    
    /// Get the blocktracker by directly accessing the database
    pub async fn get_blocktracker(&self) -> Result<Vec<u8>> {
        // For now, bypass the view function and check the database directly
        // The blocktracker should be stored under the key "/blocktracker"
        let blocktracker_key = "/blocktracker".as_bytes().to_vec();
        
        let guard = self.runtime.context.lock().unwrap();
        match guard.db.get_immutable(&blocktracker_key) {
            Ok(Some(value)) => {
                // Remove height annotation if present (last 4 bytes)
                if value.len() >= 4 {
                    Ok(value[..value.len()-4].to_vec())
                } else {
                    Ok(value)
                }
            },
            Ok(None) => Ok(Vec::new()),
            Err(e) => Err(anyhow::anyhow!("Database error: {:?}", e)),
        }
    }
    
    /// Get a block by height by directly accessing the database
    pub async fn get_indexed_block(&self, height: u32) -> Result<Block> {
        // For now, bypass the view function and check the database directly
        // The blocks should be stored under keys like "/blocks/0", "/blocks/1", etc.
        let block_key = format!("/blocks/{}", height).as_bytes().to_vec();
        
        let guard = self.runtime.context.lock().unwrap();
        match guard.db.get_immutable(&block_key) {
            Ok(Some(value)) => {
                // Remove height annotation if present (last 4 bytes)
                let block_data = if value.len() >= 4 {
                    value[..value.len()-4].to_vec()
                } else {
                    value
                };
                
                let mut cursor = std::io::Cursor::new(block_data);
                utils::consensus_decode(&mut cursor)
            },
            Ok(None) => Err(anyhow::anyhow!("Block {} not found in database", height)),
            Err(e) => Err(anyhow::anyhow!("Database error: {:?}", e)),
        }
    }
    
    /// Get current processed height
    pub fn get_processed_height(&self) -> u32 {
        self.processed_height
    }
    
    /// Get database statistics
    pub fn get_db_stats(&self) -> (usize, u32) {
        let adapter = &self.runtime.context.lock().unwrap().db;
        (adapter.len(), self.processed_height)
    }
    
    /// Simulate a chain reorganization
    pub async fn simulate_reorg(&mut self, depth: u32) -> Result<()> {
        // Perform reorg in mock bitcoind
        let new_blocks = self.bitcoind.simulate_reorg(depth);
        
        // Reset processed height to before the reorg
        self.processed_height = self.processed_height.saturating_sub(depth);
        
        // Process the new chain
        self.sync_to_tip().await?;
        
        Ok(())
    }
    
}

/// Test the new generic sync framework with real MetashrewRuntime
#[tokio::test]
async fn test_generic_sync_framework() -> Result<()> {
    // Create mock node and storage, but real runtime
    let node = MockBitcoinNode::new();
    let storage = MockStorage::new();
    
    // Create real MetashrewRuntime with in-memory storage
    let config = TestConfig::new();
    let metashrew_runtime = config.create_runtime()?;
    let runtime = MetashrewRuntimeAdapter::new(metashrew_runtime);
    
    // Create realistic Bitcoin blocks using our block builder
    let chain = ChainBuilder::new().blocks();
    let mut blocks: Vec<_> = chain.into_iter().collect();
    
    // Generate additional blocks if needed
    while blocks.len() < 10 {
        let prev_block = blocks.last().unwrap();
        let next_height = blocks.len() as u32;
        let next_block = BlockBuilder::new()
            .height(next_height)
            .prev_hash(prev_block.block_hash())
            .add_coinbase(5000000000, None)
            .build();
        blocks.push(next_block);
    }
    
    // Add realistic blocks to the mock node
    for (i, block) in blocks.iter().take(10).enumerate() {
        let height = i as u32;
        let hash = block.block_hash().to_byte_array().to_vec();
        let data = utils::consensus_encode(block)?;
        node.add_block(height, hash, data);
    }
    
    // Create sync configuration
    let config = SyncConfig {
        start_block: 0,
        exit_at: Some(5), // Process only first 5 blocks for testing
        pipeline_size: Some(2),
        max_reorg_depth: 10,
        reorg_check_threshold: 3,
    };
    
    // Create the generic sync engine
    let mut sync_engine = MetashrewSync::new(node, storage, runtime, config);
    
    // Test individual block processing
    sync_engine.process_single_block(0).await?;
    sync_engine.process_single_block(1).await?;
    
    // Test status
    let status = sync_engine.get_status().await?;
    assert_eq!(status.current_height, 2);
    assert_eq!(status.tip_height, 9);
    assert_eq!(status.blocks_behind, 7);
    
    // Test JSON-RPC interface
    let height_result = sync_engine.metashrew_height().await?;
    assert_eq!(height_result, 1); // Last processed block
    
    // Test view function - use real 'blocktracker' function
    let view_result = sync_engine.metashrew_view(
        "blocktracker".to_string(),
        "".to_string(), // blocktracker doesn't need input
        "1".to_string(),
    ).await?;
    assert!(view_result.starts_with("0x"));
    
    // Test getblock view function with height input
    let height_input = format!("{:02x}{:02x}{:02x}{:02x}", 1, 0, 0, 0); // Height 1 as little-endian hex
    let getblock_result = sync_engine.metashrew_view(
        "getblock".to_string(),
        height_input,
        "1".to_string(),
    ).await?;
    assert!(getblock_result.starts_with("0x"));
    
    // Test preview function - use real 'blocktracker' function
    let preview_result = sync_engine.metashrew_preview(
        "cafebabe".to_string(), // dummy block data
        "blocktracker".to_string(),
        "".to_string(), // blocktracker doesn't need input
        "1".to_string(),
    ).await?;
    assert!(preview_result.starts_with("0x"));
    
    // Test state root (should be empty for mock)
    let state_root_result = sync_engine.metashrew_stateroot("1".to_string()).await;
    assert!(state_root_result.is_err()); // Mock storage doesn't have state roots by default
    
    // Test snapshot info
    let snapshot_info = sync_engine.metashrew_snapshot().await?;
    assert!(snapshot_info.is_object());
    
    Ok(())
}

/// Test complete indexing pipeline with mock Bitcoin node - comprehensive E2E test
#[tokio::test]
async fn test_complete_indexing_pipeline() -> Result<()> {
    let mut indexer = MockIndexer::new().await?;
    
    // Process genesis
    indexer.process_next_block().await?;
    
    // Generate and process a chain of blocks
    indexer.generate_and_process(15).await?;
    
    // Verify all blocks are accessible
    for height in 0..=15 {
        let indexed_block = indexer.get_indexed_block(height).await?;
        let original_block = indexer.bitcoind.get_block(height).unwrap();
        assert_eq!(indexed_block.block_hash(), original_block.block_hash());
    }
    
    // Check database stats
    let (db_size, processed_height) = indexer.get_db_stats();
    assert!(db_size > 0);
    assert_eq!(processed_height, 16);
    
    // Verify blocktracker integrity
    let blocktracker = indexer.get_blocktracker().await?;
    assert_eq!(blocktracker.len(), 16);
    
    Ok(())
}
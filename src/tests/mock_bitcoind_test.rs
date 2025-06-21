//! Mock bitcoind backend test that simulates the complete rockshrew-mono pipeline
//! 
//! This test creates a mock Bitcoin node that produces realistic blocks and tests
//! the complete indexing workflow with metashrew-minimal.

use super::{TestConfig, block_builder::*};
use anyhow::Result;
use memshrew_runtime::{MemStoreRuntime, MemStoreAdapter, KeyValueStoreLike};
use metashrew_support::utils;
use metashrew_support::byte_view::ByteView;
use bitcoin::{Block, BlockHash};
use std::collections::VecDeque;
use tokio::time::{sleep, Duration};

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

#[tokio::test]
async fn test_mock_bitcoind_basic() -> Result<()> {
    let mut bitcoind = MockBitcoind::new();
    
    // Should start with genesis block
    assert_eq!(bitcoind.get_tip_height(), 0);
    assert!(bitcoind.get_block(0).is_some());
    
    // Generate some blocks
    let blocks = bitcoind.generate_blocks(5);
    assert_eq!(blocks.len(), 5);
    assert_eq!(bitcoind.get_tip_height(), 5);
    
    // Verify chain integrity
    for i in 1..=5 {
        let block = bitcoind.get_block(i).unwrap();
        let prev_block = bitcoind.get_block(i - 1).unwrap();
        assert_eq!(block.header.prev_blockhash, prev_block.block_hash());
    }
    
    Ok(())
}

#[tokio::test]
async fn test_mock_indexer_basic() -> Result<()> {
    let mut indexer = MockIndexer::new().await?;
    
    // Should start with genesis processed
    assert_eq!(indexer.get_processed_height(), 0);
    
    // Process genesis block
    let processed = indexer.process_next_block().await?;
    assert!(processed);
    assert_eq!(indexer.get_processed_height(), 1);
    
    // Generate and process more blocks
    indexer.generate_and_process(5).await?;
    assert_eq!(indexer.get_processed_height(), 6);
    
    // Verify blocktracker
    let blocktracker = indexer.get_blocktracker().await?;
    assert_eq!(blocktracker.len(), 6); // Should track 6 blocks (0-5)
    
    Ok(())
}

#[tokio::test]
async fn test_complete_indexing_pipeline() -> Result<()> {
    let mut indexer = MockIndexer::new().await?;
    
    println!("Starting complete indexing pipeline test...");
    
    // Process genesis
    indexer.process_next_block().await?;
    println!("Processed genesis block");
    
    // Generate and process a chain of blocks
    indexer.generate_and_process(10).await?;
    println!("Generated and processed 10 blocks");
    
    // Verify all blocks are accessible
    for height in 0..=10 {
        let indexed_block = indexer.get_indexed_block(height).await?;
        let original_block = indexer.bitcoind.get_block(height).unwrap();
        assert_eq!(indexed_block.block_hash(), original_block.block_hash());
    }
    println!("Verified all blocks are correctly indexed");
    
    // Check database stats
    let (db_size, processed_height) = indexer.get_db_stats();
    println!("Database contains {} entries, processed {} blocks", db_size, processed_height);
    assert!(db_size > 0);
    assert_eq!(processed_height, 11);
    
    // Verify blocktracker integrity
    let blocktracker = indexer.get_blocktracker().await?;
    assert_eq!(blocktracker.len(), 11);
    println!("Blocktracker correctly tracks {} blocks", blocktracker.len());
    
    Ok(())
}

#[tokio::test]
async fn test_chain_reorganization() -> Result<()> {
    let mut indexer = MockIndexer::new().await?;
    
    // Process initial chain
    indexer.process_next_block().await?; // Genesis
    indexer.generate_and_process(5).await?; // Blocks 1-5
    
    let initial_tip = indexer.bitcoind.get_tip_hash();
    println!("Initial chain tip: {}", initial_tip);
    
    // Simulate a 2-block reorg
    indexer.simulate_reorg(2).await?;
    
    let new_tip = indexer.bitcoind.get_tip_hash();
    println!("New chain tip after reorg: {}", new_tip);
    
    // Verify the tip changed
    assert_ne!(initial_tip, new_tip);
    
    // Verify we can still access all blocks
    for height in 0..=indexer.get_processed_height() - 1 {
        let block = indexer.get_indexed_block(height).await?;
        println!("Block {} hash: {}", height, block.block_hash());
    }
    
    println!("Chain reorganization test completed successfully");
    Ok(())
}

#[tokio::test]
async fn test_continuous_block_production() -> Result<()> {
    let mut indexer = MockIndexer::new().await?;
    
    // Process genesis
    indexer.process_next_block().await?;
    
    // Simulate continuous block production
    for round in 1..=5 {
        // Generate a few blocks
        indexer.generate_and_process(3).await?;
        
        // Verify indexer keeps up
        let (db_size, processed_height) = indexer.get_db_stats();
        println!("Round {}: {} blocks processed, {} DB entries", 
                round, processed_height, db_size);
        
        // Small delay to simulate real-world timing
        sleep(Duration::from_millis(10)).await;
    }
    
    // Final verification
    let final_height = indexer.get_processed_height();
    assert_eq!(final_height, 16); // Genesis + 5 rounds * 3 blocks
    
    let blocktracker = indexer.get_blocktracker().await?;
    assert_eq!(blocktracker.len(), final_height as usize);
    
    println!("Continuous block production test completed: {} blocks processed", final_height);
    Ok(())
}
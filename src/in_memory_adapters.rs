//! In-memory adapters for comprehensive e2e testing
use async_trait::async_trait;
use bitcoin::{Block, hashes::Hash};
use metashrew_sync::{
    BitcoinNodeAdapter, BlockInfo, ChainTip, SyncError, SyncResult,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// An in-memory Bitcoin node adapter for testing
pub struct InMemoryBitcoinNode {
    blocks: Arc<RwLock<HashMap<u32, Block>>>,
    tip: Arc<RwLock<ChainTip>>,
}

impl InMemoryBitcoinNode {
    pub fn new(genesis_block: Block) -> Self {
        let mut blocks = HashMap::new();
        let tip = ChainTip {
            height: 0,
            hash: genesis_block.block_hash().as_byte_array().to_vec(),
        };
        blocks.insert(0, genesis_block);
        Self {
            blocks: Arc::new(RwLock::new(blocks)),
            tip: Arc::new(RwLock::new(tip)),
        }
    }

    pub fn add_block(&self, block: Block, height: u32) {
        let mut blocks = self.blocks.write().unwrap();
        let mut tip = self.tip.write().unwrap();
        tip.height = height;
        tip.hash = block.block_hash().as_byte_array().to_vec();
        blocks.insert(height, block);
    }
}

#[async_trait]
impl BitcoinNodeAdapter for InMemoryBitcoinNode {
    async fn get_tip_height(&self) -> SyncResult<u32> {
        Ok(self.tip.read().unwrap().height)
    }

    async fn get_block_hash(&self, height: u32) -> SyncResult<Vec<u8>> {
        self.blocks
            .read()
            .unwrap()
            .get(&height)
            .map(|b| b.block_hash().as_byte_array().to_vec())
            .ok_or_else(|| SyncError::BitcoinNode(format!("Block not found at height {}", height)))
    }

    async fn get_block_data(&self, height: u32) -> SyncResult<Vec<u8>> {
        self.blocks
            .read()
            .unwrap()
            .get(&height)
            .map(|b| bitcoin::consensus::serialize(b))
            .ok_or_else(|| SyncError::BitcoinNode(format!("Block not found at height {}", height)))
    }

    async fn get_block_info(&self, height: u32) -> SyncResult<BlockInfo> {
        let block = self
            .blocks
            .read()
            .unwrap()
            .get(&height)
            .cloned()
            .ok_or_else(|| SyncError::BitcoinNode(format!("Block not found at height {}", height)))?;
        Ok(BlockInfo {
            height,
            hash: block.block_hash().as_byte_array().to_vec(),
            data: bitcoin::consensus::serialize(&block),
        })
    }

    async fn get_chain_tip(&self) -> SyncResult<ChainTip> {
        Ok(self.tip.read().unwrap().clone())
    }

    async fn is_connected(&self) -> bool {
        true
    }
}
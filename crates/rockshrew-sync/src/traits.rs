//! Trait definitions for the generic sync framework

use async_trait::async_trait;
use crate::{SyncResult, BlockInfo, ChainTip, ViewCall, ViewResult, PreviewCall};

/// Trait for Bitcoin node adapters that provide blockchain data
#[async_trait]
pub trait BitcoinNodeAdapter: Send + Sync {
    /// Get the current blockchain tip height
    async fn get_tip_height(&self) -> SyncResult<u32>;
    
    /// Get the hash of a block at a specific height
    async fn get_block_hash(&self, height: u32) -> SyncResult<Vec<u8>>;
    
    /// Get the raw block data at a specific height
    async fn get_block_data(&self, height: u32) -> SyncResult<Vec<u8>>;
    
    /// Get block information (height, hash, data) at a specific height
    async fn get_block_info(&self, height: u32) -> SyncResult<BlockInfo>;
    
    /// Get the current chain tip information
    async fn get_chain_tip(&self) -> SyncResult<ChainTip>;
    
    /// Check if the node is reachable and responsive
    async fn is_connected(&self) -> bool;
}

/// Trait for storage adapters that persist indexed data
#[async_trait]
pub trait StorageAdapter: Send + Sync {
    /// Get the current indexed height
    async fn get_indexed_height(&self) -> SyncResult<u32>;
    
    /// Set the current indexed height
    async fn set_indexed_height(&self, height: u32) -> SyncResult<()>;
    
    /// Store a block hash for a given height
    async fn store_block_hash(&self, height: u32, hash: &[u8]) -> SyncResult<()>;
    
    /// Get a stored block hash for a given height
    async fn get_block_hash(&self, height: u32) -> SyncResult<Option<Vec<u8>>>;
    
    /// Store a state root for a given height
    async fn store_state_root(&self, height: u32, root: &[u8]) -> SyncResult<()>;
    
    /// Get a state root for a given height
    async fn get_state_root(&self, height: u32) -> SyncResult<Option<Vec<u8>>>;
    
    /// Rollback storage to a specific height (remove data after this height)
    async fn rollback_to_height(&self, height: u32) -> SyncResult<()>;
    
    /// Check if storage is available and writable
    async fn is_available(&self) -> bool;
    
    /// Get storage statistics (size, entries, etc.)
    async fn get_stats(&self) -> SyncResult<StorageStats>;
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub total_entries: usize,
    pub indexed_height: u32,
    pub storage_size_bytes: Option<u64>,
}

/// Trait for runtime adapters that execute WASM indexer modules
#[async_trait]
pub trait RuntimeAdapter: Send + Sync {
    /// Process a block with the WASM indexer
    async fn process_block(&mut self, height: u32, block_data: &[u8]) -> SyncResult<()>;
    
    /// Process a block atomically, returning all database operations in a batch
    /// This ensures atomicity by collecting all operations before committing
    async fn process_block_atomic(&mut self, height: u32, block_data: &[u8], block_hash: &[u8]) -> SyncResult<AtomicBlockResult>;
    
    /// Execute a view function
    async fn execute_view(&self, call: ViewCall) -> SyncResult<ViewResult>;
    
    /// Execute a preview function (with block data)
    async fn execute_preview(&self, call: PreviewCall) -> SyncResult<ViewResult>;
    
    /// Get the state root at a specific height
    async fn get_state_root(&self, height: u32) -> SyncResult<Vec<u8>>;
    
    /// Refresh the runtime memory (cleanup/reset)
    async fn refresh_memory(&mut self) -> SyncResult<()>;
    
    /// Check if the runtime is ready for processing
    async fn is_ready(&self) -> bool;
    
    /// Get runtime statistics
    async fn get_stats(&self) -> SyncResult<RuntimeStats>;
}

/// Result of atomic block processing containing all operations to be committed
#[derive(Debug, Clone)]
pub struct AtomicBlockResult {
    /// The state root calculated after processing
    pub state_root: Vec<u8>,
    /// All database operations as a serialized batch
    pub batch_data: Vec<u8>,
    /// Block height that was processed
    pub height: u32,
    /// Block hash
    pub block_hash: Vec<u8>,
}

/// Runtime statistics
#[derive(Debug, Clone)]
pub struct RuntimeStats {
    pub memory_usage_bytes: usize,
    pub blocks_processed: u32,
    pub last_refresh_height: Option<u32>,
}

/// Trait for JSON-RPC API providers
#[async_trait]
pub trait JsonRpcProvider: Send + Sync {
    /// Execute a view function call
    async fn metashrew_view(&self, function_name: String, input_hex: String, height: String) -> SyncResult<String>;
    
    /// Execute a preview function call
    async fn metashrew_preview(&self, block_hex: String, function_name: String, input_hex: String, height: String) -> SyncResult<String>;
    
    /// Get the current indexed height
    async fn metashrew_height(&self) -> SyncResult<u32>;
    
    /// Get a block hash by height
    async fn metashrew_getblockhash(&self, height: u32) -> SyncResult<String>;
    
    /// Get a state root by height
    async fn metashrew_stateroot(&self, height: String) -> SyncResult<String>;
    
    /// Get snapshot information
    async fn metashrew_snapshot(&self) -> SyncResult<serde_json::Value>;
}

/// Trait for the complete sync engine that coordinates all components
#[async_trait]
pub trait SyncEngine: Send + Sync {
    /// Start the synchronization process
    async fn start(&mut self) -> SyncResult<()>;
    
    /// Stop the synchronization process
    async fn stop(&mut self) -> SyncResult<()>;
    
    /// Get the current sync status
    async fn get_status(&self) -> SyncResult<SyncStatus>;
    
    /// Process a single block (for testing)
    async fn process_single_block(&mut self, height: u32) -> SyncResult<()>;
    
    /// Detect and handle chain reorganizations
    async fn handle_reorg(&mut self) -> SyncResult<u32>;
}

/// Sync engine status
#[derive(Debug, Clone)]
pub struct SyncStatus {
    pub is_running: bool,
    pub current_height: u32,
    pub tip_height: u32,
    pub blocks_behind: u32,
    pub last_block_time: Option<std::time::SystemTime>,
    pub blocks_per_second: f64,
}
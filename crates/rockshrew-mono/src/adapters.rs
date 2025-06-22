//! Adapter implementations for rockshrew-mono to use the generic rockshrew-sync framework

use async_trait::async_trait;
use anyhow::{anyhow, Result};
use hex;
use log::{debug, error, info, warn};
use rocksdb::DB;
use serde_json::{Number, Value};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use metashrew_runtime::{MetashrewRuntime, KeyValueStoreLike};
use rockshrew_runtime::RocksDBRuntimeAdapter;
use rockshrew_sync::{
    BitcoinNodeAdapter, BlockInfo, ChainTip, PreviewCall, RuntimeAdapter, RuntimeStats,
    StorageAdapter, StorageStats, SyncError, SyncResult, ViewCall, ViewResult,
};

use crate::ssh_tunnel::{SshTunnel, SshTunnelConfig, TunneledResponse, make_request_with_tunnel};
use crate::{JsonRpcRequest, BlockCountResponse, BlockHashResponse};

/// Bitcoin node adapter that connects to a real Bitcoin node via RPC
#[derive(Clone)]
pub struct BitcoinRpcAdapter {
    rpc_url: String,
    auth: Option<String>,
    bypass_ssl: bool,
    tunnel_config: Option<SshTunnelConfig>,
    active_tunnel: Arc<tokio::sync::Mutex<Option<SshTunnel>>>,
}

impl BitcoinRpcAdapter {
    pub fn new(
        rpc_url: String,
        auth: Option<String>,
        bypass_ssl: bool,
        tunnel_config: Option<SshTunnelConfig>,
    ) -> Self {
        Self {
            rpc_url,
            auth,
            bypass_ssl,
            tunnel_config,
            active_tunnel: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    async fn post(&self, body: String) -> Result<TunneledResponse> {
        // Implement retry logic for network requests
        let max_retries = 5;
        let mut retry_delay = Duration::from_millis(500);
        let max_delay = Duration::from_secs(16);
        
        for attempt in 0..max_retries {
            // Get the existing tunnel if available
            let existing_tunnel = if self.tunnel_config.is_some() {
                let active_tunnel = self.active_tunnel.lock().await;
                active_tunnel.clone()
            } else {
                None
            };
            
            // Make the request with tunnel if needed
            match make_request_with_tunnel(
                &self.rpc_url,
                body.clone(),
                self.auth.clone(),
                self.tunnel_config.clone(),
                self.bypass_ssl,
                existing_tunnel
            ).await {
                Ok(tunneled_response) => {
                    // If this is a tunneled response with a tunnel, store it for reuse
                    if let Some(tunnel) = tunneled_response._tunnel.clone() {
                        if self.tunnel_config.is_some() {
                            debug!("Storing SSH tunnel for reuse on port {}", tunnel.local_port);
                            let mut active_tunnel = self.active_tunnel.lock().await;
                            *active_tunnel = Some(tunnel);
                        }
                    }
                    return Ok(tunneled_response);
                },
                Err(e) => {
                    error!("Request failed (attempt {}): {}", attempt + 1, e);
                    
                    // If the error might be related to the tunnel, clear the active tunnel
                    if self.tunnel_config.is_some() {
                        let mut active_tunnel = self.active_tunnel.lock().await;
                        if active_tunnel.is_some() {
                            debug!("Clearing active tunnel due to error");
                            *active_tunnel = None;
                        }
                    }
                    
                    // Calculate exponential backoff with jitter
                    let jitter = {
                        use rand::Rng;
                        rand::thread_rng().gen_range(0..=100) as u64
                    };
                    retry_delay = std::cmp::min(
                        max_delay,
                        retry_delay * 2 + Duration::from_millis(jitter)
                    );
                    
                    debug!("Request failed (attempt {}): {}, retrying in {:?}",
                           attempt + 1, e, retry_delay);
                    tokio::time::sleep(retry_delay).await;
                }
            }
        }
        
        Err(anyhow!("Max retries exceeded"))
    }
}

#[async_trait]
impl BitcoinNodeAdapter for BitcoinRpcAdapter {
    async fn get_tip_height(&self) -> SyncResult<u32> {
        let tunneled_response = self
            .post(serde_json::to_string(&JsonRpcRequest {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| SyncError::BitcoinNode(format!("Time error: {}", e)))?
                    .as_secs()
                    .try_into()
                    .map_err(|e| SyncError::BitcoinNode(format!("Time conversion error: {}", e)))?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblockcount"),
                params: vec![],
            })
            .map_err(|e| SyncError::BitcoinNode(format!("JSON serialization error: {}", e)))?)
            .await
            .map_err(|e| SyncError::BitcoinNode(format!("RPC request failed: {}", e)))?;

        let result: BlockCountResponse = tunneled_response.json().await
            .map_err(|e| SyncError::BitcoinNode(format!("JSON parsing error: {}", e)))?;
        let count = result.result
            .ok_or_else(|| SyncError::BitcoinNode("missing result from JSON-RPC response".to_string()))?;
        Ok(count)
    }
    
    async fn get_block_hash(&self, height: u32) -> SyncResult<Vec<u8>> {
        let tunneled_response = self
            .post(serde_json::to_string(&JsonRpcRequest {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| SyncError::BitcoinNode(format!("Time error: {}", e)))?
                    .as_secs()
                    .try_into()
                    .map_err(|e| SyncError::BitcoinNode(format!("Time conversion error: {}", e)))?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblockhash"),
                params: vec![Value::Number(Number::from(height))],
            })
            .map_err(|e| SyncError::BitcoinNode(format!("JSON serialization error: {}", e)))?)
            .await
            .map_err(|e| SyncError::BitcoinNode(format!("RPC request failed: {}", e)))?;

        let result: BlockHashResponse = tunneled_response.json().await
            .map_err(|e| SyncError::BitcoinNode(format!("JSON parsing error: {}", e)))?;
        let blockhash = result.result
            .ok_or_else(|| SyncError::BitcoinNode("missing result from JSON-RPC response".to_string()))?;
        hex::decode(blockhash)
            .map_err(|e| SyncError::BitcoinNode(format!("Hex decode error: {}", e)))
    }
    
    async fn get_block_data(&self, height: u32) -> SyncResult<Vec<u8>> {
        // First get the block hash
        let blockhash = self.get_block_hash(height).await?;
        
        let tunneled_response = self
            .post(serde_json::to_string(&JsonRpcRequest {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| SyncError::BitcoinNode(format!("Time error: {}", e)))?
                    .as_secs()
                    .try_into()
                    .map_err(|e| SyncError::BitcoinNode(format!("Time conversion error: {}", e)))?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblock"),
                params: vec![
                    Value::String(hex::encode(&blockhash)),
                    Value::Number(Number::from(0)),
                ],
            })
            .map_err(|e| SyncError::BitcoinNode(format!("JSON serialization error: {}", e)))?)
            .await
            .map_err(|e| SyncError::BitcoinNode(format!("RPC request failed: {}", e)))?;

        let result: BlockHashResponse = tunneled_response.json().await
            .map_err(|e| SyncError::BitcoinNode(format!("JSON parsing error: {}", e)))?;
        let block_hex = result.result
            .ok_or_else(|| SyncError::BitcoinNode("missing result from JSON-RPC response".to_string()))?;
        
        hex::decode(block_hex)
            .map_err(|e| SyncError::BitcoinNode(format!("Hex decode error: {}", e)))
    }
    
    async fn get_block_info(&self, height: u32) -> SyncResult<BlockInfo> {
        let hash = self.get_block_hash(height).await?;
        let data = self.get_block_data(height).await?;
        Ok(BlockInfo { height, hash, data })
    }
    
    async fn get_chain_tip(&self) -> SyncResult<ChainTip> {
        let height = self.get_tip_height().await?;
        let hash = self.get_block_hash(height).await?;
        Ok(ChainTip { height, hash })
    }
    
    async fn is_connected(&self) -> bool {
        // Try to get the tip height to test connectivity
        self.get_tip_height().await.is_ok()
    }
}

/// RocksDB storage adapter for persistent storage
#[derive(Clone)]
pub struct RocksDBStorageAdapter {
    db: Arc<DB>,
}

impl RocksDBStorageAdapter {
    pub fn new(db: Arc<DB>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl StorageAdapter for RocksDBStorageAdapter {
    async fn get_indexed_height(&self) -> SyncResult<u32> {
        // Use the same height tracking mechanism as the original implementation
        let height_key = b"__INTERNAL/height".to_vec();
        match self.db.get(&height_key) {
            Ok(Some(value)) => {
                if value.len() >= 4 {
                    let height_bytes: [u8; 4] = value[..4].try_into()
                        .map_err(|_| SyncError::Storage("Invalid height data".to_string()))?;
                    Ok(u32::from_le_bytes(height_bytes))
                } else {
                    Ok(0)
                }
            },
            Ok(None) => Ok(0),
            Err(e) => Err(SyncError::Storage(format!("Database error: {}", e))),
        }
    }
    
    async fn set_indexed_height(&self, height: u32) -> SyncResult<()> {
        let height_key = b"__INTERNAL/height".to_vec();
        let height_bytes = height.to_le_bytes();
        self.db.put(&height_key, &height_bytes)
            .map_err(|e| SyncError::Storage(format!("Failed to store height: {}", e)))
    }
    
    async fn store_block_hash(&self, height: u32, hash: &[u8]) -> SyncResult<()> {
        // Use the same key format as the constant in main.rs
        let blockhash_key = format!("/__INTERNAL/height-to-hash/{}", height).into_bytes();
        debug!("Storing blockhash for height {} with key: {}", height, hex::encode(&blockhash_key));
        self.db.put(&blockhash_key, hash)
            .map_err(|e| SyncError::Storage(format!("Failed to store blockhash: {}", e)))?;
        debug!("Successfully stored blockhash for height {}: {}", height, hex::encode(hash));
        Ok(())
    }
    
    async fn get_block_hash(&self, height: u32) -> SyncResult<Option<Vec<u8>>> {
        // Use the same key format as the constant in main.rs
        let blockhash_key = format!("/__INTERNAL/height-to-hash/{}", height).into_bytes();
        debug!("Looking up blockhash for height {} with key: {}", height, hex::encode(&blockhash_key));
        
        match self.db.get(&blockhash_key) {
            Ok(Some(value)) => {
                debug!("Found blockhash for height {}: {}", height, hex::encode(&value));
                Ok(Some(value))
            },
            Ok(None) => {
                debug!("No blockhash found for height {}", height);
                Ok(None)
            },
            Err(e) => {
                error!("Error looking up blockhash for height {}: {}", height, e);
                Err(SyncError::Storage(format!("Database error: {}", e)))
            }
        }
    }
    
    async fn store_state_root(&self, height: u32, root: &[u8]) -> SyncResult<()> {
        // Use the generic SMT implementation with RocksDBRuntimeAdapter
        let adapter = RocksDBRuntimeAdapter::new(self.db.clone());
        let mut smt_helper = metashrew_runtime::smt::SMTHelper::new(adapter);
        
        // Store the state root using the same format as the WASM runtime
        let root_key = format!("smt:root:{}", height).into_bytes();
        smt_helper.storage.put(&root_key, root)
            .map_err(|e| SyncError::Storage(format!("Failed to store state root: {}", e)))
    }
    
    async fn get_state_root(&self, height: u32) -> SyncResult<Option<Vec<u8>>> {
        // Use the generic SMT implementation with RocksDBRuntimeAdapter
        let adapter = RocksDBRuntimeAdapter::new(self.db.clone());
        let smt_helper = metashrew_runtime::smt::SMTHelper::new(adapter);
        
        match smt_helper.get_smt_root_at_height(height) {
            Ok(root) => Ok(Some(root.to_vec())),
            Err(_) => Ok(None),
        }
    }
    
    async fn rollback_to_height(&self, height: u32) -> SyncResult<()> {
        info!("Starting rollback to height {}", height);
        
        // Get the current indexed height
        let current_height = self.get_indexed_height().await?;
        
        if height >= current_height {
            info!("Target height {} >= current height {}, no rollback needed", height, current_height);
            return Ok(());
        }
        
        info!("Rolling back from height {} to height {}", current_height, height);
        
        // Remove blockhashes for heights > target_height
        for h in (height + 1)..=current_height {
            let blockhash_key = format!("/__INTERNAL/height-to-hash/{}", h).into_bytes();
            if let Err(e) = self.db.delete(&blockhash_key) {
                warn!("Failed to delete blockhash for height {}: {}", h, e);
            } else {
                debug!("Deleted blockhash for height {}", h);
            }
        }
        
        // Remove state roots for heights > target_height using the same format as WASM runtime
        for h in (height + 1)..=current_height {
            let root_key = format!("smt:root:{}", h).into_bytes();
            if let Err(e) = self.db.delete(&root_key) {
                warn!("Failed to delete state root for height {}: {}", h, e);
            } else {
                debug!("Deleted state root for height {}", h);
            }
        }
        
        // Update the height marker
        self.set_indexed_height(height).await?;
        
        info!("Successfully completed rollback to height {}", height);
        Ok(())
    }
    
    async fn is_available(&self) -> bool {
        // Simple availability check - try to read a key
        self.db.get(b"__test").is_ok()
    }
    
    async fn get_stats(&self) -> SyncResult<StorageStats> {
        let indexed_height = self.get_indexed_height().await?;
        
        // Get approximate database size and entry count
        // Note: RocksDB doesn't provide exact counts efficiently, so we estimate
        let storage_size_bytes = None; // Could implement if needed
        let total_entries = 0; // Could implement if needed
        
        Ok(StorageStats {
            total_entries,
            indexed_height,
            storage_size_bytes,
        })
    }
}

/// MetashrewRuntime adapter that wraps the actual MetashrewRuntime
pub struct MetashrewRuntimeAdapter {
    runtime: Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
    db: Arc<DB>,
}

impl MetashrewRuntimeAdapter {
    pub fn new(runtime: Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>, db: Arc<DB>) -> Self {
        Self { runtime, db }
    }

    /// Check if memory needs to be refreshed based on its size
    fn should_refresh_memory(&self, runtime: &mut MetashrewRuntime<RocksDBRuntimeAdapter>, height: u32) -> bool {
        // Get the memory instance
        if let Some(memory) = runtime.instance.get_memory(&mut runtime.wasmstore, "memory") {
            // Get the memory size in bytes
            let memory_size = memory.data_size(&mut runtime.wasmstore);
            
            // 1.75GB in bytes = 1.75 * 1024 * 1024 * 1024
            let threshold_gb = 1.75;
            let threshold_bytes = (threshold_gb * 1024.0 * 1024.0 * 1024.0) as usize;
            
            // Check if memory size is approaching the limit
            if memory_size >= threshold_bytes {
                info!("Memory usage approaching threshold of {:.2}GB at block {}", threshold_gb, height);
                info!("Performing preemptive memory refresh to prevent out-of-memory errors");
                return true;
            } else if height % 1000 == 0 {
                // Log memory stats periodically for monitoring
                let memory_size_mb = memory_size as f64 / 1_048_576.0;
                info!("Memory usage at block {}: {:.2} MB", height, memory_size_mb);
            }
        } else {
            debug!("Could not get memory instance for block {}", height);
        }
        
        false
    }
}

#[async_trait]
impl RuntimeAdapter for MetashrewRuntimeAdapter {
    async fn process_block(&mut self, height: u32, block_data: &[u8]) -> SyncResult<()> {
        info!("PROCESS_BLOCK: Starting to process block {} ({} bytes)", height, block_data.len());
        
        // Get a lock on the runtime
        let mut runtime = self.runtime.write().await;
        
        // Get the block data size for logging
        let block_size = block_data.len();
        
        // Set block data
        {
            let mut context = runtime.context.lock()
                .map_err(|e| SyncError::Runtime(format!("Failed to lock context: {}", e)))?;
            context.block = block_data.to_vec();
            context.height = height;
            context.db.set_height(height);
        } // Release context lock
        
        // Check if memory usage is approaching the limit and refresh if needed
        if self.should_refresh_memory(&mut runtime, height) {
            match runtime.refresh_memory() {
                Ok(_) => info!("Successfully performed preemptive memory refresh at block {}", height),
                Err(e) => {
                    error!("Failed to perform preemptive memory refresh: {}", e);
                    info!("Continuing execution despite memory refresh failure");
                    // Continue with execution even if preemptive refresh fails
                }
            }
        }
        
        // Check if this block has already been processed by checking if we have a state root for this height
        let adapter = RocksDBRuntimeAdapter::new(self.db.clone());
        let smt_helper = metashrew_runtime::smt::SMTHelper::new(adapter);
        let already_processed = smt_helper.get_smt_root_at_height(height).is_ok();
        
        if already_processed {
            info!("Block {} already processed (state root exists), skipping WASM execution", height);
            return Ok(());
        }
        
        // Execute the runtime
        debug!("About to call runtime.run() for block {}", height);
        match runtime.run() {
            Ok(_) => {
                info!("RUNTIME_RUN: Successfully executed WASM for block {} (size: {} bytes)", height, block_size);
                
                // State root calculation is now handled inside the WASM runtime's __flush function
                // This ensures the state root is calculated with access to all the key-value pairs
                // that were just flushed, providing consistency with the test suite
                debug!("State root calculation handled by WASM runtime for height {}", height);
                Ok(())
            },
            Err(_e) => {
                // Before retrying, check if the block was actually processed successfully
                let adapter = RocksDBRuntimeAdapter::new(self.db.clone());
                let smt_helper = metashrew_runtime::smt::SMTHelper::new(adapter);
                if smt_helper.get_smt_root_at_height(height).is_ok() {
                    info!("Block {} appears to have been processed despite WASM error, skipping retry", height);
                    return Ok(());
                }
                
                // Try to refresh memory
                match runtime.refresh_memory() {
                    Ok(_) => {
                        // Check again if block was processed before retrying
                        let adapter = RocksDBRuntimeAdapter::new(self.db.clone());
                        let smt_helper = metashrew_runtime::smt::SMTHelper::new(adapter);
                        if smt_helper.get_smt_root_at_height(height).is_ok() {
                            info!("Block {} was processed during memory refresh, skipping retry", height);
                            return Ok(());
                        }
                        
                        // Try running again after memory refresh
                        debug!("About to call runtime.run() RETRY for block {} after memory refresh", height);
                        match runtime.run() {
                            Ok(_) => {
                                info!("RUNTIME_RUN_RETRY: Successfully executed WASM for block {} after memory refresh (size: {} bytes)", height, block_size);
                                
                                // State root calculation is handled inside the WASM runtime's __flush function
                                debug!("State root calculation handled by WASM runtime after retry for height {}", height);
                                
                                Ok(())
                            },
                            Err(run_err) => {
                                error!("Failed to process block {} after memory refresh: {}", height, run_err);
                                Err(SyncError::Runtime(format!("Runtime execution failed after retry: {}", run_err)))
                            }
                        }
                    },
                    Err(refresh_err) => {
                        error!("Memory refresh failed: {}", refresh_err);
                        Err(SyncError::Runtime(format!("Memory refresh failed: {}", refresh_err)))
                    }
                }
            }
        }
    }
    
    async fn get_state_root(&self, height: u32) -> SyncResult<Vec<u8>> {
        let adapter = RocksDBRuntimeAdapter::new(self.db.clone());
        let smt_helper = metashrew_runtime::smt::SMTHelper::new(adapter);
        match smt_helper.get_smt_root_at_height(height) {
            Ok(root) => Ok(root.to_vec()),
            Err(e) => Err(SyncError::Runtime(format!("Failed to get state root for height {}: {}", height, e))),
        }
    }
    
    async fn execute_view(&self, call: ViewCall) -> SyncResult<ViewResult> {
        let runtime = self.runtime.read().await;
        
        let result = runtime.view(call.function_name, &call.input_data, call.height)
            .await
            .map_err(|e| SyncError::ViewFunction(format!("View function failed: {}", e)))?;
        
        Ok(ViewResult { data: result })
    }
    
    async fn execute_preview(&self, call: PreviewCall) -> SyncResult<ViewResult> {
        let runtime = self.runtime.read().await;
        
        let result = runtime.preview_async(&call.block_data, call.function_name, &call.input_data, call.height)
            .await
            .map_err(|e| SyncError::ViewFunction(format!("Preview function failed: {}", e)))?;
        
        Ok(ViewResult { data: result })
    }
    
    async fn refresh_memory(&mut self) -> SyncResult<()> {
        let mut runtime = self.runtime.write().await;
        runtime.refresh_memory()
            .map_err(|e| SyncError::Runtime(format!("Memory refresh failed: {}", e)))
    }
    
    async fn is_ready(&self) -> bool {
        // Simple readiness check - try to acquire a read lock
        self.runtime.try_read().is_ok()
    }
    
    async fn get_stats(&self) -> SyncResult<RuntimeStats> {
        let runtime = self.runtime.write().await;
        
        // Note: Getting memory usage requires mutable access to wasmstore twice,
        // which creates borrowing conflicts. For now, we'll return 0.
        // This could be improved by restructuring the MetashrewRuntime API.
        let memory_usage_bytes = 0;
        
        // Get current height as blocks processed
        let blocks_processed = {
            let context = runtime.context.lock()
                .map_err(|e| SyncError::Runtime(format!("Failed to lock context: {}", e)))?;
            context.height
        };
        
        Ok(RuntimeStats {
            memory_usage_bytes,
            blocks_processed,
            last_refresh_height: Some(blocks_processed),
        })
    }
}
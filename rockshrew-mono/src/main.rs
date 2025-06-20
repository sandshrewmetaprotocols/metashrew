use actix_cors::Cors;
use actix_web::error;
use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result as ActixResult};
use anyhow::{anyhow, Result};
use clap::Parser;
use env_logger;
use hex;
// Removed unused import
use log::{debug, info, error, warn};
use metashrew_runtime::KeyValueStoreLike;
use metashrew_runtime::MetashrewRuntime;
use num_cpus;
use rand::Rng;
use rocksdb::Options;
use rockshrew_runtime::{query_height, set_label, RocksDBRuntimeAdapter};

// Import our SMT helper module
mod smt_helper;
use smt_helper::SMTHelper;
use serde::{Deserialize, Serialize};
use serde_json::{self, Number, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio;
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;
use std::sync::atomic::{AtomicU32, Ordering};

// Import our SSH tunneling module
mod ssh_tunnel;
use ssh_tunnel::{SshTunnel, SshTunnelConfig, TunneledResponse, make_request_with_tunnel, parse_daemon_rpc_url};

// Import our snapshot module
mod snapshot;
use snapshot::{SnapshotConfig, SnapshotManager};

// Extension trait for RwLock to add async_read method
trait RwLockExt<T> {
    async fn async_read<'a>(&'a self) -> tokio::sync::RwLockReadGuard<'a, T> where T: 'a;
}

impl<T> RwLockExt<T> for tokio::sync::RwLock<T> {
    async fn async_read<'a>(&'a self) -> tokio::sync::RwLockReadGuard<'a, T> where T: 'a {
        // Properly use async/await pattern instead of blocking
        self.read().await
    }
}

const HEIGHT_TO_HASH: &'static str = "/__INTERNAL/height-to-hash/";
static CURRENT_HEIGHT: AtomicU32 = AtomicU32::new(0);

// Block processing result for the pipeline
#[derive(Debug)]
enum BlockResult {
    Success(u32),  // Block height that was successfully processed
    Error(u32, anyhow::Error),  // Block height and error
}

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    daemon_rpc_url: String,
    #[arg(long, required_unless_present = "repo")]
    indexer: Option<PathBuf>,
    #[arg(long)]
    db_path: PathBuf,
    #[arg(long)]
    start_block: Option<u32>,
    #[arg(long)]
    auth: Option<String>,
    #[arg(long, env = "HOST", default_value = "127.0.0.1")]
    host: String,
    #[arg(long, env = "PORT", default_value_t = 8080)]
    port: u16,
    #[arg(long)]
    label: Option<String>,
    #[arg(long)]
    exit_at: Option<u32>,
    #[arg(long, help = "Size of the processing pipeline (default: auto-determined based on CPU cores)")]
    pipeline_size: Option<usize>,
    #[arg(long, help = "CORS allowed origins (e.g., '*' for all origins, or specific domains)")]
    cors: Option<String>,
    #[arg(long, help = "Directory to store snapshots for remote sync")]
    snapshot_directory: Option<PathBuf>,
    #[arg(long, help = "Interval in blocks to create snapshots (e.g., 1000)", default_value_t = 1000)]
    snapshot_interval: u32,
    #[arg(long, help = "URL to a remote snapshot repository to sync from")]
    repo: Option<String>,
}

#[derive(Clone)]
struct AppState {
    runtime: Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
}

// JSON-RPC request structure
#[derive(Serialize, Deserialize)]
struct JsonRpcRequest {
    id: u32,
    jsonrpc: String,
    method: String,
    params: Vec<Value>,
}

#[derive(Serialize)]
struct JsonRpcResult {
    id: u32,
    result: String,
    jsonrpc: String,
}

#[derive(Serialize)]
struct JsonRpcError {
    id: u32,
    error: JsonRpcErrorObject,
    jsonrpc: String,
}

#[derive(Serialize)]
struct JsonRpcErrorObject {
    code: i32,
    message: String,
    data: Option<String>,
}

// JSON-RPC response structure for internal use
#[derive(Deserialize)]
#[allow(dead_code)]
struct JsonRpcResponse {
    id: u32,
    result: Option<Value>,
    error: Option<JsonRpcErrorInternal>,
    jsonrpc: String,
}

// JSON-RPC error structure for internal use
#[derive(Deserialize)]
#[allow(dead_code)]
struct JsonRpcErrorInternal {
    code: i32,
    message: String,
    data: Option<Value>,
}

#[derive(Debug)]
struct IndexerError(anyhow::Error);

impl std::fmt::Display for IndexerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<anyhow::Error> for IndexerError {
    fn from(err: anyhow::Error) -> Self {
        IndexerError(err)
    }
}

impl error::ResponseError for IndexerError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::Ok().json(JsonRpcError {
            id: 0, // Generic ID since we lost context
            error: JsonRpcErrorObject {
                code: -32000,
                message: self.0.to_string(),
                data: None,
            },
            jsonrpc: "2.0".to_string(),
        })
    }
}

// Block count response structure
#[derive(Deserialize)]
#[allow(dead_code)]
struct BlockCountResponse {
    id: u32,
    result: Option<u32>,
    error: Option<Value>,
}

// Block hash response structure
#[derive(Deserialize)]
#[allow(dead_code)]
struct BlockHashResponse {
    id: u32,
    result: Option<String>,
    error: Option<Value>,
}

pub struct MetashrewRocksDBSync {
    runtime: Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
    args: Args,
    start_block: u32,
    rpc_url: String,
    auth: Option<String>,
    bypass_ssl: bool,
    tunnel_config: Option<SshTunnelConfig>,
    active_tunnel: Arc<tokio::sync::Mutex<Option<SshTunnel>>>,
    fetcher_thread_id_tx: Option<tokio::sync::mpsc::Sender<(String, std::thread::ThreadId)>>,
    processor_thread_id_tx: Option<tokio::sync::mpsc::Sender<(String, std::thread::ThreadId)>>,
    fetcher_thread_id: std::sync::Mutex<Option<std::thread::ThreadId>>,
    processor_thread_id: std::sync::Mutex<Option<std::thread::ThreadId>>,
    snapshot_manager: Option<Arc<tokio::sync::Mutex<SnapshotManager>>>,
}

impl MetashrewRocksDBSync {
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
                    // so we'll create a new one on the next attempt
                    if self.tunnel_config.is_some() {
                        let mut active_tunnel = self.active_tunnel.lock().await;
                        if active_tunnel.is_some() {
                            debug!("Clearing active tunnel due to error");
                            *active_tunnel = None;
                        }
                    }
                    
                    // Calculate exponential backoff with jitter
                    let jitter = rand::thread_rng().gen_range(0..=100) as u64;
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

    async fn fetch_blockcount(&self) -> Result<u32> {
        let tunneled_response = self
            .post(serde_json::to_string(&JsonRpcRequest {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_secs()
                    .try_into()?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblockcount"),
                params: vec![],
            })?)
            .await?;

        let result: BlockCountResponse = tunneled_response.json().await?;
        let count = result.result
            .ok_or_else(|| anyhow!("missing result from JSON-RPC response"))?;
        Ok(count)
    }

    pub async fn poll_connection<'a>(&'a self) -> Result<Arc<rocksdb::DB>> {
        let runtime_guard = self.runtime.read().await;
        let context = runtime_guard.context.lock().map_err(|_| anyhow!("Failed to lock context"))?;
        Ok(context.db.db.clone())
    }

    pub async fn query_height(&self) -> Result<u32> {
        let db = self.poll_connection().await?;
        query_height(db, self.start_block).await
    }

    // Process a single block
    async fn process_block(&self, height: u32, block_data: Vec<u8>) -> Result<()> {
        // Get a lock on the runtime with better error handling
        let mut runtime = match self.runtime.write().await {
            runtime => runtime,
        };
        
        // Create a key-value tracker for the WASM module updates
        let snapshot_manager_clone = self.snapshot_manager.clone();
        
        // Get the block data size for logging
        let block_size = block_data.len();
        
        // Set block data with better error handling
        {
            let mut context = runtime.context.lock()
                .map_err(|e| anyhow!("Failed to lock context: {}", e))?;
            context.block = block_data;
            context.height = height;
            context.db.set_height(height);
            
            // Set up key-value tracking for the WASM module
            if let Some(snapshot_manager) = &snapshot_manager_clone {
                let sm_clone = snapshot_manager.clone();
                context.db.set_kv_tracker(Some(Box::new(move |key: Vec<u8>, value: Vec<u8>| {
                    let mut manager = match sm_clone.try_lock() {
                        Ok(m) => m,
                        Err(_) => return, // Skip if we can't get the lock
                    };
                    manager.track_key_change(key, value);
                })));
            }
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
        
        // Execute the runtime with better error handling
        match runtime.run() {
            Ok(_) => {
                info!("Successfully processed block {} (size: {} bytes)", height, block_size);
                
                // Get database handle without holding the context lock across await points
                let db = {
                    let context = runtime.context.lock()
                        .map_err(|e| anyhow!("Failed to lock context: {}", e))?;
                    
                    // Clone the database handle before releasing the lock
                    context.db.db.clone()
                }; // Release context lock
                
                // Create an SMTHelper instance
                let smt_helper = SMTHelper::new(db.clone());
                
                // Fetch blockhash after releasing the context lock
                let blockhash = self.fetch_blockhash(height).await?;
                
                // Store blockhash for future reference
                let blockhash_key = format!("height-to-hash/{}", height).into_bytes();
                if let Err(e) = smt_helper.bst_put(&blockhash_key, &blockhash, height) {
                    error!("Failed to store blockhash for height {}: {}", height, e);
                } else {
                    debug!("Successfully stored blockhash for block {}", height);
                    
                    // If snapshot manager is enabled, track this key change directly
                    // (The WASM module updates are tracked through the kv_tracker)
                    if let Some(snapshot_manager) = &self.snapshot_manager {
                        let mut manager = snapshot_manager.lock().await;
                        manager.track_key_change(blockhash_key.clone(), blockhash.clone());
                    }
                }
                
                // Calculate and store the state root
                let new_root = match smt_helper.calculate_and_store_state_root(height) {
                    Ok(root) => root,
                    Err(e) => {
                        error!("Failed to calculate state root: {}", e);
                        [0u8; 32] // Use empty root if calculation fails
                    }
                };
                
                // Handle snapshot creation if needed
                if let Some(snapshot_manager) = &self.snapshot_manager {
                    // Now we can safely await on the snapshot manager lock
                    let mut manager = snapshot_manager.lock().await;
                    if manager.should_create_snapshot(height) {
                        // Track database changes since last snapshot
                        let last_snapshot_height = manager.last_snapshot_height;
                        if let Err(e) = manager.track_db_changes(&db, last_snapshot_height, height).await {
                            error!("Failed to track database changes for snapshot: {}", e);
                        }
                        
                        // Create the snapshot
                        if let Err(e) = manager.create_snapshot(height, &new_root).await {
                            error!("Failed to create snapshot at height {}: {}", height, e);
                        }
                    }
                }
                
                Ok(())
            },
            Err(_e) => {
                // Try to refresh memory with better error handling
                match runtime.refresh_memory() {
                    Ok(_) => {
                        // Try running again after memory refresh
                        match runtime.run() {
                            Ok(_) => {
                                info!("Successfully processed block {} after memory refresh (size: {} bytes)", height, block_size);
                                
                                // Handle snapshot tracking after successful retry
                                if let Some(snapshot_manager) = &self.snapshot_manager {
                                    // Get the database handle without holding the context lock across await points
                                    let db = {
                                        let context = runtime.context.lock()
                                            .map_err(|_| anyhow!("Failed to lock context"))?;
                                        context.db.db.clone()
                                    }; // Release context lock
                                    
                                    // Create SMT helper and get root
                                    let smt_helper = SMTHelper::new(db.clone());
                                    if let Ok(new_root) = smt_helper.get_smt_root_at_height(height) {
                                        // Now we can safely await on the snapshot manager lock
                                        let mut manager = snapshot_manager.lock().await;
                                        if manager.should_create_snapshot(height) {
                                            // Track database changes since last snapshot
                                            let last_snapshot_height = manager.last_snapshot_height;
                                            if let Err(e) = manager.track_db_changes(&db, last_snapshot_height, height).await {
                                                error!("Failed to track database changes for snapshot: {}", e);
                                            }
                                            
                                            // Create the snapshot
                                            if let Err(e) = manager.create_snapshot(height, &new_root).await {
                                                error!("Failed to create snapshot at height {}: {}", height, e);
                                            }
                                        }
                                    }
                                }
                                
                                Ok(())
                            },
                            Err(run_err) => {
                                error!("Failed to process block {} after memory refresh: {}", height, run_err);
                                Err(anyhow!(run_err))
                            }
                        }
                    },
                    Err(refresh_err) => {
                        error!("Memory refresh failed: {}", refresh_err);
                        Err(anyhow!(refresh_err))
                    }
                }
            }
        }
    }

    // Parallel processing with pipeline
    async fn run_pipeline(&mut self) -> Result<()> {
        let mut height: u32 = self.query_height().await?;
        CURRENT_HEIGHT.store(height, Ordering::SeqCst);
        
        // Track the last known best height to detect reorgs
        let mut last_best_height: u32 = height;
        
        // Determine optimal pipeline size based on CPU cores if not specified
        let pipeline_size = match self.args.pipeline_size {
            Some(size) => size,
            None => {
                let available_cpus = num_cpus::get();
                // Use approximately 1/2 of available cores for pipeline size, with reasonable min/max
                let auto_size = std::cmp::min(
                    std::cmp::max(5, available_cpus / 2),
                    16  // Cap at a reasonable maximum
                );
                info!("Auto-configuring pipeline size to {} based on {} available CPU cores", auto_size, available_cpus);
                auto_size
            }
        };
        
        // Create channels for the pipeline
        let (block_sender, mut block_receiver) = mpsc::channel::<(u32, Vec<u8>)>(pipeline_size);
        let (result_sender, mut result_receiver) = mpsc::channel::<BlockResult>(pipeline_size);
        
        // Spawn block fetcher task on dedicated thread
        let fetcher_handle = {
            let args = self.args.clone();
            let indexer = self.clone();
            let result_sender_clone = result_sender.clone();
            let block_sender_clone = block_sender.clone();
            
            // Spawn a task for the block fetcher
            tokio::spawn(async move {
                // Register this thread as the fetcher thread
                indexer.register_current_thread_as_fetcher();
                info!("Block fetcher process started on thread {:?}", std::thread::current().id());
                info!("Starting to fetch blocks from height {}", height);
                
                let mut current_height = height;
                
                loop {
                    // Check if we should exit
                    if let Some(exit_at) = args.exit_at {
                        if current_height >= exit_at {
                            info!("Fetcher reached exit-at block {}, shutting down", exit_at);
                            break;
                        }
                    }
                    
                    // Detect and handle any reorgs, get the next block to process
                    let next_height = match indexer.detect_and_handle_reorg().await {
                        Ok(h) => h,
                        Err(e) => {
                            error!("Failed to detect reorg: {}", e);
                            // Send error result and continue
                            if result_sender_clone.send(BlockResult::Error(current_height, e)).await.is_err() {
                                break;
                            }
                            sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    
                    // Check if we detected a reorg (next_height < current_height)
                    if next_height < current_height {
                        info!("Chain reorganization detected: rolling back from block {} to block {}", current_height, next_height);
                        current_height = next_height;
                    }
                    
                    // Get the remote tip to see if we have blocks to fetch
                    let remote_tip = match indexer.fetch_blockcount().await {
                        Ok(tip) => tip,
                        Err(e) => {
                            error!("Failed to fetch block count: {}", e);
                            sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    
                    // If we're caught up, wait for new blocks
                    if next_height > remote_tip {
                        debug!("Caught up to remote tip at height {}, waiting for new blocks", remote_tip);
                        sleep(Duration::from_secs(3)).await;
                        continue;
                    }
                    
                    // Fetch the block
                    match indexer.pull_block(next_height).await {
                        Ok(block_data) => {
                            debug!("Fetched block {} ({} bytes)", next_height, block_data.len());
                            // Send block to processor
                            if block_sender_clone.send((next_height, block_data)).await.is_err() {
                                break;
                            }
                        },
                        Err(e) => {
                            error!("Failed to fetch block {}: {}", next_height, e);
                            // Send error result
                            if result_sender_clone.send(BlockResult::Error(next_height, e)).await.is_err() {
                                break;
                            }
                            sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    }
                    
                    current_height = next_height + 1;
                }
                
                debug!("Block fetcher task completed");
            })
        };
        
        // Spawn block processor task on dedicated thread
        let processor_handle = {
            let indexer = self.clone();
            let result_sender_clone = result_sender.clone();
            
            // Spawn a task for the block processor
            {
                // Create a separate scope to avoid Send issues with MutexGuard
                let indexer_clone = indexer.clone();
                
                tokio::spawn(async move {
                    // Register this thread as the processor thread
                    indexer_clone.register_current_thread_as_processor();
                    info!("Block processor process started on thread {:?}", std::thread::current().id());
                    info!("Ready to process incoming blocks");
                    
                    while let Some((block_height, block_data)) = block_receiver.recv().await {
                        debug!("Processing block {} ({} bytes)", block_height, block_data.len());
                        
                        let result = match indexer_clone.process_block(block_height, block_data).await {
                            Ok(_) => BlockResult::Success(block_height),
                            Err(e) => BlockResult::Error(block_height, e),
                        };
                        
                        // Send result
                        if result_sender_clone.send(result).await.is_err() {
                            break;
                        }
                    }
                    
                    debug!("Block processor task completed");
                })
            }
        };
        
        // Main loop to handle results
        while let Some(result) = result_receiver.recv().await {
            match result {
                BlockResult::Success(processed_height) => {
                    info!("Block {} successfully processed and indexed", processed_height);
                    
                    // Check if this was a reorg (processed_height < last_best_height)
                    if processed_height < last_best_height {
                        info!("Chain reorganization confirmed: processed block {} (previous chain tip was at block {})",
                              processed_height, last_best_height);
                        
                        // Update height to reflect the reorg
                        height = processed_height + 1;
                        
                        // Update CURRENT_HEIGHT to reflect the reorg
                        CURRENT_HEIGHT.store(height, Ordering::SeqCst);
                        info!("Updated CURRENT_HEIGHT to {} after reorg", height);
                    } else {
                        // Normal case - no reorg
                        height = processed_height + 1;
                        CURRENT_HEIGHT.store(height, Ordering::SeqCst);
                    }
                    
                    // Update last_best_height
                    last_best_height = processed_height;
                },
                BlockResult::Error(failed_height, error) => {
                    error!("Failed to process block {}: {}", failed_height, error);
                    info!("Waiting 5 seconds before retrying...");
                    // We could implement more sophisticated error handling here
                    // For now, just wait and continue
                    sleep(Duration::from_secs(5)).await;
                }
            }
            
            // Check if we should exit
            if let Some(exit_at) = self.args.exit_at {
                if height > exit_at {
                    info!("Reached configured exit height at block {}, shutting down gracefully", exit_at);
                    break;
                }
            }
        }
        
        // Clean up
        drop(block_sender);
        drop(result_sender);
        
        // Wait for tasks to complete
        let (_fetcher_result, _processor_result) = tokio::join!(fetcher_handle, processor_handle);
        
        Ok(())
    }

    /// Improved reorg detection that properly handles chain reorganizations
    /// Returns the height from which we should start/resume indexing
    async fn detect_and_handle_reorg(&self) -> Result<u32> {
        // Get our current indexed height (the last block we successfully processed)
        let current_indexed_height = self.query_height().await?;
        
        // If we haven't indexed any blocks yet, start from the beginning
        if current_indexed_height == 0 {
            info!("No blocks indexed yet, starting from genesis");
            return Ok(0);
        }
        
        // Get the current blockchain tip from the remote node
        let remote_tip = self.fetch_blockcount().await?;
        
        // Start checking from our current indexed height and work backwards
        let mut check_height = current_indexed_height;
        let mut divergence_point: Option<u32> = None;
        
        info!("Checking for reorg: local tip={}, remote tip={}", current_indexed_height, remote_tip);
        
        // Check up to 100 blocks back or until we reach genesis
        let max_reorg_depth = 100;
        let start_check = if current_indexed_height > max_reorg_depth {
            current_indexed_height - max_reorg_depth
        } else {
            0
        };
        
        while check_height > start_check {
            // Get our local blockhash for this height
            let local_blockhash = self.get_blockhash(check_height).await;
            
            // If we don't have a local blockhash for a height we think we indexed,
            // this indicates a serious database inconsistency
            if local_blockhash.is_none() {
                error!("Missing local blockhash for height {} that should be indexed", check_height);
                // Treat this as a divergence point to be safe
                divergence_point = Some(check_height);
                break;
            }
            
            // Fetch the remote blockhash for this height
            let remote_blockhash = match self.fetch_blockhash(check_height).await {
                Ok(hash) => hash,
                Err(e) => {
                    // If we can't fetch the remote blockhash, the remote chain might not
                    // have this block yet (remote tip < our tip), which is fine
                    if check_height > remote_tip {
                        debug!("Remote chain doesn't have block {} yet (remote tip: {})", check_height, remote_tip);
                        check_height -= 1;
                        continue;
                    } else {
                        return Err(anyhow!("Failed to fetch remote blockhash for height {}: {}", check_height, e));
                    }
                }
            };
            
            // Compare the blockhashes
            if let Some(local_hash) = local_blockhash {
                if local_hash == remote_blockhash {
                    debug!("Blockhash match at height {}", check_height);
                    // Found a matching block, this is our common ancestor
                    break;
                } else {
                    warn!("Blockhash mismatch at height {}: local={}, remote={}",
                          check_height, hex::encode(&local_hash), hex::encode(&remote_blockhash));
                    divergence_point = Some(check_height);
                }
            }
            
            check_height -= 1;
        }
        
        // Handle the results
        match divergence_point {
            Some(diverge_height) => {
                warn!("Chain reorganization detected! Divergence at height {}", diverge_height);
                warn!("Need to rollback from height {} to height {}", current_indexed_height, diverge_height);
                
                // TODO: Implement actual rollback logic here
                // This would involve:
                // 1. Rolling back all SMT/BST changes after diverge_height
                // 2. Removing state roots after diverge_height
                // 3. Updating the current height marker
                
                // For now, we'll return the divergence point as the height to resume from
                // The caller should implement the actual rollback
                Ok(diverge_height)
            },
            None => {
                // No divergence found, we can continue from where we left off
                info!("No chain reorganization detected, continuing from height {}", current_indexed_height + 1);
                Ok(current_indexed_height + 1)
            }
        }
    }
    
    #[allow(dead_code)]
    /// Simplified best_height that just returns the next block to process
    /// after handling any potential reorgs
    async fn best_height(&self, _block_number: u32) -> Result<u32> {
        // First, detect and handle any reorgs
        let resume_height = self.detect_and_handle_reorg().await?;
        
        // Get the current remote tip
        let remote_tip = self.fetch_blockcount().await?;
        
        // If we're caught up or ahead, wait for new blocks
        if resume_height > remote_tip {
            info!("Caught up to remote tip at height {}, waiting for new blocks", remote_tip);
            return Ok(remote_tip);
        }
        
        // Return the next block to process
        Ok(resume_height)
    }

    #[allow(dead_code)]
    async fn get_once(&self, key: &Vec<u8>) -> Result<Option<Vec<u8>>> {
        let runtime = self.runtime.async_read().await;
        let mut context = runtime.context.lock().map_err(|_| anyhow!("Failed to lock context"))?;
        context.db.get(key).map_err(|_| anyhow!("GET error against RocksDB"))
    }

    #[allow(dead_code)]
    async fn get(&self, key: &Vec<u8>) -> Result<Option<Vec<u8>>> {
        let mut count = 0;
        loop {
            match self.get_once(key).await {
                Ok(v) => {
                    return Ok(v);
                }
                Err(e) => {
                    if count > 100 {
                        return Err(e.into());
                    } else {
                        count = count + 1;
                        // Add a small delay to avoid tight loop
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        debug!("err: retrying GET");
                    }
                }
            }
        }
        // This line is unreachable, but we'll keep the function structure consistent
        // with the rest of the codebase
    }

    async fn get_blockhash(&self, block_number: u32) -> Option<Vec<u8>> {
        // Get database handle
        let db = match self.poll_connection().await {
            Ok(db) => db,
            Err(_) => return None,
        };
        
        // Create SMTHelper
        let smt_helper = SMTHelper::new(db);
        
        // Use BST to get blockhash
        // Look up the blockhash for the specified height
        let blockhash_key = format!("height-to-hash/{}", block_number).into_bytes();
        match smt_helper.bst_get_at_height(&blockhash_key, block_number) {
            Ok(Some(value)) => Some(value),
            _ => None
        }
    }

    async fn fetch_blockhash(&self, block_number: u32) -> Result<Vec<u8>> {
        let tunneled_response = self
            .post(serde_json::to_string(&JsonRpcRequest {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_secs()
                    .try_into()?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblockhash"),
                params: vec![Value::Number(Number::from(block_number))],
            })?)
            .await?;

        let result: BlockHashResponse = tunneled_response.json().await?;
        let blockhash = result.result
            .ok_or_else(|| anyhow!("missing result from JSON-RPC response"))?;
        Ok(hex::decode(blockhash)?)
    }

    #[allow(dead_code)]
    async fn put_once(&self, k: &Vec<u8>, v: &Vec<u8>) -> Result<()> {
        let runtime = self.runtime.async_read().await;
        let mut context = runtime.context.lock().map_err(|_| anyhow!("Failed to lock context"))?;
        context.db.put(k, v).map_err(|_| anyhow!("PUT error against RocksDB"))
    }

    #[allow(dead_code)]
    async fn put(&self, key: &Vec<u8>, val: &Vec<u8>) -> Result<()> {
        let mut count = 0;
        loop {
            match self.put_once(key, val).await {
                Ok(v) => {
                    return Ok(v);
                }
                Err(e) => {
                    if count > 100 { // Changed from i32::MAX to a reasonable retry limit
                        return Err(e.into());
                    } else {
                        // Add a small delay to avoid tight loop
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        count = count + 1;
                    }
                    debug!("err: retrying PUT");
                }
            }
        }
    }

    async fn pull_block(&self, block_number: u32) -> Result<Vec<u8>> {
        loop {
            let count = self.fetch_blockcount().await?;
            if block_number > count {
                sleep(Duration::from_millis(3000)).await;
            } else {
                break;
            }
        }
        let blockhash = self.fetch_blockhash(block_number).await?;
        
        // Store blockhash using BST approach
        let db = self.poll_connection().await?;
        let smt_helper = SMTHelper::new(db);
        // Store the blockhash for this block
        let blockhash_key = format!("height-to-hash/{}", block_number).into_bytes();
        smt_helper.bst_put(&blockhash_key, &blockhash, block_number)?;
        
        let tunneled_response = self
            .post(serde_json::to_string(&JsonRpcRequest {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_secs()
                    .try_into()?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblock"),
                params: vec![
                    Value::String(hex::encode(&blockhash)),
                    Value::Number(Number::from(0)),
                ],
            })?)
            .await?;

        let result: BlockHashResponse = tunneled_response.json().await?;
        let block_hex = result.result
            .ok_or_else(|| anyhow!("missing result from JSON-RPC response"))?;
        
        Ok(hex::decode(block_hex)?)
    }

    #[allow(dead_code)]
    async fn run(&mut self) -> Result<()> {
        let mut height: u32 = self.query_height().await?;
        CURRENT_HEIGHT.store(height, Ordering::SeqCst);
        
        // Track the last known best height to detect reorgs
        let mut last_best_height: u32 = height;
        
        loop {
            // Check if we should exit before processing the next block
            if let Some(exit_at) = self.args.exit_at {
                if height >= exit_at {
                    info!("Reached exit-at block {}, shutting down gracefully", exit_at);
                    return Ok(());
                }
            }

            // Detect and handle any reorgs, get the next block to process
            let next_height = match self.detect_and_handle_reorg().await {
                Ok(h) => h,
                Err(e) => {
                    error!("Failed to detect reorg: {}", e);
                    sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            
            // Check if we detected a reorg (next_height < height)
            if next_height < height {
                info!("Chain reorganization detected: rolling back from block {} to block {}", height, next_height);
                height = next_height;
            }
            
            // Get the remote tip to see if we have blocks to fetch
            let remote_tip = match self.fetch_blockcount().await {
                Ok(tip) => tip,
                Err(e) => {
                    error!("Failed to fetch block count: {}", e);
                    sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            
            // If we're caught up, wait for new blocks
            if next_height > remote_tip {
                debug!("Caught up to remote tip at height {}, waiting for new blocks", remote_tip);
                sleep(Duration::from_secs(3)).await;
                continue;
            }
            
            info!("Processing block {} (current indexer height: {})", next_height, height);
            
            match self.pull_block(next_height).await {
                Ok(block_data) => {
                    // Get a write lock on the runtime
                    let mut runtime_guard = self.runtime.write().await;
                    
                    // Update the context
                    {
                        let mut context = runtime_guard.context.lock()
                            .map_err(|_| anyhow!("Failed to lock context"))?;
                        context.block = block_data;
                        context.height = next_height;
                        context.db.set_height(next_height);
                    }
                    
                    // Run the runtime
                    if let Err(e) = runtime_guard.run() {
                        debug!("Runtime error, refreshing memory: {}", e);
                        runtime_guard.refresh_memory()?;
                        if let Err(e) = runtime_guard.run() {
                            error!("Runtime run failed after retry: {}", e);
                            return Err(anyhow!("Runtime run failed after retry: {}", e));
                        }
                    }
                    
                    // Check if this was a reorg (next_height < last_best_height)
                    if next_height < last_best_height {
                        info!("Chain reorganization confirmed: processed block {} (previous chain tip was at block {})",
                              next_height, last_best_height);
                    }
                    
                    // Update height and CURRENT_HEIGHT
                    height = next_height + 1;
                    CURRENT_HEIGHT.store(height, Ordering::SeqCst);
                    
                    // Update last_best_height
                    last_best_height = next_height;
                },
                Err(e) => {
                    error!("Failed to pull block {}: {}", next_height, e);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}

// Allow cloning for use in async tasks
impl Clone for MetashrewRocksDBSync {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            args: self.args.clone(),
            start_block: self.start_block,
            rpc_url: self.rpc_url.clone(),
            auth: self.auth.clone(),
            bypass_ssl: self.bypass_ssl,
            tunnel_config: self.tunnel_config.clone(),
            active_tunnel: self.active_tunnel.clone(),
            fetcher_thread_id_tx: self.fetcher_thread_id_tx.clone(),
            processor_thread_id_tx: self.processor_thread_id_tx.clone(),
            fetcher_thread_id: std::sync::Mutex::new(None),
            processor_thread_id: std::sync::Mutex::new(None),
            snapshot_manager: self.snapshot_manager.clone(),
        }
    }
}

// Add methods for thread management and memory monitoring
impl MetashrewRocksDBSync {
    // Helper method to get detailed memory statistics as a string
    #[allow(dead_code)]
    fn get_memory_stats(&self, runtime: &mut MetashrewRuntime<RocksDBRuntimeAdapter>) -> String {
        if let Some(memory) = runtime.instance.get_memory(&mut runtime.wasmstore, "memory") {
            let memory_size = memory.data_size(&mut runtime.wasmstore);
            let memory_size_gb = memory_size as f64 / 1_073_741_824.0;
            let memory_size_mb = memory_size as f64 / 1_048_576.0;
            
            // Get additional memory information if available
            let memory_pages = memory.size(&mut runtime.wasmstore);
            
            format!(
                "Memory size: {} bytes ({:.2} GB, {:.2} MB), Pages: {}, Usage: {:.2}%",
                memory_size,
                memory_size_gb,
                memory_size_mb,
                memory_pages,
                (memory_size as f64 / 4_294_967_296.0) * 100.0 // Percentage of 4GB limit
            )
        } else {
            "Could not access memory instance".to_string()
        }
    }
    
    // Helper method to check if memory needs to be refreshed based on its size
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
                if height % 1000 == 0 {
                    let memory_size_mb = if let Some(memory) = runtime.instance.get_memory(&mut runtime.wasmstore, "memory") {
                        let memory_size = memory.data_size(&mut runtime.wasmstore);
                        memory_size as f64 / 1_048_576.0
                    } else {
                        0.0
                    };
                    info!("Memory usage at block {}: {:.2} MB", height, memory_size_mb);
                }
            }
        } else {
            debug!("Could not get memory instance for block {}", height);
        }
        
        false
    }

    #[allow(dead_code)]
    fn set_thread_id_senders(
        &mut self,
        fetcher_tx: tokio::sync::mpsc::Sender<(String, std::thread::ThreadId)>,
        processor_tx: tokio::sync::mpsc::Sender<(String, std::thread::ThreadId)>
    ) {
        self.fetcher_thread_id_tx = Some(fetcher_tx);
        self.processor_thread_id_tx = Some(processor_tx);
    }
    
    fn register_current_thread_as_fetcher(&self) {
        if let Some(tx) = &self.fetcher_thread_id_tx {
            let thread_id = std::thread::current().id();
            match self.fetcher_thread_id.lock() {
                Ok(mut guard) => {
                    *guard = Some(thread_id.clone());
                    let _ = tx.try_send(("fetcher".to_string(), thread_id));
                },
                Err(e) => {
                    error!("Failed to lock fetcher_thread_id: {}", e);
                }
            }
        }
    }
    
    fn register_current_thread_as_processor(&self) {
        if let Some(tx) = &self.processor_thread_id_tx {
            let thread_id = std::thread::current().id();
            match self.processor_thread_id.lock() {
                Ok(mut guard) => {
                    *guard = Some(thread_id.clone());
                    let _ = tx.try_send(("processor".to_string(), thread_id));
                },
                Err(e) => {
                    error!("Failed to lock processor_thread_id: {}", e);
                }
            }
        }
    }
    
    #[allow(dead_code)]
    fn is_fetcher_thread(&self) -> bool {
        match self.fetcher_thread_id.lock() {
            Ok(guard) => {
                if let Some(fetcher_id) = *guard {
                    std::thread::current().id() == fetcher_id
                } else {
                    false
                }
            },
            Err(_) => false
        }
    }
    
    #[allow(dead_code)]
    fn is_processor_thread(&self) -> bool {
        match self.processor_thread_id.lock() {
            Ok(guard) => {
                if let Some(processor_id) = *guard {
                    std::thread::current().id() == processor_id
                } else {
                    false
                }
            },
            Err(_) => false
        }
    }
}

#[post("/")]
async fn handle_jsonrpc(
    body: web::Json<JsonRpcRequest>,
    state: web::Data<AppState>,
) -> ActixResult<impl Responder> {
    debug!("RPC request: {}", serde_json::to_string(&body).unwrap());

    if body.method == "metashrew_view" {
        if body.params.len() < 3 {
            return Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: "Invalid params: requires [view_name, input_data, height]".to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            }));
        }

        let view_name = match body.params[0].as_str() {
            Some(s) => s.to_string(),
            None => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: view_name must be a string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let input_hex = match body.params[1].as_str() {
            Some(s) => s.to_string(),
            None => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: input_data must be a hex string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let height = match &body.params[2] {
            Value::String(s) if s == "latest" => CURRENT_HEIGHT.load(Ordering::SeqCst),
            Value::Number(n) => n.as_u64().unwrap_or(0) as u32,
            _ => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: height must be a number or 'latest'".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        // Acquire a read lock on the runtime - this allows multiple readers
        let runtime = state.runtime.read().await;
        
        // Decode input data outside the lock if possible
        let input_data = match hex::decode(input_hex.trim_start_matches("0x")) {
            Ok(data) => data,
            Err(e) => return Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: format!("Invalid hex input: {}", e),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        };

        // Use await with the async view function
        match runtime.view(
            view_name,
            &input_data,
            height,
        ).await {
            Ok(result) => Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result: format!("0x{}", hex::encode(result)),
                jsonrpc: "2.0".to_string(),
            })),
            Err(err) => Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32000,
                    message: err.to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        }
    } else if body.method == "metashrew_preview" {
        // Acquire a read lock on the runtime - this allows multiple readers
        let runtime = state.runtime.read().await;
        // Ensure we have required params
        if body.params.len() < 4 {
            return Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: "Invalid params: requires [block_data, view_name, input_data, height]"
                        .to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            }));
        }

        let block_hex = match body.params[0].as_str() {
            Some(s) => s.to_string(),
            None => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: block_data must be a hex string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let view_name = match body.params[1].as_str() {
            Some(s) => s.to_string(),
            None => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: view_name must be a string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let input_hex = match body.params[2].as_str() {
            Some(s) => s.to_string(),
            None => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: input_data must be a hex string".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let height = match &body.params[3] {
            Value::String(s) if s == "latest" => CURRENT_HEIGHT.load(Ordering::SeqCst),
            Value::Number(n) => n.as_u64().unwrap_or(0) as u32,
            _ => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: height must be a number or 'latest'".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        // Decode input data outside the lock if possible
        let block_data = match hex::decode(block_hex.trim_start_matches("0x")) {
            Ok(data) => data,
            Err(e) => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: format!("Invalid hex block data: {}", e),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };
        
        let input_data = match hex::decode(input_hex.trim_start_matches("0x")) {
            Ok(data) => data,
            Err(e) => return Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: format!("Invalid hex input: {}", e),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        };
        
        match runtime.preview_async(
            &block_data,
            view_name,
            &input_data,
            height,
        ).await {
            Ok(result) => Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result: format!("0x{}", hex::encode(result)),
                jsonrpc: "2.0".to_string(),
            })),
            Err(err) => Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32000,
                    message: err.to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        }
    } else if body.method == "metashrew_height" {
        // Get the current height
        let current_height = CURRENT_HEIGHT.load(Ordering::SeqCst);
        
        // If the current height is greater than 0, subtract 1 to get the actual latest block
        let latest_block = if current_height > 0 { current_height - 1 } else { 0 };
        
        // Log the height being returned for debugging
        debug!("metashrew_height returning latest_block={} (from current_height={})",
               latest_block, current_height);
        
        // Return the latest block height as a number
        Ok(HttpResponse::Ok().json(serde_json::json!({
            "id": body.id,
            "result": latest_block,
            "jsonrpc": "2.0"
        })))
    } else if body.method == "metashrew_getblockhash" {
        // Acquire a read lock on the runtime
        let runtime = state.runtime.read().await;
        if body.params.len() != 1 {
            return Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32602,
                    message: "Invalid params: requires [block_number]".to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            }));
        }

        let height = match &body.params[0] {
            Value::Number(n) => n.as_u64().unwrap_or(0) as u32,
            _ => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: block_number must be a number".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };

        let key = (String::from(HEIGHT_TO_HASH) + &height.to_string()).into_bytes();
        
        // Fix lifetime issue by storing the context in a variable
        let mut context = runtime.context.lock()
            .map_err(|_| <anyhow::Error as Into<IndexerError>>::into(anyhow!("Failed to lock context")))?;
        let result = context.db.get(&key).map_err(|_| {
            <anyhow::Error as Into<IndexerError>>::into(anyhow!(
                "DB connection error while fetching blockhash"
            ))
        })?;
        
        match result {
            Some(hash) => Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result: format!("0x{}", hex::encode(hash)),
                jsonrpc: "2.0".to_string(),
            })),
            None => Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32000,
                    message: "Block hash not found".to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        }
    } else if body.method == "metashrew_stateroot" {
        // Get the stateroot from the SMT adapter
        let height = match &body.params[0] {
            Value::String(s) if s == "latest" => {
                // Get the current height
                let current_height = CURRENT_HEIGHT.load(Ordering::SeqCst);
                // If the current height is greater than 0, subtract 1 to get the actual latest block
                if current_height > 0 { current_height - 1 } else { 0 }
            },
            Value::Number(n) => n.as_u64().unwrap_or(0) as u32,
            _ => {
                return Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32602,
                        message: "Invalid params: height must be a number or 'latest'".to_string(),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        };
        
        info!("metashrew_stateroot called with height: {}", height);
        
        // Get the stateroot from the SMT adapter
        let runtime = state.runtime.read().await;
        let context = runtime.context.lock()
            .map_err(|_| <anyhow::Error as Into<IndexerError>>::into(anyhow!("Failed to lock context")))?;
        
        // Try to query the database for the state root at the specified height
        info!("Looking up state root for height {}", height);
        
        // Check for state root in the database using standard format
        let result = context.db.db.get(format!("smt:root::{}", height).into_bytes());
        
        if let Ok(Some(value)) = result {
            info!("Found state root for height {}", height);
            return Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result: format!("0x{}", hex::encode(value)),
                jsonrpc: "2.0".to_string(),
            }));
        }
        
        // Try alternative format if standard format not found
        match context.db.db.get(format!("smt:root:::{}", height).into_bytes()) {
            Ok(Some(value)) => {
                info!("Found state root for height {}", height);
                Ok(HttpResponse::Ok().json(JsonRpcResult {
                    id: body.id,
                    result: format!("0x{}", hex::encode(value)),
                    jsonrpc: "2.0".to_string(),
                }))
            },
            Ok(None) => {
                info!("No direct state root found for height {}, attempting to calculate", height);
                
                // Create an SMTHelper instance
                let smt_helper = SMTHelper::new(context.db.db.clone());
                
                // Get the stateroot using the SMTHelper
                match smt_helper.get_smt_root_at_height(height) {
                    Ok(root) => {
                        info!("Successfully retrieved state root for height {}", height);
                        Ok(HttpResponse::Ok().json(JsonRpcResult {
                            id: body.id,
                            result: format!("0x{}", hex::encode(root)),
                            jsonrpc: "2.0".to_string(),
                        }))
                    },
                    Err(e) => {
                        error!("Failed to get stateroot: {}", e);
                        Ok(HttpResponse::Ok().json(JsonRpcError {
                            id: body.id,
                            error: JsonRpcErrorObject {
                                code: -32000,
                                message: format!("Failed to get stateroot: {}", e),
                                data: None,
                            },
                            jsonrpc: "2.0".to_string(),
                        }))
                    },
                }
            },
            Err(e) => {
                error!("Error querying database: {}", e);
                Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32000,
                        message: format!("Database error: {}", e),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            }
        }
    } else if body.method == "metashrew_snapshot" {
        // Get snapshot information from the database
        let runtime = state.runtime.read().await;
        let context = runtime.context.lock()
            .map_err(|_| <anyhow::Error as Into<IndexerError>>::into(anyhow!("Failed to lock context")))?;
        
        // Get the current height
        let current_height = CURRENT_HEIGHT.load(Ordering::SeqCst);
        
        // Create an SMTHelper instance
        let smt_helper = SMTHelper::new(context.db.db.clone());
        
        // Try to find the latest snapshot height by checking for state roots
        // We'll check the last few heights to find the most recent snapshot
        let mut latest_snapshot_height = 0;
        let mut latest_root: Option<String> = None;
        
        // Check the last 1000 blocks for a state root
        let start_height = if current_height > 1000 { current_height - 1000 } else { 0 };
        for height in (start_height..=current_height).rev() {
            if let Ok(root) = smt_helper.get_smt_root_at_height(height) {
                latest_snapshot_height = height;
                latest_root = Some(hex::encode(root));
                break;
            }
        }
        
        // Try to determine if snapshots are enabled and the interval
        // We'll look for a pattern in the state roots
        let mut snapshot_interval = 1000; // Default interval
        let mut snapshot_enabled = false;
        
        if latest_snapshot_height > 0 {
            snapshot_enabled = true;
            
            // Try to find another snapshot to determine the interval
            for height in (0..latest_snapshot_height).rev().step_by(100) {
                if let Ok(_) = smt_helper.get_smt_root_at_height(height) {
                    snapshot_interval = latest_snapshot_height - height;
                    break;
                }
            }
        }
        
        // Create snapshot info response
        let snapshot_info = serde_json::json!({
            "enabled": snapshot_enabled,
            "current_height": current_height,
            "last_snapshot_height": latest_snapshot_height,
            "latest_root": latest_root,
            "estimated_interval": snapshot_interval,
            "next_snapshot_at": if snapshot_enabled && snapshot_interval > 0 {
                let next = current_height + (snapshot_interval - (current_height % snapshot_interval));
                serde_json::Value::Number(serde_json::Number::from(next))
            } else {
                serde_json::Value::Null
            }
        });
        
        Ok(HttpResponse::Ok().json(JsonRpcResult {
            id: body.id,
            result: snapshot_info.to_string(),
            jsonrpc: "2.0".to_string(),
        }))
    } else {
        Ok(HttpResponse::Ok().json(JsonRpcError {
            id: body.id,
            error: JsonRpcErrorObject {
                code: -32601,
                message: format!("Method '{}' not found", body.method),
                data: None,
            },
            jsonrpc: "2.0".to_string(),
        }))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger with timestamp
    env_logger::builder()
        .format_timestamp_secs()
        .init();
    
    info!("Starting Metashrew Indexer (rockshrew-mono)");
    info!("System has {} CPU cores available", num_cpus::get());
    
    // Parse command line arguments
    let args_arc = Arc::new(Args::parse());
    let args = Args::parse(); // Create a non-Arc version for compatibility
    
    // Set the label if provided
    if let Some(ref label) = args.label {
        set_label(label.clone());
    }
    
    // Parse the daemon RPC URL to determine if SSH tunneling is needed
    let (rpc_url, bypass_ssl, tunnel_config) = parse_daemon_rpc_url(&args_arc.daemon_rpc_url).await?;
    
    info!("Parsed RPC URL: {}", rpc_url);
    info!("Bypass SSL: {}", bypass_ssl);
    info!("Tunnel config: {:?}", tunnel_config);
    
    // Configure RocksDB options for optimal performance
    let mut opts = Options::default();
    
    // Dynamically configure RocksDB based on available CPU cores
    let available_cpus = num_cpus::get();
    
    // Calculate optimal background jobs - use approximately 1/4 of available cores
    // with a minimum of 4 and a reasonable maximum to avoid excessive context switching
    let background_jobs: i32 = std::cmp::min(
        std::cmp::max(4, available_cpus / 4),
        16  // Cap at a reasonable maximum
    ).try_into().unwrap();
    
    // Calculate write buffer number based on available cores
    let write_buffer_number: i32 = std::cmp::min(
        std::cmp::max(6, available_cpus / 6),
        12  // Cap at a reasonable maximum
    ).try_into().unwrap();
    
    info!("Configuring RocksDB with {} background jobs and {} write buffers for optimal performance", background_jobs, write_buffer_number);
    
    opts.create_if_missing(true);
    opts.set_max_open_files(10000);
    opts.set_use_fsync(false);
    opts.set_bytes_per_sync(8388608); // 8MB
    opts.optimize_for_point_lookup(1024);
    opts.set_table_cache_num_shard_bits(6);
    opts.set_max_write_buffer_number(write_buffer_number);
    opts.set_write_buffer_size(256 * 1024 * 1024);
    opts.set_target_file_size_base(256 * 1024 * 1024);
    opts.set_min_write_buffer_number_to_merge(2);
    opts.set_level_zero_file_num_compaction_trigger(4);
    opts.set_level_zero_slowdown_writes_trigger(20);
    opts.set_level_zero_stop_writes_trigger(30);
    opts.set_max_background_jobs(background_jobs);
    // Removed deprecated call to set_max_background_compactions
    opts.set_disable_auto_compactions(false);
    
    let mut start_block = args.start_block.unwrap_or(0);
    
    // Handle repo flag if provided
    let mut wasm_from_repo: Option<PathBuf> = None;
    if let Some(ref repo_url) = args.repo {
        info!("Repository URL provided: {}", repo_url);
        
        // Create a temporary snapshot manager for repo sync
        let config = SnapshotConfig {
            interval: args.snapshot_interval,
            directory: PathBuf::from("temp_sync"),
            enabled: true,
        };
        
        let mut sync_manager = SnapshotManager::new(config);
        
        // Sync from repository
        match sync_manager.sync_from_repo(repo_url, &args.db_path, args.indexer.as_ref()).await {
            Ok((height, wasm_path)) => {
                info!("Successfully synced from repository to height {}", height);
                if start_block < height {
                    info!("Adjusting start block from {} to {}", start_block, height);
                    start_block = height;
                }
                
                // If we got a WASM file from the repo and no indexer was specified, use it
                if args.indexer.is_none() {
                    if let Some(path) = wasm_path {
                        info!("Using WASM file from repository: {:?}", path);
                        wasm_from_repo = Some(path);
                    } else {
                        error!("No WASM file provided or found in repository");
                        return Err(anyhow!("No WASM file provided or found in repository"));
                    }
                }
            },
            Err(e) => {
                error!("Failed to sync from repository: {}", e);
                return Err(anyhow!("Failed to sync from repository: {}", e));
            }
        }
    }
    
    // Determine which WASM file to use
    let indexer_path = if let Some(path) = wasm_from_repo {
        path
    } else if let Some(path) = args.indexer.clone() {
        path
    } else {
        return Err(anyhow!("No indexer WASM file provided or found in repository"));
    };
    
    // Initialize snapshot manager if snapshot directory is provided
    let snapshot_manager = if let Some(ref snapshot_dir) = args.snapshot_directory {
        info!("Snapshot functionality enabled with directory: {:?}", snapshot_dir);
        info!("Snapshot interval set to {} blocks", args.snapshot_interval);
        
        let config = SnapshotConfig {
            interval: args.snapshot_interval,
            directory: snapshot_dir.clone(),
            enabled: true,
        };
        
        let mut manager = SnapshotManager::new(config);
        
        // Initialize snapshot directory structure and set last_snapshot_height
        if let Err(e) = manager.initialize_with_db(&args.db_path).await {
            error!("Failed to initialize snapshot directory: {}", e);
            return Err(anyhow!("Failed to initialize snapshot directory: {}", e));
        }
        
        // Set current WASM file
        if let Err(e) = manager.set_current_wasm(indexer_path.clone()) {
            error!("Failed to set current WASM file for snapshots: {}", e);
            return Err(anyhow!("Failed to set current WASM file for snapshots: {}", e));
        }
        
        Some(Arc::new(tokio::sync::Mutex::new(manager)))
    } else {
        None
    };
    
    // Create runtime with RocksDB adapter
    let runtime = Arc::new(RwLock::new(MetashrewRuntime::load(
        indexer_path.clone(),
        RocksDBRuntimeAdapter::open(args.db_path.to_string_lossy().to_string(), opts)?
    )?));
    
    info!("Successfully loaded WASM module from {}", indexer_path.display());
    
    // Create app state for JSON-RPC server
    let app_state = web::Data::new(AppState {
        runtime: runtime.clone(),
    });
    
    // Create a channel to communicate thread IDs
    let (thread_id_tx, mut thread_id_rx) = tokio::sync::mpsc::channel::<(String, std::thread::ThreadId)>(2);
    
    // Spawn a task to monitor thread IDs
    tokio::spawn(async move {
        while let Some((role, thread_id)) = thread_id_rx.recv().await {
            info!("Thread role registered: {} on thread {:?}", role, thread_id);
        }
    });
    
    // Create the sync object
    let mut sync = MetashrewRocksDBSync {
        runtime: runtime.clone(),
        args: args.clone(),
        start_block,
        rpc_url,
        auth: args_arc.auth.clone(),
        bypass_ssl,
        tunnel_config,
        active_tunnel: Arc::new(tokio::sync::Mutex::new(None)),
        fetcher_thread_id_tx: Some(thread_id_tx.clone()),
        processor_thread_id_tx: Some(thread_id_tx.clone()),
        fetcher_thread_id: std::sync::Mutex::new(None),
        processor_thread_id: std::sync::Mutex::new(None),
        snapshot_manager,
    };
    
    // Start the indexer in a separate task
    let indexer_handle = tokio::spawn(async move {
        info!("Starting block indexing process from height {}", start_block);
        
        if let Err(e) = sync.run_pipeline().await {
            error!("Indexer error: {}", e);
        }
    });
    
    // Start the JSON-RPC server
    let server_handle = tokio::spawn({
        let args_clone = args_arc.clone();
        HttpServer::new(move || {
            let cors = match &args_clone.cors {
                Some(cors_value) if cors_value == "*" => {
                    // Allow all origins
                    Cors::default()
                        .allow_any_origin()
                        .allow_any_method()
                        .allow_any_header()
                }
                Some(cors_value) => {
                    // Allow specific origins
                    let mut cors_builder = Cors::default();
                    for origin in cors_value.split(',') {
                        cors_builder = cors_builder.allowed_origin(origin.trim());
                    }
                    cors_builder
                }
                None => {
                    // Default: only allow localhost
                    Cors::default()
                        .allowed_origin_fn(|origin, _| {
                            if let Ok(origin_str) = origin.to_str() {
                                origin_str.starts_with("http://localhost:")
                            } else {
                                false
                            }
                        })
                }
            };
            
            App::new()
                .wrap(cors)
                .app_data(app_state.clone())
                .service(handle_jsonrpc)
        })
        .bind((args_arc.host.as_str(), args_arc.port))?
        .run()
    });
    info!("JSON-RPC server running at http://{}:{}", args_arc.host, args_arc.port);
    info!("Indexer is ready and processing blocks");
    info!("Available RPC methods: metashrew_view, metashrew_preview, metashrew_height, metashrew_getblockhash, metashrew_stateroot, metashrew_snapshot");
    
    // Wait for either component to finish (or fail)
    tokio::select! {
        result = indexer_handle => {
            if let Err(e) = result {
                error!("Indexer task failed: {}", e);
            }
        }
        result = server_handle => {
            if let Err(e) = result {
                error!("Server task failed: {}", e);
            }
        }
    }
    
    Ok(())
    }
    
    #[cfg(test)]
    mod tests {
        use super::*;
        use memshrew_store::MemStore;
        use std::path::PathBuf;
        use anyhow::Result;
    
        #[tokio::test]
        async fn test_minimal_indexer() -> Result<()> {
            let wasm_path = PathBuf::from("../target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
            if !wasm_path.exists() {
                panic!("metashrew-minimal.wasm not found at {:?}. Please run `cargo build --target wasm32-unknown-unknown --release -p metashrew-minimal` first.", wasm_path);
            }
            let mem_store = MemStore::new();
            let mut runtime = MetashrewRuntime::load(wasm_path, mem_store)?;
    
            let block_data = b"test block data".to_vec();
            runtime.context.lock().unwrap().height = 1;
            runtime.context.lock().unwrap().block = block_data.clone();
            runtime.run()?;
    
            let result = runtime.view("view_block_by_height".to_string(), &1u32.to_be_bytes().to_vec(), 1).await?;
            assert_eq!(result, block_data);
    
            Ok(())
        }
    }

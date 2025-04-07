use actix_cors::Cors;
use actix_web::error;
use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result as ActixResult};
use anyhow::{anyhow, Result};
use clap::Parser;
use env_logger;
use hex;
use itertools::Itertools;
use log::{debug, info, error};
use rand::Rng;
use metashrew_runtime::{KeyValueStoreLike, MetashrewRuntime};
use num_cpus;
use reqwest::{Response, Url};
use rocksdb::Options;
use rockshrew_runtime::{query_height, set_label, RocksDBRuntimeAdapter};
use serde::{Deserialize, Serialize};
use serde_json::{self, Number, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio;
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;

const HEIGHT_TO_HASH: &'static str = "/__INTERNAL/height-to-hash/";
use std::sync::atomic::{AtomicU32, Ordering};
static CURRENT_HEIGHT: AtomicU32 = AtomicU32::new(0);

// Block processing result for the pipeline
#[derive(Debug)]
enum BlockResult {
    Success(u32),  // Block height that was successfully processed
    Error(u32, anyhow::Error),  // Block height and error
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    daemon_rpc_url: String,
    #[arg(long)]
    indexer: String,
    #[arg(long)]
    db_path: String,
    #[arg(long)]
    start_block: Option<u32>,
    #[arg(long)]
    auth: Option<String>,
    #[arg(long)]
    label: Option<String>,
    #[arg(long)]
    exit_at: Option<u32>,
    // JSON-RPC server args
    #[arg(long, env = "HOST", default_value = "127.0.0.1")]
    host: String,
    #[arg(long, env = "PORT", default_value_t = 8080)]
    port: u16,
    #[arg(long, help = "CORS allowed origins (e.g., '*' for all origins, or specific domains)")]
    cors: Option<String>,
    // Pipeline configuration
    #[arg(long, help = "Size of the processing pipeline (default: auto-determined based on CPU cores)")]
    pipeline_size: Option<usize>,
}

#[derive(Clone)]
struct AppState {
    runtime: Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
}

#[derive(Serialize, Deserialize)]
struct JsonRpcRequest {
    id: u32,
    method: String,
    params: Vec<Value>,
    jsonrpc: String,
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
struct IndexerState {
    runtime: Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
    args: Arc<Args>,
    start_block: u32,
    fetcher_thread_id_tx: Option<tokio::sync::mpsc::Sender<(String, std::thread::ThreadId)>>,
    processor_thread_id_tx: Option<tokio::sync::mpsc::Sender<(String, std::thread::ThreadId)>>,
    fetcher_thread_id: std::sync::Mutex<Option<std::thread::ThreadId>>,
    processor_thread_id: std::sync::Mutex<Option<std::thread::ThreadId>>,
}

impl IndexerState {
    async fn post_once(&self, body: String) -> Result<Response, reqwest::Error> {
        let response = reqwest::Client::new()
            .post(match self.args.auth.clone() {
                Some(v) => {
                    let mut url = Url::parse(self.args.daemon_rpc_url.as_str()).unwrap();
                    let (username, password) = v.split(":").next_tuple().unwrap();
                    url.set_username(username).unwrap();
                    url.set_password(Some(password)).unwrap();
                    url
                }
                None => Url::parse(self.args.daemon_rpc_url.as_str()).unwrap(),
            })
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await;
        return response;
    }
#[allow(unused_assignments)]
async fn post(&self, body: String) -> Result<Response> {
    let mut retry_delay = Duration::from_millis(100);
    let max_delay = Duration::from_secs(30);
    let max_retries = 10;
    let mut response = None;
    
    for attempt in 0..=max_retries {
        match self.post_once(body.clone()).await {
            Ok(v) => {
                response = Some(v);
                return Ok(response.unwrap());
            },
            Err(e) => {
                if attempt == max_retries {
                    return Err(anyhow!("Max retries exceeded: {}", e));
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
    
    Err(anyhow!("Unreachable: max retries exceeded"))
    }

    async fn fetch_blockcount(&self) -> Result<u32> {
        let response = self
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

        let result: Value = response.json().await?;
        Ok(result["result"]
            .as_u64()
            .ok_or_else(|| anyhow!("missing result from JSON-RPC response"))? as u32)
    }

    async fn query_height(&self) -> Result<u32> {
        let (db, start_block) = {
            let runtime = self.runtime.read().await;
            let context = runtime.context.lock().unwrap();
            (context.db.db.clone(), self.start_block)
        };
        query_height(db, start_block).await
    }

    async fn best_height(&self, block_number: u32) -> Result<u32> {
        let mut best: u32 = block_number;
        let tip = self.fetch_blockcount().await?;

        if best >= tip - std::cmp::min(6, tip) {
            loop {
                if best == 0 {
                    break;
                }
                
                // Get local blockhash with better error handling
                let blockhash = match self.get_blockhash(best).await {
                    Some(hash) => hash,
                    None => {
                        // If we can't get the local blockhash, try to get it from the remote
                        debug!("Local blockhash not found for block {}, fetching from remote", best);
                        let remote_hash = self.fetch_blockhash(best).await?;
                        
                        // Store the remote hash locally for future reference
                        let runtime = self.runtime.write().await;
                        if let Ok(mut context) = runtime.context.lock() {
                            let key = (String::from(HEIGHT_TO_HASH) + &best.to_string()).into_bytes();
                            if let Err(e) = context.db.put(&key, &remote_hash) {
                                debug!("Failed to store blockhash for block {}: {}", best, e);
                            }
                        }
                        
                        remote_hash
                    }
                };
                
                let remote_blockhash = self.fetch_blockhash(best).await?;
                if blockhash == remote_blockhash {
                    break;
                } else {
                    best = best - 1;
                }
            }
        }
        Ok(best)
    }

    async fn get_blockhash(&self, block_number: u32) -> Option<Vec<u8>> {
        let key = (String::from(HEIGHT_TO_HASH) + &block_number.to_string()).into_bytes();
        let runtime = match self.runtime.read().await {
            runtime => runtime,
        };
        
        let mut context = match runtime.context.lock() {
            Ok(context) => context,
            Err(e) => {
                error!("Failed to lock context: {}", e);
                return None;
            }
        };
        
        match context.db.get(&key) {
            Ok(result) => result,
            Err(e) => {
                error!("Database error when retrieving blockhash for block {}: {}", block_number, e);
                None
            }
        }
    }

    async fn fetch_blockhash(&self, block_number: u32) -> Result<Vec<u8>> {
        let response = self
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

        let result: Value = response.json().await?;
        let blockhash = result["result"]
            .as_str()
            .ok_or_else(|| anyhow!("missing result from JSON-RPC response"))?;
        Ok(hex::decode(blockhash)?)
    }

    async fn pull_block(&self, block_number: u32) -> Result<Vec<u8>> {
        loop {
            let count = self.fetch_blockcount().await?;
            if block_number > count {
                tokio::time::sleep(Duration::from_millis(3000)).await;
            } else {
                break;
            }
        }
        let blockhash = self.fetch_blockhash(block_number).await?;

        let runtime = self.runtime.write().await;
        runtime.context.lock().unwrap().db.put(
            &(String::from(HEIGHT_TO_HASH) + &block_number.to_string()).into_bytes(),
            &blockhash,
        )?;

        let response = self
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

        let result: Value = response.json().await?;
        let block_hex = result["result"]
            .as_str()
            .ok_or_else(|| anyhow!("missing result from JSON-RPC response"))?;
        Ok(hex::decode(block_hex)?)
    }

    // Process a single block
    async fn process_block(&self, height: u32, block_data: Vec<u8>) -> Result<()> {
        // Get a lock on the runtime with better error handling
        let mut runtime = match self.runtime.write().await {
            runtime => runtime,
        };
        
        // Set block data with better error handling
        match runtime.context.lock() {
            Ok(mut context) => {
                context.block = block_data;
                context.height = height;
                context.db.set_height(height);
            },
            Err(e) => {
                return Err(anyhow!("Failed to lock context: {}", e));
            }
        }
        
        // Check if memory usage is approaching the limit and refresh if needed
        if self.should_refresh_memory(&mut runtime, height) {
            match runtime.refresh_memory() {
                Ok(_) => debug!("Successfully refreshed memory preemptively for block {}", height),
                Err(e) => {
                    error!("Failed to preemptively refresh memory: {}", e);
                    // Continue with execution even if preemptive refresh fails
                }
            }
        }
        
        // Execute the runtime with better error handling
        match runtime.run() {
            Ok(_) => {
                debug!("Successfully processed block {}", height);
                
                // Store the blockhash for this height to ensure it's available for future queries
                if let Ok(mut context) = runtime.context.lock() {
                    if let Ok(Some(_blockhash)) = context.db.get(&format!("{}{}",
                        HEIGHT_TO_HASH, height).into_bytes()) {
                        debug!("Verified blockhash is stored for block {}", height);
                    } else {
                        debug!("Blockhash not found for block {}, will be fetched if needed", height);
                    }
                }
                
                Ok(())
            },
            Err(e) => {
                // Log detailed memory stats when runtime execution fails
                let memory_stats = self.get_memory_stats(&mut runtime);
                error!("CRITICAL: Runtime execution failed for block {}: {}", height, e);
                error!("Memory stats at failure: {}", memory_stats);
                
                // Crash the process to force a restart and make the issue more visible
                panic!("Runtime execution failed with memory stats: {}", memory_stats);
                
                // The code below is unreachable due to the panic, but kept for reference
                /*
                // Try to refresh memory with better error handling
                match runtime.refresh_memory() {
                    Ok(_) => {
                        // Try running again after memory refresh
                        match runtime.run() {
                            Ok(_) => {
                                debug!("Successfully processed block {} after memory refresh", height);
                                Ok(())
                            },
                            Err(run_err) => {
                                error!("Runtime execution failed after memory refresh: {}", run_err);
                                Err(anyhow!(run_err))
                            }
                        }
                    },
                    Err(refresh_err) => {
                        error!("Memory refresh failed: {}", refresh_err);
                        Err(anyhow!(refresh_err))
                    }
                }
                */
            }
        }
    }

    // Improvement 5: Parallel processing with pipeline
    async fn run_pipeline(&mut self) -> Result<()> {
        let mut height: u32 = self.query_height().await?;
        CURRENT_HEIGHT.store(height, Ordering::SeqCst);
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
                    info!("Block fetcher task started on thread {:?}", std::thread::current().id());
                let mut current_height = height;
                
                loop {
                    // Check if we should exit
                    if let Some(exit_at) = args.exit_at {
                        if current_height >= exit_at {
                            info!("Fetcher reached exit-at block {}, shutting down", exit_at);
                            break;
                        }
                    }
                    
                    // Find the best height considering potential reorgs
                    let best_height = match indexer.best_height(current_height).await {
                        Ok(h) => h,
                        Err(e) => {
                            error!("Failed to determine best height: {}", e);
                            // Send error result and continue
                            if result_sender_clone.send(BlockResult::Error(current_height, e)).await.is_err() {
                                break;
                            }
                            sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    
                    // Fetch the block
                    match indexer.pull_block(best_height).await {
                        Ok(block_data) => {
                            debug!("Fetched block {} ({})", best_height, block_data.len());
                            // Send block to processor
                            if block_sender_clone.send((best_height, block_data)).await.is_err() {
                                break;
                            }
                        },
                        Err(e) => {
                            error!("Failed to fetch block {}: {}", best_height, e);
                            // Send error result
                            if result_sender_clone.send(BlockResult::Error(best_height, e)).await.is_err() {
                                break;
                            }
                            sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    }
                    
                    current_height = best_height + 1;
                }
                
                debug!("Block fetcher task completed");
            })
        };
        
        // Spawn block processor task on dedicated thread
        let processor_handle = {
            let indexer = self.clone();
            let result_sender_clone = result_sender.clone();
            
            // Spawn a task for the block processor
            tokio::spawn(async move {
                    // Register this thread as the processor thread
                    indexer.register_current_thread_as_processor();
                    info!("Block processor task started on thread {:?}", std::thread::current().id());
                while let Some((block_height, block_data)) = block_receiver.recv().await {
                    debug!("Processing block {} ({})", block_height, block_data.len());
                    
                    let result = match indexer.process_block(block_height, block_data).await {
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
        };
        
        // Main loop to handle results
        while let Some(result) = result_receiver.recv().await {
            match result {
                BlockResult::Success(processed_height) => {
                    debug!("Successfully processed block {}", processed_height);
                    height = processed_height + 1;
                    CURRENT_HEIGHT.store(height, Ordering::SeqCst);
                },
                BlockResult::Error(failed_height, error) => {
                    error!("Failed to process block {}: {}", failed_height, error);
                    // We could implement more sophisticated error handling here
                    // For now, just wait and continue
                    sleep(Duration::from_secs(5)).await;
                }
            }
            
            // Check if we should exit
            if let Some(exit_at) = self.args.exit_at {
                if height > exit_at {
                    info!("Reached exit-at block {}, shutting down gracefully", exit_at);
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

    #[allow(dead_code)]
    async fn run(&mut self) -> Result<()> {
        let mut height: u32 = self.query_height().await?;

        loop {
            if let Some(exit_at) = self.args.exit_at {
                if height >= exit_at {
                    info!(
                        "Reached exit-at block {}, shutting down gracefully",
                        exit_at
                    );
                    return Ok(());
                }
            }

            let best: u32 = self.best_height(height).await.unwrap_or(height);
            let block_data = self.pull_block(best).await?;

            let mut runtime = self.runtime.write().await;
            {
                let mut context = runtime.context.lock().unwrap();
                context.block = block_data;
                context.height = best;
                context.db.set_height(best);
            }

            match runtime.run() {
                Ok(_) => {},
                Err(e) => {
                    info!("Runtime execution failed: {}, refreshing memory and retrying", e);
                    runtime.refresh_memory().map_err(|refresh_err| {
                        error!("Memory refresh failed: {}", refresh_err);
                        refresh_err
                    })?;
                    
                    runtime.run().map_err(|run_err| {
                        error!("Runtime execution failed after memory refresh: {}", run_err);
                        run_err
                    })?;
                }
            }

            height = best + 1;
            CURRENT_HEIGHT.store(height, Ordering::SeqCst);
        }
    }
}

// Allow cloning for use in async tasks
impl Clone for IndexerState {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            args: self.args.clone(),
            start_block: self.start_block,
            fetcher_thread_id_tx: self.fetcher_thread_id_tx.clone(),
            processor_thread_id_tx: self.processor_thread_id_tx.clone(),
            fetcher_thread_id: std::sync::Mutex::new(*self.fetcher_thread_id.lock().unwrap()),
            processor_thread_id: std::sync::Mutex::new(*self.processor_thread_id.lock().unwrap()),
        }
    }
}

// Add methods to set and get thread ID senders
impl IndexerState {
    // Helper method to get detailed memory statistics as a string
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
            
            // Get detailed memory stats for logging
            let memory_stats = self.get_memory_stats(runtime);
            
            // Check if memory size is approaching the limit
            if memory_size >= threshold_bytes {
                info!("Memory usage approaching threshold of {:.2}GB for block {}: {}", threshold_gb, height, memory_stats);
                info!("Preemptively refreshing memory to avoid OOM errors");
                return true;
            } else if height % 1000 == 0 {
                // Log memory stats periodically for monitoring
                info!("Memory stats at block {}: {}", height, memory_stats);
            }
        } else {
            debug!("Could not get memory instance for block {}", height);
        }
        
        false
    }
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
    
    // These methods might be useful in the future, so we'll keep them but mark them as allowed dead code
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
        // No need to lock the runtime for this operation
        Ok(HttpResponse::Ok().json(JsonRpcResult {
            id: body.id,
            result: CURRENT_HEIGHT.load(Ordering::SeqCst).to_string(),
            jsonrpc: "2.0".to_string(),
        }))
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
        let mut context = runtime.context.lock().unwrap();
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

fn main() -> Result<()> {
    // Initialize the logger
    env_logger::init();
    
    // Parse command line arguments
    let args = Arc::new(Args::parse());

    if let Some(ref label) = args.label {
        set_label(label.clone());
    }

    let start_block = args.start_block.unwrap_or(0);

    // Create a custom runtime with dedicated threads for critical tasks
    // Dynamically determine the number of worker threads based on available CPUs
    let available_cpus = num_cpus::get();
    let worker_threads = std::cmp::max(8, available_cpus); // Use at least 8 threads, or more if available
    
    info!("Detected {} CPU cores, configuring tokio runtime with {} worker threads", available_cpus, worker_threads);
    
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .thread_name("metashrew-worker")
        .on_thread_start(|| {
            let thread_name = std::thread::current().name().unwrap_or("unknown").to_string();
            info!("Thread started: {}", thread_name);
        })
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    
    // Run the main application logic on the runtime
    runtime.block_on(async_main(args, start_block))
}

// The actual async main function that will run on our custom runtime
async fn async_main(args: Arc<Args>, start_block: u32) -> Result<()> {
    info!("Starting Metashrew with dedicated threads for indexer tasks");
    // No longer need thread names as they're set directly in the registration functions
    
    // Configure thread priorities using thread names
    info!("Setting up dedicated task threads");
        info!("Setting up dedicated task threads");
    
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
    
    info!("Configuring RocksDB with {} background jobs and {} write buffers", background_jobs, write_buffer_number);
    
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

    // Create runtime with RocksDB adapter
    let runtime = Arc::new(RwLock::new(MetashrewRuntime::load(
      PathBuf::from(&args.indexer),
      RocksDBRuntimeAdapter::open(args.db_path.clone(), opts)?,
  )?));

    // Create indexer state
    let mut indexer = IndexerState {
        runtime: runtime.clone(),
        args: args.clone(),
        start_block,
        fetcher_thread_id_tx: None,
        processor_thread_id_tx: None,
        fetcher_thread_id: std::sync::Mutex::new(None),
        processor_thread_id: std::sync::Mutex::new(None),
    };
    
    // Log the pipeline size configuration
    match args.pipeline_size {
        Some(size) => info!("Using user-specified pipeline size: {}", size),
        None => {
            let available_cpus = num_cpus::get();
            let auto_size = std::cmp::min(
                std::cmp::max(5, available_cpus / 2),
                16
            );
            info!("Using auto-configured pipeline size: {} (based on {} CPU cores)", auto_size, available_cpus);
        }
    }

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
    
    // Configure the indexer to use thread ID tracking
    indexer.set_thread_id_senders(thread_id_tx.clone(), thread_id_tx.clone());
    
    // Start the indexer in a separate task
    let indexer_handle = tokio::spawn(async move {
        info!("Starting indexer task");
        
        if let Err(e) = indexer.run_pipeline().await {
            error!("Indexer error: {}", e);
        }
    });

    // Start the JSON-RPC server
    let server_handle = tokio::spawn({
        let args_clone = args.clone();
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
        .bind((args.host.as_str(), args.port))?
        .run()
    });
    info!("Server running at http://{}:{}", args.host, args.port);

    // Wait for either component to finish (or fail)
    tokio::select! {
        result = indexer_handle => {
            if let Err(e) = result {
                log::error!("Indexer task failed: {}", e);
            }
        }
        result = server_handle => {
            if let Err(e) = result {
                log::error!("Server task failed: {}", e);
            }
        }
    }

    Ok(())
}

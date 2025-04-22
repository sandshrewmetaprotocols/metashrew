use anyhow::{anyhow, Result};
use clap::Parser;
use env_logger;
use hex;
use log::{debug, info, error};
use metashrew_runtime::KeyValueStoreLike;
use metashrew_runtime::MetashrewRuntime;
use rand::Rng;
use rocksdb::Options;
use rockshrew_runtime::{query_height, set_label, RocksDBRuntimeAdapter};
use serde::{Deserialize, Serialize};
use serde_json::{self, Number, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio;
use tokio::time::sleep;
use std::sync::atomic::{AtomicU32, Ordering};

// Import our SSH tunneling module
mod ssh_tunnel;
use ssh_tunnel::{SshTunnel, SshTunnelConfig, TunneledResponse, make_request_with_tunnel, parse_daemon_rpc_url};

const HEIGHT_TO_HASH: &'static str = "/__INTERNAL/height-to-hash/";
static CURRENT_HEIGHT: AtomicU32 = AtomicU32::new(0);

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    daemon_rpc_url: String,
    #[arg(long)]
    indexer: PathBuf,
    #[arg(long)]
    db_path: PathBuf,
    #[arg(long)]
    start_block: Option<u32>,
    #[arg(long)]
    auth: Option<String>,
    #[arg(long)]
    host: Option<String>,
    #[arg(long)]
    port: Option<u16>,
    #[arg(long)]
    label: Option<String>,
    #[arg(long)]
    exit_at: Option<u32>,
    #[arg(long)]
    pipeline_size: Option<usize>,
    #[arg(long)]
    cors: Option<String>,
}

// JSON-RPC request structure
#[derive(Serialize)]
struct JsonRpcRequest {
    id: u32,
    jsonrpc: String,
    method: String,
    params: Vec<Value>,
}

// JSON-RPC response structure
#[derive(Deserialize)]
struct JsonRpcResponse {
    id: u32,
    result: Option<Value>,
    error: Option<JsonRpcError>,
    jsonrpc: String,
}

// JSON-RPC error structure
#[derive(Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    data: Option<Value>,
}

// Block count response structure
#[derive(Deserialize)]
struct BlockCountResponse {
    id: u32,
    result: Option<u32>,
    error: Option<Value>,
}

// Block hash response structure
#[derive(Deserialize)]
struct BlockHashResponse {
    id: u32,
    result: Option<String>,
    error: Option<Value>,
}

pub struct MetashrewRocksDBSync {
    runtime: MetashrewRuntime<RocksDBRuntimeAdapter>,
    args: Args,
    start_block: u32,
    rpc_url: String,
    auth: Option<String>,
    bypass_ssl: bool,
    tunnel_config: Option<SshTunnelConfig>,
    active_tunnel: Arc<std::sync::Mutex<Option<SshTunnel>>>,
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
                let active_tunnel = self.active_tunnel.lock().unwrap();
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
                            let mut active_tunnel = self.active_tunnel.lock().unwrap();
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
                        let mut active_tunnel = self.active_tunnel.lock().unwrap();
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

    pub async fn poll_connection<'a>(&'a self) -> Arc<rocksdb::DB> {
        self.runtime.context.lock().unwrap().db.db.clone()
    }

    pub async fn query_height(&self) -> Result<u32> {
        query_height(self.poll_connection().await, self.start_block).await
    }

    async fn best_height(&self, block_number: u32) -> Result<u32> {
        let mut best: u32 = block_number;
        let count = self.fetch_blockcount().await?;
        
        if best >= count - std::cmp::min(6, count) {
            loop {
                if best == 0 {
                    break;
                }
                let blockhash = self
                    .get_blockhash(best)
                    .await
                    .ok_or(anyhow!("failed to retrieve blockhash"))?;
                let remote_blockhash = self.fetch_blockhash(best).await?;
                if blockhash == remote_blockhash {
                    break;
                } else {
                    best = best - 1;
                }
            }
        }
        return Ok(best);
    }

    fn get_once(&self, key: &Vec<u8>) -> Result<Option<Vec<u8>>> {
        self.runtime
            .context
            .lock()
            .unwrap()
            .db
            .get(key)
            .map_err(|_| anyhow!("GET error against RocksDB"))
    }

    fn get(&self, key: &Vec<u8>) -> Result<Option<Vec<u8>>> {
        let mut count = 0;
        let mut response: Option<Option<Vec<u8>>> = None;
        loop {
            match self.get_once(key) {
                Ok(v) => {
                    response = Some(v);
                    break;
                }
                Err(e) => {
                    if count > 100 {
                        return Err(e.into());
                    } else {
                        count = count + 1;
                    }
                    debug!("err: retrying GET");
                }
            }
        }
        Ok(response.unwrap())
    }

    async fn get_blockhash(&self, block_number: u32) -> Option<Vec<u8>> {
        self.get(&(String::from(HEIGHT_TO_HASH) + &block_number.to_string()).into_bytes())
            .unwrap()
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

    fn put_once(&self, k: &Vec<u8>, v: &Vec<u8>) -> Result<()> {
        self.runtime
            .context
            .lock()
            .unwrap()
            .db
            .put(k, v)
            .map_err(|_| anyhow!("PUT error against RocksDB"))
    }

    fn put(&self, key: &Vec<u8>, val: &Vec<u8>) -> Result<()> {
        let mut count = 0;
        loop {
            match self.put_once(key, val) {
                Ok(v) => {
                    return Ok(v);
                }
                Err(e) => {
                    if count > i32::MAX {
                        return Err(e.into());
                    } else {
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
        
        self.put(
            &(String::from(HEIGHT_TO_HASH) + &block_number.to_string()).into_bytes(),
            &blockhash,
        )?;
        
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

    async fn run(&mut self) -> Result<()> {
        let mut i: u32 = self.query_height().await?;
        CURRENT_HEIGHT.store(i, Ordering::SeqCst);
        
        loop {
            // Check if we should exit before processing the next block
            if let Some(exit_at) = self.args.exit_at {
                if i >= exit_at {
                    info!("Reached exit-at block {}, shutting down gracefully", exit_at);
                    return Ok(());
                }
            }

            let best: u32 = match self.best_height(i).await {
                Ok(v) => v,
                Err(e) => {
                    error!("Failed to determine best height: {}", e);
                    i
                },
            };
            
            info!("Processing block {} (best height: {})", best, i);
            
            match self.pull_block(best).await {
                Ok(block_data) => {
                    self.runtime.context.lock().unwrap().block = block_data;
                    self.runtime.context.lock().unwrap().height = best;
                    self.runtime.context.lock().unwrap().db.set_height(best);
                    
                    if let Err(e) = self.runtime.run() {
                        debug!("Runtime error, refreshing memory: {}", e);
                        self.runtime.refresh_memory()?;
                        if let Err(e) = self.runtime.run() {
                            error!("Runtime run failed after retry: {}", e);
                            return Err(anyhow!("Runtime run failed after retry: {}", e));
                        }
                    }
                    
                    i = best + 1;
                    CURRENT_HEIGHT.store(i, Ordering::SeqCst);
                },
                Err(e) => {
                    error!("Failed to pull block {}: {}", best, e);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();
    
    // Parse command line arguments
    let args = Args::parse();
    
    // Set the label if provided
    if let Some(ref label) = args.label {
        set_label(label.clone());
    }
    
    // Parse the daemon RPC URL to determine if SSH tunneling is needed
    let (rpc_url, bypass_ssl, tunnel_config) = parse_daemon_rpc_url(&args.daemon_rpc_url).await?;
    
    info!("Parsed RPC URL: {}", rpc_url);
    info!("Bypass SSL: {}", bypass_ssl);
    info!("Tunnel config: {:?}", tunnel_config);
    
    // Configure RocksDB options for optimal performance
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_max_open_files(10000);
    opts.set_use_fsync(false);
    opts.set_bytes_per_sync(8388608); // 8MB
    opts.optimize_for_point_lookup(1024);
    opts.set_table_cache_num_shard_bits(6);
    opts.set_max_write_buffer_number(6);
    opts.set_write_buffer_size(256 * 1024 * 1024);
    opts.set_target_file_size_base(256 * 1024 * 1024);
    opts.set_min_write_buffer_number_to_merge(2);
    opts.set_level_zero_file_num_compaction_trigger(4);
    opts.set_level_zero_slowdown_writes_trigger(20);
    opts.set_level_zero_stop_writes_trigger(30);
    opts.set_max_background_jobs(4);
    opts.set_max_background_compactions(4);
    opts.set_disable_auto_compactions(false);
    
    let start_block = args.start_block.unwrap_or(0);
    
    // Create the runtime
    let runtime = MetashrewRuntime::load(
        args.indexer.clone(),
        RocksDBRuntimeAdapter::open(args.db_path.to_string_lossy().to_string(), opts)?
    )?;
    
    // Create the sync object
    let mut sync = MetashrewRocksDBSync {
        runtime,
        args: args.clone(),
        start_block,
        rpc_url,
        auth: args.auth.clone(),
        bypass_ssl,
        tunnel_config,
        active_tunnel: Arc::new(std::sync::Mutex::new(None)),
    };
    
    // Run the sync process
    sync.run().await?;
    
    Ok(())
}

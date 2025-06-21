use actix_cors::Cors;
use actix_web::error;
use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result as ActixResult};
use anyhow::{anyhow, Result};
use clap::Parser;
use env_logger;
use hex;
use log::{debug, info, error};
use metashrew_runtime::{MetashrewRuntime};
use num_cpus;
use rocksdb::Options;
use metashrew_runtime::set_label;
use rockshrew_runtime::RocksDBRuntimeAdapter;

// Import our SMT helper module
mod smt_helper;
use smt_helper::SMTHelper;

// Import our adapters module
mod adapters;
use adapters::{BitcoinRpcAdapter, RocksDBStorageAdapter, MetashrewRuntimeAdapter};

use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio;
use tokio::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};

// Import our SSH tunneling module
mod ssh_tunnel;
use ssh_tunnel::{SshTunnelConfig, parse_daemon_rpc_url};

// Import our snapshot module
mod snapshot;
use snapshot::{SnapshotConfig, SnapshotManager};

// Import the generic sync framework
use rockshrew_sync::{MetashrewSync, SyncConfig, JsonRpcProvider, StorageAdapter};

const HEIGHT_TO_HASH: &'static str = "/__INTERNAL/height-to-hash/";
static CURRENT_HEIGHT: AtomicU32 = AtomicU32::new(0);

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
    #[arg(long, help = "Maximum reorg depth to handle", default_value_t = 100)]
    max_reorg_depth: u32,
    #[arg(long, help = "Reorg check threshold - only check for reorgs when within this many blocks of tip", default_value_t = 6)]
    reorg_check_threshold: u32,
}

#[derive(Clone)]
struct AppState {
    sync_engine: Arc<RwLock<MetashrewSync<BitcoinRpcAdapter, RocksDBStorageAdapter, MetashrewRuntimeAdapter>>>,
}

// JSON-RPC request structure
#[derive(Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub id: u32,
    pub jsonrpc: String,
    pub method: String,
    pub params: Vec<Value>,
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
pub struct JsonRpcResponse {
    pub id: u32,
    pub result: Option<Value>,
    pub error: Option<JsonRpcErrorInternal>,
    pub jsonrpc: String,
}

// JSON-RPC error structure for internal use
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct JsonRpcErrorInternal {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
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
pub struct BlockCountResponse {
    pub id: u32,
    pub result: Option<u32>,
    pub error: Option<Value>,
}

// Block hash response structure
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct BlockHashResponse {
    pub id: u32,
    pub result: Option<String>,
    pub error: Option<Value>,
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

        // Use the generic sync framework's JSON-RPC interface
        match state.sync_engine.read().await.metashrew_view(view_name, input_hex, height.to_string()).await {
            Ok(result) => Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result,
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

        // Use the generic sync framework's JSON-RPC interface
        match state.sync_engine.read().await.metashrew_preview(block_hex, view_name, input_hex, height.to_string()).await {
            Ok(result) => Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result,
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
        // Use the generic sync framework's JSON-RPC interface
        match state.sync_engine.read().await.metashrew_height().await {
            Ok(height) => Ok(HttpResponse::Ok().json(serde_json::json!({
                "id": body.id,
                "result": height,
                "jsonrpc": "2.0"
            }))),
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
    } else if body.method == "metashrew_getblockhash" {
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

        // Use the generic sync framework's storage adapter
        match state.sync_engine.read().await.storage().read().await.get_block_hash(height).await {
            Ok(Some(hash)) => Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result: format!("0x{}", hex::encode(hash)),
                jsonrpc: "2.0".to_string(),
            })),
            Ok(None) => Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32000,
                    message: "Block hash not found".to_string(),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
            Err(err) => Ok(HttpResponse::Ok().json(JsonRpcError {
                id: body.id,
                error: JsonRpcErrorObject {
                    code: -32000,
                    message: format!("Storage error: {}", err),
                    data: None,
                },
                jsonrpc: "2.0".to_string(),
            })),
        }
    } else if body.method == "metashrew_stateroot" {
        let height = match &body.params[0] {
            Value::String(s) if s == "latest" => {
                // Get the current height from the sync engine
                match state.sync_engine.read().await.metashrew_height().await {
                    Ok(h) => h,
                    Err(_) => 0,
                }
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

        // Use the generic sync framework's storage adapter
        match state.sync_engine.read().await.storage().read().await.get_state_root(height).await {
            Ok(Some(root)) => {
                info!("Successfully retrieved state root for height {}: 0x{}", height, hex::encode(&root));
                Ok(HttpResponse::Ok().json(JsonRpcResult {
                    id: body.id,
                    result: format!("0x{}", hex::encode(root)),
                    jsonrpc: "2.0".to_string(),
                }))
            },
            Ok(None) => {
                error!("No state root found for height {}", height);
                Ok(HttpResponse::Ok().json(JsonRpcError {
                    id: body.id,
                    error: JsonRpcErrorObject {
                        code: -32000,
                        message: format!("No state root found for height {}", height),
                        data: None,
                    },
                    jsonrpc: "2.0".to_string(),
                }))
            },
            Err(e) => {
                error!("Failed to get stateroot for height {}: {}", height, e);
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
    } else if body.method == "metashrew_snapshot" {
        // Use the generic sync framework to get snapshot information
        match state.sync_engine.read().await.metashrew_snapshot().await {
            Ok(snapshot_info) => Ok(HttpResponse::Ok().json(JsonRpcResult {
                id: body.id,
                result: snapshot_info.to_string(),
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
    
    info!("Starting Metashrew Indexer (rockshrew-mono) with generic sync framework");
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
    
    // Create runtime with RocksDB adapter
    let runtime = Arc::new(RwLock::new(MetashrewRuntime::load(
        indexer_path.clone(),
        RocksDBRuntimeAdapter::open(args.db_path.to_string_lossy().to_string(), opts)?
    )?));
    
    info!("Successfully loaded WASM module from {}", indexer_path.display());
    
    // Get database handle for adapters
    let db = {
        let runtime_guard = runtime.read().await;
        let context = runtime_guard.context.lock().map_err(|_| anyhow!("Failed to lock context"))?;
        context.db.db.clone()
    };
    
    // Create adapters for the generic sync framework
    let bitcoin_adapter = BitcoinRpcAdapter::new(
        rpc_url,
        args_arc.auth.clone(),
        bypass_ssl,
        tunnel_config,
    );
    
    let storage_adapter = RocksDBStorageAdapter::new(db.clone());
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime.clone(), db.clone());
    
    // Create sync configuration
    let sync_config = SyncConfig {
        start_block,
        exit_at: args.exit_at,
        pipeline_size: args.pipeline_size,
        max_reorg_depth: args.max_reorg_depth,
        reorg_check_threshold: args.reorg_check_threshold,
    };
    
    // Create the generic sync engine
    let sync_engine = Arc::new(RwLock::new(MetashrewSync::new(
        bitcoin_adapter,
        storage_adapter,
        runtime_adapter,
        sync_config,
    )));
    
    // Create app state for JSON-RPC server
    let app_state = web::Data::new(AppState {
        sync_engine: sync_engine.clone(),
    });
    
    // Start the indexer in a separate task using the generic sync framework
    let indexer_handle = {
        let sync_engine_clone = sync_engine.clone();
        tokio::spawn(async move {
            info!("Starting block indexing process from height {} using generic sync framework", start_block);
            
            // Use the generic sync framework's run method
            if let Err(e) = sync_engine_clone.write().await.run().await {
                error!("Indexer error: {}", e);
            }
        })
    };
    
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
    info!("Indexer is ready and processing blocks using generic sync framework");
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
    use memshrew_runtime::MemStoreAdapter;
    use std::path::PathBuf;
    use anyhow::Result;

    #[tokio::test]
    async fn test_generic_architecture() -> Result<()> {
        // Test that our generic architecture compiles and basic types work
        let mem_store = MemStoreAdapter::new();
        
        // Test basic key-value operations
        let mut adapter = MemStoreAdapter::new();
        adapter.set_height(42);
        assert_eq!(adapter.get_height(), 42);
        
        // Test basic key-value operations
        let key = b"test_key".to_vec();
        let value = b"test_value".to_vec();
        adapter.put(&key, &value)?;
        
        let retrieved = adapter.get(&key)?;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), value);
        
        Ok(())
    }
}

use actix_cors::Cors;
use actix_web::{post, web, App, HttpServer, HttpResponse, Responder, Result as ActixResult};
use anyhow::{anyhow, Result};
use clap::Parser;
use env_logger;
use log::{debug, info, error, warn};
use metashrew_rocksdb::RocksDBAdapter;
use metashrew_runtime::{KeyValueStoreLike, MetashrewRuntime};
use num_cpus;
use rocksdb::Options;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::sleep;
use std::sync::atomic::{AtomicU32, Ordering};

// Import our modules
mod smt_helper;
mod ssh_tunnel;
mod snapshot;

use smt_helper::SMTHelper;
use ssh_tunnel::{SshTunnel, SshTunnelConfig, parse_daemon_rpc_url};
use snapshot::{SnapshotConfig, SnapshotManager};

// Track current height
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
}

#[derive(Clone)]
struct AppState {
    runtime: Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger with timestamp
    env_logger::builder()
        .format_timestamp_secs()
        .init();
    
    info!("Starting Metashrew Indexer (rockshrew)");
    info!("System has {} CPU cores available", num_cpus::get());
    
    // Parse command line arguments
    let args = Args::parse();
    
    // Set the label if provided
    if let Some(ref label) = args.label {
        metashrew_rocksdb::set_label(label.clone());
    }
    
    // Parse the daemon RPC URL to determine if SSH tunneling is needed
    let (rpc_url, bypass_ssl, tunnel_config) = parse_daemon_rpc_url(&args.daemon_rpc_url).await?;
    
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
    opts.set_disable_auto_compactions(false);
    
    let start_block = args.start_block.unwrap_or(0);
    
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
                    CURRENT_HEIGHT.store(height, Ordering::SeqCst);
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
        RocksDBAdapter::open(args.db_path.to_string_lossy().to_string(), opts)?
    )?));
    
    info!("Successfully loaded WASM module from {}", indexer_path.display());
    
    // Create app state for JSON-RPC server
    let app_state = web::Data::new(AppState {
        runtime: runtime.clone(),
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
                // Add your JSON-RPC handlers here
        })
        .bind((args_clone.host.as_str(), args_clone.port))?
        .run()
    });
    
    info!("JSON-RPC server running at http://{}:{}", args.host, args.port);
    info!("Rockshrew initialized successfully");
    
    // Wait for the server to finish
    let _ = server_handle.await;
    
    Ok(())
}
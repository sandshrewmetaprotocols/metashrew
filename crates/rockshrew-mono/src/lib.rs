//! # Rockshrew-Mono: Monolithic Bitcoin Indexer
//!
//! ## OVERVIEW
//!
//! `rockshrew-mono` is a monolithic binary that combines a Bitcoin indexer and a view layer
//! into a single, efficient application. It leverages a generic, adapter-based synchronization
//! framework to provide flexibility and testability.
//!
//! ## ARCHITECTURE
//!
//! The application is built on the `metashrew-sync` crate, which provides a generic
//! `SnapshotMetashrewSync` engine. This engine is configured with adapters for:
//!
//! - **Bitcoin Node**: `BitcoinRpcAdapter` for connecting to a Bitcoin Core node.
//! - **Storage**: `RocksDBStorageAdapter` for persistent storage using RocksDB.
//! - **Runtime**: `MetashrewRuntimeAdapter` for executing WASM-based indexers.
//!
//! This modular design allows for easy replacement of components, such as using an
//! in-memory storage adapter for testing.
//!
//! ## CORE FUNCTIONALITY
//!
//! - **Indexing**: Synchronizes with the Bitcoin blockchain, processing blocks through a
//!   WASM indexer and storing the resulting state in RocksDB.
//! - **View Layer**: Exposes a JSON-RPC API for querying the indexed state.
//! - **Snapshotting**: Supports creating and consuming snapshots for fast synchronization.
//! - **Reorg Handling**: Automatically detects and handles blockchain reorganizations.
//!
//! ## USAGE
//!
//! `rockshrew-mono` is configured and run via command-line arguments. See `Args` for a
//! full list of options.

// In-code documentation for `rockshrew-mono` crate.
//
// PURPOSE:
// This crate serves as the main entry point for the `rockshrew-mono` binary. It is
// responsible for parsing command-line arguments, setting up the synchronization
// engine, and running the JSON-RPC server.
//
// PROMPT CONSIDERATIONS:
// - The primary goal is to refactor this crate to be a lightweight, generic
//   implementation of the indexer stack.
// - All duplicated logic should be moved to lower-level crates like `metashrew-sync`.
// - The `run` function should be generic over the adapter traits to support both
//   production (RocksDB) and testing (in-memory) environments.

pub mod smt_helper;
pub mod adapters;
pub mod snapshot;
pub mod snapshot_adapters;
pub mod ssh_tunnel;

#[cfg(test)]
mod tests;

use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer, Responder, Result as ActixResult};
use anyhow::{anyhow, Result};
use clap::Parser;
use log::{error, info};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::signal;

use crate::adapters::{BitcoinRpcAdapter, MetashrewRuntimeAdapter};
use crate::ssh_tunnel::parse_daemon_rpc_url;
use metashrew_runtime::{set_label, MetashrewRuntime};
use metashrew_sync::{
    BitcoinNodeAdapter, JsonRpcProvider, RuntimeAdapter, SnapshotMetashrewSync,
    SnapshotProvider, StorageAdapter, SyncConfig, SyncMode, SnapshotSyncEngine,
};
use rockshrew_runtime::{RocksDBRuntimeAdapter, RocksDBStorageAdapter};

/// Command-line arguments for `rockshrew-mono`.
#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(long)]
    pub daemon_rpc_url: String,
    #[arg(long)]
    pub indexer: PathBuf,
    #[arg(long)]
    pub db_path: PathBuf,
    #[arg(long)]
    pub start_block: Option<u32>,
    #[arg(long)]
    pub auth: Option<String>,
    #[arg(long, env = "HOST", default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, env = "PORT", default_value_t = 8080)]
    pub port: u16,
    #[arg(long)]
    pub label: Option<String>,
    #[arg(long)]
    pub exit_at: Option<u32>,
    #[arg(long)]
    pub pipeline_size: Option<usize>,
    #[arg(long)]
    pub cors: Option<String>,
    #[arg(long)]
    pub snapshot_directory: Option<PathBuf>,
    #[arg(long, default_value_t = 1000)]
    pub snapshot_interval: u32,
    #[arg(long)]
    pub repo: Option<String>,
    #[arg(long, default_value_t = 100)]
    pub max_reorg_depth: u32,
    #[arg(long, default_value_t = 6)]
    pub reorg_check_threshold: u32,
}

/// Shared application state for the JSON-RPC server.
#[derive(Clone)]
pub struct AppState<N, S, R>
where
    N: BitcoinNodeAdapter + 'static,
    S: StorageAdapter + 'static,
    R: RuntimeAdapter + 'static,
{
    pub sync_engine: Arc<tokio::sync::RwLock<SnapshotMetashrewSync<N, S, R>>>,
}

/// Handles JSON-RPC requests.
async fn handle_jsonrpc<N, S, R>(
    body: web::Json<serde_json::Value>,
    state: web::Data<AppState<N, S, R>>,
) -> ActixResult<impl Responder>
where
    N: BitcoinNodeAdapter + 'static,
    S: StorageAdapter + 'static,
    R: RuntimeAdapter + 'static,
{
    let request: serde_json::Value = body.into_inner();
    let method = request["method"].as_str().unwrap_or_default();
    let empty_params = vec![];
    let params = request["params"].as_array().unwrap_or(&empty_params);
    let id = request["id"].clone();

    let result = match method {
        "metashrew_view" => {
            let function_name = params[0].as_str().unwrap_or_default().to_string();
            let input_hex = params[1].as_str().unwrap_or_default().to_string();
            let height = params[2].as_str().unwrap_or("latest").to_string();
            state
                .sync_engine
                .read()
                .await
                .metashrew_view(function_name, input_hex, height)
                .await
        }
        "metashrew_preview" => {
            let block_hex = params[0].as_str().unwrap_or_default().to_string();
            let function_name = params[1].as_str().unwrap_or_default().to_string();
            let input_hex = params[2].as_str().unwrap_or_default().to_string();
            let height = params[3].as_str().unwrap_or("latest").to_string();
            state
                .sync_engine
                .read()
                .await
                .metashrew_preview(block_hex, function_name, input_hex, height)
                .await
        }
        "metashrew_height" => state.sync_engine.read().await.metashrew_height().await.map(|h| h.to_string()),
        "metashrew_getblockhash" => {
            let height = params[0].as_u64().unwrap_or_default() as u32;
            state.sync_engine.read().await.metashrew_getblockhash(height).await
        }
        "metashrew_stateroot" => {
            let height = params[0].as_str().unwrap_or("latest").to_string();
            state.sync_engine.read().await.metashrew_stateroot(height).await
        }
        "metashrew_snapshot" => state.sync_engine.read().await.metashrew_snapshot().await.map(|v| v.to_string()),
        _ => Err(anyhow::anyhow!("Method not found").into()),
    };

    let response = match result {
        Ok(res) => serde_json::json!({
            "jsonrpc": "2.0",
            "result": res,
            "id": id
        }),
        Err(e) => serde_json::json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32000,
                "message": e.to_string()
            },
            "id": id
        }),
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Sets up a signal handler for graceful shutdown.
async fn setup_signal_handler() -> Arc<AtomicBool> {
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown_requested.clone();
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("Failed to install CTRL+C signal handler");
        shutdown_clone.store(true, Ordering::SeqCst);
        info!("Shutdown signal received, initiating graceful shutdown...");
    });
    shutdown_requested
}

/// Main run function, generic over the adapter traits.
pub async fn run<N, S, R>(
    args: Args,
    node_adapter: N,
    storage_adapter: S,
    runtime_adapter: R,
    snapshot_provider: Option<Box<dyn SnapshotProvider>>,
) -> Result<()>
where
    N: BitcoinNodeAdapter + 'static,
    S: StorageAdapter + 'static,
    R: RuntimeAdapter + 'static,
{
    if let Some(ref label) = args.label {
        set_label(label.clone());
    }

    let sync_config = SyncConfig {
        start_block: args.start_block.unwrap_or(0),
        exit_at: args.exit_at,
        pipeline_size: args.pipeline_size,
        max_reorg_depth: args.max_reorg_depth,
        reorg_check_threshold: args.reorg_check_threshold,
    };

    let sync_mode = if args.snapshot_directory.is_some() {
        SyncMode::Snapshot(Default::default())
    } else if args.repo.is_some() {
        SyncMode::Repo(Default::default())
    } else {
        SyncMode::Normal
    };

    let sync_engine = SnapshotMetashrewSync::new(
        node_adapter,
        storage_adapter,
        runtime_adapter,
        sync_config,
        sync_mode,
    );

    if let Some(provider) = snapshot_provider {
        sync_engine.set_snapshot_provider(provider).await;
    }

    let sync_engine_arc = Arc::new(tokio::sync::RwLock::new(sync_engine));
    let app_state = web::Data::new(AppState {
        sync_engine: sync_engine_arc.clone(),
    });

    let indexer_handle = tokio::spawn({
        let sync_engine_clone = sync_engine_arc.clone();
        async move {
            info!("Starting block indexing process...");
            loop {
                let mut engine = sync_engine_clone.write().await;
                match engine.process_next_block().await {
                    Ok(Some(_)) => {
                        // Successfully processed a block, continue immediately
                        continue;
                    }
                    Ok(None) => {
                        // No more blocks to process, wait for new blocks
                        drop(engine); // Release the lock before sleeping
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                    Err(e) => {
                        // CRITICAL FIX: Don't advance to next block on error!
                        // The process_next_block() method internally manages height,
                        // but on error we need to retry the SAME block, not skip it.
                        error!("Indexer error: {}", e);
                        drop(engine); // Release the lock before sleeping
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        // Continue the loop to retry the same block
                    }
                }
            }
        }
    });

    let server_handle = tokio::spawn({
        let args_clone = Arc::new(args.clone());
        HttpServer::new(move || {
            let cors = match &args_clone.cors {
                Some(cors_value) if cors_value == "*" => Cors::default()
                    .allow_any_origin()
                    .allow_any_method()
                    .allow_any_header(),
                Some(cors_value) => {
                    let mut cors_builder = Cors::default();
                    for origin in cors_value.split(',') {
                        cors_builder = cors_builder.allowed_origin(origin.trim());
                    }
                    cors_builder
                }
                None => Cors::default().allowed_origin("http://localhost:8080"),
            };
            App::new()
                .wrap(cors)
                .app_data(app_state.clone())
                .service(
                    web::resource("/")
                        .route(web::post().to(handle_jsonrpc::<N, S, R>))
                )
        })
        .bind((args.host.as_str(), args.port))?
        .run()
    });

    info!("JSON-RPC server running at http://{}:{}", args.host, args.port);
    info!("Indexer is ready and processing blocks.");

    let shutdown_signal = setup_signal_handler().await;
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
        _ = async {
            loop {
                if shutdown_signal.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        } => {
            info!("Graceful shutdown complete.");
        }
    }

    Ok(())
}

/// Create optimized RocksDB options for large-scale deployment
///
/// Configuration optimized for:
/// - Database size: 500GB-2TB
/// - Key-value pairs: ~1.5 billion
/// - Key size: up to 256 bytes
/// - Value size: typically 64 bytes, up to 4MB
/// - Batch size: 50K-150K operations per batch
/// - Use case: Fast initial sync on multithreaded systems
pub fn create_optimized_rocksdb_options() -> rocksdb::Options {
    let mut opts = rocksdb::Options::default();
    
    // === BASIC CONFIGURATION ===
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);
    
    // === MEMORY CONFIGURATION ===
    // Large write buffer for batch operations (128MB per memtable)
    opts.set_write_buffer_size(128 * 1024 * 1024);
    // Multiple memtables to handle concurrent writes
    opts.set_max_write_buffer_number(6);
    opts.set_min_write_buffer_number_to_merge(2);
    
    // Block cache for reads (2GB - adjust based on available RAM)
    let cache = rocksdb::Cache::new_lru_cache(2 * 1024 * 1024 * 1024);
    
    // === TABLE OPTIONS (SST FILES) ===
    let mut table_opts = rocksdb::BlockBasedOptions::default();
    table_opts.set_block_cache(&cache);
    table_opts.set_block_size(64 * 1024); // 64KB blocks for large values
    table_opts.set_cache_index_and_filter_blocks(true);
    table_opts.set_pin_l0_filter_and_index_blocks_in_cache(true);
    // Use bloom filter to reduce disk reads
    table_opts.set_bloom_filter(10.0, false);
    // Enable compression for storage efficiency
    table_opts.set_format_version(5);
    opts.set_block_based_table_factory(&table_opts);
    
    // === COMPACTION CONFIGURATION ===
    // Use Level compaction for better space efficiency with large datasets
    opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);
    // Larger L0 to reduce compaction overhead during fast sync
    opts.set_level_zero_file_num_compaction_trigger(8);
    opts.set_level_zero_slowdown_writes_trigger(20);
    opts.set_level_zero_stop_writes_trigger(36);
    // Target file size for L1 (256MB)
    opts.set_target_file_size_base(256 * 1024 * 1024);
    opts.set_target_file_size_multiplier(2);
    // Max bytes for L1 (2GB)
    opts.set_max_bytes_for_level_base(2 * 1024 * 1024 * 1024);
    opts.set_max_bytes_for_level_multiplier(8.0);
    
    // === PARALLELISM CONFIGURATION ===
    // Utilize multiple CPU cores for compaction and flushes
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4) as i32;
    opts.set_max_background_jobs(cpu_count.max(8)); // At least 8 background jobs
    opts.set_max_subcompactions(cpu_count as u32);
    
    // === WRITE OPTIMIZATION ===
    // Optimize for large batch writes
    opts.set_max_write_buffer_size_to_maintain(512 * 1024 * 1024); // 512MB
    opts.set_db_write_buffer_size(1024 * 1024 * 1024); // 1GB total write buffer
    
    // === FILE SYSTEM CONFIGURATION ===
    // Increase file limits for large databases
    opts.set_max_open_files(50000); // Increased from 10000
    // Use direct I/O to avoid double buffering
    opts.set_use_direct_reads(true);
    opts.set_use_direct_io_for_flush_and_compaction(true);
    
    // === COMPRESSION ===
    // Use LZ4 for L0-L2 (fast compression for recent data)
    opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
    // Use Zstd for L3+ (better compression for older data)
    opts.set_compression_per_level(&[
        rocksdb::DBCompressionType::None,    // L0
        rocksdb::DBCompressionType::Lz4,     // L1
        rocksdb::DBCompressionType::Lz4,     // L2
        rocksdb::DBCompressionType::Zstd,    // L3+
        rocksdb::DBCompressionType::Zstd,
        rocksdb::DBCompressionType::Zstd,
        rocksdb::DBCompressionType::Zstd,
    ]);
    
    // === LOGGING AND MONITORING ===
    opts.set_log_level(rocksdb::LogLevel::Info);
    opts.set_keep_log_file_num(5);
    opts.set_log_file_time_to_roll(24 * 60 * 60); // 24 hours
    
    // === ADDITIONAL OPTIMIZATIONS ===
    // Optimize for sequential writes (blockchain data)
    opts.set_level_compaction_dynamic_level_bytes(true);
    // Reduce write amplification
    opts.set_bytes_per_sync(8 * 1024 * 1024); // 8MB
    opts.set_wal_bytes_per_sync(8 * 1024 * 1024); // 8MB
    
    // === WAL (Write-Ahead Log) CONFIGURATION ===
    // Large WAL for batch operations
    opts.set_max_total_wal_size(2 * 1024 * 1024 * 1024); // 2GB
    opts.set_wal_ttl_seconds(0); // Keep WAL files
    opts.set_wal_size_limit_mb(0); // No size limit
    
    opts
}

/// Production-specific run function.
pub async fn run_prod(args: Args) -> Result<()> {
    let (rpc_url, bypass_ssl, tunnel_config) =
        parse_daemon_rpc_url(&args.daemon_rpc_url).await?;

    // Use optimized RocksDB configuration for large-scale deployment
    let opts = create_optimized_rocksdb_options();
    
    info!("Initializing RocksDB with optimized configuration for large-scale deployment");
    info!("Database path: {}", args.db_path.display());
    info!("Expected scale: 500GB-2TB, ~1.5B key-value pairs");
    info!("Batch size: 50K-150K operations per batch");

    let runtime = MetashrewRuntime::load(
        args.indexer.clone(),
        RocksDBRuntimeAdapter::open(args.db_path.to_string_lossy().to_string(), opts)?,
    )?;

    let db = {
        let context = runtime.context.lock().map_err(|_| anyhow!("Failed to lock context"))?;
        context.db.db.clone()
    };

    let node_adapter = BitcoinRpcAdapter::new(rpc_url, args.auth.clone(), bypass_ssl, tunnel_config);
    let storage_adapter = RocksDBStorageAdapter::new(db.clone());
    let runtime_adapter = MetashrewRuntimeAdapter::new(Arc::new(tokio::sync::RwLock::new(runtime)));

    run(args, node_adapter, storage_adapter, runtime_adapter, None).await
}

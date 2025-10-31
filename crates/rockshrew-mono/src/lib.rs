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
use anyhow::Result;
use clap::Parser;
use log::{error, info, warn};
use std::path::PathBuf;

use std::sync::Arc;
use std::time::Instant;
use tokio::signal;
use tokio::runtime::Builder;
use tracing::{debug, instrument};

use crate::adapters::BitcoinRpcAdapter;
use crate::adapters::MetashrewRuntimeAdapter;
use crate::ssh_tunnel::parse_daemon_rpc_url;
use metashrew_runtime::{set_label, MetashrewRuntime};
use metashrew_sync::{
    BitcoinNodeAdapter, RuntimeAdapter, SnapshotMetashrewSync,
    SnapshotProvider, StorageAdapter, SyncConfig, SyncMode, SnapshotSyncEngine, ViewCall, SyncError, PreviewCall
};
use rockshrew_runtime::{
    adapter::{query_height_legacy, RocksDBRuntimeAdapter},
    fork_adapter::{ForkAdapter, LegacyRocksDBRuntimeAdapter},
    query_height, RocksDBStorageAdapter,
};

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
    pub fork: Option<PathBuf>,
    #[arg(long)]
    pub legacy_fork: bool,
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
    pub runtime: Arc<tokio::sync::RwLock<R>>,
    pub storage: Arc<tokio::sync::RwLock<S>>,
}
/// Handles JSON-RPC requests.
#[instrument(skip(body, state))]
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

    let start_time = Instant::now();
    let result: Result<serde_json::Value, anyhow::Error> = (async {
            match method {
                "metashrew_view" => {
                    let function_name = params[0].as_str().unwrap_or_default().to_string();
                    let input_hex = params[1].as_str().unwrap_or_default().to_string();
                    let height_str = match params.get(2) {
                        Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
                        Some(v) if v.is_number() => v.to_string(),
                        _ => "latest".to_string(),
                    };

                    let input_data = hex::decode(input_hex.trim_start_matches("0x"))
                        .map_err(|e| SyncError::Serialization(format!("Invalid hex input: {}", e)))?;

                    let height = if height_str == "latest" {
                        let storage = state.storage.read().await;
                        storage.get_indexed_height().await?
                    } else {
                        height_str.parse::<u32>()?
                    };

                    let call = ViewCall {
                        function_name,
                        input_data,
                        height,
                    };

                    let runtime = state.runtime.read().await;
                    let result = runtime.execute_view(call).await?;
                    Ok(serde_json::Value::String(format!("0x{}", hex::encode(result.data))))
                }
                "metashrew_preview" => {
                    let block_hex = params[0].as_str().unwrap_or_default().to_string();
                    let function_name = params[1].as_str().unwrap_or_default().to_string();
                    let input_hex = params[2].as_str().unwrap_or_default().to_string();
                    let height_str = match params.get(3) {
                        Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
                        Some(v) if v.is_number() => v.to_string(),
                        _ => "latest".to_string(),
                    };

                    let block_data = hex::decode(block_hex.trim_start_matches("0x"))
                        .map_err(|e| SyncError::Serialization(format!("Invalid hex block data: {}", e)))?;

                    let input_data = hex::decode(input_hex.trim_start_matches("0x"))
                        .map_err(|e| SyncError::Serialization(format!("Invalid hex input: {}", e)))?;

                    let height = if height_str == "latest" {
                        let storage = state.storage.read().await;
                        storage.get_indexed_height().await?
                    } else {
                        height_str.parse::<u32>()?
                    };

                    let call = PreviewCall {
                        block_data,
                        function_name,
                        input_data,
                        height,
                    };

                    let runtime = state.runtime.read().await;
                    let result = runtime.execute_preview(call).await?;
                    Ok(serde_json::Value::String(format!("0x{}", hex::encode(result.data))))
                }
                "metashrew_height" => {
            let storage = state.storage.read().await;
            let height = storage.get_indexed_height().await?;
            Ok(serde_json::Value::String(height.to_string()))
        }
                        "metashrew_getblockhash" => {
                            let height = params[0].as_u64().unwrap_or_default() as u32;
                            let storage = state.storage.read().await;
                            let hash = storage.get_block_hash(height).await?;
                            Ok(serde_json::Value::String(hash.map(|h| format!("0x{}", hex::encode(h))).unwrap_or_default()))
                        }        "metashrew_stateroot" => {
            let height_str = params[0].as_str().unwrap_or("latest").to_string();
            let height = if height_str == "latest" {
                let storage = state.storage.read().await;
                storage.get_indexed_height().await?
            } else {
                height_str.parse::<u32>()?
            };
            let storage = state.storage.read().await;
            let root = storage.get_state_root(height).await?;
            Ok(serde_json::Value::String(root.map(|r| format!("0x{}", hex::encode(r))).unwrap_or_default()))
        }
        "metashrew_snapshot" => {
            let storage = state.storage.read().await;
            let stats = storage.get_stats().await?;
            let sync_engine = state.sync_engine.read().await;
            let snapshot_stats = sync_engine.get_snapshot_stats().await?;
            Ok(serde_json::json!({
                "enabled": true,
                "current_height": snapshot_stats.current_height,
                "indexed_height": stats.indexed_height,
                "total_entries": stats.total_entries,
                "storage_size_bytes": stats.storage_size_bytes,
                "sync_mode": snapshot_stats.sync_mode,
                "snapshots_created": snapshot_stats.snapshots_created,
                "snapshots_applied": snapshot_stats.snapshots_applied,
                "last_snapshot_height": snapshot_stats.last_snapshot_height,
                "blocks_synced_normally": snapshot_stats.blocks_synced_normally,
                "blocks_synced_from_snapshots": snapshot_stats.blocks_synced_from_snapshots
            }))
        }
                _ => Err(anyhow::anyhow!("Method not found").into()),
            }
        }).await;

    let duration = start_time.elapsed();
    
    // Log slow RPC calls
    if duration > std::time::Duration::from_millis(100) {
        warn!("Slow RPC call: {} took {:?}", method, duration);
    } else {
        debug!("RPC call: {} completed in {:?}", method, duration);
    }

    let response = match result {
        Ok(res) => serde_json::json!({
            "jsonrpc": "2.0",
            "result": res,
            "id": id
        }),
        Err(e) => {
            error!("RPC error for method {}: {}", method, e);
            serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32000,
                    "message": e.to_string()
                },
                "id": id
            })
        }
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Sets up a signal handler for graceful shutdown.
async fn setup_signal_handler() -> tokio::sync::broadcast::Sender<()> {
    let (tx, _) = tokio::sync::broadcast::channel(1);
    let shutdown_tx = tx.clone();
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("Failed to install CTRL+C signal handler");
        let _ = shutdown_tx.send(());
        info!("Shutdown signal received, initiating graceful shutdown...");
    });
    tx
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

    let start_block = if let Some(fork_path) = &args.fork {
        let fork_db_path = fork_path.to_string_lossy().to_string();
        let opts = RocksDBRuntimeAdapter::get_optimized_options();
        let fork_db = rocksdb::DB::open_for_read_only(&opts, fork_db_path, false)?;
        let tip_height = if args.legacy_fork {
            query_height_legacy(Arc::new(fork_db), 0).await?
        } else {
            query_height(Arc::new(fork_db), 0).await?
        };
        info!("Forking from height: {}", tip_height);
        args.start_block.unwrap_or(tip_height)
    } else {
        args.start_block.unwrap_or(0)
    };

    let sync_config = SyncConfig {
        start_block,
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

    let storage_adapter = Arc::new(tokio::sync::RwLock::new(storage_adapter));
    let runtime_adapter = Arc::new(tokio::sync::RwLock::new(runtime_adapter));

    let sync_engine = SnapshotMetashrewSync::new(
        node_adapter,
        storage_adapter.clone(),
        runtime_adapter.clone(),
        sync_config,
        sync_mode,
    );

    if let Some(provider) = snapshot_provider {
        sync_engine.set_snapshot_provider(provider).await;
    }

    let sync_engine_arc = Arc::new(tokio::sync::RwLock::new(sync_engine));
    let app_state = web::Data::new(AppState {
        sync_engine: sync_engine_arc.clone(),
        runtime: runtime_adapter.clone(),
        storage: storage_adapter.clone(),
    });

    let shutdown_tx = setup_signal_handler().await;
    let mut shutdown_rx_main = shutdown_tx.subscribe();
    let shutdown_rx_indexer = shutdown_tx.subscribe();

    let indexer_handle = run_indexer_pipeline(sync_engine_arc.clone(), shutdown_rx_indexer)?;

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

    tokio::select! {
        result = server_handle => {
            if let Err(e) = result {
                error!("Server task failed: {}", e);
            }
        }
        _ = shutdown_rx_main.recv() => {
            info!("Graceful shutdown initiated, waiting for tasks to complete...");
        }
    }

    // Wait for the indexer thread to finish
    if let Err(e) = indexer_handle.join() {
        error!("Indexer thread panicked: {:?}", e);
    }

    info!("Graceful shutdown complete.");

    Ok(())
}

fn run_indexer_pipeline<N, S, R>(
    sync_engine_arc: Arc<tokio::sync::RwLock<SnapshotMetashrewSync<N, S, R>>>,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>, 
) -> Result<std::thread::JoinHandle<()>>
where
    N: BitcoinNodeAdapter + 'static,
    S: StorageAdapter + 'static,
    R: RuntimeAdapter + 'static,
{
    let indexer_thread = std::thread::spawn(move || {
        let runtime = Builder::new_multi_thread()
            .worker_threads(4) // Adjust the number of threads as needed
            .thread_name("indexer-pool")
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async move {
            info!("Starting block indexing process on dedicated thread pool...");
            let mut block_count = 0u64;
            let mut total_processing_time = std::time::Duration::ZERO;
            let start_time = Instant::now();

            loop {
                if let Ok(_) = shutdown_rx.try_recv() {
                    info!("Indexer pipeline shutting down.");
                    break;
                }

                let block_start = Instant::now();
                let mut engine = sync_engine_arc.write().await;

                match engine.process_next_block().await {
                    Ok(Some(height)) => {
                        let block_duration = block_start.elapsed();
                        block_count += 1;
                        total_processing_time += block_duration;

                        if block_duration > std::time::Duration::from_millis(500) {
                            warn!("Slow block processing at height {}: {:?}", height, block_duration);
                        }

                        if block_count % 100 == 0 {
                            let avg_time = total_processing_time / block_count as u32;
                            let elapsed = start_time.elapsed();
                            let blocks_per_sec = block_count as f64 / elapsed.as_secs_f64();
                            info!(
                                "Performance: {} blocks processed, avg: {:?}/block, rate: {:.2} blocks/sec",
                                block_count, avg_time, blocks_per_sec
                            );
                        }

                        debug!("Processed block {} in {:?}", height, block_duration);
                        continue;
                    }
                    Ok(None) => {
                        debug!("No new blocks available, waiting...");
                        drop(engine);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                    Err(e) => {
                        error!("Fatal indexer error: {}. Shutting down.", e);
                        break;
                    }
                }
            }
        });
    });
    Ok(indexer_thread)
}

// RocksDB configuration has been moved to rockshrew-runtime/src/optimized_config.rs
// for better organization and reusability across the codebase.

/// Production-specific run function.
async fn run_generic<R: RuntimeAdapter + 'static>(
    args: Args,
    runtime_adapter: R,
    storage_adapter: RocksDBStorageAdapter,
) -> Result<()> {
    let (rpc_url, bypass_ssl, tunnel_config) =
        parse_daemon_rpc_url(&args.daemon_rpc_url).await?;
    let node_adapter = BitcoinRpcAdapter::new(rpc_url, args.auth.clone(), bypass_ssl, tunnel_config);
    run(args, node_adapter, storage_adapter, runtime_adapter, None).await
}

pub async fn run_prod(args: Args) -> Result<()> {
    info!("Initializing RocksDB with performance-optimized configuration");
    info!("Database path: {}", args.db_path.display());
    info!("Optimizations: bloom filter tuning, cache optimization, reduced I/O overhead");

    if let Some(fork_path) = args.fork.clone() {
        info!("Fork mode enabled, forking from: {}", fork_path.display());
        let db_path = args.db_path.to_string_lossy().to_string();
        let fork_path_str = fork_path.to_string_lossy().to_string();
        let opts = RocksDBRuntimeAdapter::get_optimized_options();
        let adapter = if args.legacy_fork {
            info!("Using legacy fork adapter.");
            let primary_db = rocksdb::DB::open(&opts, db_path)?;
            let fork_db = rocksdb::DB::open_for_read_only(&opts, fork_path_str, false)?;
            let legacy_adapter = LegacyRocksDBRuntimeAdapter {
                db: Arc::new(primary_db),
                fork_db: Some(Arc::new(fork_db)),
                height: 0,
                kv_tracker: Arc::new(std::sync::Mutex::new(None)),
            };
            ForkAdapter::Legacy(legacy_adapter)
        } else {
            let modern_adapter = RocksDBRuntimeAdapter::open_fork(db_path, fork_path_str, opts)?;
            ForkAdapter::Modern(modern_adapter)
        };
        let runtime = MetashrewRuntime::load(args.indexer.clone(), adapter)?;
        let storage_adapter = match runtime.context.lock().unwrap().db {
            ForkAdapter::Modern(ref modern_adapter) => {
                RocksDBStorageAdapter::new(modern_adapter.db.clone())
            }
            ForkAdapter::Legacy(ref legacy_adapter) => {
                RocksDBStorageAdapter::new(legacy_adapter.db.clone())
            }
        };
        let runtime_adapter =
            MetashrewRuntimeAdapter::new(Arc::new(tokio::sync::RwLock::new(runtime)));
        run_generic(args, runtime_adapter, storage_adapter).await
    } else {
        let adapter =
            RocksDBRuntimeAdapter::open_optimized(args.db_path.to_string_lossy().to_string())?;
        let runtime = MetashrewRuntime::load(args.indexer.clone(), adapter.clone())?;
        let storage_adapter = RocksDBStorageAdapter::new(adapter.db.clone());
        let runtime_adapter =
            MetashrewRuntimeAdapter::new(Arc::new(tokio::sync::RwLock::new(runtime)));
        run_generic(args, runtime_adapter, storage_adapter).await
    }
}

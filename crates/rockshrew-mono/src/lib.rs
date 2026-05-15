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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::{debug, instrument};

use crate::adapters::BitcoinRpcAdapter;
use crate::adapters::MetashrewRuntimeAdapter;
use crate::ssh_tunnel::parse_daemon_rpc_url;
use metashrew_runtime::{set_label, MetashrewRuntime};
use metashrew_sync::{
    BitcoinNodeAdapter, JsonRpcProvider, RuntimeAdapter, SnapshotMetashrewSync,
    SnapshotProvider, StorageAdapter, SyncConfig, SyncMode,
};
use rockshrew_runtime::{
    adapter::{query_height_legacy, RocksDBRuntimeAdapter},
    fork_adapter::{ForkAdapter, LegacyRocksDBRuntimeAdapter},
    query_height, RocksDBStorageAdapter,
};
use tokio::sync::mpsc;

const DEFAULT_PREFETCH_SIZE: usize = 64;

#[derive(Debug)]
struct BlockData {
    height: u32,
    block_data: Vec<u8>,
    block_hash: Vec<u8>,
}

#[derive(Debug)]
enum BlockResult {
    Success(u32),
    Error(u32, anyhow::Error),
}

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
    #[arg(long)]
    pub prefetch_size: Option<usize>,
    /// Enable the `metashrew_preview` JSON-RPC method.
    ///
    /// Default: disabled. With the flag absent (production indexer
    /// pods), `metashrew_preview` returns `Method not enabled`. We
    /// observed state divergence on every pod that took sustained
    /// view/preview LB traffic for several hours under v9.0.4-rc.2,
    /// despite the `create_isolated_copy()` fix that closed the
    /// original write-through leak (commit 535679c). Disabling
    /// preview entirely on production pods is a load-bearing
    /// diagnostic: if drift stops in v9.0.4-rc.4 with preview off,
    /// the residual leak is in the preview path; if drift persists,
    /// the leak is elsewhere (view path, snapshot path, etc.).
    #[arg(long, default_value_t = false)]
    pub enable_preview: bool,
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
    /// Mirror of `Args::enable_preview`. Set once at startup, never
    /// changed at runtime; checked in `handle_jsonrpc` before
    /// dispatching `metashrew_preview`.
    pub enable_preview: bool,
}

/// Per-method server-side timeout. Bounds how long a single JSON-RPC request
/// can occupy the view layer before being cancelled, even if the upstream
/// HTTP proxy has already given up on the response. Without this guard,
/// disconnected requests run to completion (`async/await` does not propagate
/// client disconnect by default) and the worker pool fills with orphaned WASM
/// executions until the whole process halts. v9.0.4-rc.2.
///
/// `metashrew_view` and `metashrew_preview` can legitimately take seconds on
/// expensive contracts — give them headroom. Everything else hits storage
/// directly and should return in milliseconds; a tight cap there frees worker
/// slots fast when RocksDB itself is stuck.
fn rpc_method_timeout(method: &str) -> Duration {
    match method {
        "metashrew_view" => Duration::from_secs(60),
        "metashrew_preview" => Duration::from_secs(120),
        // Pure storage reads — height, blockhash, stateroot, snapshot.
        _ => Duration::from_secs(10),
    }
}

/// Handles JSON-RPC requests with per-method timeouts and cancellation
/// propagation. The actual per-method work is spawned as a child task tied
/// to a `CancellationToken`. The handler races three outcomes:
///
/// 1. **Work completes**     — return the JSON-RPC response.
/// 2. **Timeout fires**      — cancel the work, return JSON-RPC error -32000
///                             "timed out".
/// 3. **Handler dropped**    — actix-web drops this future when the response
///                             writer detects a closed socket on its next
///                             write attempt. The `DropGuard` held by this
///                             future then cancels the spawned work, which
///                             unwinds the WASM at its next yield point
///                             (≤10k fuel units, configured in
///                             `MetashrewRuntime::view`).
///
/// Cancellation works because `wasmtime` async support + the
/// `fuel_async_yield_interval(Some(10000))` set in `runtime.rs` make the
/// `call_async` future cancel-aware. Dropping it terminates WASM execution
/// at the next yield. The `RwLockReadGuard` returned by
/// `state.sync_engine.read().await` is also dropped via RAII, releasing the
/// read lock immediately so block ingestion (writer) is not held off any
/// longer than necessary.
///
/// This patch only affects the read-only view layer — the indexer block
/// processing loop runs through a separate code path on the sync engine.
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
    let method = request["method"].as_str().unwrap_or_default().to_string();
    let empty_params = vec![];
    let params_owned = request["params"].as_array().cloned().unwrap_or(empty_params);
    let id = request["id"].clone();

    let timeout = rpc_method_timeout(&method);
    let cancel = CancellationToken::new();
    // Held by this handler future. When the handler is dropped (client
    // disconnect, or `tokio::select!` arm wins), this guard drops and
    // triggers `cancel.cancel()` on every clone — propagating the abort to
    // the spawned task below.
    let _drop_guard = cancel.clone().drop_guard();

    let state_clone = state.clone();
    let method_for_work = method.clone();
    let cancel_for_work = cancel.clone();

    // Spawn the actual per-method work as a child task. Spawning is
    // necessary because actix's handler future is bound to the request
    // lifecycle; the spawned task observes cancellation via the shared token
    // and exits at the next WASM yield point.
    let work = tokio::spawn(async move {
        let work_fut = async move {
            match method_for_work.as_str() {
                "metashrew_view" => {
                    // Acquire the outer read lock just long enough to clone
                    // the runtime Arc and capture current_height; drop the
                    // guard BEFORE running the WASM. This is the same data
                    // the in-trait metashrew_view would have read, but the
                    // ~seconds-long WASM execution that follows no longer
                    // blocks any reconfiguration/write that needs the outer
                    // lock. Snapshot isolation in execute_view (v9.0.4-rc.1)
                    // makes early release safe.
                    let function_name = params_owned.get(0).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let input_hex = params_owned.get(1).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let height_str = match params_owned.get(2) {
                        Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
                        Some(v) if v.is_number() => v.to_string(),
                        _ => "latest".to_string(),
                    };

                    let input_data = match hex::decode(input_hex.trim_start_matches("0x")) {
                        Ok(b) => b,
                        Err(e) => return Err(metashrew_sync::error::SyncError::Serialization(format!("Invalid hex input: {}", e))),
                    };

                    let (runtime, current) = {
                        let g = state_clone.sync_engine.read().await;
                        (Arc::clone(g.runtime()), g.current_height())
                    };

                    let height = if height_str == "latest" {
                        current.saturating_sub(1)
                    } else {
                        metashrew_sync::snapshot_sync::parse_height_string(&height_str)?
                    };

                    let call = metashrew_sync::ViewCall { function_name, input_data, height };
                    let result = runtime.execute_view(call).await?;
                    Ok(format!("0x{}", hex::encode(result.data)))
                }
                "metashrew_preview" => {
                    // Gated behind --enable-preview. Production pods leave
                    // this off so the WASM host functions that historically
                    // leaked hypothetical writes into prod state never run.
                    // v9.0.4-rc.2 fixed `create_isolated_copy()` but we
                    // still observed drift under sustained preview load;
                    // disabling preview entirely on indexer pods narrows
                    // the leak hunt.
                    if !state_clone.enable_preview {
                        return Err(metashrew_sync::error::SyncError::Generic(
                            anyhow::anyhow!(
                                "metashrew_preview is not enabled on this node \
                                 (start rockshrew-mono with --enable-preview to allow it)"
                            )
                        ));
                    }
                    // Same early-release pattern as metashrew_view —
                    // preview also runs WASM via runtime.execute_preview
                    // and snapshot isolation means we don't need the outer
                    // lock held across the WASM call.
                    let block_hex = params_owned.get(0).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let function_name = params_owned.get(1).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let input_hex = params_owned.get(2).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let height_str = match params_owned.get(3) {
                        Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
                        Some(v) if v.is_number() => v.to_string(),
                        _ => "latest".to_string(),
                    };

                    let block_data = match hex::decode(block_hex.trim_start_matches("0x")) {
                        Ok(b) => b,
                        Err(e) => return Err(metashrew_sync::error::SyncError::Serialization(format!("Invalid hex block data: {}", e))),
                    };
                    let input_data = match hex::decode(input_hex.trim_start_matches("0x")) {
                        Ok(b) => b,
                        Err(e) => return Err(metashrew_sync::error::SyncError::Serialization(format!("Invalid hex input: {}", e))),
                    };

                    let (runtime, current) = {
                        let g = state_clone.sync_engine.read().await;
                        (Arc::clone(g.runtime()), g.current_height())
                    };

                    let height = if height_str == "latest" {
                        current.saturating_sub(1)
                    } else {
                        metashrew_sync::snapshot_sync::parse_height_string(&height_str)?
                    };

                    let call = metashrew_sync::PreviewCall { block_data, function_name, input_data, height };
                    let result = runtime.execute_preview(call).await?;
                    Ok(format!("0x{}", hex::encode(result.data)))
                }
                // Storage-read methods stay on the trait API (sub-ms work;
                // the read lock is briefly contended but never held across
                // anything long-running).
                "metashrew_height" => state_clone.sync_engine.read().await.metashrew_height().await.map(|h| h.to_string()),
                "metashrew_getblockhash" => {
                    let height = params_owned.get(0).and_then(|v| v.as_u64()).unwrap_or_default() as u32;
                    state_clone.sync_engine.read().await.metashrew_getblockhash(height).await
                }
                "metashrew_stateroot" => {
                    let height = params_owned.get(0).and_then(|v| v.as_str()).unwrap_or("latest").to_string();
                    state_clone.sync_engine.read().await.metashrew_stateroot(height).await
                }
                "metashrew_snapshot" => state_clone.sync_engine.read().await.metashrew_snapshot().await.map(|v| v.to_string()),
                _ => Err(anyhow::anyhow!("Method not found").into()),
            }
        };

        // Inner race: cancellation vs work-complete. On cancel, the work
        // future is dropped at the next `.await` point inside it — including
        // any active WASM execution (wasmtime async yield).
        tokio::select! {
            biased;
            _ = cancel_for_work.cancelled() => Err(
                metashrew_sync::error::SyncError::Generic(anyhow::anyhow!("cancelled"))
            ),
            r = work_fut => r,
        }
    });

    let start_time = Instant::now();

    // Outer race: server-side timeout vs work-complete. On timeout, we
    // signal cancel and let the spawned task unwind on its own — no
    // need to abort, the cancellation token does it cleanly.
    let outcome = tokio::select! {
        biased;
        _ = tokio::time::sleep(timeout) => {
            cancel.cancel();
            warn!(
                "RPC method {} exceeded {}s timeout — cancelled",
                method,
                timeout.as_secs()
            );
            Err(format!("Request timed out after {}s", timeout.as_secs()))
        }
        joined = work => match joined {
            Ok(r) => r.map_err(|e| e.to_string()),
            Err(join_err) => Err(format!("Worker task panicked: {}", join_err)),
        },
    };

    let duration = start_time.elapsed();

    // Log slow RPC calls
    if duration > Duration::from_millis(100) {
        warn!("Slow RPC call: {} took {:?}", method, duration);
    } else {
        debug!("RPC call: {} completed in {:?}", method, duration);
    }

    let response = match outcome {
        Ok(res) => serde_json::json!({
            "jsonrpc": "2.0",
            "result": res,
            "id": id
        }),
        Err(msg) => {
            error!("RPC error for method {}: {}", method, msg);
            serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32000,
                    "message": msg
                },
                "id": id
            })
        }
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

    let sync_engine = SnapshotMetashrewSync::new(
        node_adapter,
        storage_adapter,
        runtime_adapter,
        sync_config,
        sync_mode,
    );

    sync_engine.init().await;

    if let Some(provider) = snapshot_provider {
        sync_engine.set_snapshot_provider(provider).await;
    }

    let sync_engine_arc = Arc::new(tokio::sync::RwLock::new(sync_engine));
    if args.enable_preview {
        warn!(
            "metashrew_preview is ENABLED on this node — only safe on a \
             dedicated preview tier, NOT on indexer pods serving production \
             LB traffic (preview path historically leaks hypothetical writes \
             into prod state under sustained load)"
        );
    } else {
        info!(
            "metashrew_preview disabled (default); pass --enable-preview to \
             allow it"
        );
    }
    let app_state = web::Data::new(AppState {
        sync_engine: sync_engine_arc.clone(),
        enable_preview: args.enable_preview,
    });

    // Cap the prefetch buffer at reorg_check_threshold so the fetcher can
    // never have more in-flight blocks than the chain-validator's safety
    // window. Without this cap (channel size up to 64 in v9.0.4-alpha.x),
    // a reorg can land while N>threshold prefetched blocks are queued from
    // the discarded fork, then committed before the chain-link discontinuity
    // is detected — leaving rollback-invisible state behind. Capping makes
    // strict-serial-near-tip enforcement automatic.
    let requested = args.prefetch_size.unwrap_or(DEFAULT_PREFETCH_SIZE);
    let prefetch_size = requested.min(args.reorg_check_threshold as usize).max(1);
    if prefetch_size != requested {
        info!(
            "Block prefetch buffer requested={} capped={} (reorg_check_threshold={})",
            requested, prefetch_size, args.reorg_check_threshold,
        );
    } else {
        info!("Block prefetch buffer size: {}", prefetch_size);
    }

    // Prefetch channel: fetcher fills this buffer with blocks fetched concurrently,
    // processor pulls them out sequentially. The channel enforces backpressure —
    // when full, the fetcher waits until the processor consumes blocks.
    let (block_sender, mut block_receiver) = mpsc::channel::<BlockData>(prefetch_size);
    let (result_sender, mut result_receiver) = mpsc::channel::<BlockResult>(prefetch_size);

    let fetcher_handle = tokio::spawn({
        let sync_engine_clone = sync_engine_arc.clone();
        let block_sender_clone = block_sender.clone();
        let exit_at = args.exit_at;

        async move {
            info!("Block fetcher task started (prefetch_size={}).", prefetch_size);
            loop {
                // Get current state
                let engine = sync_engine_clone.read().await;

                if let Some(exit_at) = exit_at {
                    let current_indexed_height = match engine.get_height().await {
                        Ok(h) => h,
                        Err(e) => {
                            error!("Failed to get current indexed height: {}", e);
                            drop(engine);
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    if current_indexed_height >= exit_at {
                        info!("Fetcher reached exit-at block {}, shutting down", exit_at);
                        break;
                    }
                }

                let fetch_start = engine.current_height();
                let remote_tip = match engine.node().get_tip_height().await {
                    Ok(tip) => tip,
                    Err(e) => {
                        error!("Failed to get tip height: {}", e);
                        drop(engine);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                };

                // Check for reorgs when close to tip
                if remote_tip.saturating_sub(fetch_start) <= engine.config.reorg_check_threshold {
                    // Fall back to single-block fetch near tip (reorg-safe)
                    match engine.get_next_block_data().await {
                        Ok(Some((height, block_data, block_hash))) => {
                            drop(engine);
                            if block_sender_clone.send(BlockData { height, block_data, block_hash }).await.is_err() {
                                break;
                            }
                        }
                        Ok(None) => {
                            drop(engine);
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                        Err(e) => {
                            error!("Failed to fetch block: {}", e);
                            drop(engine);
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                    }
                    continue;
                }

                // Batch prefetch: fetch up to prefetch_size blocks concurrently
                let fetch_end = std::cmp::min(
                    fetch_start + prefetch_size as u32,
                    remote_tip + 1,
                );
                if fetch_start >= fetch_end {
                    drop(engine);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }

                let batch_size = (fetch_end - fetch_start) as usize;
                let node = engine.node().clone();
                drop(engine); // Release engine lock before concurrent fetches

                debug!("FETCHER: Prefetching blocks {}..{} ({} blocks)", fetch_start, fetch_end - 1, batch_size);

                // Spawn concurrent fetch tasks
                let mut fetch_handles = Vec::with_capacity(batch_size);
                for h in fetch_start..fetch_end {
                    let node_clone = node.clone();
                    fetch_handles.push(tokio::spawn(async move {
                        let info = node_clone.get_block_info(h).await?;
                        Ok::<BlockData, metashrew_sync::SyncError>(BlockData {
                            height: h,
                            block_data: info.data,
                            block_hash: info.hash,
                        })
                    }));
                }

                // Collect results in order and send to processor
                let mut abort_remaining = false;
                for (i, handle) in fetch_handles.into_iter().enumerate() {
                    if abort_remaining {
                        handle.abort();
                        continue;
                    }
                    match handle.await {
                        Ok(Ok(block)) => {
                            debug!("FETCHER: Fetched block {} ({} bytes)", block.height, block.block_data.len());
                            if block_sender_clone.send(block).await.is_err() {
                                abort_remaining = true;
                            }
                        }
                        Ok(Err(e)) => {
                            error!("FETCHER: Failed to fetch block {}: {}", fetch_start + i as u32, e);
                            abort_remaining = true;
                        }
                        Err(e) => {
                            error!("FETCHER: Fetch task panicked for block {}: {}", fetch_start + i as u32, e);
                            abort_remaining = true;
                        }
                    }
                }

                if abort_remaining {
                    // On error, the processor will handle retry via the result channel
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
            debug!("Block fetcher task completed.");
        }
    });

    // Block processor runs on a dedicated tokio runtime so it never competes
    // with RPC view calls for thread pool time. This ensures indexing progresses
    // steadily regardless of RPC load, and views are never starved by indexing.
    let processor_handle = std::thread::Builder::new()
        .name("block-processor".into())
        .spawn({
            let sync_engine_clone = sync_engine_arc.clone();
            let result_sender_clone = result_sender.clone();

            move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(4)
                    .thread_name("processor-worker")
                    .enable_all()
                    .build()
                    .expect("Failed to create processor runtime");

                rt.block_on(async move {
                    info!("Block processor task started (dedicated runtime).");
                    while let Some(block_data) = block_receiver.recv().await {
                        let block_start = Instant::now();

                        let engine = sync_engine_clone.read().await;
                        let result = match engine.process_block(block_data.height, block_data.block_data, block_data.block_hash).await {
                            Ok(_) => {
                                let elapsed = block_start.elapsed();
                                if elapsed > std::time::Duration::from_secs(1) {
                                    warn!("Slow block {} took {:?}", block_data.height, elapsed);
                                }
                                BlockResult::Success(block_data.height)
                            },
                            Err(e) => {
                                error!("PROCESSOR: Failed block {} after {:?}: {}", block_data.height, block_start.elapsed(), e);
                                BlockResult::Error(block_data.height, e.into())
                            },
                        };
                        drop(engine);

                        if result_sender_clone.send(result).await.is_err() {
                            break;
                        }
                    }
                    debug!("Block processor task completed.");
                });
            }
        })
        .expect("Failed to spawn processor thread");

    let indexer_handle = tokio::spawn({
        let sync_engine_clone = sync_engine_arc.clone();
        async move {
        info!("Starting block indexing process...");
        let mut block_count = 0u64;
        let start_time = Instant::now();

        while let Some(result) = result_receiver.recv().await {
            match result {
                BlockResult::Success(height) => {
                    block_count += 1;

                    if block_count % 100 == 0 {
                        let elapsed = start_time.elapsed();
                        let blocks_per_sec = block_count as f64 / elapsed.as_secs_f64();
                        info!(
                            "Sync: height={}, {} blocks, {:.1} blocks/sec",
                            height, block_count, blocks_per_sec
                        );
                    }
                }
                BlockResult::Error(height, error) => {
                    let error_str = error.to_string();

                    // Check if this is a chain validation error - trigger reorg handling
                    if error_str.contains("does not connect to previous block") || error_str.contains("CHAIN DISCONTINUITY") {
                        warn!("Chain discontinuity at height {}. Triggering reorg.", height);

                        let engine = sync_engine_clone.read().await;
                        match metashrew_sync::sync::handle_reorg(
                            height,
                            engine.node().clone(),
                            engine.storage().clone(),
                            engine.runtime().clone(),
                            &engine.config,
                        )
                        .await
                        {
                            Ok(rollback_height) => {
                                info!("Rolled back to height {}. Resuming sync.", rollback_height);
                            }
                            Err(e) => {
                                error!("Failed to handle reorg: {}", e);
                            }
                        }
                        drop(engine);
                    }

                    error!("Failed to process block {}: {}", height, error_str);
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
            if let Some(exit_at) = args.exit_at {
                if block_count as u32 >= exit_at {
                    info!("Reached exit-at block {}, shutting down gracefully", exit_at);
                    break;
                }
            }
        }
    }});

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
        result = fetcher_handle => {
            if let Err(e) = result {
                error!("Fetcher task failed: {}", e);
            }
        }
        result = tokio::task::spawn_blocking(move || processor_handle.join()) => {
            match result {
                Ok(Ok(_)) => {},
                Ok(Err(_)) => error!("Processor thread panicked"),
                Err(e) => error!("Processor join task failed: {}", e),
            }
        }
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
        let mut config_engine = wasmtime::Config::default();
        config_engine.async_support(true);
        let engine = wasmtime::Engine::new(&config_engine)?;
        let runtime = MetashrewRuntime::load(args.indexer.clone(), adapter, engine).await?;
        let storage_adapter = match runtime.context.read().unwrap().db {
            ForkAdapter::Modern(ref modern_adapter) => {
                RocksDBStorageAdapter::new(modern_adapter.db.clone())
            }
            ForkAdapter::Legacy(ref legacy_adapter) => {
                RocksDBStorageAdapter::new(legacy_adapter.db.clone())
            }
        };
        let runtime_adapter =
            MetashrewRuntimeAdapter::new(Arc::new(runtime));
        run_generic(args, runtime_adapter, storage_adapter).await
    } else {
        let adapter =
            RocksDBRuntimeAdapter::open_optimized(args.db_path.to_string_lossy().to_string())?;
        let mut config_engine = wasmtime::Config::default();
        config_engine.async_support(true);
        let engine = wasmtime::Engine::new(&config_engine)?;
        let runtime = MetashrewRuntime::load(args.indexer.clone(), adapter.clone(), engine).await?;
        let storage_adapter = RocksDBStorageAdapter::new(adapter.db.clone());
        let runtime_adapter =
            MetashrewRuntimeAdapter::new(Arc::new(runtime));
        run_generic(args, runtime_adapter, storage_adapter).await
    }
}

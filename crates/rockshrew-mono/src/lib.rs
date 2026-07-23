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
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
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

/// Sentinel for `Arc<AtomicI64>`-encoded `last_sent_height`. We use a
/// signed atomic so that -1 ("the fetcher has not sent anything yet")
/// is distinguishable from height 0. Heights are u32 so they always fit
/// in the positive range of an i64.
pub(crate) const LAST_SENT_UNSET: i64 = -1;

/// v9.0.5-rc.6 fetcher-dedup helper.
///
/// Pure function (no I/O, no async) that computes the next half-open
/// `[fetch_start, fetch_end)` range the prefetcher should fetch. Tested
/// directly without spinning up the full fetcher loop.
///
/// ## The bug this guards against
///
/// In v9.0.5-rc.5 the fetcher computed `fetch_start =
/// engine.current_height()` every loop iteration. The engine's
/// `current_height` is the PROCESSOR's committed tip — it lags behind
/// what the fetcher has already enqueued. When the processor was slow
/// (e.g. a single block taking 90 seconds to apply), the fetcher would
/// loop, re-read the unchanged `current_height = N`, and re-enqueue the
/// same range `[N, N+prefetch_size)`. Once the processor caught up, it
/// would see (and try to commit) the duplicate `N` AFTER the original
/// `N..N+prefetch_size-1` had already been committed, hitting the
/// rc.3 strict-in-order check in `commit_atomic` and looping forever
/// in the rc.3 infinite-retry backoff.
///
/// ## Invariants enforced
///
/// 1. The fetcher MUST NOT send the same height twice consecutively.
/// 2. The fetcher MUST NOT send a height below the processor's tip
///    (the processor would reject it).
/// 3. Following a rollback (the processor's tip drops because a reorg
///    handler rewound it), the fetcher MUST resume fetching from the
///    new tip — `last_sent_height` is reset down to `LAST_SENT_UNSET`
///    by the rollback path.
///
/// ## Arguments
///
/// * `processor_tip` — the processor's committed tip
///   (`engine.current_height()`).
/// * `last_sent` — the height of the most recent block the fetcher
///   successfully sent to the channel, or `LAST_SENT_UNSET` (-1) if
///   the fetcher hasn't sent anything yet OR if a rollback reset it.
/// * `prefetch_size` — the max number of blocks per batch.
/// * `remote_tip` — the bitcoind tip height.
///
/// ## Return
///
/// `Some((start, end))` — a half-open range of heights to fetch this
/// iteration, or `None` if there's nothing new to fetch
/// (caller should sleep and retry).
pub(crate) fn compute_next_fetch_range(
    processor_tip: u32,
    last_sent: i64,
    prefetch_size: usize,
    remote_tip: u32,
) -> Option<(u32, u32)> {
    // The fetcher's own high-water-mark — what it has already enqueued.
    // If unset, fall back to the processor's tip (first iteration).
    let after_last_sent: u32 = if last_sent < 0 {
        processor_tip
    } else {
        // last_sent is u32-in-i64; the next height to fetch is last_sent + 1.
        (last_sent as u32).saturating_add(1)
    };

    // Never go BELOW the processor's tip — the processor would reject
    // anything below its committed tip. (Rollback path covers
    // "processor went backward": it resets last_sent to UNSET, which
    // makes after_last_sent fall through to processor_tip here.)
    let fetch_start = std::cmp::max(after_last_sent, processor_tip);

    // Cap the batch by prefetch_size and the remote tip.
    if remote_tip < fetch_start {
        return None;
    }
    let fetch_end = std::cmp::min(
        fetch_start.saturating_add(prefetch_size as u32),
        remote_tip.saturating_add(1),
    );
    if fetch_start >= fetch_end {
        return None;
    }
    Some((fetch_start, fetch_end))
}

/// v9.0.5-rc.7 near-tip dedup helper.
///
/// Pure function (no I/O, no async).  Computes the minimum height the
/// near-tip single-block path may send to the prefetch channel, given
/// the fetcher's current `last_sent` watermark.  The near-tip path
/// gets its height from `engine.get_next_block_data()`, which returns
/// the processor's `current_height` atomic — that atomic lags behind
/// `last_sent_height` while the processor is slow, and can also be
/// lowered by `handle_reorg`.  Any height strictly below
/// `near_tip_min_send_height(last_sent)` is a stale duplicate and
/// MUST be dropped.
///
/// Returns 0 when `last_sent` is `LAST_SENT_UNSET` (anything is
/// acceptable on a cold fetcher); otherwise `last_sent + 1`.
pub(crate) fn near_tip_min_send_height(last_sent: i64) -> u32 {
    if last_sent < 0 {
        0
    } else {
        (last_sent as u32).saturating_add(1)
    }
}

/// v9.0.5-rc.7 near-tip dedup decision.
///
/// Returns true iff the near-tip single-block path should drop the
/// block returned by `engine.get_next_block_data()` rather than send
/// it to the prefetch channel.  Encapsulates the dedup invariant so
/// the live loop in `crate::run` and the unit tests share one source
/// of truth.
pub(crate) fn near_tip_should_drop(last_sent: i64, candidate_height: u32) -> bool {
    candidate_height < near_tip_min_send_height(last_sent)
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

    /// v9.0.5-rc.2 view-runtime isolation: maximum number of
    /// concurrent `metashrew_view` calls. Excess calls wait for a
    /// permit up to the per-method JSON-RPC timeout, then return
    /// "view-runtime saturated" instead of OOM-killing the indexer.
    /// Default: 16.
    #[arg(long, default_value_t = metashrew_runtime::DEFAULT_VIEW_CONCURRENCY)]
    pub view_concurrency: usize,

    /// v9.0.5-rc.2 view-runtime isolation: per-view WASM linear-memory
    /// cap, in MB. A `memory.grow` past this budget traps and the
    /// JSON-RPC handler returns a `ResourceExhausted` error
    /// (-32002) rather than letting the view runtime allocate
    /// unboundedly. Default: 256 MB. Not applied to the indexer
    /// runtime — block-application stays unbounded.
    #[arg(long, default_value_t = metashrew_runtime::DEFAULT_VIEW_MEMORY_MB)]
    pub view_memory_mb: usize,

    /// v9.0.5-rc.2 view-runtime isolation: host-memory floor, in MB.
    /// When the host's available memory falls below this threshold
    /// the view-runtime refuses NEW calls (returning "system memory
    /// pressure, view runtime backing off"). The indexer keeps
    /// processing normally. Default: 8192 MB (8 GB).
    #[arg(long, default_value_t = metashrew_runtime::DEFAULT_VIEW_MEMORY_FLOOR_MB)]
    pub view_memory_floor_mb: u64,

    /// v9.0.5-rc.2 view-runtime isolation: cadence at which the
    /// cached host-memory state is refreshed, in milliseconds. Set
    /// higher to reduce per-call overhead; lower to react faster to
    /// memory pressure. Default: 1000 (1 second).
    #[arg(long, default_value_t = metashrew_runtime::DEFAULT_MEMORY_FLOOR_REFRESH_MS)]
    pub view_memory_floor_refresh_ms: u64,

    /// v9.0.5-rc.6: disable the init-time pointer-divergence heal.
    ///
    /// By default, on startup the sync engine reads the three on-disk
    /// height pointers (`__INTERNAL/height`,
    /// `/__INTERNAL/tip-height`, max-stored-blockhash height) and, if
    /// they disagree, rolls back to the minimum. This was added to
    /// recover from the crash-loop class of incident seen on a
    /// mainnet pod where the sync engine's in-memory current_height
    /// fell behind the on-disk tip without a reorg firing, causing
    /// `commit_atomic` to reject every commit with
    /// "out-of-order commit rejected" until the operator
    /// manually restarted.
    ///
    /// Set `--no-startup-heal` to disable. Useful when you want to
    /// inspect divergent state on a damaged DB before letting the
    /// indexer advance.
    #[arg(long, default_value_t = false)]
    pub no_startup_heal: bool,

    /// v10 fresh-sync tuning: open RocksDB with sync-optimized options
    /// instead of the default general-purpose options.
    ///
    /// Sync mode disables bloom filters and uncompresses L0/L1/L2 — both
    /// useful only when catching up from genesis (no point filtering
    /// reads on data that is being written for the first time, no point
    /// compressing what will be re-compressed at compaction). Operators
    /// should restart WITHOUT this flag once the indexer reaches tip so
    /// bloom filters rebuild for query-path perf.
    ///
    /// Default: disabled (production behaviour unchanged). Set for
    /// fresh-sync deployments only.
    #[arg(long, default_value_t = false)]
    pub sync_mode: bool,

    /// Path to a UNIX control socket for operator commands. When set,
    /// rockshrew-mono binds it and accepts newline-delimited JSON commands:
    ///   `{"cmd":"rollback","height":N}` — orphan all indexed state above N and
    ///      resume indexing from N+1 (a programmatic, in-process rollback — a
    ///      cleaner alternative to forcing it via the `/v4/reorg/*` HTTP trick);
    ///   `{"cmd":"status"}` — returns the current indexed height;
    ///   `{"cmd":"ping"}`   — liveness.
    /// Each command is one JSON object per line; the reply is one JSON line.
    /// A rollback runs in the indexer task at a block boundary — intended for use
    /// at/near tip (idle processor); issuing one mid-block-application could race.
    #[arg(long)]
    pub control_socket: Option<PathBuf>,
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
    /// v9.0.5-rc.2 view-runtime isolation: per-process limiter that
    /// gates `metashrew_view` and `metashrew_preview` calls behind a
    /// concurrency semaphore + host-memory floor. None disables both
    /// (kept for tests). The per-view WASM memory cap is enforced
    /// separately by the `RuntimeAdapter` via `view_with_limits`.
    pub view_limiter: Option<Arc<metashrew_runtime::ViewLimiter>>,
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
    let view_limiter = state.view_limiter.clone();
    let acquire_timeout = timeout;
    let work = tokio::spawn(async move {
        let work_fut = async move {
            match method_for_work.as_str() {
                "metashrew_view" => {
                    // v9.0.5-rc.2: acquire a view-runtime permit (concurrency
                    // bound + host-memory floor) BEFORE we touch the
                    // sync_engine read lock or instantiate any WASM. Bound
                    // the wait by the per-method timeout so the JSON-RPC
                    // outer timeout still wins.
                    let _permit = if let Some(limiter) = view_limiter.as_ref() {
                        match limiter.acquire(Some(acquire_timeout)).await {
                            Ok(p) => Some(p),
                            Err(metashrew_runtime::ViewAcquireError::Saturated) => {
                                return Err(metashrew_sync::error::SyncError::Unavailable(
                                    "view-runtime saturated, retry later".to_string(),
                                ));
                            }
                            Err(metashrew_runtime::ViewAcquireError::MemoryFloor) => {
                                return Err(metashrew_sync::error::SyncError::Unavailable(
                                    "system memory pressure, view runtime backing off"
                                        .to_string(),
                                ));
                            }
                            Err(metashrew_runtime::ViewAcquireError::Closed) => {
                                return Err(metashrew_sync::error::SyncError::Unavailable(
                                    "view runtime is shutting down".to_string(),
                                ));
                            }
                        }
                    } else {
                        None
                    };
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
                    // v9.0.5-rc.2: preview also runs WASM with an
                    // unbounded store — gate it behind the same limiter
                    // as metashrew_view so a preview burst can't OOM the
                    // indexer either.
                    let _permit = if let Some(limiter) = view_limiter.as_ref() {
                        match limiter.acquire(Some(acquire_timeout)).await {
                            Ok(p) => Some(p),
                            Err(metashrew_runtime::ViewAcquireError::Saturated) => {
                                return Err(metashrew_sync::error::SyncError::Unavailable(
                                    "view-runtime saturated, retry later".to_string(),
                                ));
                            }
                            Err(metashrew_runtime::ViewAcquireError::MemoryFloor) => {
                                return Err(metashrew_sync::error::SyncError::Unavailable(
                                    "system memory pressure, view runtime backing off"
                                        .to_string(),
                                ));
                            }
                            Err(metashrew_runtime::ViewAcquireError::Closed) => {
                                return Err(metashrew_sync::error::SyncError::Unavailable(
                                    "view runtime is shutting down".to_string(),
                                ));
                            }
                        }
                    } else {
                        None
                    };
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
    // Outcome carries a typed error code so we can map Unavailable /
    // ResourceExhausted to dedicated JSON-RPC error codes for client
    // back-off logic. Code -32000 stays the generic "everything else"
    // bucket for backwards compatibility.
    enum RpcErrorCode {
        Generic,        // -32000
        Unavailable,    // -32001  (view-runtime semaphore / memory-floor)
        ResourceExhausted, // -32002  (per-view memory cap)
    }

    let outcome: Result<String, (RpcErrorCode, String)> = tokio::select! {
        biased;
        _ = tokio::time::sleep(timeout) => {
            cancel.cancel();
            warn!(
                "RPC method {} exceeded {}s timeout — cancelled",
                method,
                timeout.as_secs()
            );
            Err((RpcErrorCode::Generic, format!("Request timed out after {}s", timeout.as_secs())))
        }
        joined = work => match joined {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => {
                let code = match &e {
                    metashrew_sync::error::SyncError::Unavailable(_) => RpcErrorCode::Unavailable,
                    metashrew_sync::error::SyncError::ResourceExhausted(_) => RpcErrorCode::ResourceExhausted,
                    _ => RpcErrorCode::Generic,
                };
                Err((code, e.to_string()))
            }
            Err(join_err) => Err((RpcErrorCode::Generic, format!("Worker task panicked: {}", join_err))),
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
        Err((code, msg)) => {
            let json_code = match code {
                RpcErrorCode::Generic => -32000,
                RpcErrorCode::Unavailable => -32001,
                RpcErrorCode::ResourceExhausted => -32002,
            };
            error!("RPC error for method {}: code={} {}", method, json_code, msg);
            serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": json_code,
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
        // v9.0.5-rc.6: default-on startup-heal of divergent on-disk
        // pointers. Operators can pass `--no-startup-heal` to disable.
        enable_startup_heal: !args.no_startup_heal,
    };
    if args.no_startup_heal {
        warn!(
            "Startup-heal of divergent on-disk pointers is DISABLED (--no-startup-heal). \
             Indexer will trust __INTERNAL/height as-is and refuse to advance on divergence."
        );
    } else {
        info!(
            "Startup-heal of divergent on-disk pointers is ENABLED (default). \
             init() will reconcile __INTERNAL/height, /__INTERNAL/tip-height, and the highest \
             stored block-hash record before sync advances."
        );
    }

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
    // v9.0.5-rc.2 view-runtime isolation: build the per-process ViewLimiter
    // from CLI flags. The same config is also handed to the runtime adapter
    // below (via run_prod) so that StoreLimits get applied to each view
    // store.
    let view_limits_cfg = metashrew_runtime::ViewLimitsConfig {
        view_concurrency: args.view_concurrency,
        view_memory_bytes: args.view_memory_mb.saturating_mul(1024 * 1024),
        view_memory_floor_bytes: args.view_memory_floor_mb.saturating_mul(1024 * 1024),
        memory_floor_refresh: Duration::from_millis(args.view_memory_floor_refresh_ms),
        acquire_timeout: metashrew_runtime::DEFAULT_ACQUIRE_TIMEOUT,
    };
    info!(
        "View-runtime isolation: concurrency={} memory_cap={}MB floor={}MB refresh={}ms",
        view_limits_cfg.view_concurrency,
        view_limits_cfg.view_memory_bytes / (1024 * 1024),
        view_limits_cfg.view_memory_floor_bytes / (1024 * 1024),
        view_limits_cfg.memory_floor_refresh.as_millis(),
    );
    let view_limiter = Arc::new(metashrew_runtime::ViewLimiter::new(view_limits_cfg));

    let app_state = web::Data::new(AppState {
        sync_engine: sync_engine_arc.clone(),
        enable_preview: args.enable_preview,
        view_limiter: Some(view_limiter.clone()),
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

    // v9.0.5-rc.6 fetcher-dedup: the fetcher tracks its OWN high-water
    // mark of what it has already sent to the prefetch channel,
    // SEPARATE from `engine.current_height()` (which is the processor's
    // committed tip — lags behind the fetcher's enqueued tip). Without
    // this, when the processor is slow the fetcher re-reads the
    // unchanged processor tip on every loop iteration and re-enqueues
    // the same range, which the processor then rejects with
    // "out-of-order commit rejected". See `compute_next_fetch_range`
    // for the full reasoning. Encoded as `Arc<AtomicI64>` so the
    // reorg-handler (in the indexer task) can reset it down to
    // LAST_SENT_UNSET when a rollback fires, without needing a watch
    // channel or other notification plumbing.
    let last_sent_height = Arc::new(AtomicI64::new(LAST_SENT_UNSET));

    let fetcher_handle = tokio::spawn({
        let sync_engine_clone = sync_engine_arc.clone();
        let block_sender_clone = block_sender.clone();
        let exit_at = args.exit_at;
        let last_sent_height = last_sent_height.clone();

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

                let processor_tip = engine.current_height();
                let remote_tip = match engine.node().get_tip_height().await {
                    Ok(tip) => tip,
                    Err(e) => {
                        error!("Failed to get tip height: {}", e);
                        drop(engine);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                };
                // v10 sync-mode: this fetcher loop polls bitcoind directly
                // via engine.node().get_tip_height() — bypassing the
                // SnapshotMetashrewSync wrappers that already set the
                // storage adapter's bitcoind_tip. Inform the storage layer
                // directly so commit_atomic's WAL-off gate sees the real
                // gap. Without this, the fetcher races ahead at startup
                // and the storage adapter never learns of the bitcoind
                // tip during the deep-catch-up window.
                engine.storage().read().await.set_bitcoind_tip(remote_tip).await;

                // v9.0.5-rc.6 fetcher-dedup: compute the next range from
                // BOTH the processor's tip AND the fetcher's own
                // last-sent watermark.  If the processor is behind the
                // fetcher's already-enqueued tip (the channel-buffered
                // case), use last_sent + 1 to advance.  If the processor
                // is AHEAD of last_sent (a rollback reset us, or this
                // is the first iteration), use processor_tip.
                let last_sent_snapshot = last_sent_height.load(Ordering::SeqCst);

                // Check for reorgs when close to tip — fall back to single-block
                // fetch (which goes through the engine's reorg-safe path and
                // updates its own internal cursor; we also bump last_sent_height
                // on the way out so the batch path stays consistent).
                let near_tip_fetch_start = if last_sent_snapshot < 0 {
                    processor_tip
                } else {
                    std::cmp::max((last_sent_snapshot as u32).saturating_add(1), processor_tip)
                };
                if remote_tip.saturating_sub(near_tip_fetch_start) <= engine.config.reorg_check_threshold {
                    // v9.0.5-rc.7 near-tip dedup guard.
                    //
                    // rc.6 added `last_sent_height` and a "bump on
                    // send-Ok" on this path, but did NOT guard the
                    // send itself.  `engine.get_next_block_data()`
                    // returns `engine.current_height()` — the
                    // PROCESSOR's atomic — which lags behind
                    // `last_sent_height` whenever the processor is
                    // slow.  When the processor takes >1s on block N
                    // and we re-enter this near-tip branch, the
                    // engine returns N again, we send N a second
                    // time, the processor commits N, then the second
                    // copy hits the rc.5 strict-in-order check in
                    // `commit_atomic` and triggers the rc.3 infinite
                    // retry.  Production observed exactly this on
                    // v9.0.5-rc.6 mainnet pods (block 949728 was
                    // fetched twice in the same second, processed
                    // once at 01:16:47, then "processed" again at
                    // 01:20:58 with "out-of-order commit rejected:
                    // attempted 949728, current tip 949733").
                    //
                    // Fix: compute the minimum acceptable height
                    // BEFORE calling `get_next_block_data`, then drop
                    // anything strictly below it.  Defensive against
                    // `get_next_block_data` itself returning a lower
                    // height (which it can do if `handle_reorg` lowers
                    // `current_height` between iterations).
                    let min_send_height = near_tip_min_send_height(last_sent_snapshot);
                    match engine.get_next_block_data().await {
                        Ok(Some((height, block_data, block_hash))) => {
                            drop(engine);
                            if near_tip_should_drop(last_sent_snapshot, height) {
                                // Duplicate: the processor's atomic
                                // has not yet caught up to what the
                                // fetcher has already enqueued. Sleep
                                // briefly, let the processor advance,
                                // and retry on the next loop.
                                debug!(
                                    "FETCHER: dropping near-tip duplicate height={} (last_sent={} min_send={})",
                                    height, last_sent_snapshot, min_send_height,
                                );
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                continue;
                            }
                            if block_sender_clone.send(BlockData { height, block_data, block_hash: block_hash.clone() }).await.is_err() {
                                break;
                            }
                            // Bump fetcher watermark only AFTER a
                            // successful send.  fetch_max so a
                            // concurrent rollback-reset can't be
                            // clobbered by a stale send that happened
                            // to win the race.
                            last_sent_height.fetch_max(height as i64, Ordering::SeqCst);
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

                // Batch prefetch: fetch up to prefetch_size blocks concurrently.
                // Pure-function range computation tested in unit tests below.
                let (fetch_start, fetch_end) = match compute_next_fetch_range(
                    processor_tip,
                    last_sent_snapshot,
                    prefetch_size,
                    remote_tip,
                ) {
                    Some(range) => range,
                    None => {
                        drop(engine);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                };

                let batch_size = (fetch_end - fetch_start) as usize;
                let node = engine.node().clone();
                drop(engine); // Release engine lock before concurrent fetches

                debug!(
                    "FETCHER: Prefetching blocks {}..{} ({} blocks) [processor_tip={}, last_sent={}]",
                    fetch_start, fetch_end - 1, batch_size, processor_tip, last_sent_snapshot,
                );

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
                            let h = block.height;
                            debug!("FETCHER: Fetched block {} ({} bytes)", h, block.block_data.len());
                            if block_sender_clone.send(block).await.is_err() {
                                abort_remaining = true;
                            } else {
                                // v9.0.5-rc.6: bump watermark only AFTER
                                // a successful send.  fetch_max so a
                                // concurrent rollback-reset can't be
                                // clobbered by a stale send that
                                // happened to win the race.
                                last_sent_height.fetch_max(h as i64, Ordering::SeqCst);
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

    // ── operator control socket: on-demand rollback ────────────────────────
    // Channel from the UNIX control socket to the indexer task. Each request is
    // (target_height, oneshot reply). The rollback itself runs in the indexer
    // task's select loop below so it happens at a block boundary and reuses the
    // same fetcher-watermark reset the reorg path uses. `rollback_tx` is kept
    // alive in this scope (even when no socket is configured) so `rollback_rx`
    // pends instead of returning a closed-channel None that would busy-loop the
    // select.
    let (rollback_tx, mut rollback_rx) =
        mpsc::channel::<(u32, tokio::sync::oneshot::Sender<Result<u32, String>>)>(4);
    if let Some(sock_path) = args.control_socket.clone() {
        let rollback_tx = rollback_tx.clone();
        let engine_for_status = sync_engine_arc.clone();
        tokio::spawn(async move {
            let _ = std::fs::remove_file(&sock_path);
            let listener = match UnixListener::bind(&sock_path) {
                Ok(l) => l,
                Err(e) => {
                    error!("control-socket: failed to bind {}: {}", sock_path.display(), e);
                    return;
                }
            };
            info!("control-socket: listening on {}", sock_path.display());
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(e) => {
                        error!("control-socket: accept error: {}", e);
                        continue;
                    }
                };
                let rollback_tx = rollback_tx.clone();
                let engine_for_status = engine_for_status.clone();
                tokio::spawn(async move {
                    let (rd, mut wr) = stream.into_split();
                    let mut lines = BufReader::new(rd).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        let reply: String = match serde_json::from_str::<serde_json::Value>(line) {
                            Ok(v) => {
                                let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
                                match cmd {
                                    "ping" => "{\"ok\":true}".to_string(),
                                    "status" => {
                                        let h = engine_for_status.read().await.current_height();
                                        format!("{{\"ok\":true,\"height\":{}}}", h)
                                    }
                                    "rollback" => match v.get("height").and_then(|h| h.as_u64()) {
                                        Some(h) => {
                                            let (tx, rx) = tokio::sync::oneshot::channel();
                                            if rollback_tx.send((h as u32, tx)).await.is_err() {
                                                "{\"error\":\"indexer task unavailable\"}".to_string()
                                            } else {
                                                match rx.await {
                                                    Ok(Ok(rb)) => {
                                                        format!("{{\"ok\":true,\"rolled_back_to\":{}}}", rb)
                                                    }
                                                    Ok(Err(e)) => {
                                                        format!("{{\"error\":{}}}", serde_json::Value::String(e))
                                                    }
                                                    Err(_) => {
                                                        "{\"error\":\"rollback reply dropped\"}".to_string()
                                                    }
                                                }
                                            }
                                        }
                                        None => "{\"error\":\"missing or invalid height\"}".to_string(),
                                    },
                                    other => format!(
                                        "{{\"error\":{}}}",
                                        serde_json::Value::String(format!("unknown cmd: {}", other))
                                    ),
                                }
                            }
                            Err(e) => format!(
                                "{{\"error\":{}}}",
                                serde_json::Value::String(format!("bad json: {}", e))
                            ),
                        };
                        if wr.write_all(format!("{}\n", reply).as_bytes()).await.is_err() {
                            break;
                        }
                        let _ = wr.flush().await;
                    }
                });
            }
        });
    }

    let indexer_handle = tokio::spawn({
        let sync_engine_clone = sync_engine_arc.clone();
        let last_sent_height_idx = last_sent_height.clone();
        async move {
        info!("Starting block indexing process...");
        let mut block_count = 0u64;
        let start_time = Instant::now();

        loop {
        tokio::select! {
            // Bias to draining block results first; the control socket is low-rate.
            biased;
            maybe_result = result_receiver.recv() => {
            let Some(result) = maybe_result else { break; };
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
                                // v9.0.5-rc.6: rollback dropped the
                                // processor's tip DOWN. The fetcher's
                                // own last-sent watermark could be
                                // ahead of the rollback point; reset
                                // it to UNSET so the next fetch loop
                                // picks up processor_tip as the new
                                // starting height. We use store, not
                                // fetch_min, to ensure full reset —
                                // we never want to keep a stale
                                // last_sent past a rollback.
                                last_sent_height_idx.store(LAST_SENT_UNSET, Ordering::SeqCst);
                                warn!(
                                    "FETCHER: last_sent_height reset to UNSET after rollback to {} — fetcher will resume from processor_tip",
                                    rollback_height
                                );
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
            } // end block-result arm
            Some((target, reply)) = rollback_rx.recv() => {
                warn!("control-socket: manual rollback to height {} requested", target);
                let engine = sync_engine_clone.read().await;
                let res: Result<(), String> = async {
                    engine
                        .storage()
                        .write()
                        .await
                        .rollback_to_height(target)
                        .await
                        .map_err(|e| format!("rollback_to_height: {}", e))?;
                    engine
                        .runtime()
                        .refresh_memory()
                        .await
                        .map_err(|e| format!("refresh_memory: {}", e))?;
                    Ok(())
                }
                .await;
                drop(engine);
                match res {
                    Ok(()) => {
                        // Mirror the reorg path (see the Error arm above): reset the
                        // fetcher's last-sent watermark so it resumes fetching from
                        // the now-lowered processor tip.
                        last_sent_height_idx.store(LAST_SENT_UNSET, Ordering::SeqCst);
                        warn!(
                            "control-socket: rolled back to {} — fetcher watermark reset; resuming from processor tip",
                            target
                        );
                        let _ = reply.send(Ok(target));
                    }
                    Err(e) => {
                        error!("control-socket: rollback to {} failed: {}", target, e);
                        let _ = reply.send(Err(e));
                    }
                }
            }
        } // end tokio::select!
        } // end loop
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

    // v9.0.5-rc.2: per-view memory cap config — Arc'd once and shared with
    // both the runtime adapter (so each view store gets the StoreLimits)
    // and the JSON-RPC handler (so the semaphore + memory-floor gate
    // matches).
    let view_limits_cfg = Arc::new(metashrew_runtime::ViewLimitsConfig {
        view_concurrency: args.view_concurrency,
        view_memory_bytes: args.view_memory_mb.saturating_mul(1024 * 1024),
        view_memory_floor_bytes: args.view_memory_floor_mb.saturating_mul(1024 * 1024),
        memory_floor_refresh: Duration::from_millis(args.view_memory_floor_refresh_ms),
        acquire_timeout: metashrew_runtime::DEFAULT_ACQUIRE_TIMEOUT,
    });

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
        // Engine construction now lives inside MetashrewRuntime::load (see
        // metashrew_runtime::indexer_config for the deterministic-flags
        // rationale). Previously this site built a bare-defaults engine
        // and passed it in, with the consequence that ONLY the view-side
        // async engine got the deterministic flags — the indexer engine
        // that actually wrote state was missing memory_reservation,
        // NaN canonicalization, SIMD determinism, etc. Leading
        // hypothesis for the g vs h 907-DIESEL drift at h=950299.
        let runtime = MetashrewRuntime::load(args.indexer.clone(), adapter).await?;
        let storage_adapter = match runtime.context.read().unwrap().db {
            ForkAdapter::Modern(ref modern_adapter) => RocksDBStorageAdapter::with_cache(
                modern_adapter.db.clone(),
                modern_adapter.length_cache.clone(),
            ),
            ForkAdapter::Legacy(ref legacy_adapter) => {
                // LegacyRocksDBRuntimeAdapter doesn't expose a length cache —
                // fork-mode is rare in production and the cache only helps
                // forward sync, so a fresh-empty cache here is fine.
                RocksDBStorageAdapter::new(legacy_adapter.db.clone())
            }
        };
        let runtime_adapter =
            MetashrewRuntimeAdapter::new(Arc::new(runtime))
                .with_view_limits(view_limits_cfg.clone());
        run_generic(args, runtime_adapter, storage_adapter).await
    } else {
        let db_path = args.db_path.to_string_lossy().to_string();
        let adapter = if args.sync_mode {
            info!(
                "v10 sync-mode: opening RocksDB with bloom-off + L0/L1/L2 uncompressed (restart without --sync-mode at tip)"
            );
            let opts = rockshrew_runtime::optimized_config::create_sync_options();
            RocksDBRuntimeAdapter::open(db_path, opts)?
        } else {
            RocksDBRuntimeAdapter::open_optimized(db_path)?
        };
        // See the previous call site for the rationale on letting
        // MetashrewRuntime::load build the engine internally.
        let runtime = MetashrewRuntime::load(args.indexer.clone(), adapter.clone()).await?;
        // Share the length cache with the runtime adapter so block-apply
        // (which goes through `RocksDBRuntimeAdapter::multi_get_immutable` +
        // chain_entries) and post-commit cache population (this adapter)
        // see the same warmup.
        let storage_adapter =
            RocksDBStorageAdapter::with_cache(adapter.db.clone(), adapter.length_cache.clone());
        let runtime_adapter =
            MetashrewRuntimeAdapter::new(Arc::new(runtime))
                .with_view_limits(view_limits_cfg.clone());
        run_generic(args, runtime_adapter, storage_adapter).await
    }
}

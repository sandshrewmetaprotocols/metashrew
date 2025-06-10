//! Combined indexer and view layer for Metashrew
//!
//! This binary combines the indexer and view layer into a single process.

mod improvements;
mod ssh_tunnel;
mod fast_sync;

use clap::Parser;
use log::{info, warn, error};
use rockshrew_runtime::{RocksDBRuntimeAdapter, MerkleizedRocksDBRuntimeAdapter};
use runtime::KeyValueStoreLike;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;

/// Combined indexer and view layer for Metashrew
#[derive(Parser)]
#[clap(version, about, long_about = None)]
struct Opts {
    /// URL of the Bitcoin daemon RPC
    #[clap(long, default_value = "http://localhost:8332")]
    daemon_rpc_url: String,
    
    /// Authentication for the Bitcoin daemon RPC
    #[clap(long)]
    auth: Option<String>,
    
    /// Path to the WASM indexer
    #[clap(long)]
    indexer: PathBuf,
    
    /// Path to the database
    #[clap(long, default_value = ".metashrew")]
    db_path: PathBuf,
    
    /// Host to bind to
    #[clap(long, default_value = "127.0.0.1")]
    host: String,
    
    /// Port to listen on
    #[clap(long, default_value = "8080")]
    port: u16,
    
    /// SSH tunnel configuration
    #[clap(long)]
    ssh_tunnel: Option<String>,
    
    /// URL of the repository to use for fast sync (default: https://repo.sandshrew.io)
    #[clap(long, default_value = "https://repo.sandshrew.io")]
    repo: Option<String>,
    
    /// Disable fast sync and perform full sync instead
    #[clap(long)]
    no_fast_sync: bool,
    
    /// Disable merkleized database (use regular RocksDB adapter)
    #[clap(long)]
    no_merkle: bool,
    
    /// Automatically create snapshots at specified interval (in blocks)
    #[clap(long)]
    snapshot_interval: Option<u32>,
    
    /// Port for the repository server (if snapshot_interval is specified)
    #[clap(long, default_value = "8090")]
    repo_port: u16,
    
    /// Host for the repository server (if snapshot_interval is specified)
    #[clap(long, default_value = "0.0.0.0")]
    repo_host: String,
    
    /// Directory to store snapshots (if snapshot_interval is specified)
    #[clap(long, default_value = "./snapshots")]
    snapshot_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    
    // Parse command line arguments
    let opts = Opts::parse();
    
    // Create SSH tunnel if requested
    if let Some(ssh_config) = &opts.ssh_tunnel {
        ssh_tunnel::create_tunnel(ssh_config)?;
    }
    
    // Get metaprotocol ID from indexer path
    let metaprotocol_id = opts.indexer
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    
    // Create database path if it doesn't exist
    if !opts.db_path.exists() {
        std::fs::create_dir_all(&opts.db_path)?;
    }
    
    // Initialize database
    if opts.no_merkle {
        // Use regular RocksDB adapter
        let mut db = RocksDBRuntimeAdapter::new(opts.db_path.to_str().unwrap())?;
        
        // Run the indexer and view layer
        run_with_regular_db(opts, db).await?;
    } else {
        // Use merkleized RocksDB adapter
        let mut db = MerkleizedRocksDBRuntimeAdapter::new(&opts.db_path)?;
        
        // Check if fast sync is enabled and a repo is specified
        if !opts.no_fast_sync && opts.repo.is_some() {
            // Check if we need to sync
            if fast_sync::needs_sync(&db, &metaprotocol_id) {
                let repo_url = opts.repo.unwrap_or_else(|| "https://repo.sandshrew.io".to_string());
                
                // Attempt fast sync
                match fast_sync::fast_sync(&mut db, &repo_url, &metaprotocol_id).await {
                    Ok(block_height) => {
                        info!("Fast sync completed successfully to block {}", block_height);
                    },
                    Err(e) => {
                        warn!("Fast sync failed: {}, falling back to full sync", e);
                    }
                }
            } else {
                info!("Database already contains data, skipping fast sync");
            }
        }
        
        // Start snapshot server if snapshot_interval is specified
        if let Some(interval) = opts.snapshot_interval {
            info!("Starting snapshot server with interval of {} blocks", interval);
            
            // Get metaprotocol ID from indexer path
            let metaprotocol_id = opts.indexer
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            
            // Create snapshot directory if it doesn't exist
            if !opts.snapshot_dir.exists() {
                std::fs::create_dir_all(&opts.snapshot_dir)?;
            }
            
            // Start metashrew-repo process
            let repo_process = std::process::Command::new("metashrew-repo")
                .arg("serve")
                .arg("--snapshot-dir").arg(&opts.snapshot_dir)
                .arg("--port").arg(opts.repo_port.to_string())
                .arg("--host").arg(&opts.repo_host)
                .arg("--snapshot-interval").arg(interval.to_string())
                .arg("--db-path").arg(&opts.db_path)
                .arg("--metaprotocol-id").arg(&metaprotocol_id)
                .spawn();
            
            match repo_process {
                Ok(child) => {
                    info!("Started metashrew-repo process with PID {}", child.id());
                },
                Err(e) => {
                    warn!("Failed to start metashrew-repo process: {}", e);
                    warn!("Automatic snapshots will not be available");
                }
            }
        }
        
        // Run the indexer and view layer
        run_with_merkleized_db(opts, db).await?;
    }
    
    Ok(())
}

/// Run with regular RocksDB adapter
async fn run_with_regular_db(
    opts: Opts,
    db: RocksDBRuntimeAdapter,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load the indexer
    let indexer_wasm = std::fs::read(&opts.indexer)?;
    
    // Create runtime
    let mut runtime = runtime::Runtime::new(Box::new(db));
    
    // Initialize the runtime with the indexer
    runtime.init(&indexer_wasm)?;
    
    // Start the indexer
    let indexer_handle = tokio::spawn(async move {
        // Connect to Bitcoin daemon
        let rpc_client = bitcoincore_rpc::Client::new(
            &opts.daemon_rpc_url,
            bitcoincore_rpc::Auth::None,
        ).expect("Failed to connect to Bitcoin daemon");
        
        // Start indexing
        loop {
            // TODO: Implement indexing logic
            sleep(Duration::from_secs(1)).await;
        }
    });
    
    // Start the view layer
    let view_handle = tokio::spawn(async move {
        // TODO: Implement view layer logic
        loop {
            sleep(Duration::from_secs(1)).await;
        }
    });
    
    // Wait for both tasks to complete
    tokio::select! {
        _ = indexer_handle => {
            error!("Indexer task exited unexpectedly");
        }
        _ = view_handle => {
            error!("View layer task exited unexpectedly");
        }
    }
    
    Ok(())
}

/// Run with merkleized RocksDB adapter
async fn run_with_merkleized_db(
    opts: Opts,
    db: MerkleizedRocksDBRuntimeAdapter,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load the indexer
    let indexer_wasm = std::fs::read(&opts.indexer)?;
    
    // Create runtime
    let mut runtime = runtime::Runtime::new(Box::new(db));
    
    // Initialize the runtime with the indexer
    runtime.init(&indexer_wasm)?;
    
    // Start the indexer
    let indexer_handle = tokio::spawn(async move {
        // Connect to Bitcoin daemon
        let rpc_client = bitcoincore_rpc::Client::new(
            &opts.daemon_rpc_url,
            bitcoincore_rpc::Auth::None,
        ).expect("Failed to connect to Bitcoin daemon");
        
        // Start indexing
        loop {
            // TODO: Implement indexing logic
            sleep(Duration::from_secs(1)).await;
        }
    });
    
    // Start the view layer
    let view_handle = tokio::spawn(async move {
        // TODO: Implement view layer logic
        loop {
            sleep(Duration::from_secs(1)).await;
        }
    });
    
    // Wait for both tasks to complete
    tokio::select! {
        _ = indexer_handle => {
            error!("Indexer task exited unexpectedly");
        }
        _ = view_handle => {
            error!("View layer task exited unexpectedly");
        }
    }
    
    Ok(())
}

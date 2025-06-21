use actix_cors::Cors;
use actix_web::{post, web, App, HttpServer};
use anyhow::Result;
use clap::Parser;
use log::info;
use metashrew_rocksdb::RocksDBAdapter;
use metashrew_runtime::MetashrewRuntime;
use rocksdb::Options;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

// Placeholder for modules that will be copied over
// mod ssh_tunnel;
// mod snapshot;
// mod improvements;

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    daemon_rpc_url: String,
    #[arg(long, required_unless_present = "repo")]
    indexer: Option<PathBuf>,
    #[arg(long)]
    db_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::builder()
        .format_timestamp_secs()
        .init();
    
    info!("Starting Metashrew Indexer (rockshrew)");
    
    // Parse command line arguments
    let args = Args::parse();
    
    // Configure RocksDB options
    let mut opts = Options::default();
    opts.create_if_missing(true);
    
    // Create runtime with RocksDB adapter
    let _runtime = Arc::new(RwLock::new(MetashrewRuntime::load(
        args.indexer.unwrap(),
        RocksDBAdapter::open(args.db_path.to_string_lossy().to_string(), opts)?
    )?));
    
    info!("Rockshrew initialized successfully");
    
    Ok(())
}
//! Main binary for running the repository server
//!
//! This binary provides a command-line interface for running the repository server
//! and creating snapshots.

use metashrew_repo::{
    RepoConfig, Result,
    server::{RepoServer, MetaprotocolInfo},
};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use log::{info, warn, error};
use std::time::{SystemTime, UNIX_EPOCH};

/// Repository service for Metashrew fast sync
#[derive(Parser)]
#[clap(version, about, long_about = None)]
struct Cli {
    /// Subcommand to execute
    #[clap(subcommand)]
    command: Commands,
}

/// Available commands
#[derive(Subcommand)]
enum Commands {
    /// Run the repository server
    Serve {
        /// Directory to store snapshots
        #[clap(long, default_value = "./snapshots")]
        snapshot_dir: PathBuf,
        
        /// Port to listen on
        #[clap(long, default_value = "8090")]
        port: u16,
        
        /// Host address to bind to
        #[clap(long, default_value = "0.0.0.0")]
        host: String,
        
        /// Automatically create snapshots at specified interval (in blocks)
        #[clap(long)]
        snapshot_interval: Option<u32>,
        
        /// Path to the RocksDB database
        #[clap(long)]
        db_path: Option<PathBuf>,
        
        /// Path to the WASM indexer
        #[clap(long)]
        indexer: Option<PathBuf>,
        
        /// Metaprotocol ID (derived from WASM filename if not specified)
        #[clap(long)]
        metaprotocol_id: Option<String>,
        
        /// URL of the Bitcoin daemon RPC
        #[clap(long, default_value = "http://localhost:8332")]
        daemon_rpc_url: String,
        
        /// Authentication for the Bitcoin daemon RPC
        #[clap(long)]
        auth: Option<String>,
    },
    
    /// Create a snapshot from a running instance
    CreateSnapshot {
        /// Directory to store snapshots
        #[clap(long, default_value = "./snapshots")]
        snapshot_dir: PathBuf,
        
        /// Metaprotocol ID
        #[clap(long)]
        metaprotocol_id: String,
        
        /// Path to the RocksDB database
        #[clap(long)]
        db_path: PathBuf,
        
        /// Block hash (hex encoded)
        #[clap(long)]
        block_hash: String,
        
        /// Block height
        #[clap(long)]
        block_height: u32,
        
        /// Chunk size for large snapshots
        #[clap(long, default_value = "10000")]
        chunk_size: usize,
    },
    
    /// Register a metaprotocol
    RegisterMetaprotocol {
        /// Directory to store snapshots
        #[clap(long, default_value = "./snapshots")]
        snapshot_dir: PathBuf,
        
        /// Metaprotocol ID
        #[clap(long)]
        id: String,
        
        /// Human-readable name
        #[clap(long)]
        name: String,
        
        /// Description
        #[clap(long)]
        description: String,
        
        /// Latest block height
        #[clap(long)]
        latest_block: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    
    // Parse command line arguments
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Serve {
            snapshot_dir,
            port,
            host,
            snapshot_interval,
            db_path,
            indexer,
            metaprotocol_id,
            daemon_rpc_url,
            auth,
        } => {
            // Create server configuration
            let config = RepoConfig {
                snapshot_dir,
                port,
                host,
            };
            
            // Create and run server
            let server = RepoServer::new(config);
            
            // If automatic snapshots are enabled, start the snapshot task
            if let (Some(interval), Some(db_path)) = (snapshot_interval, db_path) {
                // Get metaprotocol ID from indexer path or use provided ID
                let metaprotocol_id = if let Some(ref indexer_path) = indexer {
                    metaprotocol_id.unwrap_or_else(|| {
                        indexer_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("default")
                            .to_string()
                    })
                } else {
                    metaprotocol_id.unwrap_or_else(|| "default".to_string())
                };
                
                // If indexer is provided, start syncing process
                if let Some(indexer_path) = indexer {
                    info!("Starting indexer from {}", indexer_path.display());
                    
                    // Start syncing process in a separate thread
                    let db_path_clone = db_path.clone();
                    let daemon_rpc_url_clone = daemon_rpc_url.clone();
                    let auth_clone = auth.clone();
                    
                    tokio::spawn(async move {
                        // Load the indexer
                        match std::fs::read(&indexer_path) {
                            Ok(indexer_wasm) => {
                                info!("Loaded indexer WASM ({} bytes)", indexer_wasm.len());
                                
                                // Open the database
                                match open_merkleized_database_for_sync(&db_path_clone) {
                                    Ok(mut db) => {
                                        info!("Opened database at {}", db_path_clone.display());
                                        
                                        // Create runtime
                                        let mut runtime = runtime::Runtime::new(Box::new(db));
                                        
                                        // Initialize the runtime with the indexer
                                        match runtime.init(&indexer_wasm) {
                                            Ok(_) => {
                                                info!("Initialized runtime with indexer");
                                                
                                                // Connect to Bitcoin daemon
                                                let rpc_client = match auth_clone {
                                                    Some(auth) => {
                                                        let parts: Vec<&str> = auth.splitn(2, ':').collect();
                                                        if parts.len() == 2 {
                                                            bitcoincore_rpc::Client::new(
                                                                &daemon_rpc_url_clone,
                                                                bitcoincore_rpc::Auth::UserPass(
                                                                    parts[0].to_string(),
                                                                    parts[1].to_string(),
                                                                ),
                                                            )
                                                        } else {
                                                            bitcoincore_rpc::Client::new(
                                                                &daemon_rpc_url_clone,
                                                                bitcoincore_rpc::Auth::None,
                                                            )
                                                        }
                                                    },
                                                    None => {
                                                        bitcoincore_rpc::Client::new(
                                                            &daemon_rpc_url_clone,
                                                            bitcoincore_rpc::Auth::None,
                                                        )
                                                    }
                                                };
                                                
                                                match rpc_client {
                                                    Ok(rpc) => {
                                                        info!("Connected to Bitcoin daemon at {}", daemon_rpc_url_clone);
                                                        
                                                        // Start indexing
                                                        info!("Starting indexing process");
                                                        
                                                        // TODO: Implement actual indexing logic
                                                        // This is a placeholder for the actual indexing logic
                                                        loop {
                                                            // Sleep to avoid busy loop
                                                            std::thread::sleep(std::time::Duration::from_secs(1));
                                                        }
                                                    },
                                                    Err(e) => {
                                                        error!("Failed to connect to Bitcoin daemon: {}", e);
                                                    }
                                                }
                                            },
                                            Err(e) => {
                                                error!("Failed to initialize runtime: {}", e);
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        error!("Failed to open database: {}", e);
                                    }
                                }
                            },
                            Err(e) => {
                                error!("Failed to load indexer: {}", e);
                            }
                        }
                    });
                }
                
                // Start automatic snapshot task
                let server_clone = server.clone();
                tokio::spawn(async move {
                    info!("Starting automatic snapshot task with interval of {} blocks", interval);
                    
                    // Open the database
                    let db = match open_merkleized_database(&db_path) {
                        Ok(db) => db,
                        Err(e) => {
                            error!("Failed to open database: {}", e);
                            return;
                        }
                    };
                    
                    let mut last_snapshot_height = 0;
                    
                    loop {
                        // Sleep for a while
                        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                        
                        // Get current block height
                        let current_height = get_current_block_height(&db);
                        
                        // Check if we need to create a snapshot
                        if current_height >= last_snapshot_height + interval {
                            info!("Creating snapshot at block height {}", current_height);
                            
                            // Get current block hash
                            let block_hash = get_current_block_hash(&db);
                            
                            match server_clone.create_snapshot(
                                &metaprotocol_id,
                                &db,
                                block_hash,
                                current_height,
                            ) {
                                Ok(snapshot) => {
                                    // Save snapshot
                                    match server_clone.save_snapshot(&snapshot, 10000) {
                                        Ok(path) => {
                                            info!("Snapshot saved to {}", path.display());
                                            
                                            // Register metaprotocol if it doesn't exist
                                            let info = MetaprotocolInfo {
                                                id: metaprotocol_id.clone(),
                                                name: metaprotocol_id.clone(),
                                                description: format!("Metaprotocol {}", metaprotocol_id),
                                                latest_block: current_height,
                                                latest_snapshot: SystemTime::now()
                                                    .duration_since(UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_secs(),
                                            };
                                            
                                            if let Err(e) = server_clone.register_metaprotocol(info) {
                                                warn!("Failed to register metaprotocol: {}", e);
                                            }
                                        },
                                        Err(e) => {
                                            error!("Failed to save snapshot: {}", e);
                                        }
                                    }
                                },
                                Err(e) => {
                                    error!("Failed to create snapshot: {}", e);
                                }
                            }
                            
                            // Update last snapshot height
                            last_snapshot_height = current_height;
                        }
                    }
                });
            }
            
            // Run the server
            server.run().await?;
        },
        
        Commands::CreateSnapshot {
            snapshot_dir,
            metaprotocol_id,
            db_path,
            block_hash,
            block_height,
            chunk_size,
        } => {
            // Create server configuration
            let config = RepoConfig {
                snapshot_dir,
                port: 0, // Not used for snapshot creation
                host: "".to_string(), // Not used for snapshot creation
            };
            
            // Create server
            let server = RepoServer::new(config);
            
            // Open the database
            let db = open_merkleized_database(&db_path)?;
            
            // Parse block hash
            let block_hash_bytes = hex::decode(&block_hash)
                .map_err(|e| metashrew_repo::Error::InvalidData(format!("Invalid block hash: {}", e)))?;
            
            info!("Creating snapshot for metaprotocol {}", metaprotocol_id);
            
            // Create snapshot
            let snapshot = server.create_snapshot(
                &metaprotocol_id,
                &db,
                block_hash_bytes,
                block_height,
            )?;
            
            // Save snapshot
            let path = server.save_snapshot(&snapshot, chunk_size)?;
            
            info!("Snapshot saved to {}", path.display());
            
            // Register metaprotocol if it doesn't exist
            let info = MetaprotocolInfo {
                id: metaprotocol_id.clone(),
                name: metaprotocol_id.clone(), // Use ID as name
                description: format!("Metaprotocol {}", metaprotocol_id),
                latest_block: block_height,
                latest_snapshot: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };
            
            if let Err(e) = server.register_metaprotocol(info) {
                warn!("Failed to register metaprotocol: {}", e);
            }
        },
        
        Commands::RegisterMetaprotocol {
            snapshot_dir,
            id,
            name,
            description,
            latest_block,
        } => {
            // Create server configuration
            let config = RepoConfig {
                snapshot_dir,
                port: 0, // Not used for registration
                host: "".to_string(), // Not used for registration
            };
            
            // Create server
            let server = RepoServer::new(config);
            
            // Create metaprotocol info
            let info = MetaprotocolInfo {
                id: id.clone(),
                name,
                description,
                latest_block,
                latest_snapshot: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };
            
            // Register metaprotocol
            server.register_metaprotocol(info)?;
            
            info!("Metaprotocol {} registered", id);
        },
    }
    
    Ok(())
}

/// Open a merkleized database
fn open_merkleized_database(path: &PathBuf) -> Result<impl metashrew_repo::server::MerkleizedDatabaseReader> {
    // This is a placeholder. In a real implementation, we would open the RocksDB database
    // and return a MerkleizedRocksDBRuntimeAdapter.
    
    // For now, we'll return a mock implementation
    struct MockDatabase;
    
    impl metashrew_repo::server::MerkleizedDatabaseReader for MockDatabase {
        fn state_root(&self) -> metashrew_repo::merkle::Hash {
            [0; 64] // Mock state root
        }
        
        fn prefix_iterator(&self, _prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
            // Mock iterator with some sample data
            let data = vec![
                (vec![1, 2, 3], vec![4, 5, 6]),
                (vec![7, 8, 9], vec![10, 11, 12]),
            ];
            Box::new(data.into_iter())
        }
    }
    
    Ok(MockDatabase)
}

/// Get current block height from database
fn get_current_block_height(_db: &impl metashrew_repo::server::MerkleizedDatabaseReader) -> u32 {
    // This is a placeholder. In a real implementation, we would get the current block height
    // from the database.
    
    // For now, we'll return a mock value
    100
}

/// Get current block hash from database
fn get_current_block_hash(_db: &impl metashrew_repo::server::MerkleizedDatabaseReader) -> Vec<u8> {
    // This is a placeholder. In a real implementation, we would get the current block hash
    // from the database.
    
    // For now, we'll return a mock value
    vec![1, 2, 3, 4]
}
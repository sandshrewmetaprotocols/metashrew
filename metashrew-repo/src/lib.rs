//! Repository service for Metashrew fast sync
//! 
//! This crate provides functionality for creating, hosting, and downloading state snapshots
//! to enable fast synchronization of Metashrew nodes.

pub mod client;
pub mod server;
pub mod snapshot;
pub mod merkle;

use std::path::PathBuf;
use thiserror::Error;

/// Error type for metashrew-repo operations
#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Database error: {0}")]
    Database(#[from] rocksdb::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    
    #[error("HTTP error: {0}")]
    Http(#[from] actix_web::Error),
    
    #[error("Request error: {0}")]
    Request(String),
    
    #[error("Verification failed: {0}")]
    Verification(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// Result type for metashrew-repo operations
pub type Result<T> = std::result::Result<T, Error>;

/// Configuration for the repository server
#[derive(Clone, Debug)]
pub struct RepoConfig {
    /// Directory to store snapshots
    pub snapshot_dir: PathBuf,
    /// Port to listen on
    pub port: u16,
    /// Host address to bind to
    pub host: String,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            snapshot_dir: PathBuf::from("./snapshots"),
            port: 8090,
            host: "0.0.0.0".to_string(),
        }
    }
}

/// Configuration for the repository client
#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// URL of the repository server
    pub repo_url: String,
    /// Timeout for requests in seconds
    pub timeout_secs: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            repo_url: "https://repo.sandshrew.io".to_string(),
            timeout_secs: 60,
        }
    }
}
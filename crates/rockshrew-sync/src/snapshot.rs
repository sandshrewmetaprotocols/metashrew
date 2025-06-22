//! Snapshot and repository mode traits and implementations
//! 
//! This module provides the infrastructure for snapshot-based synchronization
//! where one instance can create snapshots at regular intervals and another
//! instance can sync from those snapshots (repo mode).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{SyncResult, SyncError};

/// Snapshot metadata containing information about a checkpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Height at which this snapshot was taken
    pub height: u32,
    /// Block hash at this height
    pub block_hash: Vec<u8>,
    /// State root at this height
    pub state_root: Vec<u8>,
    /// Timestamp when snapshot was created
    pub timestamp: u64,
    /// Size of the snapshot data in bytes
    pub size_bytes: u64,
    /// Checksum of the snapshot data
    pub checksum: String,
    /// WASM module hash used for this snapshot
    pub wasm_hash: String,
}

/// Snapshot data containing the actual state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotData {
    /// Metadata about this snapshot
    pub metadata: SnapshotMetadata,
    /// Raw state data (compressed)
    pub state_data: Vec<u8>,
    /// Block hashes for recent blocks (for reorg detection)
    pub recent_block_hashes: HashMap<u32, Vec<u8>>,
}

/// Configuration for snapshot creation
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Interval between snapshots (in blocks)
    pub snapshot_interval: u32,
    /// Maximum number of snapshots to keep
    pub max_snapshots: usize,
    /// Compression level (0-9)
    pub compression_level: u32,
    /// Number of recent blocks to include for reorg detection
    pub reorg_buffer_size: u32,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            snapshot_interval: 1000,
            max_snapshots: 10,
            compression_level: 6,
            reorg_buffer_size: 100,
        }
    }
}

/// Configuration for repository mode
#[derive(Debug, Clone)]
pub struct RepoConfig {
    /// Base URL or path for the snapshot repository
    pub repo_url: String,
    /// Interval to check for new snapshots (in seconds)
    pub check_interval: u64,
    /// Maximum age of snapshots to consider (in seconds)
    pub max_snapshot_age: u64,
    /// Whether to continue syncing after catching up to snapshots
    pub continue_sync: bool,
    /// Minimum blocks behind before using snapshots
    pub min_blocks_behind: u32,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            repo_url: "http://localhost:8080/snapshots".to_string(),
            check_interval: 300, // 5 minutes
            max_snapshot_age: 86400, // 24 hours
            continue_sync: true,
            min_blocks_behind: 100,
        }
    }
}

/// Trait for creating and managing snapshots
#[async_trait]
pub trait SnapshotProvider: Send + Sync {
    /// Create a snapshot at the current height
    async fn create_snapshot(&mut self, height: u32) -> SyncResult<SnapshotMetadata>;
    
    /// Get available snapshots
    async fn list_snapshots(&self) -> SyncResult<Vec<SnapshotMetadata>>;
    
    /// Get a specific snapshot by height
    async fn get_snapshot(&self, height: u32) -> SyncResult<Option<SnapshotData>>;
    
    /// Get the latest snapshot
    async fn get_latest_snapshot(&self) -> SyncResult<Option<SnapshotData>>;
    
    /// Delete old snapshots beyond the configured limit
    async fn cleanup_snapshots(&mut self) -> SyncResult<usize>;
    
    /// Check if a snapshot should be created at this height
    fn should_create_snapshot(&self, height: u32) -> bool;
}

/// Trait for consuming snapshots in repository mode
#[async_trait]
pub trait SnapshotConsumer: Send + Sync {
    /// Check for available snapshots from the repository
    async fn check_available_snapshots(&self) -> SyncResult<Vec<SnapshotMetadata>>;
    
    /// Download and apply a snapshot
    async fn apply_snapshot(&mut self, metadata: &SnapshotMetadata) -> SyncResult<()>;
    
    /// Get the best snapshot to use for catching up
    async fn get_best_snapshot(&self, current_height: u32, tip_height: u32) -> SyncResult<Option<SnapshotMetadata>>;
    
    /// Verify a snapshot's integrity
    async fn verify_snapshot(&self, data: &SnapshotData) -> SyncResult<bool>;
    
    /// Check if we should use snapshots given current state
    async fn should_use_snapshots(&self, current_height: u32, tip_height: u32) -> SyncResult<bool>;
}

/// Trait for serving snapshots over HTTP or filesystem
#[async_trait]
pub trait SnapshotServer: Send + Sync {
    /// Start the snapshot server
    async fn start(&mut self) -> SyncResult<()>;
    
    /// Stop the snapshot server
    async fn stop(&mut self) -> SyncResult<()>;
    
    /// Get server status
    async fn get_status(&self) -> SyncResult<SnapshotServerStatus>;
    
    /// Register a new snapshot
    async fn register_snapshot(&mut self, metadata: SnapshotMetadata, data: Vec<u8>) -> SyncResult<()>;
    
    /// Get snapshot metadata by height
    async fn get_snapshot_metadata(&self, height: u32) -> SyncResult<Option<SnapshotMetadata>>;
    
    /// Get snapshot data by height
    async fn get_snapshot_data(&self, height: u32) -> SyncResult<Option<Vec<u8>>>;
    
    /// List all available snapshots
    async fn list_available_snapshots(&self) -> SyncResult<Vec<SnapshotMetadata>>;
}

/// Status of a snapshot server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotServerStatus {
    pub is_running: bool,
    pub total_snapshots: usize,
    pub latest_snapshot_height: Option<u32>,
    pub total_size_bytes: u64,
    pub uptime_seconds: u64,
}

/// Trait for HTTP client to download snapshots
#[async_trait]
pub trait SnapshotClient: Send + Sync {
    /// Download snapshot metadata from URL
    async fn download_metadata(&self, url: &str) -> SyncResult<SnapshotMetadata>;
    
    /// Download snapshot data from URL
    async fn download_data(&self, url: &str) -> SyncResult<Vec<u8>>;
    
    /// List available snapshots from repository
    async fn list_remote_snapshots(&self, base_url: &str) -> SyncResult<Vec<SnapshotMetadata>>;
    
    /// Check if repository is available
    async fn check_repository(&self, base_url: &str) -> SyncResult<bool>;
}

/// Combined sync mode that can operate in both snapshot and repo modes
#[derive(Debug, Clone)]
pub enum SyncMode {
    /// Normal synchronization mode
    Normal,
    /// Snapshot creation mode
    Snapshot(SnapshotConfig),
    /// Repository consumption mode
    Repo(RepoConfig),
    /// Combined mode (create snapshots and serve them)
    SnapshotServer(SnapshotConfig),
}

/// Sync engine that supports snapshot and repository modes
#[async_trait]
pub trait SnapshotSyncEngine: Send + Sync {
    /// Get current sync mode
    fn get_sync_mode(&self) -> &SyncMode;
    
    /// Switch sync mode
    async fn set_sync_mode(&mut self, mode: SyncMode) -> SyncResult<()>;
    
    /// Process a block with snapshot considerations
    async fn process_block_with_snapshots(&mut self, height: u32, block_data: &[u8]) -> SyncResult<()>;
    
    /// Check and apply snapshots if in repo mode
    async fn check_and_apply_snapshots(&mut self) -> SyncResult<bool>;
    
    /// Create snapshot if in snapshot mode
    async fn create_snapshot_if_needed(&mut self, height: u32) -> SyncResult<bool>;
    
    /// Get sync statistics including snapshot info
    async fn get_snapshot_stats(&self) -> SyncResult<SnapshotSyncStats>;
}

/// Statistics for snapshot-enabled sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSyncStats {
    pub current_height: u32,
    pub tip_height: u32,
    pub sync_mode: String,
    pub snapshots_created: u32,
    pub snapshots_applied: u32,
    pub last_snapshot_height: Option<u32>,
    pub blocks_synced_normally: u32,
    pub blocks_synced_from_snapshots: u32,
}

/// Error types specific to snapshot operations
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("Snapshot not found at height {height}")]
    SnapshotNotFound { height: u32 },
    
    #[error("Invalid snapshot data: {reason}")]
    InvalidSnapshot { reason: String },
    
    #[error("Snapshot verification failed: {reason}")]
    VerificationFailed { reason: String },
    
    #[error("Repository unavailable: {url}")]
    RepositoryUnavailable { url: String },
    
    #[error("Compression error: {message}")]
    CompressionError { message: String },
    
    #[error("Network error: {message}")]
    NetworkError { message: String },
    
    #[error("Filesystem error: {message}")]
    FilesystemError { message: String },
}

impl From<SnapshotError> for SyncError {
    fn from(err: SnapshotError) -> Self {
        SyncError::Runtime(err.to_string())
    }
}
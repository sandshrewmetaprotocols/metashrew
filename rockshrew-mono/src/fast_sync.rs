//! Fast sync support for rockshrew-mono
//!
//! This module provides functionality for fast syncing using the metashrew-repo service.

use metashrew_repo::{
    client::{RepoClient, MerkleizedDatabase, MerkleizedBatch},
    Error as RepoError,
};
use rockshrew_runtime::{
    MerkleizedRocksDBRuntimeAdapter,
    MerkleizedRocksDBBatch,
    Hash,
};
use log::{info, warn, debug, error};
use std::path::Path;

/// Check if fast sync is needed
pub fn needs_sync(db: &MerkleizedRocksDBRuntimeAdapter, metaprotocol_id: &str) -> bool {
    // Check if the database is empty or has very few entries
    let prefix = format!("{}/", metaprotocol_id).into_bytes();
    let mut count = 0;
    
    for _ in db.prefix_iterator(&prefix) {
        count += 1;
        if count > 10 {
            // Database already has entries, no need to sync
            return false;
        }
    }
    
    // Database is empty or has very few entries, sync is needed
    true
}

/// Implement MerkleizedDatabase trait for MerkleizedRocksDBRuntimeAdapter
impl MerkleizedDatabase for MerkleizedRocksDBRuntimeAdapter {
    type Error = rocksdb::Error;
    type Batch = MerkleizedRocksDBBatch;
    
    fn create_batch(&self) -> Self::Batch {
        MerkleizedRocksDBBatch::new()
    }
    
    fn write_batch(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        self.write(batch)
    }
    
    fn state_root(&self) -> Hash {
        self.state_root()
    }
}

/// Implement MerkleizedBatch trait for MerkleizedRocksDBBatch
impl metashrew_repo::client::MerkleizedBatch for MerkleizedRocksDBBatch {
    fn insert<K, V>(&mut self, key: K, value: V)
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        runtime::BatchLike::insert(self, key, value);
    }
    
    fn remove<K>(&mut self, key: K)
    where
        K: AsRef<[u8]>,
    {
        runtime::BatchLike::remove(self, key);
    }
}

/// Perform fast sync
pub async fn fast_sync(
    db: &mut MerkleizedRocksDBRuntimeAdapter,
    repo_url: &str,
    metaprotocol_id: &str,
) -> Result<u32, RepoError> {
    info!("Starting fast sync from {}", repo_url);
    
    // Create repo client
    let client = RepoClient::new(repo_url);
    
    // Perform sync
    let block_height = client.sync(db, metaprotocol_id).await?;
    
    info!("Fast sync completed successfully to block {}", block_height);
    
    Ok(block_height)
}
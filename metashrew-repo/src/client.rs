//! Client for downloading state snapshots from a repository
//!
//! This module provides functionality for downloading state snapshots from a repository
//! and importing them into a local database.

use crate::{
    ClientConfig, Error, Result,
    snapshot::{MetaprotocolSnapshot, SnapshotMetadata, SnapshotChunk, KeyValueEntry},
    merkle::SparseMerkleTree,
};
use std::path::{Path, PathBuf};
use std::fs;
use std::time::Duration;
use tokio::time::timeout;
use log::{info, warn, debug};

/// Client for downloading state snapshots
pub struct RepoClient {
    /// Configuration for the client
    config: ClientConfig,
    /// HTTP client for making requests
    http_client: reqwest::Client,
    /// Temporary directory for downloaded snapshots
    temp_dir: PathBuf,
}

impl RepoClient {
    /// Create a new repository client
    pub fn new(repo_url: impl Into<String>) -> Self {
        Self::with_config(ClientConfig {
            repo_url: repo_url.into(),
            ..Default::default()
        })
    }
    
    /// Create a new repository client with custom configuration
    pub fn with_config(config: ClientConfig) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_default();
            
        let temp_dir = std::env::temp_dir().join("metashrew-repo");
        if !temp_dir.exists() {
            fs::create_dir_all(&temp_dir).ok();
        }
        
        Self {
            config,
            http_client,
            temp_dir,
        }
    }
    
    /// Get available metaprotocols from the repository
    pub async fn get_available_metaprotocols(&self) -> Result<Vec<String>> {
        let url = format!("{}/metaprotocols", self.config.repo_url);
        let response = self.http_client.get(&url)
            .send()
            .await
            .map_err(|e| Error::Request(e.to_string()))?;
            
        if !response.status().is_success() {
            return Err(Error::Request(format!(
                "Failed to get metaprotocols: HTTP {}", 
                response.status()
            )));
        }
        
        let metaprotocols: Vec<String> = response.json()
            .await
            .map_err(|e| Error::Request(e.to_string()))?;
            
        Ok(metaprotocols)
    }
    
    /// Get metadata for a snapshot
    pub async fn get_snapshot_metadata(&self, metaprotocol_id: &str) -> Result<SnapshotMetadata> {
        let url = format!(
            "{}/snapshots/{}/metadata", 
            self.config.repo_url, 
            metaprotocol_id
        );
        
        let response = self.http_client.get(&url)
            .send()
            .await
            .map_err(|e| Error::Request(e.to_string()))?;
            
        if !response.status().is_success() {
            return Err(Error::Request(format!(
                "Failed to get snapshot metadata: HTTP {}", 
                response.status()
            )));
        }
        
        let metadata: SnapshotMetadata = response.json()
            .await
            .map_err(|e| Error::Request(e.to_string()))?;
            
        Ok(metadata)
    }
    
    /// Download a snapshot chunk
    pub async fn download_chunk(
        &self, 
        metaprotocol_id: &str, 
        chunk_index: u32
    ) -> Result<SnapshotChunk> {
        let url = format!(
            "{}/snapshots/{}/chunks/{}", 
            self.config.repo_url, 
            metaprotocol_id, 
            chunk_index
        );
        
        let response = self.http_client.get(&url)
            .send()
            .await
            .map_err(|e| Error::Request(e.to_string()))?;
            
        if !response.status().is_success() {
            return Err(Error::Request(format!(
                "Failed to download chunk {}: HTTP {}", 
                chunk_index, 
                response.status()
            )));
        }
        
        let chunk: SnapshotChunk = response.json()
            .await
            .map_err(|e| Error::Request(e.to_string()))?;
            
        Ok(chunk)
    }
    
    /// Download a complete snapshot
    pub async fn download_snapshot(&self, metaprotocol_id: &str) -> Result<MetaprotocolSnapshot> {
        info!("Downloading snapshot for metaprotocol: {}", metaprotocol_id);
        
        // Get metadata first
        let metadata = self.get_snapshot_metadata(metaprotocol_id).await?;
        info!(
            "Found snapshot for block {} with {} chunks ({} bytes total)",
            metadata.block_height,
            metadata.total_chunks,
            metadata.total_size_bytes
        );
        
        // For small snapshots, download directly
        if metadata.total_chunks == 1 {
            let url = format!(
                "{}/snapshots/{}/full", 
                self.config.repo_url, 
                metaprotocol_id
            );
            
            let response = self.http_client.get(&url)
                .send()
                .await
                .map_err(|e| Error::Request(e.to_string()))?;
                
            if !response.status().is_success() {
                return Err(Error::Request(format!(
                    "Failed to download snapshot: HTTP {}", 
                    response.status()
                )));
            }
            
            let snapshot: MetaprotocolSnapshot = response.json()
                .await
                .map_err(|e| Error::Request(e.to_string()))?;
                
            return Ok(snapshot);
        }
        
        // For larger snapshots, download chunks in parallel
        info!("Downloading {} chunks in parallel", metadata.total_chunks);
        let mut state_entries = Vec::new();
        
        let mut tasks = Vec::new();
        for i in 0..metadata.total_chunks {
            let client = self.clone();
            let metaprotocol_id = metaprotocol_id.to_string();
            
            let task = tokio::spawn(async move {
                let result = timeout(
                    Duration::from_secs(client.config.timeout_secs),
                    client.download_chunk(&metaprotocol_id, i)
                ).await;
                
                match result {
                    Ok(Ok(chunk)) => Ok(chunk),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(Error::Request(format!("Timeout downloading chunk {}", i))),
                }
            });
            
            tasks.push(task);
        }
        
        // Wait for all chunks to download
        for task in tasks {
            match task.await {
                Ok(Ok(chunk)) => {
                    state_entries.extend(chunk.state_entries);
                },
                Ok(Err(e)) => {
                    return Err(e);
                },
                Err(e) => {
                    return Err(Error::Request(format!("Task failed: {}", e)));
                }
            }
        }
        
        info!("Downloaded {} state entries", state_entries.len());
        
        // Create the snapshot
        let snapshot = MetaprotocolSnapshot {
            metaprotocol_id: metadata.metaprotocol_id,
            block_hash: metadata.block_hash,
            block_height: metadata.block_height,
            state_root: metadata.state_root,
            state_entries,
            merkle_proofs: None, // We'll generate these if needed
        };
        
        Ok(snapshot)
    }
    
    /// Verify a snapshot
    pub fn verify_snapshot(&self, snapshot: &MetaprotocolSnapshot) -> Result<bool> {
        // If the snapshot has proofs, verify them
        if let Some(proofs) = &snapshot.merkle_proofs {
            info!("Verifying snapshot with {} proofs", proofs.len());
            
            // For very large snapshots, use sparse verification
            if snapshot.state_entries.len() > 100_000 {
                info!("Large snapshot detected, using sparse verification (1%)");
                return Ok(snapshot.verify_sparse(0.01));
            } else {
                return Ok(snapshot.verify());
            }
        }
        
        // If no proofs, we need to generate them first
        info!("Snapshot has no proofs, generating them");
        let mut snapshot_copy = snapshot.clone();
        snapshot_copy.generate_proofs();
        
        // Now verify
        if snapshot_copy.state_entries.len() > 100_000 {
            info!("Large snapshot detected, using sparse verification (1%)");
            Ok(snapshot_copy.verify_sparse(0.01))
        } else {
            Ok(snapshot_copy.verify())
        }
    }
    
    /// Import a snapshot into a database
    pub fn import_snapshot<DB>(
        &self, 
        snapshot: &MetaprotocolSnapshot, 
        db: &mut DB
    ) -> Result<()> 
    where
        DB: MerkleizedDatabase,
    {
        info!(
            "Importing snapshot for metaprotocol {} at block {}",
            snapshot.metaprotocol_id,
            snapshot.block_height
        );
        
        // Create a batch for all operations
        let mut batch = db.create_batch();
        
        // Add all entries to the batch
        for entry in &snapshot.state_entries {
            batch.insert(&entry.key, &entry.value);
        }
        
        // Write the batch
        db.write_batch(batch)?;
        
        info!("Imported {} state entries", snapshot.state_entries.len());
        Ok(())
    }
    
    /// Perform a complete sync operation
    pub async fn sync<DB>(
        &self, 
        db: &mut DB, 
        metaprotocol_id: &str
    ) -> Result<u32> 
    where
        DB: MerkleizedDatabase,
    {
        // Download the snapshot
        let snapshot = self.download_snapshot(metaprotocol_id).await?;
        let block_height = snapshot.block_height;
        
        // Verify the snapshot
        if !self.verify_snapshot(&snapshot)? {
            return Err(Error::Verification("Snapshot verification failed".to_string()));
        }
        
        // Import the snapshot
        self.import_snapshot(&snapshot, db)?;
        
        Ok(block_height)
    }
}

impl Clone for RepoClient {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            http_client: self.http_client.clone(),
            temp_dir: self.temp_dir.clone(),
        }
    }
}

/// Trait for databases that support merkleized operations
pub trait MerkleizedDatabase {
    /// Error type for database operations
    type Error: std::error::Error + Send + Sync + 'static;
    
    /// Batch type for database operations
    type Batch;
    
    /// Create a new batch
    fn create_batch(&self) -> Self::Batch;
    
    /// Write a batch to the database
    fn write_batch(&mut self, batch: Self::Batch) -> std::result::Result<(), Self::Error>;
    
    /// Get the current state root
    fn state_root(&self) -> crate::merkle::Hash;
}

/// Batch operations for merkleized databases
pub trait MerkleizedBatch {
    /// Insert a key-value pair
    fn insert<K, V>(&mut self, key: K, value: V)
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>;
        
    /// Remove a key
    fn remove<K>(&mut self, key: K)
    where
        K: AsRef<[u8]>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{mock, server_url};
    use serde_json::json;
    
    #[tokio::test]
    async fn test_get_available_metaprotocols() {
        let _m = mock("GET", "/metaprotocols")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"["alkanes", "ordinals", "brc20"]"#)
            .create();
            
        let client = RepoClient::new(server_url());
        let metaprotocols = client.get_available_metaprotocols().await.unwrap();
        
        assert_eq!(metaprotocols.len(), 3);
        assert!(metaprotocols.contains(&"alkanes".to_string()));
        assert!(metaprotocols.contains(&"ordinals".to_string()));
        assert!(metaprotocols.contains(&"brc20".to_string()));
    }
    
    #[tokio::test]
    async fn test_get_snapshot_metadata() {
        let metadata = SnapshotMetadata {
            metaprotocol_id: "test".to_string(),
            block_hash: vec![1, 2, 3, 4],
            block_height: 100,
            state_root: [0; 64],
            total_chunks: 2,
            total_size_bytes: 1024,
            chunk_hashes: vec![vec![1, 2, 3], vec![4, 5, 6]],
        };
        
        let _m = mock("GET", "/snapshots/test/metadata")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&metadata).unwrap())
            .create();
            
        let client = RepoClient::new(server_url());
        let result = client.get_snapshot_metadata("test").await.unwrap();
        
        assert_eq!(result.metaprotocol_id, "test");
        assert_eq!(result.block_height, 100);
        assert_eq!(result.total_chunks, 2);
    }
}
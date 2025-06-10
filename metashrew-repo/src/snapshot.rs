//! State snapshot functionality for fast sync
//!
//! This module provides types and functions for creating, serializing, and verifying
//! state snapshots used in fast sync.

use crate::{Error, Result, merkle::{Hash, SparseMerkleTree}};
use serde::{Serialize, Deserialize};
use std::path::Path;
use std::fs::{self, File};
use std::io::{Read, Write};
use bincode;

/// A snapshot of a metaprotocol's state at a specific block height
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MetaprotocolSnapshot {
    /// The metaprotocol identifier
    pub metaprotocol_id: String,
    /// The block hash this snapshot represents
    pub block_hash: Vec<u8>,
    /// The block height this snapshot represents
    pub block_height: u32,
    /// The state root hash for verification
    pub state_root: Hash,
    /// The key-value pairs representing the state
    pub state_entries: Vec<KeyValueEntry>,
    /// Optional Merkle proofs for efficient verification
    pub merkle_proofs: Option<Vec<MerkleProof>>,
}

/// A key-value entry in the state
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KeyValueEntry {
    /// The key in the database
    pub key: Vec<u8>,
    /// The value associated with the key
    pub value: Vec<u8>,
}

/// A Merkle proof for a specific key-value pair
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MerkleProof {
    /// The key this proof is for
    pub key: Vec<u8>,
    /// The proof data
    pub proof: Vec<Hash>,
}

/// Metadata for a chunked snapshot
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SnapshotMetadata {
    /// The metaprotocol identifier
    pub metaprotocol_id: String,
    /// The block hash this snapshot represents
    pub block_hash: Vec<u8>,
    /// The block height this snapshot represents
    pub block_height: u32,
    /// The state root hash for verification
    pub state_root: Hash,
    /// Total number of chunks
    pub total_chunks: u32,
    /// Total size in bytes
    pub total_size_bytes: u64,
    /// Hash of each chunk for verification
    pub chunk_hashes: Vec<Vec<u8>>,
}

/// A chunk of a large snapshot
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SnapshotChunk {
    /// The chunk index
    pub chunk_index: u32,
    /// The key-value pairs in this chunk
    pub state_entries: Vec<KeyValueEntry>,
}

impl MetaprotocolSnapshot {
    /// Create a new snapshot
    pub fn new(
        metaprotocol_id: String,
        block_hash: Vec<u8>,
        block_height: u32,
        state_root: Hash,
        state_entries: Vec<KeyValueEntry>,
    ) -> Self {
        Self {
            metaprotocol_id,
            block_hash,
            block_height,
            state_root,
            state_entries,
            merkle_proofs: None,
        }
    }
    
    /// Generate Merkle proofs for all entries
    pub fn generate_proofs(&mut self) {
        let mut tree = SparseMerkleTree::new();
        
        // First, update the tree with all entries
        for entry in &self.state_entries {
            tree.update(&entry.key, &entry.value);
        }
        
        // Then generate proofs for each entry
        let mut proofs = Vec::with_capacity(self.state_entries.len());
        for entry in &self.state_entries {
            let proof = MerkleProof {
                key: entry.key.clone(),
                proof: tree.generate_proof(&entry.key),
            };
            proofs.push(proof);
        }
        
        self.merkle_proofs = Some(proofs);
    }
    
    /// Verify the integrity of the snapshot
    pub fn verify(&self) -> bool {
        // If there are no proofs, we can't verify
        let Some(proofs) = &self.merkle_proofs else {
            return false;
        };
        
        // Verify each entry against its proof
        for (i, entry) in self.state_entries.iter().enumerate() {
            let proof = &proofs[i];
            if !SparseMerkleTree::verify_proof(&entry.key, &entry.value, &proof.proof, &self.state_root) {
                return false;
            }
        }
        
        true
    }
    
    /// Verify a random subset of entries
    pub fn verify_sparse(&self, verification_ratio: f64) -> bool {
        // If there are no proofs, we can't verify
        let Some(proofs) = &self.merkle_proofs else {
            return false;
        };
        
        // Calculate how many entries to verify
        let num_entries = self.state_entries.len();
        let num_to_verify = (num_entries as f64 * verification_ratio).ceil() as usize;
        if num_to_verify == 0 {
            return false;
        }
        
        // Select random entries to verify
        let mut rng = rand::thread_rng();
        let indices_to_verify: Vec<usize> = (0..num_entries)
            .collect::<Vec<_>>()
            .choose_multiple(&mut rng, num_to_verify)
            .cloned()
            .collect();
        
        // Verify selected entries
        for &index in &indices_to_verify {
            let entry = &self.state_entries[index];
            let proof = &proofs[index];
            if !SparseMerkleTree::verify_proof(&entry.key, &entry.value, &proof.proof, &self.state_root) {
                return false;
            }
        }
        
        true
    }
    
    /// Split the snapshot into chunks
    pub fn split_into_chunks(&self, chunk_size: usize) -> (SnapshotMetadata, Vec<SnapshotChunk>) {
        let mut chunks = Vec::new();
        let mut chunk_hashes = Vec::new();
        let mut total_size_bytes = 0;
        
        // Split state entries into chunks
        for (i, chunk) in self.state_entries.chunks(chunk_size).enumerate() {
            let chunk_entries = chunk.to_vec();
            let chunk_data = bincode::serialize(&chunk_entries).unwrap_or_default();
            total_size_bytes += chunk_data.len() as u64;
            
            // Calculate hash of chunk for verification
            let mut hasher = blake2::Blake2b512::new();
            hasher.update(&chunk_data);
            let chunk_hash = hasher.finalize().to_vec();
            chunk_hashes.push(chunk_hash);
            
            chunks.push(SnapshotChunk {
                chunk_index: i as u32,
                state_entries: chunk_entries,
            });
        }
        
        let metadata = SnapshotMetadata {
            metaprotocol_id: self.metaprotocol_id.clone(),
            block_hash: self.block_hash.clone(),
            block_height: self.block_height,
            state_root: self.state_root,
            total_chunks: chunks.len() as u32,
            total_size_bytes,
            chunk_hashes,
        };
        
        (metadata, chunks)
    }
    
    /// Save the snapshot to a file
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let data = bincode::serialize(self).map_err(Error::Serialization)?;
        let mut file = File::create(path).map_err(Error::Io)?;
        file.write_all(&data).map_err(Error::Io)?;
        Ok(())
    }
    
    /// Load a snapshot from a file
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let mut file = File::open(path).map_err(Error::Io)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(Error::Io)?;
        let snapshot = bincode::deserialize(&data).map_err(Error::Serialization)?;
        Ok(snapshot)
    }
}

/// Save snapshot metadata to a file
pub fn save_metadata(metadata: &SnapshotMetadata, path: &Path) -> Result<()> {
    let data = bincode::serialize(metadata).map_err(Error::Serialization)?;
    let mut file = File::create(path).map_err(Error::Io)?;
    file.write_all(&data).map_err(Error::Io)?;
    Ok(())
}

/// Load snapshot metadata from a file
pub fn load_metadata(path: &Path) -> Result<SnapshotMetadata> {
    let mut file = File::open(path).map_err(Error::Io)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(Error::Io)?;
    let metadata = bincode::deserialize(&data).map_err(Error::Serialization)?;
    Ok(metadata)
}

/// Save a snapshot chunk to a file
pub fn save_chunk(chunk: &SnapshotChunk, path: &Path) -> Result<()> {
    let data = bincode::serialize(chunk).map_err(Error::Serialization)?;
    let mut file = File::create(path).map_err(Error::Io)?;
    file.write_all(&data).map_err(Error::Io)?;
    Ok(())
}

/// Load a snapshot chunk from a file
pub fn load_chunk(path: &Path) -> Result<SnapshotChunk> {
    let mut file = File::open(path).map_err(Error::Io)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(Error::Io)?;
    let chunk = bincode::deserialize(&data).map_err(Error::Serialization)?;
    Ok(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_snapshot_serialization() {
        let snapshot = MetaprotocolSnapshot {
            metaprotocol_id: "test".to_string(),
            block_hash: vec![1, 2, 3, 4],
            block_height: 100,
            state_root: [0; 64],
            state_entries: vec![
                KeyValueEntry {
                    key: vec![1, 2, 3],
                    value: vec![4, 5, 6],
                },
                KeyValueEntry {
                    key: vec![7, 8, 9],
                    value: vec![10, 11, 12],
                },
            ],
            merkle_proofs: None,
        };
        
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("snapshot.bin");
        
        // Save to file
        snapshot.save_to_file(&file_path).unwrap();
        
        // Load from file
        let loaded = MetaprotocolSnapshot::load_from_file(&file_path).unwrap();
        
        // Verify
        assert_eq!(loaded.metaprotocol_id, snapshot.metaprotocol_id);
        assert_eq!(loaded.block_hash, snapshot.block_hash);
        assert_eq!(loaded.block_height, snapshot.block_height);
        assert_eq!(loaded.state_root, snapshot.state_root);
        assert_eq!(loaded.state_entries.len(), snapshot.state_entries.len());
    }
    
    #[test]
    fn test_chunk_serialization() {
        let chunk = SnapshotChunk {
            chunk_index: 1,
            state_entries: vec![
                KeyValueEntry {
                    key: vec![1, 2, 3],
                    value: vec![4, 5, 6],
                },
                KeyValueEntry {
                    key: vec![7, 8, 9],
                    value: vec![10, 11, 12],
                },
            ],
        };
        
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("chunk.bin");
        
        // Save to file
        save_chunk(&chunk, &file_path).unwrap();
        
        // Load from file
        let loaded = load_chunk(&file_path).unwrap();
        
        // Verify
        assert_eq!(loaded.chunk_index, chunk.chunk_index);
        assert_eq!(loaded.state_entries.len(), chunk.state_entries.len());
    }
}
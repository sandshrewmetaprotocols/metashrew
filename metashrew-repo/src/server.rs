//! Server for hosting state snapshots
//!
//! This module provides functionality for creating and hosting state snapshots
//! for fast sync.

use crate::{
    RepoConfig, Error, Result,
    snapshot::{
        MetaprotocolSnapshot, SnapshotMetadata, SnapshotChunk, KeyValueEntry,
        save_metadata, save_chunk,
    },
    merkle::SparseMerkleTree,
};
use std::path::{Path, PathBuf};
use std::fs::{self, File};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use actix_web::{
    web, App, HttpServer, HttpResponse, Responder,
    middleware::Logger,
};
use serde::{Serialize, Deserialize};
use log::{info, warn, debug, error};

/// Information about a metaprotocol
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetaprotocolInfo {
    /// Metaprotocol identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Latest block height
    pub latest_block: u32,
    /// Latest snapshot timestamp
    pub latest_snapshot: u64,
}

/// Server for hosting state snapshots
#[derive(Clone)]
pub struct RepoServer {
    /// Configuration for the server
    config: RepoConfig,
    /// Available metaprotocols
    metaprotocols: Arc<RwLock<HashMap<String, MetaprotocolInfo>>>,
}

impl RepoServer {
    /// Create a new repository server
    pub fn new(config: RepoConfig) -> Self {
        // Create snapshot directory if it doesn't exist
        if !config.snapshot_dir.exists() {
            fs::create_dir_all(&config.snapshot_dir).ok();
        }
        
        Self {
            config,
            metaprotocols: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Register a metaprotocol
    pub fn register_metaprotocol(&self, info: MetaprotocolInfo) -> Result<()> {
        let mut metaprotocols = self.metaprotocols.write().map_err(|_| {
            Error::Request("Failed to acquire write lock on metaprotocols".to_string())
        })?;
        
        metaprotocols.insert(info.id.clone(), info);
        Ok(())
    }
    
    /// Create a snapshot from a database
    pub fn create_snapshot<DB>(
        &self,
        metaprotocol_id: &str,
        db: &DB,
        block_hash: Vec<u8>,
        block_height: u32,
    ) -> Result<MetaprotocolSnapshot>
    where
        DB: MerkleizedDatabaseReader,
    {
        info!(
            "Creating snapshot for metaprotocol {} at block {}",
            metaprotocol_id,
            block_height
        );
        
        // Get the state root
        let state_root = db.state_root();
        
        // Collect all key-value pairs for the metaprotocol
        let mut state_entries = Vec::new();
        let prefix = format!("{}/", metaprotocol_id).into_bytes();
        
        let mut iter = db.prefix_iterator(&prefix);
        while let Some((key, value)) = iter.next() {
            state_entries.push(KeyValueEntry {
                key: key.to_vec(),
                value: value.to_vec(),
            });
        }
        
        info!("Collected {} state entries", state_entries.len());
        
        // Create the snapshot
        let mut snapshot = MetaprotocolSnapshot {
            metaprotocol_id: metaprotocol_id.to_string(),
            block_hash,
            block_height,
            state_root,
            state_entries,
            merkle_proofs: None,
        };
        
        // Generate Merkle proofs
        info!("Generating Merkle proofs");
        snapshot.generate_proofs();
        
        Ok(snapshot)
    }
    
    /// Save a snapshot to disk
    pub fn save_snapshot(
        &self,
        snapshot: &MetaprotocolSnapshot,
        chunk_size: usize,
    ) -> Result<PathBuf> {
        let metaprotocol_id = &snapshot.metaprotocol_id;
        let block_height = snapshot.block_height;
        
        // Create directory for this metaprotocol
        let metaprotocol_dir = self.config.snapshot_dir.join(metaprotocol_id);
        if !metaprotocol_dir.exists() {
            fs::create_dir_all(&metaprotocol_dir).map_err(Error::Io)?;
        }
        
        // Create directory for this snapshot
        let snapshot_dir = metaprotocol_dir.join(block_height.to_string());
        if !snapshot_dir.exists() {
            fs::create_dir_all(&snapshot_dir).map_err(Error::Io)?;
        }
        
        // For small snapshots, save the full snapshot
        if snapshot.state_entries.len() <= chunk_size {
            let snapshot_path = snapshot_dir.join("full.bin");
            snapshot.save_to_file(&snapshot_path)?;
            return Ok(snapshot_path);
        }
        
        // For larger snapshots, split into chunks
        info!("Splitting snapshot into chunks of size {}", chunk_size);
        let (metadata, chunks) = snapshot.split_into_chunks(chunk_size);
        
        // Save metadata
        let metadata_path = snapshot_dir.join("metadata.bin");
        save_metadata(&metadata, &metadata_path)?;
        
        // Save chunks
        for chunk in chunks {
            let chunk_path = snapshot_dir.join(format!("chunk_{}.bin", chunk.chunk_index));
            save_chunk(&chunk, &chunk_path)?;
        }
        
        Ok(snapshot_dir)
    }
    
    /// Start the HTTP server
    pub async fn run(&self) -> Result<()> {
        let config = self.config.clone();
        let metaprotocols = self.metaprotocols.clone();
        
        info!("Starting repo server on {}:{}", config.host, config.port);
        
        HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(config.clone()))
                .app_data(web::Data::new(metaprotocols.clone()))
                .wrap(Logger::default())
                .service(web::resource("/metaprotocols").route(web::get().to(get_metaprotocols)))
                .service(web::resource("/snapshots/{id}/metadata").route(web::get().to(get_snapshot_metadata)))
                .service(web::resource("/snapshots/{id}/full").route(web::get().to(get_full_snapshot)))
                .service(web::resource("/snapshots/{id}/full/{block_height}").route(web::get().to(get_full_snapshot)))
                .service(web::resource("/snapshots/{id}/chunks/{chunk_index}").route(web::get().to(get_snapshot_chunk)))
                .service(web::resource("/snapshots/{id}/chunks/{chunk_index}/{block_height}").route(web::get().to(get_snapshot_chunk)))
        })
        .bind(format!("{}:{}", config.host, config.port))
        .map_err(|e| Error::Request(format!("Failed to bind server: {}", e)))?
        .run()
        .await
        .map_err(|e| Error::Request(format!("Server error: {}", e)))?;
        
        Ok(())
    }
}

/// Handler for GET /metaprotocols
async fn get_metaprotocols(
    metaprotocols: web::Data<Arc<RwLock<HashMap<String, MetaprotocolInfo>>>>,
) -> impl Responder {
    match metaprotocols.read() {
        Ok(protocols) => {
            let ids: Vec<String> = protocols.keys().cloned().collect();
            HttpResponse::Ok().json(ids)
        },
        Err(_) => {
            HttpResponse::InternalServerError().body("Failed to read metaprotocols")
        }
    }
}

/// Handler for GET /snapshots/{id}/metadata
async fn get_snapshot_metadata(
    path: web::Path<String>,
    config: web::Data<RepoConfig>,
    metaprotocols: web::Data<Arc<RwLock<HashMap<String, MetaprotocolInfo>>>>,
) -> impl Responder {
    let id = path.into_inner();
    
    // Check if metaprotocol exists
    let latest_block = match metaprotocols.read() {
        Ok(protocols) => {
            match protocols.get(&id) {
                Some(info) => info.latest_block,
                None => return HttpResponse::NotFound().body("Metaprotocol not found"),
            }
        },
        Err(_) => {
            return HttpResponse::InternalServerError().body("Failed to read metaprotocols");
        }
    };
    
    // Find the latest snapshot
    let metaprotocol_dir = config.snapshot_dir.join(&id);
    let latest_snapshot_dir = metaprotocol_dir.join(latest_block.to_string());
    let metadata_path = latest_snapshot_dir.join("metadata.bin");
    
    // If metadata file exists, read it
    if metadata_path.exists() {
        match crate::snapshot::load_metadata(&metadata_path) {
            Ok(metadata) => HttpResponse::Ok().json(metadata),
            Err(_) => HttpResponse::InternalServerError().body("Failed to read metadata"),
        }
    } else {
        // Check if full snapshot exists
        let full_path = latest_snapshot_dir.join("full.bin");
        if full_path.exists() {
            match crate::snapshot::MetaprotocolSnapshot::load_from_file(&full_path) {
                Ok(snapshot) => {
                    // Create metadata from full snapshot
                    let metadata = SnapshotMetadata {
                        metaprotocol_id: snapshot.metaprotocol_id,
                        block_hash: snapshot.block_hash,
                        block_height: snapshot.block_height,
                        state_root: snapshot.state_root,
                        total_chunks: 1,
                        total_size_bytes: fs::metadata(&full_path).map(|m| m.len()).unwrap_or(0),
                        chunk_hashes: vec![],
                    };
                    HttpResponse::Ok().json(metadata)
                },
                Err(_) => HttpResponse::InternalServerError().body("Failed to read snapshot"),
            }
        } else {
            HttpResponse::NotFound().body("Snapshot not found")
        }
    }
}

/// Handler for GET /snapshots/{id}/full
async fn get_full_snapshot(
    path: web::Path<(String, Option<u32>)>,
    config: web::Data<RepoConfig>,
    metaprotocols: web::Data<Arc<RwLock<HashMap<String, MetaprotocolInfo>>>>,
) -> impl Responder {
    let (id, block_height) = path.into_inner();
    
    // Check if metaprotocol exists
    let latest_block = match metaprotocols.read() {
        Ok(protocols) => {
            match protocols.get(&id) {
                Some(info) => info.latest_block,
                None => return HttpResponse::NotFound().body("Metaprotocol not found"),
            }
        },
        Err(_) => {
            return HttpResponse::InternalServerError().body("Failed to read metaprotocols");
        }
    };
    
    // Use specified block height or latest
    let block_height = block_height.unwrap_or(latest_block);
    
    // Find the snapshot
    let metaprotocol_dir = config.snapshot_dir.join(&id);
    let latest_snapshot_dir = metaprotocol_dir.join(block_height.to_string());
    let full_path = latest_snapshot_dir.join("full.bin");
    
    // If full snapshot exists, read it
    if full_path.exists() {
        match crate::snapshot::MetaprotocolSnapshot::load_from_file(&full_path) {
            Ok(snapshot) => HttpResponse::Ok().json(snapshot),
            Err(_) => HttpResponse::InternalServerError().body("Failed to read snapshot"),
        }
    } else {
        HttpResponse::NotFound().body("Full snapshot not available")
    }
}

/// Handler for GET /snapshots/{id}/chunks/{chunk_index}
async fn get_snapshot_chunk(
    path: web::Path<(String, u32, Option<u32>)>,
    config: web::Data<RepoConfig>,
    metaprotocols: web::Data<Arc<RwLock<HashMap<String, MetaprotocolInfo>>>>,
) -> impl Responder {
    let (id, chunk_index, block_height) = path.into_inner();
    
    // Check if metaprotocol exists
    let latest_block = match metaprotocols.read() {
        Ok(protocols) => {
            match protocols.get(&id) {
                Some(info) => info.latest_block,
                None => return HttpResponse::NotFound().body("Metaprotocol not found"),
            }
        },
        Err(_) => {
            return HttpResponse::InternalServerError().body("Failed to read metaprotocols");
        }
    };
    
    // Use specified block height or latest
    let block_height = block_height.unwrap_or(latest_block);
    
    // Find the snapshot
    let metaprotocol_dir = config.snapshot_dir.join(&id);
    let latest_snapshot_dir = metaprotocol_dir.join(block_height.to_string());
    let chunk_path = latest_snapshot_dir.join(format!("chunk_{}.bin", chunk_index));
    
    // If chunk exists, read it
    if chunk_path.exists() {
        match crate::snapshot::load_chunk(&chunk_path) {
            Ok(chunk) => HttpResponse::Ok().json(chunk),
            Err(_) => HttpResponse::InternalServerError().body("Failed to read chunk"),
        }
    } else {
        HttpResponse::NotFound().body("Chunk not found")
    }
}

/// Trait for reading from a merkleized database
pub trait MerkleizedDatabaseReader {
    /// Get the current state root
    fn state_root(&self) -> crate::merkle::Hash;
    
    /// Iterator over key-value pairs with a prefix
    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, web, App};
    use tempfile::tempdir;
    
    #[actix_web::test]
    async fn test_get_metaprotocols() {
        let metaprotocols = Arc::new(RwLock::new(HashMap::new()));
        {
            let mut map = metaprotocols.write().unwrap();
            map.insert(
                "test".to_string(),
                MetaprotocolInfo {
                    id: "test".to_string(),
                    name: "Test Protocol".to_string(),
                    description: "A test protocol".to_string(),
                    latest_block: 100,
                    latest_snapshot: 1622505600,
                },
            );
        }
        
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(metaprotocols.clone()))
                .service(web::resource("/metaprotocols").route(web::get().to(get_metaprotocols)))
        ).await;
        
        let req = test::TestRequest::get().uri("/metaprotocols").to_request();
        let resp = test::call_service(&app, req).await;
        
        assert!(resp.status().is_success());
        
        let body = test::read_body(resp).await;
        let ids: Vec<String> = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "test");
    }
}
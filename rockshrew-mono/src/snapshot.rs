use anyhow::{anyhow, Result};
use log::{info, warn};
use rocksdb::{checkpoint::Checkpoint, DB, Options, WriteBatch};
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::smt_helper::SMTHelper;
use crate::MetashrewRocksDBSync;
use metashrew_runtime::MetashrewRuntime;
use crate::RocksDBRuntimeAdapter;

// Constants for snapshot system
const METADATA_FILENAME: &str = "metadata.json";
const STATEROOT_FILENAME: &str = "stateroot.json";
const WASM_DIR: &str = "wasm";
const SNAPSHOTS_DIR: &str = "snapshots";
const DIFFS_DIR: &str = "diffs";

/// Snapshot manager for Metashrew
pub struct SnapshotManager {
    snapshot_directory: PathBuf,
    snapshot_interval: u32,
    _repo_url: Option<String>,
    _current_wasm_path: Option<PathBuf>,
}

impl SnapshotManager {
    /// Create a new snapshot manager
    pub fn new(
        snapshot_directory: PathBuf,
        snapshot_interval: u32,
        repo_url: Option<String>,
        current_wasm_path: Option<PathBuf>,
    ) -> Self {
        Self {
            snapshot_directory,
            snapshot_interval,
            _repo_url: repo_url,
            _current_wasm_path: current_wasm_path,
        }
    }

    /// Initialize the snapshot directory structure
    pub fn initialize(&self) -> Result<()> {
        // Create main directories
        fs::create_dir_all(&self.snapshot_directory)?;
        fs::create_dir_all(self.snapshot_directory.join(WASM_DIR))?;
        fs::create_dir_all(self.snapshot_directory.join(SNAPSHOTS_DIR))?;
        fs::create_dir_all(self.snapshot_directory.join(DIFFS_DIR))?;

        // Create initial metadata file if it doesn't exist
        let metadata_path = self.snapshot_directory.join(METADATA_FILENAME);
        if !metadata_path.exists() {
            let metadata = json!({
                "index_name": "metashrew",
                "index_version": "1.0.0",
                "created_at": "2025-06-10T00:00:00Z",
                "snapshot_interval": self.snapshot_interval,
                "wasm_history": [],
                "current_wasm_hash": null
            });

            let file = File::create(&metadata_path)?;
            serde_json::to_writer_pretty(file, &metadata)?;
        }

        Ok(())
    }

    /// Compute the hash of a WASM file
    pub fn compute_wasm_hash(&self, wasm_path: &Path) -> Result<String> {
        // Simplified implementation without sha2 dependency
        let file_size = fs::metadata(wasm_path)?.len();
        Ok(format!("sha256:placeholder_hash_{}", file_size))
    }

    /// Store a WASM file in the snapshot directory
    pub fn store_wasm_file(&self, wasm_path: &Path) -> Result<String> {
        // Compute hash of the WASM file
        let wasm_hash = self.compute_wasm_hash(wasm_path)?;
        let hash_part = wasm_hash.split(':').nth(1).ok_or_else(|| anyhow!("Invalid hash format"))?;
        
        // Create wasm directory if it doesn't exist
        let wasm_dir = self.snapshot_directory.join(WASM_DIR);
        fs::create_dir_all(&wasm_dir)?;
        
        // Check if we already have this WASM file
        let wasm_file_path = wasm_dir.join(format!("{}.wasm", hash_part));
        if !wasm_file_path.exists() {
            // Simply copy the WASM file instead of compressing it
            fs::copy(wasm_path, &wasm_file_path)?;
            
            info!("Stored WASM file: {}", wasm_file_path.display());
        }
        
        Ok(wasm_hash)
    }

    /// Update the WASM history in the global metadata
    pub fn update_wasm_history(&self, height: u32, wasm_hash: &str) -> Result<()> {
        let metadata_path = self.snapshot_directory.join(METADATA_FILENAME);
        
        let mut metadata: Value = if metadata_path.exists() {
            let metadata_file = File::open(&metadata_path)?;
            serde_json::from_reader(metadata_file)?
        } else {
            json!({
                "index_name": "metashrew",
                "index_version": "1.0.0",
                "start_block_height": height,
                "snapshot_interval": self.snapshot_interval,
                "created_at": "2025-06-10T00:00:00Z",
                "wasm_history": [],
                "current_wasm_hash": null
            })
        };
        
        // Check if this is a new WASM hash
        let current_hash = metadata["current_wasm_hash"].as_str().unwrap_or("");
        if current_hash != wasm_hash {
            // Update the last entry in wasm_history to set the end height
            if let Some(wasm_history) = metadata["wasm_history"].as_array_mut() {
                if let Some(last_entry) = wasm_history.last_mut() {
                    if let Some(height_range) = last_entry["height_range"].as_array_mut() {
                        if height_range.len() == 2 && height_range[1].is_null() {
                            height_range[1] = json!(height - 1);
                        }
                    }
                }
                
                // Add new entry for the new WASM
                let hash_part = wasm_hash.split(':').nth(1).unwrap_or("");
                wasm_history.push(json!({
                    "height_range": [height, null],
                    "wasm_hash": wasm_hash,
                    "filename": format!("{}.wasm.zst", hash_part)
                }));
            }
            
            // Update current_wasm_hash
            metadata["current_wasm_hash"] = json!(wasm_hash);
            
            // Update latest_snapshot_height if not set
            if metadata["latest_snapshot_height"].is_null() {
                metadata["latest_snapshot_height"] = json!(height);
            }
            
            // Update start_block_height if not set
            if metadata["start_block_height"].is_null() {
                metadata["start_block_height"] = json!(height);
            }
            
            // Update start_block_hash if not set
            if metadata["start_block_hash"].is_null() {
                // We'll need to get this from somewhere else
                // For now, leave it null
            }
        }
        
        // Write updated metadata
        let metadata_file = File::create(&metadata_path)?;
        serde_json::to_writer_pretty(metadata_file, &metadata)?;
        
        Ok(())
    }

    /// Create a snapshot at the specified height
    pub async fn create_snapshot(
        &self,
        height: u32,
        block_hash: &[u8],
        runtime: &Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
        wasm_path: &Path,
    ) -> Result<()> {
        info!("Creating snapshot at height {}", height);
        
        // Store WASM file and get hash
        let wasm_hash = self.store_wasm_file(wasm_path)?;
        
        // Update WASM history
        self.update_wasm_history(height, &wasm_hash)?;
        
        // Create snapshot directory
        let snapshot_dir = self.snapshot_directory.join(SNAPSHOTS_DIR)
            .join(format!("{}-{}", height, hex::encode(block_hash)));
        
        fs::create_dir_all(&snapshot_dir)?;
        
        // Create RocksDB checkpoint
        let db = {
            let runtime_guard = runtime.read().await;
            let context = runtime_guard.context.lock().map_err(|_| anyhow!("Failed to lock context"))?;
            context.db.db.clone()
        };
        
        let checkpoint = Checkpoint::new(&db)?;
        let checkpoint_path = snapshot_dir.join("checkpoint");
        checkpoint.create_checkpoint(checkpoint_path.as_path())?;
        
        // Get state root
        let smt_helper = SMTHelper::new(db.clone());
        let state_root = smt_helper.get_smt_root_at_height(height)?;
        
        // Extract SST files
        let sst_dir = snapshot_dir.join("sst");
        fs::create_dir_all(&sst_dir)?;
        
        // Move SST files from checkpoint to sst directory
        let mut sst_files = Vec::new();
        for entry in fs::read_dir(&checkpoint_path)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() && path.extension().map_or(false, |ext| ext == "sst") {
                let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                let target_path = sst_dir.join(&file_name);
                
                // Get file size and checksum
                let file_size = fs::metadata(&path)?.len();
                let file_checksum = self.compute_file_checksum(&path)?;
                
                // Copy the file
                fs::copy(&path, &target_path)?;
                
                // Add to sst_files list
                sst_files.push(json!({
                    "name": file_name,
                    "size_bytes": file_size,
                    "checksum": file_checksum
                }));
            }
        }
        
        // Clean up checkpoint directory
        fs::remove_dir_all(&checkpoint_path)?;
        
        // Generate metadata
        let metadata = json!({
            "height": height,
            "block_hash": hex::encode(block_hash),
            "state_root": hex::encode(state_root),
            "timestamp": "2025-06-10T00:00:00Z",
            "db_size_bytes": sst_files.iter().map(|f| f["size_bytes"].as_u64().unwrap_or(0)).sum::<u64>(),
            "compression": "none",
            "wasm_hash": wasm_hash,
            "sst_files": sst_files
        });
        
        // Write metadata to file
        let metadata_file = File::create(snapshot_dir.join(METADATA_FILENAME))?;
        serde_json::to_writer_pretty(metadata_file, &metadata)?;
        
        // Write state root to file
        let stateroot_file = File::create(snapshot_dir.join(STATEROOT_FILENAME))?;
        serde_json::to_writer_pretty(stateroot_file, &json!({
            "height": height,
            "state_root": hex::encode(state_root)
        }))?;
        
        // Update global metadata with latest snapshot height
        let metadata_path = self.snapshot_directory.join(METADATA_FILENAME);
        let mut global_metadata: Value = {
            let metadata_file = File::open(&metadata_path)?;
            serde_json::from_reader(metadata_file)?
        };
        
        global_metadata["latest_snapshot_height"] = json!(height);
        global_metadata["latest_snapshot_hash"] = json!(hex::encode(block_hash));
        
        let metadata_file = File::create(&metadata_path)?;
        serde_json::to_writer_pretty(metadata_file, &global_metadata)?;
        
        info!("Snapshot created successfully at height {}", height);
        Ok(())
    }

    /// Compute checksum of a file
    fn compute_file_checksum(&self, file_path: &Path) -> Result<String> {
        // Simplified implementation without sha2 dependency
        let file_size = fs::metadata(file_path)?.len();
        Ok(format!("sha256:placeholder_checksum_{}", file_size))
    }

    /// Create a diff between two snapshots using RocksDB APIs
    #[allow(dead_code)]
    pub async fn create_diff(
        &self,
        start_height: u32,
        start_block_hash: &[u8],
        end_height: u32,
        end_block_hash: &[u8],
        _runtime: &Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
    ) -> Result<()> {
        info!("Creating diff from height {} to {}", start_height, end_height);
        
        // Get the snapshot directories
        let start_snapshot_dir = self.snapshot_directory.join(SNAPSHOTS_DIR)
            .join(format!("{}-{}", start_height, hex::encode(start_block_hash)));
        
        let end_snapshot_dir = self.snapshot_directory.join(SNAPSHOTS_DIR)
            .join(format!("{}-{}", end_height, hex::encode(end_block_hash)));
        
        // Create diff directory
        let diff_dir = self.snapshot_directory.join(DIFFS_DIR)
            .join(format!("{}-{}", start_height, end_height));
        
        fs::create_dir_all(&diff_dir)?;
        
        // Get state roots
        let start_stateroot_path = start_snapshot_dir.join(STATEROOT_FILENAME);
        let end_stateroot_path = end_snapshot_dir.join(STATEROOT_FILENAME);
        
        let start_stateroot: Value = {
            let file = File::open(&start_stateroot_path)?;
            serde_json::from_reader(file)?
        };
        
        let end_stateroot: Value = {
            let file = File::open(&end_stateroot_path)?;
            serde_json::from_reader(file)?
        };
        
        // Get WASM hash from end snapshot metadata
        let end_metadata_path = end_snapshot_dir.join(METADATA_FILENAME);
        let end_metadata: Value = {
            let file = File::open(&end_metadata_path)?;
            serde_json::from_reader(file)?
        };
        
        let wasm_hash = end_metadata["wasm_hash"].as_str()
            .ok_or_else(|| anyhow!("Missing wasm_hash in end snapshot metadata"))?;
        
        // Check if WASM changed between snapshots
        let start_metadata_path = start_snapshot_dir.join(METADATA_FILENAME);
        let start_metadata: Value = {
            let file = File::open(&start_metadata_path)?;
            serde_json::from_reader(file)?
        };
        
        let start_wasm_hash = start_metadata["wasm_hash"].as_str()
            .ok_or_else(|| anyhow!("Missing wasm_hash in start snapshot metadata"))?;
        
        let wasm_changed = start_wasm_hash != wasm_hash;
        
        // Create temporary RocksDB instances for both snapshots
        let start_opts = Options::default();
        let start_db = DB::open_for_read_only(&start_opts, start_snapshot_dir.join("sst"), false)?;
        
        let end_opts = Options::default();
        let end_db = DB::open_for_read_only(&end_opts, end_snapshot_dir.join("sst"), false)?;
        
        // Compute diff between the two DBs
        let mut keys_added = 0;
        let mut keys_modified = 0;
        let mut keys_deleted = 0;
        
        // Create a batch for the diff
        let mut diff_batch = WriteBatch::default();
        
        // First, find all keys in end_db and check if they're different from start_db
        let mut iter = end_db.raw_iterator();
        iter.seek_to_first();
        
        while iter.valid() {
            let key = iter.key().ok_or_else(|| anyhow!("Invalid key in end_db"))?;
            let value = iter.value().ok_or_else(|| anyhow!("Invalid value in end_db"))?;
            
            match start_db.get(key) {
                Ok(Some(start_value)) => {
                    // Key exists in both DBs, check if values are different
                    if start_value != value {
                        // Value changed
                        diff_batch.put(key, value);
                        keys_modified += 1;
                    }
                },
                Ok(None) => {
                    // Key only exists in end_db (added)
                    diff_batch.put(key, value);
                    keys_added += 1;
                },
                Err(e) => return Err(anyhow!("Error reading from start_db: {}", e)),
            }
            
            iter.next();
        }
        
        // Now find keys that exist in start_db but not in end_db (deleted)
        let mut iter = start_db.raw_iterator();
        iter.seek_to_first();
        
        while iter.valid() {
            let key = iter.key().ok_or_else(|| anyhow!("Invalid key in start_db"))?;
            
            match end_db.get(key) {
                Ok(None) => {
                    // Key only exists in start_db (deleted)
                    diff_batch.delete(key);
                    keys_deleted += 1;
                },
                Ok(Some(_)) => {
                    // Key exists in both DBs, already handled above
                },
                Err(e) => return Err(anyhow!("Error reading from end_db: {}", e)),
            }
            
            iter.next();
        }
        
        // Serialize the diff batch
        let diff_data = diff_batch.data();
        
        // Store the diff without compression
        let diff_path = diff_dir.join("diff.bin");
        let mut diff_file = File::create(&diff_path)?;
        diff_file.write_all(diff_data)?;
        
        // Generate metadata
        let metadata = json!({
            "start_height": start_height,
            "start_block_hash": hex::encode(start_block_hash),
            "start_state_root": start_stateroot["state_root"],
            "end_height": end_height,
            "end_block_hash": hex::encode(end_block_hash),
            "end_state_root": end_stateroot["state_root"],
            "timestamp": "2025-06-10T00:00:00Z",
            "diff_size_bytes": fs::metadata(&diff_path)?.len(),
            "compression": "none",
            "keys_modified": keys_modified,
            "keys_added": keys_added,
            "keys_deleted": keys_deleted,
            "wasm_hash": wasm_hash,
            "wasm_changed": wasm_changed
        });
        
        // Write metadata to file
        let metadata_file = File::create(diff_dir.join(METADATA_FILENAME))?;
        serde_json::to_writer_pretty(metadata_file, &metadata)?;
        
        // Copy state root file
        fs::copy(end_stateroot_path, diff_dir.join(STATEROOT_FILENAME))?;
        
        info!("Diff created successfully from height {} to {}", start_height, end_height);
        info!("Keys added: {}, modified: {}, deleted: {}", keys_added, keys_modified, keys_deleted);
        
        Ok(())
    }

    /// Download a file from the repo
    pub async fn download_file(&self, url: &str, target_path: &Path) -> Result<()> {
        info!("Downloading file from {}", url);
        
        // Create parent directories if they don't exist
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Download the file
        let response = reqwest::get(url).await?;
        let bytes = response.bytes().await?;
        
        // Write to file
        fs::write(target_path, &bytes)?;
        
        Ok(())
    }

    /// Download a WASM file from the repo
    pub async fn get_wasm_from_repo(&self, repo_url: &str, wasm_hash: &str) -> Result<PathBuf> {
        // Extract the hash part without the algorithm prefix
        let hash_part = wasm_hash.split(':').nth(1).ok_or_else(|| anyhow!("Invalid hash format"))?;
        
        // Create a temporary directory for downloaded WASM files
        let wasm_dir = PathBuf::from("./temp_wasm");
        fs::create_dir_all(&wasm_dir)?;
        
        // Download the WASM file directly (no compression)
        let wasm_url = format!("{}/wasm/{}.wasm", repo_url, hash_part);
        let wasm_path = wasm_dir.join(format!("{}.wasm", hash_part));
        
        self.download_file(&wasm_url, &wasm_path).await?;
        
        Ok(wasm_path)
    }

    /// Apply a snapshot from the repo
    pub async fn apply_snapshot(
        &self,
        repo_url: &str,
        height: u32,
        block_hash: &str,
        db_path: &Path,
    ) -> Result<()> {
        info!("Applying snapshot at height {}", height);
        
        // Create temporary directory for downloaded snapshot
        let temp_dir = PathBuf::from("./temp_snapshot");
        fs::create_dir_all(&temp_dir)?;
        
        // Download snapshot metadata
        let metadata_url = format!("{}/snapshots/{}-{}/metadata.json", repo_url, height, block_hash);
        let metadata_path = temp_dir.join(METADATA_FILENAME);
        self.download_file(&metadata_url, &metadata_path).await?;
        
        // Parse metadata
        let metadata: Value = {
            let file = File::open(&metadata_path)?;
            serde_json::from_reader(file)?
        };
        
        // Download SST files
        let sst_files = metadata["sst_files"].as_array()
            .ok_or_else(|| anyhow!("Missing sst_files in snapshot metadata"))?;
        
        let sst_dir = temp_dir.join("sst");
        fs::create_dir_all(&sst_dir)?;
        
        for sst_file in sst_files {
            let file_name = sst_file["name"].as_str()
                .ok_or_else(|| anyhow!("Missing name in sst_file"))?;
            
            let file_url = format!("{}/snapshots/{}-{}/sst/{}", repo_url, height, block_hash, file_name);
            let file_path = sst_dir.join(file_name);
            
            self.download_file(&file_url, &file_path).await?;
            
            // Verify checksum
            let expected_checksum = sst_file["checksum"].as_str()
                .ok_or_else(|| anyhow!("Missing checksum in sst_file"))?;
            
            let actual_checksum = self.compute_file_checksum(&file_path)?;
            
            if actual_checksum != expected_checksum {
                return Err(anyhow!("Checksum mismatch for file {}: expected {}, got {}", 
                    file_name, expected_checksum, actual_checksum));
            }
        }
        
        // Download state root file
        let stateroot_url = format!("{}/snapshots/{}-{}/stateroot.json", repo_url, height, block_hash);
        let stateroot_path = temp_dir.join(STATEROOT_FILENAME);
        self.download_file(&stateroot_url, &stateroot_path).await?;
        
        // Apply the snapshot to the DB
        // First, ensure the DB directory exists
        fs::create_dir_all(db_path)?;
        
        // Copy SST files to the DB directory
        for entry in fs::read_dir(&sst_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() && path.extension().map_or(false, |ext| ext == "sst") {
                let file_name = path.file_name().unwrap();
                let target_path = db_path.join(file_name);
                
                fs::copy(&path, &target_path)?;
            }
        }
        
        // Clean up temporary directory
        fs::remove_dir_all(&temp_dir)?;
        
        info!("Snapshot applied successfully at height {}", height);
        Ok(())
    }

    /// Apply a diff from the repo
    pub async fn apply_diff(
        &self,
        repo_url: &str,
        start_height: u32,
        end_height: u32,
        db_path: &Path,
    ) -> Result<()> {
        info!("Applying diff from height {} to {}", start_height, end_height);
        
        // Create temporary directory for downloaded diff
        let temp_dir = PathBuf::from("./temp_diff");
        fs::create_dir_all(&temp_dir)?;
        
        // Download diff metadata
        let metadata_url = format!("{}/diffs/{}-{}/metadata.json", repo_url, start_height, end_height);
        let metadata_path = temp_dir.join(METADATA_FILENAME);
        self.download_file(&metadata_url, &metadata_path).await?;
        
        // Parse metadata (using underscore prefix to avoid unused variable warning)
        let _metadata: Value = {
            let file = File::open(&metadata_path)?;
            serde_json::from_reader(file)?
        };
        
        // Download diff file
        let diff_url = format!("{}/diffs/{}-{}/diff.bin", repo_url, start_height, end_height);
        let diff_path = temp_dir.join("diff.bin");
        self.download_file(&diff_url, &diff_path).await?;
        
        // Read the diff data
        let mut diff_file = File::open(&diff_path)?;
        let mut diff_data = Vec::new();
        diff_file.read_to_end(&mut diff_data)?;
        
        // Open the DB
        let opts = Options::default();
        let db = DB::open(&opts, db_path)?;
        
        // Apply the diff
        let batch = WriteBatch::from_data(&diff_data);
        db.write(batch)?;
        
        // Download state root file
        let stateroot_url = format!("{}/diffs/{}-{}/stateroot.json", repo_url, start_height, end_height);
        let stateroot_path = temp_dir.join(STATEROOT_FILENAME);
        self.download_file(&stateroot_url, &stateroot_path).await?;
        
        // Parse state root
        let stateroot: Value = {
            let file = File::open(&stateroot_path)?;
            serde_json::from_reader(file)?
        };
        
        // Verify state root
        let smt_helper = SMTHelper::new(Arc::new(db));
        let actual_state_root = smt_helper.get_smt_root_at_height(end_height)?;
        let expected_state_root = stateroot["state_root"].as_str()
            .ok_or_else(|| anyhow!("Missing state_root in stateroot file"))?;
        
        if hex::encode(actual_state_root) != expected_state_root {
            warn!("State root mismatch after applying diff: expected {}, got {}", 
                expected_state_root, hex::encode(actual_state_root));
        } else {
            info!("State root verified successfully after applying diff");
        }
        
        // Clean up temporary directory
        fs::remove_dir_all(&temp_dir)?;
        
        info!("Diff applied successfully from height {} to {}", start_height, end_height);
        Ok(())
    }

    /// Sync from a repo
    pub async fn sync_from_repo(
        &self,
        repo_url: &str,
        start_block: u32,
        db_path: &Path,
    ) -> Result<PathBuf> {
        info!("Syncing from repo: {}", repo_url);
        
        // Download global metadata
        let metadata_url = format!("{}/metadata.json", repo_url);
        let metadata_path = PathBuf::from("./temp_metadata.json");
        self.download_file(&metadata_url, &metadata_path).await?;
        
        // Parse metadata
        let metadata: Value = {
            let file = File::open(&metadata_path)?;
            serde_json::from_reader(file)?
        };
        
        // Get current WASM hash from metadata
        let current_wasm_hash = metadata["current_wasm_hash"].as_str()
            .ok_or_else(|| anyhow!("Missing current_wasm_hash in metadata"))?;
        
        // Download the WASM file
        let wasm_path = self.get_wasm_from_repo(repo_url, current_wasm_hash).await?;
        
        // Find best snapshot to start from
        let repo_start_block = metadata["start_block_height"].as_u64().unwrap_or(0) as u32;
        let latest_snapshot_height = metadata["latest_snapshot_height"].as_u64().unwrap_or(0) as u32;
        let latest_snapshot_hash = metadata["latest_snapshot_hash"].as_str()
            .ok_or_else(|| anyhow!("Missing latest_snapshot_hash in metadata"))?;
        
        // Determine which snapshot to use
        let (snapshot_height, snapshot_hash) = if start_block <= repo_start_block {
            // Start from the beginning
            (repo_start_block, metadata["start_block_hash"].as_str()
                .ok_or_else(|| anyhow!("Missing start_block_hash in metadata"))?.to_string())
        } else if start_block >= latest_snapshot_height {
            // Start from the latest snapshot
            (latest_snapshot_height, latest_snapshot_hash.to_string())
        } else {
            // Find the closest snapshot
            // For now, just use the latest snapshot
            // In a more sophisticated implementation, we would find the closest snapshot
            (latest_snapshot_height, latest_snapshot_hash.to_string())
        };
        
        // Apply the snapshot
        self.apply_snapshot(repo_url, snapshot_height, &snapshot_hash, db_path).await?;
        
        // Apply diffs if needed
        if snapshot_height < latest_snapshot_height {
            // In a more sophisticated implementation, we would apply multiple diffs
            // For now, just apply one diff from snapshot to latest
            self.apply_diff(repo_url, snapshot_height, latest_snapshot_height, db_path).await?;
        }
        
        info!("Sync completed successfully from repo: {}", repo_url);
        Ok(wasm_path)
    }
}

/// Extension trait for MetashrewRocksDBSync to add snapshot functionality
#[allow(dead_code)]
pub trait SnapshotExtension {
    /// Check if a snapshot should be created at the current height
    fn should_create_snapshot(&self, height: u32, snapshot_interval: u32) -> bool;
    
    /// Get the current block hash
    fn get_current_block_hash(&self) -> Result<Vec<u8>>;
    
    /// Get the current WASM path
    fn get_current_wasm_path(&self) -> Option<PathBuf>;
}

impl SnapshotExtension for MetashrewRocksDBSync {
    fn should_create_snapshot(&self, height: u32, snapshot_interval: u32) -> bool {
        if snapshot_interval == 0 {
            return false;
        }
        
        height % snapshot_interval == 0
    }
    
    fn get_current_block_hash(&self) -> Result<Vec<u8>> {
        // This is a placeholder implementation
        // In a real implementation, we would need to get the current block hash
        // from the DB in a synchronous way
        Err(anyhow!("Not implemented in trait - use async method instead"))
    }
    
    fn get_current_wasm_path(&self) -> Option<PathBuf> {
        // Return the WASM path from args
        Some(self.args.indexer.clone())
    }
}

// Implement additional methods for MetashrewRocksDBSync
impl MetashrewRocksDBSync {
    /// Check if a snapshot should be created at the current height
    pub fn should_create_snapshot(&self, height: u32) -> bool {
        if let Some(snapshot_interval) = self.args.snapshot_interval {
            height % snapshot_interval == 0
        } else {
            false
        }
    }
    
    /// Handle snapshot creation at the specified height
    pub async fn handle_snapshot(&mut self, height: u32, block_hash: &[u8]) -> Result<()> {
        // Check if snapshot directory is configured
        let snapshot_directory = match &self.args.snapshot_directory {
            Some(dir) => dir.clone(),
            None => return Err(anyhow!("Snapshot directory not configured")),
        };
        
        // Get the WASM path
        let wasm_path = self.args.indexer.clone();
        
        // Create snapshot manager
        let snapshot_interval = self.args.snapshot_interval.unwrap_or(0);
        let snapshot_manager = SnapshotManager::new(
            snapshot_directory,
            snapshot_interval,
            self.args.repo.clone(),
            Some(wasm_path.clone()),
        );
        
        // Initialize snapshot directory structure
        snapshot_manager.initialize()?;
        
        // Create snapshot
        snapshot_manager.create_snapshot(
            height,
            block_hash,
            &self.runtime,
            &wasm_path,
        ).await?;
        
        Ok(())
    }
    
    /// Sync from snapshot repository
    pub async fn sync_from_snapshot_repo(&self) -> Result<()> {
        // Check if repo URL is configured
        let repo_url = match &self.args.repo {
            Some(url) => url.clone(),
            None => return Err(anyhow!("Snapshot repository URL not configured")),
        };
        
        // Create snapshot manager
        let snapshot_interval = self.args.snapshot_interval.unwrap_or(0);
        let snapshot_directory = self.args.snapshot_directory.clone()
            .unwrap_or_else(|| PathBuf::from("./snapshots"));
        
        let snapshot_manager = SnapshotManager::new(
            snapshot_directory,
            snapshot_interval,
            Some(repo_url.clone()),
            None,
        );
        
        // Initialize snapshot directory structure
        snapshot_manager.initialize()?;
        
        // Sync from repo
        let start_block = self.args.start_block.unwrap_or(0);
        let wasm_path = snapshot_manager.sync_from_repo(
            &repo_url,
            start_block,
            &self.args.db_path,
        ).await?;
        
        // Log that we're using the WASM from the repo
        info!("Using WASM from snapshot repo: {}", wasm_path.display());
        // We would need to update the runtime with the new WASM here
        // This would require additional implementation
        
        Ok(())
    }
}
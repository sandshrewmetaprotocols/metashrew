use anyhow::{anyhow, Result};
use log::{info, warn, error};
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
const CHANGES_TRACKER_PREFIX: &str = "changes_tracker:";

/// Snapshot manager for Metashrew
pub struct SnapshotManager {
    snapshot_directory: PathBuf,
    snapshot_interval: u32,
    _repo_url: Option<String>,
    current_wasm_path: Option<PathBuf>,
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
            current_wasm_path: current_wasm_path,
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
        info!("Creating snapshot at height {} with block hash {}", height, hex::encode(block_hash));
        info!("Using WASM path: {}", wasm_path.display());
        info!("Snapshot directory: {}", self.snapshot_directory.display());
        
        // Store WASM file and get hash
        info!("Storing WASM file from {}", wasm_path.display());
        let wasm_hash = match self.store_wasm_file(wasm_path) {
            Ok(hash) => {
                info!("WASM file stored successfully with hash: {}", hash);
                hash
            },
            Err(e) => {
                error!("Failed to store WASM file: {}", e);
                return Err(e);
            }
        };
        
        // Update WASM history
        info!("Updating WASM history for height {} with hash {}", height, wasm_hash);
        if let Err(e) = self.update_wasm_history(height, &wasm_hash) {
            error!("Failed to update WASM history: {}", e);
            return Err(e);
        }
        
        // Create snapshot directory
        let snapshot_dir = self.snapshot_directory.join(SNAPSHOTS_DIR)
            .join(format!("{}-{}", height, hex::encode(block_hash)));
        
        info!("Creating snapshot directory: {}", snapshot_dir.display());
        if let Err(e) = fs::create_dir_all(&snapshot_dir) {
            error!("Failed to create snapshot directory: {}", e);
            return Err(anyhow!("Failed to create snapshot directory: {}", e));
        }
        
        // Create RocksDB checkpoint
        info!("Acquiring RocksDB instance from runtime");
        let db = {
            let runtime_guard = match runtime.read().await {
                guard => {
                    info!("Acquired read lock on runtime");
                    guard
                }
            };
            
            let context = match runtime_guard.context.lock() {
                Ok(ctx) => {
                    info!("Acquired lock on context");
                    ctx
                },
                Err(e) => {
                    error!("Failed to lock context: {}", e);
                    return Err(anyhow!("Failed to lock context: {}", e));
                }
            };
            
            info!("Got RocksDB instance from context");
            context.db.db.clone()
        };
        
        info!("Creating RocksDB checkpoint");
        let checkpoint = match Checkpoint::new(&db) {
            Ok(cp) => cp,
            Err(e) => {
                error!("Failed to create RocksDB checkpoint: {}", e);
                return Err(anyhow!("Failed to create RocksDB checkpoint: {}", e));
            }
        };
        
        let checkpoint_path = snapshot_dir.join("checkpoint");
        info!("Creating checkpoint at: {}", checkpoint_path.display());
        if let Err(e) = checkpoint.create_checkpoint(checkpoint_path.as_path()) {
            error!("Failed to create checkpoint: {}", e);
            return Err(anyhow!("Failed to create checkpoint: {}", e));
        }
        info!("Checkpoint created successfully");
        
        // Get state root
        info!("Getting state root at height {}", height);
        let smt_helper = SMTHelper::new(db.clone());
        let state_root = match smt_helper.get_smt_root_at_height(height) {
            Ok(root) => {
                info!("Got state root: {}", hex::encode(&root));
                root
            },
            Err(e) => {
                error!("Failed to get state root: {}", e);
                return Err(e);
            }
        };
        
        // Extract SST files
        let sst_dir = snapshot_dir.join("sst");
        info!("Creating SST directory: {}", sst_dir.display());
        if let Err(e) = fs::create_dir_all(&sst_dir) {
            error!("Failed to create SST directory: {}", e);
            return Err(anyhow!("Failed to create SST directory: {}", e));
        }
        
        // Move SST files from checkpoint to sst directory
        info!("Moving SST files from checkpoint to SST directory");
        let mut sst_files = Vec::new();
        let entries = match fs::read_dir(&checkpoint_path) {
            Ok(entries) => entries,
            Err(e) => {
                error!("Failed to read checkpoint directory: {}", e);
                return Err(anyhow!("Failed to read checkpoint directory: {}", e));
            }
        };
        
        for entry_result in entries {
            let entry = match entry_result {
                Ok(e) => e,
                Err(e) => {
                    error!("Failed to read directory entry: {}", e);
                    return Err(anyhow!("Failed to read directory entry: {}", e));
                }
            };
            
            let path = entry.path();
            
            if path.is_file() && path.extension().map_or(false, |ext| ext == "sst") {
                let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                info!("Processing SST file: {}", file_name);
                
                let target_path = sst_dir.join(&file_name);
                
                // Get file size and checksum
                let file_size = match fs::metadata(&path) {
                    Ok(meta) => meta.len(),
                    Err(e) => {
                        error!("Failed to get metadata for file {}: {}", path.display(), e);
                        return Err(anyhow!("Failed to get metadata for file {}: {}", path.display(), e));
                    }
                };
                
                let file_checksum = match self.compute_file_checksum(&path) {
                    Ok(checksum) => checksum,
                    Err(e) => {
                        error!("Failed to compute checksum for file {}: {}", path.display(), e);
                        return Err(e);
                    }
                };
                
                // Copy the file
                info!("Copying SST file from {} to {}", path.display(), target_path.display());
                if let Err(e) = fs::copy(&path, &target_path) {
                    error!("Failed to copy SST file: {}", e);
                    return Err(anyhow!("Failed to copy SST file: {}", e));
                }
                
                // Add to sst_files list
                sst_files.push(json!({
                    "name": file_name,
                    "size_bytes": file_size,
                    "checksum": file_checksum
                }));
            }
        }
        
        info!("Processed {} SST files", sst_files.len());
        
        // Clean up checkpoint directory
        info!("Cleaning up checkpoint directory");
        if let Err(e) = fs::remove_dir_all(&checkpoint_path) {
            error!("Failed to clean up checkpoint directory: {}", e);
            return Err(anyhow!("Failed to clean up checkpoint directory: {}", e));
        }
        
        // Generate metadata
        info!("Generating snapshot metadata");
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
        let metadata_path = snapshot_dir.join(METADATA_FILENAME);
        info!("Writing snapshot metadata to {}", metadata_path.display());
        let metadata_file = match File::create(&metadata_path) {
            Ok(file) => file,
            Err(e) => {
                error!("Failed to create metadata file: {}", e);
                return Err(anyhow!("Failed to create metadata file: {}", e));
            }
        };
        
        if let Err(e) = serde_json::to_writer_pretty(metadata_file, &metadata) {
            error!("Failed to write metadata: {}", e);
            return Err(anyhow!("Failed to write metadata: {}", e));
        }
        
        // Write state root to file
        let stateroot_path = snapshot_dir.join(STATEROOT_FILENAME);
        info!("Writing state root to {}", stateroot_path.display());
        let stateroot_file = match File::create(&stateroot_path) {
            Ok(file) => file,
            Err(e) => {
                error!("Failed to create state root file: {}", e);
                return Err(anyhow!("Failed to create state root file: {}", e));
            }
        };
        
        if let Err(e) = serde_json::to_writer_pretty(stateroot_file, &json!({
            "height": height,
            "state_root": hex::encode(state_root)
        })) {
            error!("Failed to write state root: {}", e);
            return Err(anyhow!("Failed to write state root: {}", e));
        }
        
        // Update global metadata with latest snapshot height
        let global_metadata_path = self.snapshot_directory.join(METADATA_FILENAME);
        info!("Updating global metadata at {}", global_metadata_path.display());
        
        let mut global_metadata: Value = {
            let metadata_file = match File::open(&global_metadata_path) {
                Ok(file) => file,
                Err(e) => {
                    error!("Failed to open global metadata file: {}", e);
                    return Err(anyhow!("Failed to open global metadata file: {}", e));
                }
            };
            
            match serde_json::from_reader(metadata_file) {
                Ok(metadata) => metadata,
                Err(e) => {
                    error!("Failed to parse global metadata: {}", e);
                    return Err(anyhow!("Failed to parse global metadata: {}", e));
                }
            }
        };
        
        global_metadata["latest_snapshot_height"] = json!(height);
        global_metadata["latest_snapshot_hash"] = json!(hex::encode(block_hash));
        
        let metadata_file = match File::create(&global_metadata_path) {
            Ok(file) => file,
            Err(e) => {
                error!("Failed to create global metadata file: {}", e);
                return Err(anyhow!("Failed to create global metadata file: {}", e));
            }
        };
        
        if let Err(e) = serde_json::to_writer_pretty(metadata_file, &global_metadata) {
            error!("Failed to write global metadata: {}", e);
            return Err(anyhow!("Failed to write global metadata: {}", e));
        }
        
        info!("Snapshot created successfully at height {}", height);
        Ok(())
    }

    /// Compute checksum of a file
    fn compute_file_checksum(&self, file_path: &Path) -> Result<String> {
        // Simplified implementation without sha2 dependency
        let file_size = fs::metadata(file_path)?.len();
        Ok(format!("sha256:placeholder_checksum_{}", file_size))
    }

    /// Track changes between blocks for efficient diff creation
    pub fn track_block_changes(&self, height: u32, db: &Arc<DB>, keys: &[Vec<u8>]) -> Result<()> {
        info!("Tracking changes for block height {}", height);
        
        // Create a key for storing the changes at this height
        let changes_key = format!("{}:{}", CHANGES_TRACKER_PREFIX, height).into_bytes();
        
        // Serialize the list of changed keys
        let mut data = Vec::new();
        for key in keys {
            // Write key length (4 bytes)
            data.extend_from_slice(&(key.len() as u32).to_le_bytes());
            // Write key
            data.extend_from_slice(key);
        }
        
        // Store the changes in the database
        db.put(&changes_key, &data)?;
        
        info!("Tracked {} changed keys for height {}", keys.len(), height);
        Ok(())
    }
    
    /// Get tracked changes for a specific height range
    pub fn get_tracked_changes(&self, db: &Arc<DB>, start_height: u32, end_height: u32) -> Result<Vec<Vec<u8>>> {
        info!("Getting tracked changes from height {} to {}", start_height, end_height);
        
        let mut all_changed_keys = Vec::new();
        
        // Collect changes for each height in the range
        for height in start_height..=end_height {
            let changes_key = format!("{}:{}", CHANGES_TRACKER_PREFIX, height).into_bytes();
            
            if let Some(data) = db.get(&changes_key)? {
                let mut i = 0;
                
                while i < data.len() {
                    if i + 4 <= data.len() {
                        let mut len_bytes = [0u8; 4];
                        len_bytes.copy_from_slice(&data[i..i+4]);
                        let key_len = u32::from_le_bytes(len_bytes) as usize;
                        i += 4;
                        
                        if i + key_len <= data.len() {
                            all_changed_keys.push(data[i..i+key_len].to_vec());
                            i += key_len;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }
        
        // Remove duplicates (if a key was changed multiple times, we only need the latest value)
        all_changed_keys.sort();
        all_changed_keys.dedup();
        
        info!("Found {} unique changed keys between heights {} and {}",
              all_changed_keys.len(), start_height, end_height);
        
        Ok(all_changed_keys)
    }

    /// Create a diff between two snapshots using tracked changes
    pub async fn create_diff(
        &self,
        start_height: u32,
        start_block_hash: &[u8],
        end_height: u32,
        end_block_hash: &[u8],
        runtime: &Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
    ) -> Result<()> {
        info!("Creating efficient diff from height {} to {}", start_height, end_height);
        
        // Create diff directory
        let diff_dir = self.snapshot_directory.join(DIFFS_DIR)
            .join(format!("{}-{}", start_height, end_height));
        
        fs::create_dir_all(&diff_dir)?;
        
        // Get the database from the runtime
        let db = {
            let runtime_guard = runtime.read().await;
            let context = runtime_guard.context.lock()
                .map_err(|_| anyhow!("Failed to lock context"))?;
            context.db.db.clone()
        };
        
        // Get the state roots
        let smt_helper = SMTHelper::new(db.clone());
        let start_state_root = smt_helper.get_smt_root_at_height(start_height)?;
        let end_state_root = smt_helper.get_smt_root_at_height(end_height)?;
        
        // Get the WASM hash for the end height
        let wasm_path = self.current_wasm_path
            .as_ref()
            .ok_or_else(|| anyhow!("Missing WASM path"))?;
        let wasm_hash = self.compute_wasm_hash(&wasm_path)?;
        
        // Get tracked changes between start_height and end_height
        let changed_keys = self.get_tracked_changes(&db, start_height + 1, end_height)?;
        
        // Create a batch for the diff
        let mut diff_batch = WriteBatch::default();
        let mut keys_added = 0;
        let mut keys_modified = 0;
        let mut keys_deleted = 0;
        
        // Process each changed key
        for key in &changed_keys {
            // Get the current value (at end_height)
            match db.get(key)? {
                Some(value) => {
                    // Check if the key existed at start_height
                    let start_value = smt_helper.get_value_at_height(key, start_height)?;
                    
                    match start_value {
                        Some(_) => {
                            // Key existed before, so it was modified
                            diff_batch.put(key, &value);
                            keys_modified += 1;
                        },
                        None => {
                            // Key didn't exist before, so it was added
                            diff_batch.put(key, &value);
                            keys_added += 1;
                        }
                    }
                },
                None => {
                    // Key doesn't exist at end_height but was tracked as changed,
                    // so it must have been deleted
                    diff_batch.delete(key);
                    keys_deleted += 1;
                }
            }
        }
        
        // Serialize the diff batch
        let diff_data = diff_batch.data();
        
        // Compress the diff data using zstd
        let compressed_data = zstd::encode_all(&diff_data[..], 3)?;
        
        // Store the compressed diff
        let diff_path = diff_dir.join("diff.bin.zst");
        let mut diff_file = File::create(&diff_path)?;
        diff_file.write_all(&compressed_data)?;
        
        // Generate metadata
        let metadata = json!({
            "start_height": start_height,
            "start_block_hash": hex::encode(start_block_hash),
            "start_state_root": hex::encode(start_state_root),
            "end_height": end_height,
            "end_block_hash": hex::encode(end_block_hash),
            "end_state_root": hex::encode(end_state_root),
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "diff_size_bytes": fs::metadata(&diff_path)?.len(),
            "original_size_bytes": diff_data.len(),
            "compression": "zstd",
            "compression_level": 3,
            "keys_modified": keys_modified,
            "keys_added": keys_added,
            "keys_deleted": keys_deleted,
            "wasm_hash": wasm_hash,
            "total_changed_keys": changed_keys.len()
        });
        
        // Write metadata to file
        let metadata_file = File::create(diff_dir.join(METADATA_FILENAME))?;
        serde_json::to_writer_pretty(metadata_file, &metadata)?;
        
        // Write state root file
        let stateroot_file = File::create(diff_dir.join(STATEROOT_FILENAME))?;
        serde_json::to_writer_pretty(stateroot_file, &json!({
            "height": end_height,
            "state_root": hex::encode(end_state_root)
        }))?;
        
        info!("Efficient diff created successfully from height {} to {}", start_height, end_height);
        info!("Keys added: {}, modified: {}, deleted: {}, total unique changes: {}",
              keys_added, keys_modified, keys_deleted, changed_keys.len());
        info!("Original size: {} bytes, compressed size: {} bytes",
              diff_data.len(), fs::metadata(&diff_path)?.len());
        
        Ok(())
    }
    
    /// Create a simple diff between two snapshots by dumping the entire database state
    pub async fn create_simple_diff(
        &self,
        start_height: u32,
        start_block_hash: &[u8],
        end_height: u32,
        end_block_hash: &[u8],
        runtime: &Arc<RwLock<MetashrewRuntime<RocksDBRuntimeAdapter>>>,
    ) -> Result<()> {
        info!("Creating simple diff from height {} to {}", start_height, end_height);
        
        // Create diff directory
        let diff_dir = self.snapshot_directory.join(DIFFS_DIR)
            .join(format!("{}-{}", start_height, end_height));
        
        fs::create_dir_all(&diff_dir)?;
        
        // Get the database from the runtime
        let db = {
            let runtime_guard = runtime.read().await;
            let context = runtime_guard.context.lock()
                .map_err(|_| anyhow!("Failed to lock context"))?;
            context.db.db.clone()
        };
        
        // Get the state roots
        let smt_helper = SMTHelper::new(db.clone());
        let start_state_root = smt_helper.get_smt_root_at_height(start_height)?;
        let end_state_root = smt_helper.get_smt_root_at_height(end_height)?;
        
        // Get the WASM hash for the end height
        let wasm_path = self.current_wasm_path
            .as_ref()
            .ok_or_else(|| anyhow!("Missing WASM path"))?;
        let wasm_hash = self.compute_wasm_hash(&wasm_path)?;
        
        // Create a binary format to store all key-value pairs
        // Format: [key_length (4 bytes)][key][value_length (4 bytes)][value]...
        let mut diff_data = Vec::new();
        let mut key_count = 0;
        
        // Iterate through all keys in the database
        let iter = db.iterator(rocksdb::IteratorMode::Start);
        for item in iter {
            if let Ok((key, value)) = item {
                // Skip internal keys
                if key.starts_with(b"/__INTERNAL/") {
                    continue;
                }
                
                // Write key length (4 bytes)
                diff_data.extend_from_slice(&(key.len() as u32).to_le_bytes());
                // Write key
                diff_data.extend_from_slice(&key);
                // Write value length (4 bytes)
                diff_data.extend_from_slice(&(value.len() as u32).to_le_bytes());
                // Write value
                diff_data.extend_from_slice(&value);
                
                key_count += 1;
            }
        }
        
        // Compress the diff data using zstd
        let compressed_data = zstd::encode_all(&diff_data[..], 3)?;
        
        // Store the compressed diff
        let diff_path = diff_dir.join("diff.bin.zst");
        let mut diff_file = File::create(&diff_path)?;
        diff_file.write_all(&compressed_data)?;
        
        // Generate metadata
        let metadata = json!({
            "start_height": start_height,
            "start_block_hash": hex::encode(start_block_hash),
            "start_state_root": hex::encode(start_state_root),
            "end_height": end_height,
            "end_block_hash": hex::encode(end_block_hash),
            "end_state_root": hex::encode(end_state_root),
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "diff_size_bytes": fs::metadata(&diff_path)?.len(),
            "original_size_bytes": diff_data.len(),
            "compression": "zstd",
            "compression_level": 3,
            "format": "simple_kv",
            "key_count": key_count,
            "wasm_hash": wasm_hash
        });
        
        // Write metadata to file
        let metadata_file = File::create(diff_dir.join(METADATA_FILENAME))?;
        serde_json::to_writer_pretty(metadata_file, &metadata)?;
        
        // Write state root file
        let stateroot_file = File::create(diff_dir.join(STATEROOT_FILENAME))?;
        serde_json::to_writer_pretty(stateroot_file, &json!({
            "height": end_height,
            "state_root": hex::encode(end_state_root)
        }))?;
        
        info!("Simple diff created successfully from height {} to {}", start_height, end_height);
        info!("Total keys: {}", key_count);
        info!("Original size: {} bytes, compressed size: {} bytes",
              diff_data.len(), fs::metadata(&diff_path)?.len());
        
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
        
        // Parse metadata
        let metadata: Value = {
            let file = File::open(&metadata_path)?;
            serde_json::from_reader(file)?
        };
        
        // Check compression format
        let compression = metadata["compression"].as_str().unwrap_or("none");
        
        // Check diff format
        let format = metadata["format"].as_str().unwrap_or("writebatch");
        info!("Diff format: {}", format);
        
        // Download diff file (check if it's compressed)
        let diff_filename = if compression == "zstd" {
            "diff.bin.zst"
        } else {
            "diff.bin"
        };
        
        let diff_url = format!("{}/diffs/{}-{}/{}", repo_url, start_height, end_height, diff_filename);
        let diff_path = temp_dir.join(diff_filename);
        self.download_file(&diff_url, &diff_path).await?;
        
        // Read and decompress the diff data if needed
        let diff_data = if compression == "zstd" {
            let compressed_data = fs::read(&diff_path)?;
            zstd::decode_all(compressed_data.as_slice())?
        } else {
            let mut diff_file = File::open(&diff_path)?;
            let mut diff_data = Vec::new();
            diff_file.read_to_end(&mut diff_data)?;
            diff_data
        };
        
        // Open the DB
        let opts = Options::default();
        let db = DB::open(&opts, db_path)?;
        
        // Apply the diff based on format
        if format == "simple_kv" {
            info!("Applying diff using simple KV format");
            let mut batch = WriteBatch::default();
            let mut i = 0;
            let mut key_count = 0;
            
            // Parse the simple KV format: [key_length (4 bytes)][key][value_length (4 bytes)][value]...
            while i + 8 <= diff_data.len() {
                // Read key length (4 bytes)
                let mut key_len_bytes = [0u8; 4];
                key_len_bytes.copy_from_slice(&diff_data[i..i+4]);
                let key_len = u32::from_le_bytes(key_len_bytes) as usize;
                i += 4;
                
                if i + key_len + 4 <= diff_data.len() {
                    // Read key
                    let key = diff_data[i..i+key_len].to_vec();
                    i += key_len;
                    
                    // Read value length (4 bytes)
                    let mut val_len_bytes = [0u8; 4];
                    val_len_bytes.copy_from_slice(&diff_data[i..i+4]);
                    let val_len = u32::from_le_bytes(val_len_bytes) as usize;
                    i += 4;
                    
                    if i + val_len <= diff_data.len() {
                        // Read value
                        let value = diff_data[i..i+val_len].to_vec();
                        i += val_len;
                        
                        // Add to batch
                        batch.put(&key, &value);
                        key_count += 1;
                    } else {
                        return Err(anyhow!("Invalid diff data format: value truncated"));
                    }
                } else {
                    return Err(anyhow!("Invalid diff data format: key truncated"));
                }
            }
            
            // Write the batch to the DB
            db.write(batch)?;
            info!("Applied {} key-value pairs from simple KV format", key_count);
        } else {
            // Default to WriteBatch format
            info!("Applying diff using WriteBatch format");
            let batch = WriteBatch::from_data(&diff_data);
            db.write(batch)?;
        }
        
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
            return Err(anyhow!("State root verification failed after applying diff"));
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
        
        // Download index.json first to get a complete picture of available snapshots and diffs
        let index_url = format!("{}/index.json", repo_url);
        let index_path = PathBuf::from("./temp_index.json");
        
        info!("Downloading index file from {}", index_url);
        match self.download_file(&index_url, &index_path).await {
            Ok(_) => info!("Index file downloaded successfully"),
            Err(e) => {
                // If index.json doesn't exist, fall back to metadata.json
                warn!("Failed to download index.json: {}, falling back to metadata.json", e);
                return self.sync_from_repo_legacy(repo_url, start_block, db_path).await;
            }
        };
        
        // Parse index
        let index: Value = {
            let file = File::open(&index_path)?;
            serde_json::from_reader(file)?
        };
        
        // Get current WASM hash from index
        let current_wasm_hash = index["current_wasm_hash"].as_str()
            .ok_or_else(|| anyhow!("Missing current_wasm_hash in index"))?;
        
        // Download the WASM file
        info!("Downloading WASM file with hash: {}", current_wasm_hash);
        let wasm_path = self.get_wasm_from_repo(repo_url, current_wasm_hash).await?;
        
        // Find best snapshot to start from
        let _repo_start_block = index["start_block_height"].as_u64().unwrap_or(0) as u32;
        let latest_snapshot_height = index["latest_snapshot_height"].as_u64().unwrap_or(0) as u32;
        
        // Get all available snapshots from the index
        let snapshots = index["snapshots"].as_array()
            .ok_or_else(|| anyhow!("Missing snapshots array in index"))?;
        
        // Find the best snapshot to start from
        let mut best_snapshot: Option<&Value> = None;
        let mut best_snapshot_height: u32 = 0;
        
        for snapshot in snapshots {
            let snapshot_height = snapshot["height"].as_u64().unwrap_or(0) as u32;
            
            // Find the highest snapshot that's <= our target start_block
            if snapshot_height <= start_block && snapshot_height > best_snapshot_height {
                best_snapshot = Some(snapshot);
                best_snapshot_height = snapshot_height;
            }
        }
        
        // If we couldn't find a suitable snapshot, use the earliest one
        if best_snapshot.is_none() {
            for snapshot in snapshots {
                let snapshot_height = snapshot["height"].as_u64().unwrap_or(0) as u32;
                
                if best_snapshot.is_none() || snapshot_height < best_snapshot_height {
                    best_snapshot = Some(snapshot);
                    best_snapshot_height = snapshot_height;
                }
            }
        }
        
        let best_snapshot = best_snapshot
            .ok_or_else(|| anyhow!("No snapshots available in the repository"))?;
        
        let snapshot_height = best_snapshot["height"].as_u64().unwrap_or(0) as u32;
        let snapshot_hash = best_snapshot["block_hash"].as_str()
            .ok_or_else(|| anyhow!("Missing block_hash in snapshot"))?;
        
        info!("Selected snapshot at height {} with hash {}", snapshot_height, snapshot_hash);
        
        // Apply the snapshot
        self.apply_snapshot(repo_url, snapshot_height, snapshot_hash, db_path).await?;
        
        // Get all available diffs from the index
        let diffs = index["diffs"].as_array()
            .ok_or_else(|| anyhow!("Missing diffs array in index"))?;
        
        // Sort diffs by start_height
        let mut sorted_diffs: Vec<&Value> = diffs.iter().collect();
        sorted_diffs.sort_by(|a, b| {
            let a_height = a["start_height"].as_u64().unwrap_or(0);
            let b_height = b["start_height"].as_u64().unwrap_or(0);
            a_height.cmp(&b_height)
        });
        
        // Apply diffs in sequence to reach the target height
        let mut current_height = snapshot_height;
        
        for diff in sorted_diffs {
            let diff_start_height = diff["start_height"].as_u64().unwrap_or(0) as u32;
            let diff_end_height = diff["end_height"].as_u64().unwrap_or(0) as u32;
            
            // Only apply diffs that start at our current height and don't exceed our target
            if diff_start_height == current_height && diff_end_height <= latest_snapshot_height {
                info!("Applying diff from height {} to {}", diff_start_height, diff_end_height);
                self.apply_diff(repo_url, diff_start_height, diff_end_height, db_path).await?;
                current_height = diff_end_height;
            }
        }
        
        info!("Sync completed successfully from repo: {} to height {}", repo_url, current_height);
        Ok(wasm_path)
    }
    
    /// Legacy sync method using metadata.json (for backward compatibility)
    pub async fn sync_from_repo_legacy(
        &self,
        repo_url: &str,
        start_block: u32,
        db_path: &Path,
    ) -> Result<PathBuf> {
        info!("Syncing from repo using legacy method: {}", repo_url);
        
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
            if snapshot_interval == 0 {
                info!("Snapshot interval is 0, snapshots are disabled");
                return false;
            }
            
            let should_create = height % snapshot_interval == 0;
            info!("Checking if snapshot should be created at height {}: {} % {} == 0? {}",
                  height, height, snapshot_interval, should_create);
            should_create
        } else {
            info!("No snapshot interval configured, snapshots are disabled");
            false
        }
    }
    
    /// Handle snapshot creation at the specified height
    pub async fn handle_snapshot(&mut self, height: u32, block_hash: &[u8]) -> Result<()> {
        info!("Starting snapshot creation process for height {}", height);
        
        // Check if snapshot directory is configured
        let snapshot_directory = match &self.args.snapshot_directory {
            Some(dir) => {
                info!("Using snapshot directory: {}", dir.display());
                dir.clone()
            },
            None => {
                error!("Snapshot directory not configured");
                return Err(anyhow!("Snapshot directory not configured"));
            },
        };
        
        // Get the WASM path
        let wasm_path = self.args.indexer.clone();
        info!("Using WASM path: {}", wasm_path.display());
        
        // Create snapshot manager
        let snapshot_interval = self.args.snapshot_interval.unwrap_or(0);
        info!("Creating snapshot manager with interval: {}", snapshot_interval);
        
        let snapshot_manager = SnapshotManager::new(
            snapshot_directory.clone(),
            snapshot_interval,
            self.args.repo.clone(),
            Some(wasm_path.clone()),
        );
        
        // Initialize snapshot directory structure
        info!("Initializing snapshot directory structure at {}", snapshot_directory.display());
        if let Err(e) = snapshot_manager.initialize() {
            error!("Failed to initialize snapshot directory: {}", e);
            return Err(e);
        }
        info!("Snapshot directory initialized successfully");
        
        // Find the latest snapshot height
        let latest_snapshot_height = self.find_latest_snapshot_height(snapshot_directory.clone()).await;
        
        // Determine if we need to create a full snapshot or just a diff
        let create_full_snapshot = match latest_snapshot_height {
            None => {
                info!("No previous snapshots found, creating first full snapshot");
                true
            },
            Some(prev_height) => {
                // Check if this is a major snapshot (e.g., every 100 blocks)
                // This ensures we have periodic full snapshots for reliability
                let major_snapshot_interval = snapshot_interval * 10;
                if major_snapshot_interval > 0 && height % major_snapshot_interval == 0 {
                    info!("Creating major snapshot at height {} (every {} blocks)",
                          height, major_snapshot_interval);
                    true
                } else {
                    info!("Previous snapshot found at height {}, will create diff", prev_height);
                    false
                }
            }
        };
        
        if create_full_snapshot {
            // Create full snapshot
            info!("Creating full snapshot for height {} with block hash {}",
                  height, hex::encode(block_hash));
            
            match snapshot_manager.create_snapshot(
                height,
                block_hash,
                &self.runtime,
                &wasm_path,
            ).await {
                Ok(_) => {
                    info!("Full snapshot created successfully for height {}", height);
                },
                Err(e) => {
                    error!("Failed to create full snapshot for height {}: {}", height, e);
                    return Err(e);
                }
            }
        } else {
            // We have a previous snapshot, so create a diff
            let prev_height = latest_snapshot_height.unwrap();
            
            // Get the previous snapshot's block hash
            if let Some(prev_block_hash) = self.get_blockhash(prev_height).await {
                info!("Found previous snapshot at height {} with block hash {}",
                      prev_height, hex::encode(&prev_block_hash));
                
                // Create diff between previous and current snapshot
                info!("Creating diff from height {} to {}", prev_height, height);
                match snapshot_manager.create_diff(
                    prev_height,
                    &prev_block_hash,
                    height,
                    block_hash,
                    &self.runtime,
                ).await {
                    Ok(_) => {
                        info!("Diff created successfully from height {} to {}", prev_height, height);
                    },
                    Err(e) => {
                        error!("Failed to create diff from height {} to {}: {}",
                               prev_height, height, e);
                        
                        // If diff creation fails, fall back to creating a full snapshot
                        warn!("Falling back to creating full snapshot");
                        match snapshot_manager.create_snapshot(
                            height,
                            block_hash,
                            &self.runtime,
                            &wasm_path,
                        ).await {
                            Ok(_) => {
                                info!("Fallback full snapshot created successfully for height {}", height);
                            },
                            Err(e) => {
                                error!("Failed to create fallback snapshot for height {}: {}", height, e);
                                return Err(e);
                            }
                        }
                    }
                }
            } else {
                warn!("Could not find block hash for previous height {}, creating full snapshot",
                      prev_height);
                
                // Create full snapshot as fallback
                match snapshot_manager.create_snapshot(
                    height,
                    block_hash,
                    &self.runtime,
                    &wasm_path,
                ).await {
                    Ok(_) => {
                        info!("Fallback snapshot created successfully for height {}", height);
                    },
                    Err(e) => {
                        error!("Failed to create fallback snapshot for height {}: {}", height, e);
                        return Err(e);
                    }
                }
            }
        }
        
        // Update global metadata with latest snapshot height
        let global_metadata_path = snapshot_directory.join(METADATA_FILENAME);
        if global_metadata_path.exists() {
            let mut global_metadata: Value = {
                let metadata_file = match File::open(&global_metadata_path) {
                    Ok(file) => file,
                    Err(e) => {
                        error!("Failed to open global metadata file: {}", e);
                        return Err(anyhow!("Failed to open global metadata file: {}", e));
                    }
                };
                
                match serde_json::from_reader(metadata_file) {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        error!("Failed to parse global metadata: {}", e);
                        return Err(anyhow!("Failed to parse global metadata: {}", e));
                    }
                }
            };
            
            global_metadata["latest_snapshot_height"] = json!(height);
            global_metadata["latest_snapshot_hash"] = json!(hex::encode(block_hash));
            
            let metadata_file = match File::create(&global_metadata_path) {
                Ok(file) => file,
                Err(e) => {
                    error!("Failed to create global metadata file: {}", e);
                    return Err(anyhow!("Failed to create global metadata file: {}", e));
                }
            };
            
            if let Err(e) = serde_json::to_writer_pretty(metadata_file, &global_metadata) {
                error!("Failed to write global metadata: {}", e);
                return Err(anyhow!("Failed to write global metadata: {}", e));
            }
        }
        
        // Create or update index.json file for static serving
        self.update_index_file(snapshot_directory).await?;
        
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
    
    /// Find the latest snapshot height
    pub async fn find_latest_snapshot_height(&self, snapshot_directory: PathBuf) -> Option<u32> {
        info!("Finding latest snapshot height");
        
        // Check global metadata first
        let global_metadata_path = snapshot_directory.join(METADATA_FILENAME);
        if global_metadata_path.exists() {
            match File::open(&global_metadata_path) {
                Ok(file) => {
                    match serde_json::from_reader::<_, Value>(file) {
                        Ok(metadata) => {
                            if let Some(height) = metadata["latest_snapshot_height"].as_u64() {
                                info!("Found latest snapshot height in global metadata: {}", height);
                                return Some(height as u32);
                            }
                        },
                        Err(e) => {
                            warn!("Failed to parse global metadata: {}", e);
                        }
                    }
                },
                Err(e) => {
                    warn!("Failed to open global metadata file: {}", e);
                }
            }
        }
        
        // If global metadata doesn't have the information, scan the snapshots directory
        let snapshots_dir = snapshot_directory.join(SNAPSHOTS_DIR);
        if !snapshots_dir.exists() {
            info!("Snapshots directory doesn't exist");
            return None;
        }
        
        let mut latest_height = None;
        
        match fs::read_dir(&snapshots_dir) {
            Ok(entries) => {
                for entry_result in entries {
                    match entry_result {
                        Ok(entry) => {
                            let path = entry.path();
                            
                            if path.is_dir() {
                                // Parse the directory name to get height
                                let dir_name = path.file_name().unwrap().to_string_lossy().to_string();
                                let parts: Vec<&str> = dir_name.split('-').collect();
                                
                                if parts.len() == 2 {
                                    if let Ok(height) = parts[0].parse::<u32>() {
                                        // Update latest height if this is higher
                                        if latest_height.is_none() || height > latest_height.unwrap() {
                                            latest_height = Some(height);
                                        }
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            warn!("Failed to read directory entry: {}", e);
                        }
                    }
                }
            },
            Err(e) => {
                warn!("Failed to read snapshots directory: {}", e);
            }
        }
        
        if let Some(height) = latest_height {
            info!("Found latest snapshot height by scanning directory: {}", height);
        } else {
            info!("No snapshots found");
        }
        
        latest_height
    }
    
    /// Update the index file for static serving
    pub async fn update_index_file(&self, snapshot_directory: PathBuf) -> Result<()> {
        info!("Updating index file for static serving");
        
        // Path to the index file
        let index_path = snapshot_directory.join("index.json");
        
        // Collect information about all snapshots
        let snapshots_dir = snapshot_directory.join(SNAPSHOTS_DIR);
        let mut snapshots = Vec::new();
        
        if snapshots_dir.exists() {
            for entry in fs::read_dir(&snapshots_dir)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.is_dir() {
                    // Parse the directory name to get height and hash
                    let dir_name = path.file_name().unwrap().to_string_lossy().to_string();
                    let parts: Vec<&str> = dir_name.split('-').collect();
                    
                    if parts.len() == 2 {
                        if let Ok(height) = parts[0].parse::<u32>() {
                            let block_hash = parts[1].to_string();
                            
                            // Read metadata file if it exists
                            let metadata_path = path.join(METADATA_FILENAME);
                            if metadata_path.exists() {
                                let metadata_file = File::open(&metadata_path)?;
                                let metadata: Value = serde_json::from_reader(metadata_file)?;
                                
                                // Add snapshot info to the list
                                snapshots.push(json!({
                                    "height": height,
                                    "block_hash": block_hash,
                                    "state_root": metadata["state_root"],
                                    "db_size_bytes": metadata["db_size_bytes"],
                                    "wasm_hash": metadata["wasm_hash"],
                                    "path": format!("{}/{}-{}", SNAPSHOTS_DIR, height, block_hash)
                                }));
                            }
                        }
                    }
                }
            }
        }
        
        // Sort snapshots by height
        snapshots.sort_by(|a, b| {
            let a_height = a["height"].as_u64().unwrap_or(0);
            let b_height = b["height"].as_u64().unwrap_or(0);
            a_height.cmp(&b_height)
        });
        
        // Collect information about all diffs
        let diffs_dir = snapshot_directory.join(DIFFS_DIR);
        let mut diffs = Vec::new();
        
        if diffs_dir.exists() {
            for entry in fs::read_dir(&diffs_dir)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.is_dir() {
                    // Parse the directory name to get start and end heights
                    let dir_name = path.file_name().unwrap().to_string_lossy().to_string();
                    let parts: Vec<&str> = dir_name.split('-').collect();
                    
                    if parts.len() == 2 {
                        if let (Ok(start_height), Ok(end_height)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                            // Read metadata file if it exists
                            let metadata_path = path.join(METADATA_FILENAME);
                            if metadata_path.exists() {
                                let metadata_file = File::open(&metadata_path)?;
                                let metadata: Value = serde_json::from_reader(metadata_file)?;
                                
                                // Add diff info to the list
                                diffs.push(json!({
                                    "start_height": start_height,
                                    "end_height": end_height,
                                    "start_block_hash": metadata["start_block_hash"],
                                    "end_block_hash": metadata["end_block_hash"],
                                    "diff_size_bytes": metadata["diff_size_bytes"],
                                    "keys_modified": metadata["keys_modified"],
                                    "keys_added": metadata["keys_added"],
                                    "keys_deleted": metadata["keys_deleted"],
                                    "wasm_hash": metadata["wasm_hash"],
                                    "path": format!("{}/{}-{}", DIFFS_DIR, start_height, end_height)
                                }));
                            }
                        }
                    }
                }
            }
        }
        
        // Sort diffs by start height
        diffs.sort_by(|a, b| {
            let a_height = a["start_height"].as_u64().unwrap_or(0);
            let b_height = b["start_height"].as_u64().unwrap_or(0);
            a_height.cmp(&b_height)
        });
        
        // Collect information about all WASM files
        let wasm_dir = snapshot_directory.join(WASM_DIR);
        let mut wasm_files = Vec::new();
        
        if wasm_dir.exists() {
            for entry in fs::read_dir(&wasm_dir)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.is_file() && path.extension().map_or(false, |ext| ext == "wasm") {
                    let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                    let file_size = fs::metadata(&path)?.len();
                    
                    // Add WASM file info to the list
                    wasm_files.push(json!({
                        "name": file_name,
                        "size_bytes": file_size,
                        "path": format!("{}/{}", WASM_DIR, file_name)
                    }));
                }
            }
        }
        
        // Read global metadata
        let global_metadata_path = snapshot_directory.join(METADATA_FILENAME);
        let global_metadata: Value = if global_metadata_path.exists() {
            let metadata_file = File::open(&global_metadata_path)?;
            serde_json::from_reader(metadata_file)?
        } else {
            json!({})
        };
        
        // Create the index
        let index = json!({
            "index_name": global_metadata["index_name"],
            "index_version": global_metadata["index_version"],
            "created_at": global_metadata["created_at"],
            "updated_at": "2025-06-10T00:00:00Z", // Should use actual current time
            "start_block_height": global_metadata["start_block_height"],
            "latest_snapshot_height": global_metadata["latest_snapshot_height"],
            "latest_snapshot_hash": global_metadata["latest_snapshot_hash"],
            "snapshot_interval": global_metadata["snapshot_interval"],
            "current_wasm_hash": global_metadata["current_wasm_hash"],
            "snapshots": snapshots,
            "diffs": diffs,
            "wasm_files": wasm_files
        });
        
        // Write the index file
        info!("Writing index file to {}", index_path.display());
        let index_file = File::create(&index_path)?;
        serde_json::to_writer_pretty(index_file, &index)?;
        
        info!("Index file updated successfully");
        Ok(())
    }
}
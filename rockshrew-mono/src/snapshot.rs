use anyhow::{anyhow, Result};
use log::info;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs as async_fs;
use zstd;

/// Represents a snapshot interval configuration
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    pub interval: u32,
    pub directory: PathBuf,
    pub enabled: bool,
}

/// Represents the state root at a specific height
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateRoot {
    pub height: u32,
    pub root: String, // hex-encoded hash
    pub timestamp: u64,
}

/// Represents metadata for a snapshot interval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    pub start_height: u32,
    pub end_height: u32,
    pub state_root: String,
    pub diff_file: String,
    pub wasm_file: String,
    pub wasm_hash: String,
    pub created_at: u64,
}

/// Repository index for streaming snapshots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoIndex {
    pub intervals: Vec<SnapshotMetadata>,
    pub latest_height: u32,
    pub created_at: u64,
}

/// Manages snapshot creation and repository structure
pub struct SnapshotManager {
    pub config: SnapshotConfig,
    pub current_wasm: Option<PathBuf>,
    pub current_wasm_hash: Option<String>,
    pub last_snapshot_height: u32,
    pub key_changes: HashMap<Vec<u8>, Vec<u8>>,
}

impl SnapshotManager {
    pub fn new(config: SnapshotConfig) -> Self {
        Self {
            config,
            current_wasm: None,
            current_wasm_hash: None,
            last_snapshot_height: 0,
            key_changes: HashMap::new(),
        }
    }

    /// Initialize the snapshot directory structure
    pub async fn initialize(&self) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Create main snapshot directory
        async_fs::create_dir_all(&self.config.directory).await?;
        
        // Create subdirectories
        let intervals_dir = self.config.directory.join("intervals");
        let wasm_dir = self.config.directory.join("wasm");
        
        async_fs::create_dir_all(&intervals_dir).await?;
        async_fs::create_dir_all(&wasm_dir).await?;
        
        // Create initial index.json if it doesn't exist
        let index_path = self.config.directory.join("index.json");
        if !index_path.exists() {
            let index = RepoIndex {
                intervals: Vec::new(),
                latest_height: 0,
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };
            
            let index_json = serde_json::to_string_pretty(&index)?;
            async_fs::write(&index_path, index_json).await?;
        }
        
        info!("Initialized snapshot directory at {:?}", self.config.directory);
        Ok(())
    }

    /// Set the current WASM file being used
    pub fn set_current_wasm(&mut self, wasm_path: PathBuf) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Calculate hash of WASM file
        let wasm_bytes = std::fs::read(&wasm_path)?;
        let hash = hex::encode(Sha256::digest(&wasm_bytes));
        
        let filename = wasm_path.file_name()
            .ok_or_else(|| anyhow!("Invalid WASM file path"))?
            .to_string_lossy()
            .to_string();
        
        // Copy WASM file to snapshot directory
        let wasm_dir = self.config.directory.join("wasm");
        let dest_path = wasm_dir.join(format!("{}_{}.wasm", filename, hash[..8].to_string()));
        
        if !dest_path.exists() {
            std::fs::copy(&wasm_path, &dest_path)?;
            info!("Copied WASM file to snapshot directory: {:?}", dest_path);
        }
        
        self.current_wasm = Some(wasm_path);
        self.current_wasm_hash = Some(hash);
        
        Ok(())
    }

    /// Track a key-value change for the current snapshot interval
    pub fn track_key_change(&mut self, key: Vec<u8>, value: Vec<u8>) {
        if self.config.enabled {
            self.key_changes.insert(key, value);
        }
    }

    /// Check if we should create a snapshot at the given height
    pub fn should_create_snapshot(&self, height: u32) -> bool {
        if !self.config.enabled || height == 0 {
            return false;
        }
        
        height % self.config.interval == 0
    }

    /// Create a snapshot for the given height
    pub async fn create_snapshot(&mut self, height: u32, state_root: &[u8]) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let start_height = self.last_snapshot_height;
        let end_height = height;
        
        info!("Creating snapshot for height range {}-{}", start_height, end_height);
        
        // Create interval directory
        let interval_dir = self.config.directory.join("intervals")
            .join(format!("{}-{}", start_height, end_height));
        async_fs::create_dir_all(&interval_dir).await?;
        
        // Create diff.bin.zst file
        let diff_path = interval_dir.join("diff.bin.zst");
        let mut diff_data = Vec::new();
        
        // Format: [key_len(4 bytes)][key][value_len(4 bytes)][value]
        for (key, value) in &self.key_changes {
            diff_data.extend_from_slice(&(key.len() as u32).to_le_bytes());
            diff_data.extend_from_slice(key);
            diff_data.extend_from_slice(&(value.len() as u32).to_le_bytes());
            diff_data.extend_from_slice(value);
        }
        
        // Compress with zstd
        let compressed = zstd::encode_all(&diff_data[..], 3)?;
        async_fs::write(&diff_path, compressed).await?;
        
        // Create stateroot.json file
        let state_root_hex = hex::encode(state_root);
        let state_root_obj = StateRoot {
            height: end_height,
            root: state_root_hex.clone(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        
        let state_root_json = serde_json::to_string_pretty(&state_root_obj)?;
        async_fs::write(interval_dir.join("stateroot.json"), state_root_json).await?;
        
        // Update index.json
        let index_path = self.config.directory.join("index.json");
        let index_content = async_fs::read(&index_path).await?;
        let mut index: RepoIndex = serde_json::from_slice(&index_content)?;
        
        let wasm_hash = self.current_wasm_hash.clone().unwrap_or_else(|| "unknown".to_string());
        let wasm_filename = self.current_wasm.as_ref()
            .and_then(|p| p.file_name())
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown.wasm".to_string());
        
        let metadata = SnapshotMetadata {
            start_height,
            end_height,
            state_root: state_root_hex,
            diff_file: format!("intervals/{}-{}/diff.bin.zst", start_height, end_height),
            wasm_file: format!("wasm/{}_{}.wasm", wasm_filename, wasm_hash[..8].to_string()),
            wasm_hash,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        
        index.intervals.push(metadata);
        index.latest_height = end_height;
        index.created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let index_json = serde_json::to_string_pretty(&index)?;
        async_fs::write(&index_path, index_json).await?;
        
        // Reset for next interval
        self.last_snapshot_height = end_height;
        self.key_changes.clear();
        
        info!("Created snapshot for height {}", height);
        Ok(())
    }

    /// Track database changes for a specific height range
    pub async fn track_db_changes(&mut self, db: &rocksdb::DB, start_height: u32, end_height: u32) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        info!("Tracking database changes for height range {}-{}", start_height, end_height);
        
        // Get all keys updated in this height range
        let mut updated_keys = Vec::new();
        
        for height in start_height..=end_height {
            // Get keys updated at this height
            let length_key = format!("{}FFFFFFFF", height).into_bytes();
            
            if let Some(length_bytes) = db.get(&length_key)? {
                if length_bytes.len() >= 4 {
                    let length = u32::from_le_bytes([
                        length_bytes[0], length_bytes[1], length_bytes[2], length_bytes[3]
                    ]);
                    
                    for i in 0..length {
                        let list_key = format!("{}{}", height, i).into_bytes();
                        if let Some(key_bytes) = db.get(&list_key)? {
                            updated_keys.push(key_bytes);
                        }
                    }
                }
            }
        }
        
        // Get the latest value for each key
        for key in updated_keys {
            if let Some(value) = db.get(&key)? {
                self.key_changes.insert(key, value);
            }
        }
        
        info!("Tracked {} key-value changes for snapshot", self.key_changes.len());
        Ok(())
    }

    /// Sync from a remote repository
    pub async fn sync_from_repo(&mut self, repo_url: &str, db_path: &std::path::Path) -> Result<u32> {
        use std::path::Path;
        use tokio::io::AsyncWriteExt;
        use reqwest;

        info!("Syncing from repository: {}", repo_url);
        
        // Ensure URL ends with a slash
        let repo_url = if repo_url.ends_with('/') {
            repo_url.to_string()
        } else {
            format!("{}/", repo_url)
        };
        
        // Download index.json
        let index_url = format!("{}index.json", repo_url);
        info!("Downloading index from: {}", index_url);
        
        let client = reqwest::Client::new();
        let index_response = client.get(&index_url)
            .send()
            .await?
            .error_for_status()?;
        
        let index_json = index_response.text().await?;
        let index: RepoIndex = serde_json::from_str(&index_json)?;
        
        info!("Repository contains {} intervals up to height {}",
            index.intervals.len(), index.latest_height);
        
        if index.intervals.is_empty() {
            return Ok(0);
        }
        
        // Create temporary directory for downloads
        let temp_dir = std::env::temp_dir().join("metashrew_sync");
        async_fs::create_dir_all(&temp_dir).await?;
        
        // Process intervals in order
        let mut current_height = 0;
        for interval in &index.intervals {
            info!("Processing interval {}-{}", interval.start_height, interval.end_height);
            
            // Download WASM file if needed
            let wasm_url = format!("{}{}", repo_url, interval.wasm_file);
            let wasm_path = temp_dir.join(Path::new(&interval.wasm_file).file_name().unwrap());
            
            if !wasm_path.exists() {
                info!("Downloading WASM file: {}", wasm_url);
                let wasm_response = client.get(&wasm_url)
                    .send()
                    .await?
                    .error_for_status()?;
                
                let wasm_bytes = wasm_response.bytes().await?;
                let mut file = tokio::fs::File::create(&wasm_path).await?;
                file.write_all(&wasm_bytes).await?;
                file.flush().await?;
            }
            
            // Download diff file
            let diff_url = format!("{}{}", repo_url, interval.diff_file);
            info!("Downloading diff file: {}", diff_url);
            
            let diff_response = client.get(&diff_url)
                .send()
                .await?
                .error_for_status()?;
            
            let compressed_diff = diff_response.bytes().await?;
            let diff_data = zstd::decode_all(compressed_diff.as_ref())?;
            
            // Apply diff to database
            info!("Applying diff to database ({})", diff_data.len());
            
            // Open RocksDB
            let mut opts = rocksdb::Options::default();
            opts.create_if_missing(true);
            let db = rocksdb::DB::open(&opts, db_path)?;
            
            // Parse and apply key-value pairs
            let mut i = 0;
            while i < diff_data.len() {
                // Read key length
                if i + 4 > diff_data.len() {
                    break;
                }
                let key_len = u32::from_le_bytes([
                    diff_data[i], diff_data[i+1], diff_data[i+2], diff_data[i+3]
                ]) as usize;
                i += 4;
                
                // Read key
                if i + key_len > diff_data.len() {
                    break;
                }
                let key = diff_data[i..i+key_len].to_vec();
                i += key_len;
                
                // Read value length
                if i + 4 > diff_data.len() {
                    break;
                }
                let value_len = u32::from_le_bytes([
                    diff_data[i], diff_data[i+1], diff_data[i+2], diff_data[i+3]
                ]) as usize;
                i += 4;
                
                // Read value
                if i + value_len > diff_data.len() {
                    break;
                }
                let value = diff_data[i..i+value_len].to_vec();
                i += value_len;
                
                // Apply to database
                db.put(&key, &value)?;
            }
            
            // Download and verify stateroot
            let stateroot_url = format!("{}{}/stateroot.json",
                repo_url,
                interval.diff_file.trim_end_matches("/diff.bin.zst"));
            
            info!("Downloading stateroot: {}", stateroot_url);
            let stateroot_response = client.get(&stateroot_url)
                .send()
                .await?
                .error_for_status()?;
            
            let stateroot_json = stateroot_response.text().await?;
            let stateroot: StateRoot = serde_json::from_str(&stateroot_json)?;
            
            // Store stateroot in database
            let root_key = format!("{}:{}", "smt:root:", interval.end_height).into_bytes();
            let root_value = hex::decode(&stateroot.root)?;
            db.put(&root_key, &root_value)?;
            
            // Update current height
            current_height = interval.end_height;
            
            // Store tip height
            let tip_key = "/__INTERNAL/tip-height".as_bytes();
            let tip_value = current_height.to_le_bytes().to_vec();
            db.put(tip_key, &tip_value)?;
            
            info!("Successfully processed interval up to height {}", current_height);
        }
        
        info!("Repository sync complete, database at height {}", current_height);
        Ok(current_height)
    }
}
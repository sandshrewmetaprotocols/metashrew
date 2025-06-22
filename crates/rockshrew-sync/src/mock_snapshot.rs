//! Mock implementations for snapshot and repository mode testing
//! 
//! This module provides in-memory implementations of snapshot providers,
//! consumers, servers, and clients for comprehensive testing scenarios.

use async_trait::async_trait;
use serde_json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use crate::snapshot::*;
use crate::{SyncResult, SyncError};

/// In-memory filesystem for testing
#[derive(Debug, Clone)]
pub struct MockFilesystem {
    files: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl MockFilesystem {
    pub fn new() -> Self {
        Self {
            files: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn write_file(&self, path: &str, data: Vec<u8>) -> SyncResult<()> {
        let mut files = self.files.write().unwrap();
        files.insert(path.to_string(), data);
        Ok(())
    }
    
    pub fn read_file(&self, path: &str) -> SyncResult<Option<Vec<u8>>> {
        let files = self.files.read().unwrap();
        Ok(files.get(path).cloned())
    }
    
    pub fn list_files(&self, prefix: &str) -> SyncResult<Vec<String>> {
        let files = self.files.read().unwrap();
        let matching_files: Vec<String> = files
            .keys()
            .filter(|path| path.starts_with(prefix))
            .cloned()
            .collect();
        Ok(matching_files)
    }
    
    pub fn delete_file(&self, path: &str) -> SyncResult<bool> {
        let mut files = self.files.write().unwrap();
        Ok(files.remove(path).is_some())
    }
    
    pub fn file_exists(&self, path: &str) -> bool {
        let files = self.files.read().unwrap();
        files.contains_key(path)
    }
    
    pub fn get_file_size(&self, path: &str) -> SyncResult<Option<u64>> {
        let files = self.files.read().unwrap();
        Ok(files.get(path).map(|data| data.len() as u64))
    }
}

/// Mock snapshot provider that stores snapshots in memory
#[derive(Debug)]
pub struct MockSnapshotProvider {
    filesystem: MockFilesystem,
    config: SnapshotConfig,
    snapshots: Arc<RwLock<HashMap<u32, SnapshotMetadata>>>,
    current_height: Arc<RwLock<u32>>,
    wasm_hash: String,
}

impl MockSnapshotProvider {
    pub fn new(config: SnapshotConfig, wasm_hash: String) -> Self {
        Self {
            filesystem: MockFilesystem::new(),
            config,
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            current_height: Arc::new(RwLock::new(0)),
            wasm_hash,
        }
    }
    
    pub fn get_filesystem(&self) -> &MockFilesystem {
        &self.filesystem
    }
    
    pub fn set_current_height(&self, height: u32) {
        let mut current = self.current_height.write().unwrap();
        *current = height;
    }
    
    fn generate_mock_state_data(&self, height: u32) -> Vec<u8> {
        // Generate deterministic mock state data based on height
        let mut data = Vec::new();
        data.extend_from_slice(&height.to_le_bytes());
        data.extend_from_slice(b"mock_state_data_");
        data.extend_from_slice(&height.to_string().as_bytes());
        // Simulate some state growth
        data.resize(1000 + (height as usize * 10), 0x42);
        data
    }
    
    fn calculate_checksum(&self, data: &[u8]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

#[async_trait]
impl SnapshotProvider for MockSnapshotProvider {
    async fn create_snapshot(&mut self, height: u32) -> SyncResult<SnapshotMetadata> {
        let state_data = self.generate_mock_state_data(height);
        let checksum = self.calculate_checksum(&state_data);
        
        let metadata = SnapshotMetadata {
            height,
            block_hash: format!("block_hash_{}", height).into_bytes(),
            state_root: format!("state_root_{}", height).into_bytes(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            size_bytes: state_data.len() as u64,
            checksum: checksum.clone(),
            wasm_hash: self.wasm_hash.clone(),
        };
        
        // Create snapshot data
        let snapshot_data = SnapshotData {
            metadata: metadata.clone(),
            state_data,
            recent_block_hashes: {
                let mut hashes = HashMap::new();
                for h in height.saturating_sub(self.config.reorg_buffer_size)..=height {
                    hashes.insert(h, format!("block_hash_{}", h).into_bytes());
                }
                hashes
            },
        };
        
        // Store in filesystem
        let snapshot_path = format!("snapshots/snapshot_{}.json", height);
        let data_path = format!("snapshots/snapshot_{}.data", height);
        
        let metadata_json = serde_json::to_vec(&snapshot_data.metadata)
            .map_err(|e| SyncError::Serialization(e.to_string()))?;
        let snapshot_json = serde_json::to_vec(&snapshot_data)
            .map_err(|e| SyncError::Serialization(e.to_string()))?;
        
        self.filesystem.write_file(&snapshot_path, metadata_json)?;
        self.filesystem.write_file(&data_path, snapshot_json)?;
        
        // Update in-memory index
        {
            let mut snapshots = self.snapshots.write().unwrap();
            snapshots.insert(height, metadata.clone());
        }
        
        Ok(metadata)
    }
    
    async fn list_snapshots(&self) -> SyncResult<Vec<SnapshotMetadata>> {
        let snapshots = self.snapshots.read().unwrap();
        let mut list: Vec<SnapshotMetadata> = snapshots.values().cloned().collect();
        list.sort_by_key(|s| s.height);
        Ok(list)
    }
    
    async fn get_snapshot(&self, height: u32) -> SyncResult<Option<SnapshotData>> {
        let data_path = format!("snapshots/snapshot_{}.data", height);
        
        if let Some(data_bytes) = self.filesystem.read_file(&data_path)? {
            let snapshot_data: SnapshotData = serde_json::from_slice(&data_bytes)
                .map_err(|e| SyncError::Serialization(e.to_string()))?;
            Ok(Some(snapshot_data))
        } else {
            Ok(None)
        }
    }
    
    async fn get_latest_snapshot(&self) -> SyncResult<Option<SnapshotData>> {
        let latest_height = {
            let snapshots = self.snapshots.read().unwrap();
            snapshots.keys().max().cloned()
        };
        
        if let Some(height) = latest_height {
            self.get_snapshot(height).await
        } else {
            Ok(None)
        }
    }
    
    async fn cleanup_snapshots(&mut self) -> SyncResult<usize> {
        let mut snapshots = self.snapshots.write().unwrap();
        let mut heights: Vec<u32> = snapshots.keys().cloned().collect();
        heights.sort();
        
        let mut deleted = 0;
        while heights.len() > self.config.max_snapshots {
            if let Some(old_height) = heights.first() {
                let old_height = *old_height;
                
                // Remove from filesystem
                let snapshot_path = format!("snapshots/snapshot_{}.json", old_height);
                let data_path = format!("snapshots/snapshot_{}.data", old_height);
                
                self.filesystem.delete_file(&snapshot_path)?;
                self.filesystem.delete_file(&data_path)?;
                
                // Remove from memory
                snapshots.remove(&old_height);
                heights.remove(0);
                deleted += 1;
            } else {
                break;
            }
        }
        
        Ok(deleted)
    }
    
    fn should_create_snapshot(&self, height: u32) -> bool {
        height > 0 && height % self.config.snapshot_interval == 0
    }
}

/// Mock snapshot server that serves snapshots over a simulated HTTP interface
#[derive(Debug)]
pub struct MockSnapshotServer {
    filesystem: MockFilesystem,
    status: Arc<RwLock<SnapshotServerStatus>>,
    is_running: Arc<RwLock<bool>>,
    snapshots: Arc<RwLock<HashMap<u32, SnapshotMetadata>>>,
    start_time: Arc<RwLock<Option<SystemTime>>>,
}

impl MockSnapshotServer {
    pub fn new(filesystem: MockFilesystem) -> Self {
        Self {
            filesystem,
            status: Arc::new(RwLock::new(SnapshotServerStatus {
                is_running: false,
                total_snapshots: 0,
                latest_snapshot_height: None,
                total_size_bytes: 0,
                uptime_seconds: 0,
            })),
            is_running: Arc::new(RwLock::new(false)),
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            start_time: Arc::new(RwLock::new(None)),
        }
    }
    
    pub fn get_filesystem(&self) -> &MockFilesystem {
        &self.filesystem
    }
    
    fn update_status(&self) {
        // Collect data without holding locks simultaneously
        let (total_snapshots, latest_snapshot_height, total_size_bytes) = {
            let snapshots = self.snapshots.read().unwrap();
            let total_snapshots = snapshots.len();
            let latest_snapshot_height = snapshots.keys().max().cloned();
            let total_size_bytes = snapshots.values().map(|s| s.size_bytes).sum();
            (total_snapshots, latest_snapshot_height, total_size_bytes)
        };
        
        let is_running = {
            let running = self.is_running.read().unwrap();
            *running
        };
        
        let uptime_seconds = {
            let start_time = self.start_time.read().unwrap();
            if let Some(start) = *start_time {
                SystemTime::now()
                    .duration_since(start)
                    .unwrap_or_default()
                    .as_secs()
            } else {
                0
            }
        };
        
        // Now update status with all collected data
        {
            let mut status = self.status.write().unwrap();
            *status = SnapshotServerStatus {
                is_running,
                total_snapshots,
                latest_snapshot_height,
                total_size_bytes,
                uptime_seconds,
            };
        }
    }
}

#[async_trait]
impl SnapshotServer for MockSnapshotServer {
    async fn start(&mut self) -> SyncResult<()> {
        {
            let mut running = self.is_running.write().unwrap();
            *running = true;
        }
        
        {
            let mut start_time = self.start_time.write().unwrap();
            *start_time = Some(SystemTime::now());
        }
        
        // Load existing snapshots from filesystem
        let snapshot_files = self.filesystem.list_files("snapshots/snapshot_")?;
        {
            let mut snapshots = self.snapshots.write().unwrap();
            
            for file_path in snapshot_files {
                if file_path.ends_with(".json") {
                    if let Some(metadata_bytes) = self.filesystem.read_file(&file_path)? {
                        if let Ok(metadata) = serde_json::from_slice::<SnapshotMetadata>(&metadata_bytes) {
                            snapshots.insert(metadata.height, metadata);
                        }
                    }
                }
            }
        } // Release the lock before calling update_status
        
        self.update_status();
        Ok(())
    }
    
    async fn stop(&mut self) -> SyncResult<()> {
        {
            let mut running = self.is_running.write().unwrap();
            *running = false;
        }
        
        self.update_status();
        Ok(())
    }
    
    async fn get_status(&self) -> SyncResult<SnapshotServerStatus> {
        self.update_status();
        let status = self.status.read().unwrap();
        Ok(status.clone())
    }
    
    async fn register_snapshot(&mut self, metadata: SnapshotMetadata, data: Vec<u8>) -> SyncResult<()> {
        let snapshot_path = format!("snapshots/snapshot_{}.json", metadata.height);
        let data_path = format!("snapshots/snapshot_{}.data", metadata.height);
        
        let metadata_json = serde_json::to_vec(&metadata)
            .map_err(|e| SyncError::Serialization(e.to_string()))?;
        
        self.filesystem.write_file(&snapshot_path, metadata_json)?;
        self.filesystem.write_file(&data_path, data)?;
        
        {
            let mut snapshots = self.snapshots.write().unwrap();
            snapshots.insert(metadata.height, metadata);
        } // Release the lock before calling update_status
        
        self.update_status();
        Ok(())
    }
    
    async fn get_snapshot_metadata(&self, height: u32) -> SyncResult<Option<SnapshotMetadata>> {
        let snapshots = self.snapshots.read().unwrap();
        Ok(snapshots.get(&height).cloned())
    }
    
    async fn get_snapshot_data(&self, height: u32) -> SyncResult<Option<Vec<u8>>> {
        let data_path = format!("snapshots/snapshot_{}.data", height);
        self.filesystem.read_file(&data_path)
    }
    
    async fn list_available_snapshots(&self) -> SyncResult<Vec<SnapshotMetadata>> {
        let snapshots = self.snapshots.read().unwrap();
        let mut list: Vec<SnapshotMetadata> = snapshots.values().cloned().collect();
        list.sort_by_key(|s| s.height);
        Ok(list)
    }
}

/// Mock snapshot client that simulates HTTP downloads from the mock server
#[derive(Debug)]
pub struct MockSnapshotClient {
    server: Arc<Mutex<MockSnapshotServer>>,
    network_delay_ms: u64,
    failure_rate: f64, // 0.0 = never fail, 1.0 = always fail
}

impl MockSnapshotClient {
    pub fn new(server: Arc<Mutex<MockSnapshotServer>>) -> Self {
        Self {
            server,
            network_delay_ms: 10,
            failure_rate: 0.0,
        }
    }
    
    pub fn with_network_delay(mut self, delay_ms: u64) -> Self {
        self.network_delay_ms = delay_ms;
        self
    }
    
    pub fn with_failure_rate(mut self, rate: f64) -> Self {
        self.failure_rate = rate.clamp(0.0, 1.0);
        self
    }
    
    async fn simulate_network_delay(&self) {
        if self.network_delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.network_delay_ms)).await;
        }
    }
    
    fn should_fail(&self) -> bool {
        if self.failure_rate <= 0.0 {
            return false;
        }
        if self.failure_rate >= 1.0 {
            return true;
        }
        
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::SystemTime;
        
        let mut hasher = DefaultHasher::new();
        SystemTime::now().hash(&mut hasher);
        let random_value = (hasher.finish() % 1000) as f64 / 1000.0;
        random_value < self.failure_rate
    }
}

#[async_trait]
impl SnapshotClient for MockSnapshotClient {
    async fn download_metadata(&self, url: &str) -> SyncResult<SnapshotMetadata> {
        self.simulate_network_delay().await;
        
        if self.should_fail() {
            return Err(SyncError::Runtime("Simulated network failure".to_string()));
        }
        
        // Parse height from URL (e.g., "http://localhost:8080/snapshots/1000/metadata")
        let height = url
            .split('/')
            .nth_back(1)
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| SyncError::Runtime("Invalid URL format".to_string()))?;
        
        let server = self.server.lock().await;
        server.get_snapshot_metadata(height).await?
            .ok_or_else(|| SyncError::Runtime(format!("Snapshot not found at height {}", height)))
    }
    
    async fn download_data(&self, url: &str) -> SyncResult<Vec<u8>> {
        self.simulate_network_delay().await;
        
        if self.should_fail() {
            return Err(SyncError::Runtime("Simulated network failure".to_string()));
        }
        
        // Parse height from URL
        let height = url
            .split('/')
            .nth_back(1)
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| SyncError::Runtime("Invalid URL format".to_string()))?;
        
        let server = self.server.lock().await;
        server.get_snapshot_data(height).await?
            .ok_or_else(|| SyncError::Runtime(format!("Snapshot data not found at height {}", height)))
    }
    
    async fn list_remote_snapshots(&self, _base_url: &str) -> SyncResult<Vec<SnapshotMetadata>> {
        self.simulate_network_delay().await;
        
        if self.should_fail() {
            return Err(SyncError::Runtime("Simulated network failure".to_string()));
        }
        
        let server = self.server.lock().await;
        server.list_available_snapshots().await
    }
    
    async fn check_repository(&self, _base_url: &str) -> SyncResult<bool> {
        self.simulate_network_delay().await;
        
        if self.should_fail() {
            return Ok(false);
        }
        
        let server = self.server.lock().await;
        let status = server.get_status().await?;
        Ok(status.is_running)
    }
}

/// Mock snapshot consumer that can apply snapshots
#[derive(Debug)]
pub struct MockSnapshotConsumer {
    client: MockSnapshotClient,
    config: RepoConfig,
    applied_snapshots: Arc<RwLock<Vec<u32>>>,
    current_height: Arc<RwLock<u32>>,
}

impl MockSnapshotConsumer {
    pub fn new(client: MockSnapshotClient, config: RepoConfig) -> Self {
        Self {
            client,
            config,
            applied_snapshots: Arc::new(RwLock::new(Vec::new())),
            current_height: Arc::new(RwLock::new(0)),
        }
    }
    
    pub fn set_current_height(&self, height: u32) {
        let mut current = self.current_height.write().unwrap();
        *current = height;
    }
    
    pub fn get_applied_snapshots(&self) -> Vec<u32> {
        let applied = self.applied_snapshots.read().unwrap();
        applied.clone()
    }
}

#[async_trait]
impl SnapshotConsumer for MockSnapshotConsumer {
    async fn check_available_snapshots(&self) -> SyncResult<Vec<SnapshotMetadata>> {
        self.client.list_remote_snapshots(&self.config.repo_url).await
    }
    
    async fn apply_snapshot(&mut self, metadata: &SnapshotMetadata) -> SyncResult<()> {
        // Download the snapshot data
        let data_url = format!("{}/{}/data", self.config.repo_url, metadata.height);
        let _snapshot_data = self.client.download_data(&data_url).await?;
        
        // Simulate applying the snapshot
        {
            let mut current = self.current_height.write().unwrap();
            *current = metadata.height;
        }
        
        {
            let mut applied = self.applied_snapshots.write().unwrap();
            applied.push(metadata.height);
        }
        
        Ok(())
    }
    
    async fn get_best_snapshot(&self, current_height: u32, tip_height: u32) -> SyncResult<Option<SnapshotMetadata>> {
        let available = self.check_available_snapshots().await?;
        
        // Find the best snapshot: latest one that's not too far ahead
        let best = available
            .into_iter()
            .filter(|s| s.height > current_height && s.height <= tip_height)
            .max_by_key(|s| s.height);
        
        Ok(best)
    }
    
    async fn verify_snapshot(&self, data: &SnapshotData) -> SyncResult<bool> {
        // Simple verification: check that metadata matches
        Ok(data.metadata.height > 0 && !data.state_data.is_empty())
    }
    
    async fn should_use_snapshots(&self, current_height: u32, tip_height: u32) -> SyncResult<bool> {
        let blocks_behind = tip_height.saturating_sub(current_height);
        Ok(blocks_behind >= self.config.min_blocks_behind)
    }
}
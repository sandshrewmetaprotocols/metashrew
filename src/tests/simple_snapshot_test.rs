//! Simple snapshot functionality tests
//! 
//! This test suite validates the basic snapshot mock implementations
//! without the complex sync engine integration.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

use rockshrew_sync::{
    MockSnapshotProvider, MockSnapshotServer, MockSnapshotClient, MockSnapshotConsumer, 
    MockFilesystem, SnapshotConfig, RepoConfig,
    SnapshotProvider, SnapshotConsumer, SnapshotServer, SnapshotClient,
};

/// Test basic mock filesystem operations
#[tokio::test]
async fn test_mock_filesystem() -> Result<()> {
    println!("🗂️ Testing mock filesystem operations");
    
    let fs = MockFilesystem::new();
    
    // Test file operations
    let test_data = b"Hello, World!".to_vec();
    fs.write_file("test.txt", test_data.clone())?;
    
    assert!(fs.file_exists("test.txt"));
    assert!(!fs.file_exists("nonexistent.txt"));
    
    let read_data = fs.read_file("test.txt")?;
    assert_eq!(read_data, Some(test_data.clone()));
    
    let size = fs.get_file_size("test.txt")?;
    assert_eq!(size, Some(test_data.len() as u64));
    
    // Test listing files
    fs.write_file("snapshots/snap1.json", b"data1".to_vec())?;
    fs.write_file("snapshots/snap2.json", b"data2".to_vec())?;
    fs.write_file("other/file.txt", b"other".to_vec())?;
    
    let snapshot_files = fs.list_files("snapshots/")?;
    assert_eq!(snapshot_files.len(), 2);
    assert!(snapshot_files.contains(&"snapshots/snap1.json".to_string()));
    assert!(snapshot_files.contains(&"snapshots/snap2.json".to_string()));
    
    // Test deletion
    assert!(fs.delete_file("test.txt")?);
    assert!(!fs.file_exists("test.txt"));
    assert!(!fs.delete_file("nonexistent.txt")?);
    
    println!("✅ Mock filesystem test passed!");
    Ok(())
}

/// Test basic snapshot provider functionality
#[tokio::test]
async fn test_mock_snapshot_provider() -> Result<()> {
    println!("📸 Testing mock snapshot provider");
    
    let config = SnapshotConfig {
        snapshot_interval: 5,
        max_snapshots: 3,
        compression_level: 6,
        reorg_buffer_size: 10,
    };
    
    let mut provider = MockSnapshotProvider::new(config.clone(), "test_wasm".to_string());
    
    // Test snapshot creation logic
    assert!(!provider.should_create_snapshot(0));
    assert!(!provider.should_create_snapshot(3));
    assert!(provider.should_create_snapshot(5));
    assert!(provider.should_create_snapshot(10));
    assert!(!provider.should_create_snapshot(7));
    
    // Test creating snapshots
    provider.set_current_height(5);
    let metadata1 = provider.create_snapshot(5).await?;
    assert_eq!(metadata1.height, 5);
    assert_eq!(metadata1.wasm_hash, "test_wasm");
    assert!(metadata1.size_bytes > 0);
    
    provider.set_current_height(10);
    let metadata2 = provider.create_snapshot(10).await?;
    assert_eq!(metadata2.height, 10);
    
    // Test listing snapshots
    let snapshots = provider.list_snapshots().await?;
    assert_eq!(snapshots.len(), 2);
    assert!(snapshots.iter().any(|s| s.height == 5));
    assert!(snapshots.iter().any(|s| s.height == 10));
    
    // Test getting specific snapshot
    let snapshot_data = provider.get_snapshot(5).await?;
    assert!(snapshot_data.is_some());
    let data = snapshot_data.unwrap();
    assert_eq!(data.metadata.height, 5);
    assert!(!data.state_data.is_empty());
    
    // Test latest snapshot
    let latest = provider.get_latest_snapshot().await?;
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().metadata.height, 10);
    
    println!("✅ Mock snapshot provider test passed!");
    Ok(())
}

/// Test snapshot server and client interaction
#[tokio::test]
async fn test_snapshot_server_client() -> Result<()> {
    println!("🌐 Testing snapshot server and client");
    
    let filesystem = MockFilesystem::new();
    let mut server = MockSnapshotServer::new(filesystem.clone());
    
    // Start server
    server.start().await?;
    let status = server.get_status().await?;
    assert!(status.is_running);
    assert_eq!(status.total_snapshots, 0);
    
    // Register a snapshot
    let metadata = rockshrew_sync::SnapshotMetadata {
        height: 100,
        block_hash: b"block_hash_100".to_vec(),
        state_root: b"state_root_100".to_vec(),
        timestamp: 1234567890,
        size_bytes: 1000,
        checksum: "checksum_100".to_string(),
        wasm_hash: "test_wasm".to_string(),
    };
    
    let test_data = b"snapshot_data_100".to_vec();
    server.register_snapshot(metadata.clone(), test_data.clone()).await?;
    
    // Check server status
    let status = server.get_status().await?;
    assert_eq!(status.total_snapshots, 1);
    assert_eq!(status.latest_snapshot_height, Some(100));
    
    // Test client operations
    let server_arc = Arc::new(Mutex::new(server));
    let client = MockSnapshotClient::new(server_arc.clone());
    
    // Check repository availability
    let available = client.check_repository("http://localhost:8080").await?;
    assert!(available);
    
    // List remote snapshots
    let remote_snapshots = client.list_remote_snapshots("http://localhost:8080").await?;
    assert_eq!(remote_snapshots.len(), 1);
    assert_eq!(remote_snapshots[0].height, 100);
    
    // Download metadata
    let downloaded_metadata = client.download_metadata("http://localhost:8080/snapshots/100/metadata").await?;
    assert_eq!(downloaded_metadata.height, 100);
    assert_eq!(downloaded_metadata.checksum, "checksum_100");
    
    // Download data
    let downloaded_data = client.download_data("http://localhost:8080/snapshots/100/data").await?;
    assert_eq!(downloaded_data, test_data);
    
    // Stop server
    let mut server_guard = server_arc.lock().await;
    server_guard.stop().await?;
    let status = server_guard.get_status().await?;
    assert!(!status.is_running);
    
    println!("✅ Snapshot server and client test passed!");
    Ok(())
}

/// Test snapshot consumer functionality
#[tokio::test]
async fn test_snapshot_consumer() -> Result<()> {
    println!("📥 Testing snapshot consumer");
    
    // Set up server with snapshots
    let filesystem = MockFilesystem::new();
    let mut server = MockSnapshotServer::new(filesystem.clone());
    server.start().await?;
    
    // Register multiple snapshots
    for height in [50, 100, 150] {
        let metadata = rockshrew_sync::SnapshotMetadata {
            height,
            block_hash: format!("block_hash_{}", height).into_bytes(),
            state_root: format!("state_root_{}", height).into_bytes(),
            timestamp: 1234567890 + height as u64,
            size_bytes: 1000 + height as u64,
            checksum: format!("checksum_{}", height),
            wasm_hash: "test_wasm".to_string(),
        };
        
        let data = format!("snapshot_data_{}", height).into_bytes();
        server.register_snapshot(metadata, data).await?;
    }
    
    // Set up consumer
    let server_arc = Arc::new(Mutex::new(server));
    let client = MockSnapshotClient::new(server_arc.clone());
    let config = RepoConfig {
        repo_url: "http://localhost:8080/snapshots".to_string(),
        check_interval: 60,
        max_snapshot_age: 3600,
        continue_sync: true,
        min_blocks_behind: 10,
    };
    
    let mut consumer = MockSnapshotConsumer::new(client, config);
    
    // Test checking available snapshots
    let available = consumer.check_available_snapshots().await?;
    assert_eq!(available.len(), 3);
    
    // Test should use snapshots logic
    assert!(consumer.should_use_snapshots(0, 200).await?); // 200 blocks behind
    assert!(!consumer.should_use_snapshots(195, 200).await?); // Only 5 blocks behind
    
    // Test getting best snapshot
    let best = consumer.get_best_snapshot(75, 200).await?;
    assert!(best.is_some());
    assert_eq!(best.unwrap().height, 150); // Should pick the latest one <= tip
    
    let best = consumer.get_best_snapshot(25, 75).await?;
    assert!(best.is_some());
    assert_eq!(best.unwrap().height, 50); // Should pick 50, not 100 (which is > tip)
    
    // Test applying snapshot
    consumer.set_current_height(0);
    let snapshot_to_apply = consumer.get_best_snapshot(0, 200).await?.unwrap();
    consumer.apply_snapshot(&snapshot_to_apply).await?;
    
    let applied = consumer.get_applied_snapshots();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0], 150);
    
    println!("✅ Snapshot consumer test passed!");
    Ok(())
}

/// Test network failure scenarios
#[tokio::test]
async fn test_network_failures() -> Result<()> {
    println!("🔥 Testing network failure scenarios");
    
    let filesystem = MockFilesystem::new();
    let mut server = MockSnapshotServer::new(filesystem.clone());
    server.start().await?;
    
    // Register a snapshot
    let metadata = rockshrew_sync::SnapshotMetadata {
        height: 100,
        block_hash: b"block_hash_100".to_vec(),
        state_root: b"state_root_100".to_vec(),
        timestamp: 1234567890,
        size_bytes: 1000,
        checksum: "checksum_100".to_string(),
        wasm_hash: "test_wasm".to_string(),
    };
    server.register_snapshot(metadata, b"test_data".to_vec()).await?;
    
    // Create client with high failure rate
    let server_arc = Arc::new(Mutex::new(server));
    let client = MockSnapshotClient::new(server_arc.clone())
        .with_failure_rate(0.8) // 80% failure rate
        .with_network_delay(1); // 1ms delay
    
    // Test that operations fail most of the time
    let mut failures = 0;
    let mut successes = 0;
    
    for _ in 0..20 {
        match client.check_repository("http://localhost:8080").await {
            Ok(true) => successes += 1,
            Ok(false) | Err(_) => failures += 1,
        }
    }
    
    // With 80% failure rate, we should see mostly failures
    assert!(failures > successes, "Expected more failures than successes with 80% failure rate");
    println!("  📊 Failures: {}, Successes: {} (as expected with 80% failure rate)", failures, successes);
    
    // Test with 0% failure rate
    let client_reliable = MockSnapshotClient::new(server_arc)
        .with_failure_rate(0.0);
    
    let available = client_reliable.check_repository("http://localhost:8080").await?;
    assert!(available);
    
    let snapshots = client_reliable.list_remote_snapshots("http://localhost:8080").await?;
    assert_eq!(snapshots.len(), 1);
    
    println!("✅ Network failure scenarios test passed!");
    Ok(())
}
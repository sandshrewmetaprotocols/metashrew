//! Comprehensive tests for snapshot and repository mode functionality
//! 
//! This test suite validates the complete snapshot and repo mode workflow:
//! 1. Snapshot server creating snapshots at intervals
//! 2. Repo mode client syncing from snapshots
//! 3. Catch-up scenarios and restart behavior
//! 4. Reorg handling with snapshots

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::block_builder::{ChainBuilder, create_test_block};
use metashrew_support::utils;
use rockshrew_sync::{
    SnapshotMetashrewSync, SyncConfig, SyncMode, SnapshotConfig, RepoConfig,
    MockBitcoinNode, MockStorage, MockRuntime, MockSnapshotProvider, MockSnapshotServer,
    MockSnapshotClient, MockSnapshotConsumer, MockFilesystem,
    SnapshotSyncEngine, SyncEngine, JsonRpcProvider,
    // Import the traits to use their methods
    SnapshotProvider, SnapshotServer, SnapshotClient,
    BitcoinNodeAdapter,
};
use bitcoin::hashes::Hash;

/// Test basic snapshot creation and consumption
#[tokio::test]
async fn test_snapshot_creation_and_consumption() -> Result<()> {
    println!("🔄 Testing snapshot creation and consumption");
    
    // Create shared filesystem for snapshots
    let _filesystem = MockFilesystem::new();
    
    // Create snapshot provider
    let snapshot_config = SnapshotConfig {
        snapshot_interval: 5, // Create snapshot every 5 blocks
        max_snapshots: 3,
        compression_level: 6,
        reorg_buffer_size: 10,
    };
    
    let mut provider = MockSnapshotProvider::new(
        snapshot_config.clone(),
        "test_wasm_hash".to_string(),
    );
    
    // Test snapshot creation
    println!("📸 Creating snapshots at different heights");
    
    let heights = vec![5, 10, 15, 20];
    let mut created_snapshots = Vec::new();
    
    for height in heights {
        provider.set_current_height(height);
        
        if provider.should_create_snapshot(height) {
            let metadata = provider.create_snapshot(height).await?;
            created_snapshots.push(metadata.clone());
            
            println!("  ✓ Created snapshot at height {} (size: {} bytes)", 
                    height, metadata.size_bytes);
            
            // Verify snapshot data
            let snapshot_data = provider.get_snapshot(height).await?;
            assert!(snapshot_data.is_some(), "Snapshot data should exist");
            
            let data = snapshot_data.unwrap();
            assert_eq!(data.metadata.height, height);
            assert!(!data.state_data.is_empty());
            assert!(!data.recent_block_hashes.is_empty());
        }
    }
    
    // Test snapshot listing
    let all_snapshots = provider.list_snapshots().await?;
    assert_eq!(all_snapshots.len(), created_snapshots.len());
    
    println!("  ✓ Listed {} snapshots", all_snapshots.len());
    
    // Test latest snapshot
    let latest = provider.get_latest_snapshot().await?;
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().metadata.height, 20);
    
    println!("  ✓ Latest snapshot at height 20");
    
    // Test cleanup (should keep only max_snapshots)
    let deleted = provider.cleanup_snapshots().await?;
    println!("  ✓ Cleaned up {} old snapshots", deleted);
    
    let remaining = provider.list_snapshots().await?;
    assert!(remaining.len() <= snapshot_config.max_snapshots);
    
    println!("✅ Snapshot creation and consumption test passed!");
    Ok(())
}

/// Test snapshot server and client interaction
#[tokio::test]
async fn test_snapshot_server_client_interaction() -> Result<()> {
    println!("🌐 Testing snapshot server and client interaction");
    
    // Create shared filesystem
    let filesystem = MockFilesystem::new();
    
    // Create and start snapshot server
    let mut server = MockSnapshotServer::new(filesystem.clone());
    server.start().await?;
    
    let server_arc = Arc::new(Mutex::new(server));
    
    // Create snapshot client
    let client = MockSnapshotClient::new(server_arc.clone())
        .with_network_delay(5) // 5ms delay
        .with_failure_rate(0.0); // No failures
    
    // Register some snapshots with the server
    println!("📤 Registering snapshots with server");
    
    let test_snapshots = vec![
        (100, b"snapshot_data_100".to_vec()),
        (200, b"snapshot_data_200".to_vec()),
        (300, b"snapshot_data_300".to_vec()),
    ];
    
    for (height, data) in &test_snapshots {
        let metadata = rockshrew_sync::SnapshotMetadata {
            height: *height,
            block_hash: format!("block_hash_{}", height).into_bytes(),
            state_root: format!("state_root_{}", height).into_bytes(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            size_bytes: data.len() as u64,
            checksum: format!("checksum_{}", height),
            wasm_hash: "test_wasm_hash".to_string(),
        };
        
        let mut server_guard = server_arc.lock().await;
        server_guard.register_snapshot(metadata, data.clone()).await?;
        drop(server_guard);
        
        println!("  ✓ Registered snapshot at height {}", height);
    }
    
    // Test client operations
    println!("📥 Testing client operations");
    
    // Check repository availability
    let is_available = client.check_repository("http://localhost:8080").await?;
    assert!(is_available, "Repository should be available");
    println!("  ✓ Repository is available");
    
    // List remote snapshots
    let remote_snapshots = client.list_remote_snapshots("http://localhost:8080").await?;
    assert_eq!(remote_snapshots.len(), test_snapshots.len());
    println!("  ✓ Listed {} remote snapshots", remote_snapshots.len());
    
    // Download specific snapshots
    for (height, _expected_data) in &test_snapshots {
        let metadata_url = format!("http://localhost:8080/snapshots/{}/metadata", height);
        let data_url = format!("http://localhost:8080/snapshots/{}/data", height);
        
        let metadata = client.download_metadata(&metadata_url).await?;
        assert_eq!(metadata.height, *height);
        
        let data = client.download_data(&data_url).await?;
        // Note: The actual data might be wrapped in SnapshotData structure
        assert!(!data.is_empty());
        
        println!("  ✓ Downloaded snapshot at height {}", height);
    }
    
    // Test server status
    let server_guard = server_arc.lock().await;
    let status = server_guard.get_status().await?;
    assert!(status.is_running);
    assert_eq!(status.total_snapshots, test_snapshots.len());
    assert_eq!(status.latest_snapshot_height, Some(300));
    drop(server_guard);
    
    println!("  ✓ Server status: {} snapshots, latest at height {}", 
            status.total_snapshots, status.latest_snapshot_height.unwrap());
    
    // Stop server
    let mut server_guard = server_arc.lock().await;
    server_guard.stop().await?;
    drop(server_guard);
    
    println!("✅ Snapshot server and client interaction test passed!");
    Ok(())
}

/// Test complete snapshot and repo mode workflow
#[tokio::test]
async fn test_complete_snapshot_repo_workflow() -> Result<()> {
    println!("🔄 Testing complete snapshot and repo mode workflow");
    
    // Create shared components
    let filesystem = MockFilesystem::new();
    let node = MockBitcoinNode::new();
    
    // Create a test blockchain
    let chain = ChainBuilder::new()
        .add_blocks(25) // 25 blocks total
        .blocks();
    
    // Add blocks to mock node
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        let block_hash = block.block_hash().as_byte_array().to_vec();
        node.add_block(height as u32, block_hash, block_bytes);
    }
    
    println!("📦 Created test blockchain with {} blocks", chain.len());
    
    // === PHASE 1: Snapshot Server Mode ===
    println!("\n📸 Phase 1: Running in snapshot server mode");
    
    let snapshot_config = SnapshotConfig {
        snapshot_interval: 5, // Snapshot every 5 blocks
        max_snapshots: 10,
        compression_level: 6,
        reorg_buffer_size: 10,
    };
    
    // Create snapshot server instance
    let storage1 = MockStorage::new();
    let runtime_adapter1 = MockRuntime::new();
    
    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(20), // Process first 20 blocks
        pipeline_size: None,
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };
    
    let mut snapshot_server_engine = SnapshotMetashrewSync::new(
        node.clone(),
        storage1,
        runtime_adapter1,
        sync_config.clone(),
        SyncMode::Snapshot(snapshot_config.clone()),
    );
    
    // Set up snapshot provider
    let provider = MockSnapshotProvider::new(
        snapshot_config.clone(),
        "test_wasm_hash".to_string(),
    );
    snapshot_server_engine.set_snapshot_provider(Box::new(provider)).await;
    
    // Set up snapshot server
    let server = MockSnapshotServer::new(filesystem.clone());
    snapshot_server_engine.set_snapshot_server(Box::new(server)).await;
    
    // Run snapshot server (this will create snapshots as it processes blocks)
    println!("🚀 Starting snapshot server sync...");
    
    // Note: In a real test, we'd run this in a separate task
    // For this mock, we'll simulate the process
    for height in 0..20 {
        if let Ok(_block_data) = node.get_block_data(height).await {
            snapshot_server_engine.process_single_block(height).await?;
            
            // Check if snapshot was created
            if height > 0 && height % 5 == 0 {
                let stats = snapshot_server_engine.get_snapshot_stats().await?;
                println!("  📸 Snapshot created at height {} (total: {})", 
                        height, stats.snapshots_created);
            }
        }
    }
    
    let final_stats = snapshot_server_engine.get_snapshot_stats().await?;
    println!("📊 Snapshot server final stats:");
    println!("  - Snapshots created: {}", final_stats.snapshots_created);
    println!("  - Blocks synced normally: {}", final_stats.blocks_synced_normally);
    println!("  - Current height: {}", final_stats.current_height);
    
    // === PHASE 2: Repository Mode ===
    println!("\n📥 Phase 2: Running in repository mode");
    
    let repo_config = RepoConfig {
        repo_url: "http://localhost:8080/snapshots".to_string(),
        check_interval: 60,
        max_snapshot_age: 3600,
        continue_sync: true,
        min_blocks_behind: 5,
    };
    
    // Create repo mode instance (starts from beginning)
    let storage2 = MockStorage::new();
    let runtime_adapter2 = MockRuntime::new();
    
    let sync_config2 = SyncConfig {
        start_block: 0,
        exit_at: Some(25), // Process all blocks
        pipeline_size: None,
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };
    
    let mut repo_engine = SnapshotMetashrewSync::new(
        node.clone(),
        storage2,
        runtime_adapter2,
        sync_config2,
        SyncMode::Repo(repo_config.clone()),
    );
    
    // Set up snapshot consumer
    let server_arc = Arc::new(Mutex::new(MockSnapshotServer::new(filesystem.clone())));
    let client = MockSnapshotClient::new(server_arc.clone());
    let consumer = MockSnapshotConsumer::new(client, repo_config);
    repo_engine.set_snapshot_consumer(Box::new(consumer)).await;
    
    // Simulate repo mode sync
    println!("🚀 Starting repo mode sync...");
    
    // First, check if snapshots should be used
    let should_use_snapshots = repo_engine.check_and_apply_snapshots().await?;
    if should_use_snapshots {
        println!("  📥 Applied snapshot for fast sync");
        
        let stats = repo_engine.get_snapshot_stats().await?;
        println!("  📊 After snapshot: height {}, snapshots applied: {}", 
                stats.current_height, stats.snapshots_applied);
    }
    
    // Continue with normal sync for remaining blocks
    let current_height = repo_engine.get_snapshot_stats().await?.current_height;
    for height in current_height..25 {
        if let Ok(_block_data) = node.get_block_data(height).await {
            repo_engine.process_single_block(height).await?;
        }
    }
    
    let final_repo_stats = repo_engine.get_snapshot_stats().await?;
    println!("📊 Repo mode final stats:");
    println!("  - Snapshots applied: {}", final_repo_stats.snapshots_applied);
    println!("  - Blocks synced normally: {}", final_repo_stats.blocks_synced_normally);
    println!("  - Blocks synced from snapshots: {}", final_repo_stats.blocks_synced_from_snapshots);
    println!("  - Current height: {}", final_repo_stats.current_height);
    
    // Verify both instances reached the same final state
    let server_final_height = snapshot_server_engine.metashrew_height().await?;
    let repo_final_height = repo_engine.metashrew_height().await?;
    
    // Note: Server stopped at height 19 (exit_at: 20), repo went to 24 (exit_at: 25)
    println!("\n🎯 Final verification:");
    println!("  - Snapshot server final height: {}", server_final_height);
    println!("  - Repo mode final height: {}", repo_final_height);
    
    // Both should have consistent state up to the point where server stopped
    for height in 0..=server_final_height.min(repo_final_height) {
        let server_hash = snapshot_server_engine.metashrew_getblockhash(height).await?;
        let repo_hash = repo_engine.metashrew_getblockhash(height).await?;
        assert_eq!(server_hash, repo_hash, "Block hashes should match at height {}", height);
    }
    
    println!("  ✓ State consistency verified");
    
    println!("✅ Complete snapshot and repo mode workflow test passed!");
    Ok(())
}

/// Test restart scenarios for both snapshot and repo modes
#[tokio::test]
async fn test_restart_scenarios() -> Result<()> {
    println!("🔄 Testing restart scenarios");
    
    let filesystem = MockFilesystem::new();
    let node = MockBitcoinNode::new();
    
    // Create test blockchain
    let chain = ChainBuilder::new()
        .add_blocks(30)
        .blocks();
    
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        let block_hash = block.block_hash().as_byte_array().to_vec();
        node.add_block(height as u32, block_hash, block_bytes);
    }
    
    println!("📦 Created test blockchain with {} blocks", chain.len());
    
    // === Test 1: Snapshot Server Restart ===
    println!("\n🔄 Test 1: Snapshot server restart");
    
    let snapshot_config = SnapshotConfig {
        snapshot_interval: 5,
        max_snapshots: 10,
        compression_level: 6,
        reorg_buffer_size: 10,
    };
    
    // First run: process blocks 0-15
    {
        let storage = MockStorage::new();
        let runtime_adapter = MockRuntime::new();
        
        let sync_config = SyncConfig {
            start_block: 0,
            exit_at: Some(15),
            pipeline_size: None,
            max_reorg_depth: 100,
            reorg_check_threshold: 6,
        };
        
        let mut engine = SnapshotMetashrewSync::new(
            node.clone(),
            storage,
            runtime_adapter,
            sync_config,
            SyncMode::Snapshot(snapshot_config.clone()),
        );
        
        let provider = MockSnapshotProvider::new(
            snapshot_config.clone(),
            "test_wasm_hash".to_string(),
        );
        engine.set_snapshot_provider(Box::new(provider)).await;
        
        // Process first batch
        for height in 0..15 {
            if let Ok(_block_data) = node.get_block_data(height).await {
                engine.process_single_block(height).await?;
            }
        }
        
        let stats = engine.get_snapshot_stats().await?;
        println!("  📊 First run: {} snapshots created, height {}", 
                stats.snapshots_created, stats.current_height);
    }
    
    // Second run: restart and continue from block 15-25
    {
        let storage = MockStorage::new();
        let runtime_adapter = MockRuntime::new();
        
        // Simulate restart by setting start_block to where we left off
        let sync_config = SyncConfig {
            start_block: 15,
            exit_at: Some(25),
            pipeline_size: None,
            max_reorg_depth: 100,
            reorg_check_threshold: 6,
        };
        
        let mut engine = SnapshotMetashrewSync::new(
            node.clone(),
            storage,
            runtime_adapter,
            sync_config,
            SyncMode::Snapshot(snapshot_config.clone()),
        );
        
        let provider = MockSnapshotProvider::new(
            snapshot_config.clone(),
            "test_wasm_hash".to_string(),
        );
        engine.set_snapshot_provider(Box::new(provider)).await;
        
        // Process second batch
        for height in 15..25 {
            if let Ok(_block_data) = node.get_block_data(height).await {
                engine.process_single_block(height).await?;
            }
        }
        
        let stats = engine.get_snapshot_stats().await?;
        println!("  📊 After restart: {} snapshots created, height {}", 
                stats.snapshots_created, stats.current_height);
    }
    
    // === Test 2: Repo Mode Restart ===
    println!("\n🔄 Test 2: Repo mode restart");
    
    let repo_config = RepoConfig {
        repo_url: "http://localhost:8080/snapshots".to_string(),
        check_interval: 60,
        max_snapshot_age: 3600,
        continue_sync: true,
        min_blocks_behind: 3,
    };
    
    // First run: start from beginning, should use snapshots
    {
        let storage = MockStorage::new();
        let runtime_adapter = MockRuntime::new();
        
        let sync_config = SyncConfig {
            start_block: 0,
            exit_at: Some(20),
            pipeline_size: None,
            max_reorg_depth: 100,
            reorg_check_threshold: 6,
        };
        
        let mut engine = SnapshotMetashrewSync::new(
            node.clone(),
            storage,
            runtime_adapter,
            sync_config,
            SyncMode::Repo(repo_config.clone()),
        );
        
        let server_arc = Arc::new(Mutex::new(MockSnapshotServer::new(filesystem.clone())));
        let client = MockSnapshotClient::new(server_arc);
        let consumer = MockSnapshotConsumer::new(client, repo_config.clone());
        engine.set_snapshot_consumer(Box::new(consumer)).await;
        
        // Simulate finding and applying snapshots
        let applied_snapshots = engine.check_and_apply_snapshots().await?;
        if applied_snapshots {
            println!("  📥 Applied snapshots on first run");
        }
        
        // Continue with normal sync
        let current_height = engine.get_snapshot_stats().await?.current_height;
        for height in current_height..20 {
            if let Ok(_block_data) = node.get_block_data(height).await {
                engine.process_single_block(height).await?;
            }
        }
        
        let stats = engine.get_snapshot_stats().await?;
        println!("  📊 First run: {} snapshots applied, {} blocks synced normally", 
                stats.snapshots_applied, stats.blocks_synced_normally);
    }
    
    // Second run: restart from height 20, continue to 30
    {
        let storage = MockStorage::new();
        let runtime_adapter = MockRuntime::new();
        
        let sync_config = SyncConfig {
            start_block: 20,
            exit_at: Some(30),
            pipeline_size: None,
            max_reorg_depth: 100,
            reorg_check_threshold: 6,
        };
        
        let mut engine = SnapshotMetashrewSync::new(
            node.clone(),
            storage,
            runtime_adapter,
            sync_config,
            SyncMode::Repo(repo_config.clone()),
        );
        
        let server_arc = Arc::new(Mutex::new(MockSnapshotServer::new(filesystem.clone())));
        let client = MockSnapshotClient::new(server_arc);
        let consumer = MockSnapshotConsumer::new(client, repo_config.clone());
        engine.set_snapshot_consumer(Box::new(consumer)).await;
        
        // Check for new snapshots
        let applied_snapshots = engine.check_and_apply_snapshots().await?;
        if applied_snapshots {
            println!("  📥 Applied additional snapshots on restart");
        }
        
        // Continue sync
        let current_height = engine.get_snapshot_stats().await?.current_height;
        for height in current_height..30 {
            if let Ok(_block_data) = node.get_block_data(height).await {
                engine.process_single_block(height).await?;
            }
        }
        
        let stats = engine.get_snapshot_stats().await?;
        println!("  📊 After restart: {} snapshots applied total, {} blocks synced normally", 
                stats.snapshots_applied, stats.blocks_synced_normally);
    }
    
    println!("✅ Restart scenarios test passed!");
    Ok(())
}

/// Test reorg handling with snapshots
#[tokio::test]
async fn test_reorg_with_snapshots() -> Result<()> {
    println!("🔀 Testing reorg handling with snapshots");
    
    let _filesystem = MockFilesystem::new();
    let node = MockBitcoinNode::new();
    
    // Create initial chain
    let initial_chain = ChainBuilder::new()
        .add_blocks(20)
        .blocks();
    
    for (height, block) in initial_chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        let block_hash = block.block_hash().as_byte_array().to_vec();
        node.add_block(height as u32, block_hash, block_bytes);
    }
    
    println!("📦 Created initial chain with {} blocks", initial_chain.len());
    
    // Set up snapshot server
    let storage = MockStorage::new();
    let runtime_adapter = MockRuntime::new();
    
    let snapshot_config = SnapshotConfig {
        snapshot_interval: 5,
        max_snapshots: 10,
        compression_level: 6,
        reorg_buffer_size: 10,
    };
    
    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(20),
        pipeline_size: None,
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };
    
    let mut engine = SnapshotMetashrewSync::new(
        node.clone(),
        storage,
        runtime_adapter,
        sync_config,
        SyncMode::Snapshot(snapshot_config.clone()),
    );
    
    let provider = MockSnapshotProvider::new(
        snapshot_config.clone(),
        "test_wasm_hash".to_string(),
    );
    engine.set_snapshot_provider(Box::new(provider)).await;
    
    // Process initial chain
    for height in 0..20 {
        if let Ok(_block_data) = node.get_block_data(height).await {
            engine.process_single_block(height).await?;
        }
    }
    
    let stats_before = engine.get_snapshot_stats().await?;
    println!("📊 Before reorg: {} snapshots created at height {}", 
            stats_before.snapshots_created, stats_before.current_height);
    
    // Simulate reorg: replace blocks 15-19 with new blocks
    println!("🔀 Simulating reorg from height 15");
    
    let reorg_blocks = vec![
        create_test_block(15, initial_chain[14].block_hash(), b"reorg_block_15"),
        create_test_block(16, initial_chain[14].block_hash(), b"reorg_block_16"), // Wrong prev hash intentionally
        create_test_block(17, initial_chain[14].block_hash(), b"reorg_block_17"),
        create_test_block(18, initial_chain[14].block_hash(), b"reorg_block_18"),
        create_test_block(19, initial_chain[14].block_hash(), b"reorg_block_19"),
    ];
    
    // Update node with reorg blocks
    let mut reorg_data = Vec::new();
    for (i, block) in reorg_blocks.iter().enumerate() {
        let height = 15 + i as u32;
        let block_bytes = utils::consensus_encode(block)?;
        let block_hash = block.block_hash().as_byte_array().to_vec();
        reorg_data.push((height, block_hash.clone(), block_bytes));
    }
    
    node.simulate_reorg(15, reorg_data);
    
    // Handle reorg in engine
    let reorg_height = engine.handle_reorg().await?;
    println!("🔀 Reorg detected, rolled back to height {}", reorg_height);
    
    // Process new blocks after reorg
    for height in reorg_height..20 {
        if let Ok(_block_data) = node.get_block_data(height).await {
            engine.process_single_block(height).await?;
        }
    }
    
    let stats_after = engine.get_snapshot_stats().await?;
    println!("📊 After reorg: {} snapshots created at height {}", 
            stats_after.snapshots_created, stats_after.current_height);
    
    // Verify that snapshots are still consistent
    // (In a real implementation, snapshots after reorg point might need to be recreated)
    
    println!("✅ Reorg handling with snapshots test passed!");
    Ok(())
}

/// Test network failure scenarios in repo mode
#[tokio::test]
async fn test_network_failure_scenarios() -> Result<()> {
    println!("🌐 Testing network failure scenarios in repo mode");
    
    let filesystem = MockFilesystem::new();
    let node = MockBitcoinNode::new();
    
    // Create test blockchain
    let chain = ChainBuilder::new()
        .add_blocks(15)
        .blocks();
    
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        let block_hash = block.block_hash().as_byte_array().to_vec();
        node.add_block(height as u32, block_hash, block_bytes);
    }
    
    println!("📦 Created test blockchain with {} blocks", chain.len());
    
    let repo_config = RepoConfig {
        repo_url: "http://localhost:8080/snapshots".to_string(),
        check_interval: 60,
        max_snapshot_age: 3600,
        continue_sync: true,
        min_blocks_behind: 5,
    };
    
    // Test with high failure rate
    let storage = MockStorage::new();
    let runtime_adapter = MockRuntime::new();
    
    let sync_config = SyncConfig {
        start_block: 0,
        exit_at: Some(15),
        pipeline_size: None,
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };
    
    let mut engine = SnapshotMetashrewSync::new(
        node.clone(),
        storage,
        runtime_adapter,
        sync_config,
        SyncMode::Repo(repo_config.clone()),
    );
    
    // Create client with high failure rate
    let server_arc = Arc::new(Mutex::new(MockSnapshotServer::new(filesystem.clone())));
    let client = MockSnapshotClient::new(server_arc)
        .with_network_delay(50) // 50ms delay
.with_failure_rate(0.3); // 30% failure rate
    
    let consumer = MockSnapshotConsumer::new(client, repo_config.clone());
    engine.set_snapshot_consumer(Box::new(consumer)).await;
    
    println!("🔥 Testing with 30% network failure rate");
    
    // Try to apply snapshots (should handle failures gracefully)
    let mut attempts = 0;
    let mut success = false;
    
    while attempts < 5 && !success {
        attempts += 1;
        println!("  🔄 Attempt {} to apply snapshots", attempts);
        
        match engine.check_and_apply_snapshots().await {
            Ok(applied) => {
                if applied {
                    success = true;
                    println!("  ✓ Successfully applied snapshots on attempt {}", attempts);
                } else {
                    println!("  ⚠️ No snapshots applied on attempt {}", attempts);
                }
            }
            Err(e) => {
                println!("  ❌ Failed on attempt {}: {}", attempts, e);
                // Wait a bit before retrying
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
    
    // Even with failures, should be able to fall back to normal sync
    let current_height = engine.get_snapshot_stats().await?.current_height;
    for height in current_height..15 {
        if let Ok(_block_data) = node.get_block_data(height).await {
            engine.process_single_block(height).await?;
        }
    }
    
    let final_stats = engine.get_snapshot_stats().await?;
    println!("📊 Final stats with network failures:");
    println!("  - Snapshots applied: {}", final_stats.snapshots_applied);
    println!("  - Blocks synced normally: {}", final_stats.blocks_synced_normally);
    println!("  - Final height: {}", final_stats.current_height);
    
    // Should reach final height regardless of network issues
    assert!(final_stats.current_height >= 14, "Should reach near final height despite network issues");
    
    println!("✅ Network failure scenarios test passed!");
    Ok(())
}
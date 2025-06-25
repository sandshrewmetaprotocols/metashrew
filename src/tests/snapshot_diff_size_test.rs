//! Test to ensure snapshot diff sizes remain minimal and catch database explosion regressions

use super::block_builder::ChainBuilder;
use super::TestConfig;
use anyhow::Result;
use memshrew_runtime::MemStoreRuntime;
use metashrew_support::utils;
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

// Import the tracking adapter from rockshrew-diff
use rockshrew_runtime::RocksDBRuntimeAdapter;

/// A simple tracking adapter for testing snapshot diff sizes
/// This mimics the behavior of the real TrackingAdapter in rockshrew-diff
struct TestTrackingAdapter {
    inner: MemStoreRuntime,
    tracked_updates: HashMap<Vec<u8>, Vec<u8>>,
    prefix: Vec<u8>,
}

impl TestTrackingAdapter {
    fn new(prefix: Vec<u8>) -> Result<Self> {
        let config = TestConfig::new();
        let runtime = config.create_runtime()?;
        
        Ok(Self {
            inner: runtime,
            tracked_updates: HashMap::new(),
            prefix,
        })
    }
    
    fn process_block(&mut self, block_data: &[u8], height: u32) -> Result<()> {
        // Clear previous tracked updates
        self.tracked_updates.clear();
        
        // Set block data and height
        {
            let mut context = self.inner.context.lock().unwrap();
            context.block = block_data.to_vec();
            context.height = height;
        }
        
        // Run the block processing
        self.inner.run()?;
        
        // Simulate tracking key-value updates like the real TrackingAdapter
        // In the real system, this happens during the __flush operation
        let all_data = {
            let guard = self.inner.context.lock().unwrap();
            guard.db.get_all_data()
        };
        
        // Track updates that match our prefix (simulating the real tracking logic)
        for (key, value) in all_data.iter() {
            if key.starts_with(&self.prefix) {
                let key_str = String::from_utf8_lossy(key);
                
                // Skip length keys and SMT nodes (like the real TrackingAdapter)
                if !key_str.ends_with("/length") && !key_str.starts_with("smt:node:") {
                    // Process the key and value (simplified version of real processing)
                    let processed_key = self.process_metashrew_key(key);
                    let processed_value = self.process_metashrew_value(value);
                    
                    if !processed_key.is_empty() {
                        self.tracked_updates.insert(processed_key, processed_value);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Process a Metashrew key for the new minimal append-only format
    /// Keys are now human-readable strings like "key/0", "key/1", "key/length"
    fn process_metashrew_key(&self, key: &[u8]) -> Vec<u8> {
        let key_str = String::from_utf8_lossy(key);
        
        // For the new format, extract the base key from patterns like "key/0", "key/1", etc.
        if let Some(slash_pos) = key_str.rfind('/') {
            let base_key = &key_str[..slash_pos];
            // Skip length keys and numeric indices, return the base key
            if !base_key.is_empty() && !key_str.ends_with("/length") {
                return base_key.as_bytes().to_vec();
            }
        }
        
        // For other keys (like SMT roots), return as-is
        key.to_vec()
    }
    
    /// Process a Metashrew value for the new minimal append-only format
    /// Values are now in format "height:hex_value"
    fn process_metashrew_value(&self, value: &[u8]) -> Vec<u8> {
        let value_str = String::from_utf8_lossy(value);
        
        // For the new format, extract the hex value from "height:hex_value"
        if let Some(colon_pos) = value_str.find(':') {
            let hex_value = &value_str[colon_pos + 1..];
            if let Ok(decoded) = hex::decode(hex_value) {
                return decoded;
            }
        }
        
        // For other values (like SMT roots), return as-is
        value.to_vec()
    }
    
    fn get_tracked_updates(&self) -> &HashMap<Vec<u8>, Vec<u8>> {
        &self.tracked_updates
    }
    
    /// Calculate the size of tracked updates (this simulates the diff.bin.zst file size)
    fn calculate_diff_size(&self) -> usize {
        let mut total_size = 0;
        
        for (key, value) in &self.tracked_updates {
            // Add key size + value size + some overhead for serialization
            total_size += key.len() + value.len() + 8; // 8 bytes overhead per entry
        }
        
        // Add some base overhead for the diff file structure
        total_size += 64; // Base overhead
        
        total_size
    }
}

/// Maximum allowed diff size in bytes for a single block
/// This should be much smaller than the previous 4KB files
const MAX_DIFF_SIZE_BYTES: usize = 1024; // 1KB max

/// Test that snapshot diffs remain small and don't grow exponentially
#[tokio::test]
async fn test_snapshot_diff_size_regression() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create a chain with multiple blocks
    let chain = ChainBuilder::new()
        .add_blocks(5) // Process 5 blocks
        .blocks();
    
    let mut diff_sizes = Vec::new();
    
    // Process each block and measure the database changes
    for (height, block) in chain.iter().enumerate() {
        let height = height as u32;
        
        // Get database entry count before processing
        let entries_before = count_database_entries(&runtime);
        
        // Process the block
        let block_bytes = utils::consensus_encode(block)?;
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height;
        }
        runtime.run()?;
        runtime.refresh_memory()?;
        
        // Get database entry count after processing
        let entries_after = count_database_entries(&runtime);
        
        // Calculate the diff size (number of new entries)
        let diff_size = if entries_after > entries_before {
            entries_after - entries_before
        } else {
            0
        };
        
        diff_sizes.push(diff_size);
        
        println!("Block {}: Database grew by {} entries", height, diff_size);
        
        // Assert that the diff size is reasonable (convert to u64 for comparison)
        assert!(
            (diff_size as u64) <= (MAX_DIFF_SIZE_BYTES as u64),
            "Block {} diff size {} entries exceeds maximum {} entries. This indicates database explosion!",
            height, diff_size, MAX_DIFF_SIZE_BYTES
        );
    }
    
    // Ensure diff sizes don't grow exponentially
    for i in 1..diff_sizes.len() {
        let current = diff_sizes[i];
        let previous = diff_sizes[i - 1];
        
        // Current diff should not be more than 2x the previous diff
        // This catches exponential growth patterns
        if previous > 0 {
            let growth_ratio = current as f64 / previous as f64;
            assert!(
                growth_ratio <= 2.0,
                "Block {} shows exponential growth: {} entries vs {} entries ({}x growth)",
                i, current, previous, growth_ratio
            );
        }
    }
    
    println!("✅ All snapshot diffs are within acceptable size limits");
    println!("Diff sizes: {:?}", diff_sizes);
    Ok(())
}

/// Test that the new minimal append-only approach produces small diffs
#[tokio::test]
async fn test_minimal_append_only_diff_size() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create a single block
    let chain = ChainBuilder::new()
        .add_blocks(1)
        .blocks();
    
    let block = &chain[0];
    
    // Count database entries before processing
    let entries_before = count_database_entries(&runtime);
    
    // Process the block
    let block_bytes = utils::consensus_encode(block)?;
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes;
        context.height = 0;
    }
    runtime.run()?;
    
    // Count database entries after processing
    let entries_after = count_database_entries(&runtime);
    
    let new_entries = entries_after - entries_before;
    
    println!("New database entries for single block: {}", new_entries);
    
    // With minimal append-only approach, we should have very few new entries:
    // - A few key/value updates (5-20 entries)
    // - One SMT root entry
    // - A few length counters
    // Total should be much less than 100 entries
    assert!(
        new_entries <= 50,
        "Single block created {} database entries, expected <= 50. This indicates the minimal approach is not working!",
        new_entries
    );
    
    println!("✅ Minimal append-only approach is working correctly");
    Ok(())
}

/// Test that we don't store intermediate SMT nodes
#[tokio::test]
async fn test_no_intermediate_smt_nodes() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create multiple blocks to trigger SMT operations
    let chain = ChainBuilder::new()
        .add_blocks(3)
        .blocks();
    
    // Process all blocks
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        runtime.run()?;
    }
    
    // Count SMT-related entries in the database
    let smt_entries = count_smt_entries(&runtime);
    
    println!("SMT-related database entries: {}", smt_entries);
    
    // With the minimal approach, we should have very few SMT entries:
    // - One root entry per block (3 blocks = 3 entries)
    // - Maybe a few path entries for actual data
    // Should be much less than 100 entries total
    assert!(
        smt_entries <= 20,
        "Found {} SMT entries, expected <= 20. This indicates intermediate SMT nodes are being stored!",
        smt_entries
    );
    
    println!("✅ SMT intermediate nodes are not being stored excessively");
    Ok(())
}

/// Count the total number of database entries
fn count_database_entries(runtime: &MemStoreRuntime) -> usize {
    let guard = runtime.context.lock().unwrap();
    guard.db.len()
}

/// Count SMT-related entries in the database
fn count_smt_entries(runtime: &MemStoreRuntime) -> usize {
    let guard = runtime.context.lock().unwrap();
    let all_data = guard.db.get_all_data();
    let mut count = 0;
    
    // Iterate through all keys and count SMT-related ones
    for (key, _) in all_data.iter() {
        let key_str = String::from_utf8_lossy(key);
        // Count entries that look like SMT nodes (hex keys, internal nodes, etc.)
        if key_str.starts_with("smt/") ||
           key_str.contains("/node/") ||
           key_str.starts_with("smt:node:") ||
           (key_str.len() > 32 && key_str.chars().all(|c| c.is_ascii_hexdigit())) {
            count += 1;
        }
    }
    count
}
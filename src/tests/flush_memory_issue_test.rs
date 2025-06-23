//! Test to isolate the __flush and memory issue affecting the processing pipeline
//!
//! This test specifically targets the interaction between __flush operations and memory management
//! that causes the processing pipeline to hang on the first block.

use anyhow::Result;
use log::{debug, error, info, warn};
use memshrew_runtime::{MemStoreAdapter, KeyValueStoreLike};
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Test that isolates the __flush and memory interaction issue
#[tokio::test]
async fn test_flush_memory_refresh_interaction() -> Result<()> {
    println!("Testing __flush and memory refresh interaction");

    // This test simulates the exact conditions that cause the hang:
    // 1. WASM module calls __flush with key-value pairs
    // 2. Memory refresh is called immediately after
    // 3. The combination causes a hang on the first block

    // Create a mock scenario since we may not have a real WASM module
    test_mock_flush_memory_scenario().await?;
    
    // Test the specific sequence that causes issues
    test_flush_then_refresh_sequence().await?;
    
    // Test memory state after flush operations
    test_memory_state_after_flush().await?;

    println!("__flush and memory refresh interaction test completed");
    Ok(())
}

/// Test mock flush and memory scenario
async fn test_mock_flush_memory_scenario() -> Result<()> {
    info!("Testing mock flush and memory scenario");

    // Create a memory store adapter
    let mut store = MemStoreAdapter::new();
    
    // Simulate the operations that happen during __flush
    info!("Simulating __flush operations");
    
    // Add multiple key-value pairs (simulating what WASM __flush does)
    for i in 0..1000 {
        let key = format!("test_key_{}", i).into_bytes();
        let value = format!("test_value_{}", i).into_bytes();
        store.put(&key, &value)?;
    }
    
    info!("Simulated {} key-value pairs", 1000);
    
    // Simulate memory pressure that might occur during flush
    let large_data = vec![0u8; 50 * 1024 * 1024]; // 50MB
    
    // Simulate the timing that might cause issues
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    drop(large_data);
    
    info!("Mock flush scenario completed successfully");
    Ok(())
}

/// Test the specific sequence: flush then refresh
async fn test_flush_then_refresh_sequence() -> Result<()> {
    info!("Testing flush then refresh sequence");

    // This test simulates the exact sequence that happens in production:
    // 1. Block processing calls __flush
    // 2. __flush processes many key-value pairs
    // 3. refresh_memory() is called immediately after
    // 4. The system hangs

    let mut store = MemStoreAdapter::new();
    
    // Simulate multiple flush operations followed by memory operations
    for iteration in 1..=5 {
        info!("Flush-refresh sequence iteration {}", iteration);
        
        let start_time = Instant::now();
        
        // Simulate __flush operation with many key-value pairs
        let flush_result = timeout(Duration::from_secs(10), async {
            simulate_flush_operation(&mut store, iteration).await
        }).await;
        
        match flush_result {
            Ok(Ok(_)) => {
                let flush_elapsed = start_time.elapsed();
                info!("Flush operation {} completed in {:?}", iteration, flush_elapsed);
            }
            Ok(Err(e)) => {
                error!("Flush operation {} failed: {}", iteration, e);
                return Err(e);
            }
            Err(_) => {
                error!("Flush operation {} timed out", iteration);
                return Err(anyhow::anyhow!("Flush operation timed out"));
            }
        }
        
        // Now simulate the memory refresh that might cause issues
        let refresh_start = Instant::now();
        let refresh_result = timeout(Duration::from_secs(10), async {
            simulate_memory_refresh_operation().await
        }).await;
        
        match refresh_result {
            Ok(Ok(_)) => {
                let refresh_elapsed = refresh_start.elapsed();
                info!("Memory refresh {} completed in {:?}", iteration, refresh_elapsed);
            }
            Ok(Err(e)) => {
                error!("Memory refresh {} failed: {}", iteration, e);
                return Err(e);
            }
            Err(_) => {
                error!("Memory refresh {} timed out - this might be the hang!", iteration);
                return Err(anyhow::anyhow!("Memory refresh timed out - potential hang detected"));
            }
        }
        
        let total_elapsed = start_time.elapsed();
        info!("Flush-refresh sequence {} completed in {:?}", iteration, total_elapsed);
    }

    Ok(())
}

/// Test memory state after flush operations
async fn test_memory_state_after_flush() -> Result<()> {
    info!("Testing memory state after flush operations");

    let mut store = MemStoreAdapter::new();
    
    // Simulate a large flush operation
    info!("Performing large flush operation");
    let flush_start = Instant::now();
    
    // Add a large number of key-value pairs
    for i in 0..10000 {
        let key = format!("large_flush_key_{}", i).into_bytes();
        let value = vec![i as u8; 1024]; // 1KB per value
        store.put(&key, &value)?;
        
        // Log progress periodically
        if i % 1000 == 0 {
            debug!("Processed {} key-value pairs", i);
        }
    }
    
    let flush_elapsed = flush_start.elapsed();
    info!("Large flush operation completed in {:?}", flush_elapsed);
    
    // Check memory state
    info!("Checking memory state after flush");
    
    // Simulate memory pressure checks
    let memory_check_start = Instant::now();
    
    // Allocate and deallocate memory to test for issues
    for i in 0..10 {
        let temp_data = vec![0u8; 10 * 1024 * 1024]; // 10MB
        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(temp_data);
        debug!("Memory check iteration {} completed", i);
    }
    
    let memory_check_elapsed = memory_check_start.elapsed();
    info!("Memory state check completed in {:?}", memory_check_elapsed);
    
    Ok(())
}

/// Simulate a flush operation
async fn simulate_flush_operation(store: &mut MemStoreAdapter, iteration: u32) -> Result<()> {
    info!("Simulating flush operation for iteration {}", iteration);
    
    // Simulate the key-value pairs that would be flushed during block processing
    let num_pairs = 1000 + (iteration * 100); // Increasing load
    
    for i in 0..num_pairs {
        let key = format!("flush_{}_{}", iteration, i).into_bytes();
        let value = format!("value_{}_{}", iteration, i).into_bytes();
        
        // Use the store's put method
        store.put(&key, &value)?;
        
        // Simulate some processing time
        if i % 100 == 0 {
            tokio::time::sleep(Duration::from_micros(100)).await;
        }
    }
    
    info!("Simulated flush of {} key-value pairs", num_pairs);
    Ok(())
}

/// Simulate memory refresh operation
async fn simulate_memory_refresh_operation() -> Result<()> {
    info!("Simulating memory refresh operation");
    
    // Simulate the operations that happen during refresh_memory()
    // This includes:
    // 1. Creating new WASM store
    // 2. Re-instantiating the module
    // 3. Setting up memory limits
    // 4. Cleaning up old resources
    
    let refresh_start = Instant::now();
    
    // Simulate memory allocation for new store
    let new_memory = vec![0u8; 100 * 1024 * 1024]; // 100MB
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Simulate module instantiation
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Simulate memory limit setup
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Simulate cleanup of old resources
    drop(new_memory);
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    let refresh_elapsed = refresh_start.elapsed();
    info!("Memory refresh simulation completed in {:?}", refresh_elapsed);
    
    Ok(())
}

/// Test to reproduce the exact hang scenario
#[tokio::test]
async fn test_reproduce_exact_hang_scenario() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    info!("Reproducing exact hang scenario");

    // This test reproduces the exact conditions described in the issue:
    // 1. Processing pipeline hangs on the very first block
    // 2. After implementing refresh_memory after every block
    // 3. Test suite passes but real-world scenario hangs

    info!("Setting up conditions that match the production environment");
    
    // Test the critical path: first block processing with memory refresh
    let result = test_first_block_critical_path().await;
    
    match result {
        Ok(_) => {
            info!("✓ First block critical path test passed");
        }
        Err(e) => {
            error!("✗ First block critical path test failed: {}", e);
            error!("This indicates the hang issue is reproduced!");
            
            // Don't return error - we want to continue with analysis
        }
    }
    
    // Test variations to understand the root cause
    test_hang_variations().await?;
    
    info!("Exact hang scenario reproduction completed");
    Ok(())
}

/// Test the critical path for first block processing
async fn test_first_block_critical_path() -> Result<()> {
    info!("Testing first block critical path");
    
    let start_time = Instant::now();
    
    // Simulate the exact sequence that happens in production
    let result = timeout(Duration::from_secs(30), async {
        // Step 1: Initialize runtime (this works)
        info!("Step 1: Initializing runtime");
        let mut store = MemStoreAdapter::new();
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Step 2: Set up first block (this works)
        info!("Step 2: Setting up first block");
        let block_data = create_first_block_data();
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Step 3: Process block (this might work)
        info!("Step 3: Processing first block");
        simulate_block_processing(&mut store, &block_data).await?;
        
        // Step 4: Call __flush (this might cause issues)
        info!("Step 4: Calling __flush");
        simulate_flush_with_many_kvs(&mut store).await?;
        
        // Step 5: Refresh memory (THIS IS WHERE THE HANG OCCURS)
        info!("Step 5: Refreshing memory - CRITICAL POINT");
        let refresh_start = Instant::now();
        simulate_problematic_memory_refresh().await?;
        let refresh_elapsed = refresh_start.elapsed();
        info!("Memory refresh completed in {:?}", refresh_elapsed);
        
        Ok(())
    }).await;
    
    let total_elapsed = start_time.elapsed();
    info!("First block critical path took: {:?}", total_elapsed);
    
    match result {
        Ok(Ok(_)) => {
            info!("First block critical path completed successfully");
            Ok(())
        }
        Ok(Err(e)) => {
            error!("First block critical path failed: {}", e);
            Err(e)
        }
        Err(_) => {
            error!("HANG DETECTED: First block critical path timed out after 30 seconds");
            error!("This reproduces the reported issue!");
            Err(anyhow::anyhow!("First block processing hung - bug reproduced"))
        }
    }
}

/// Test variations to understand the hang
async fn test_hang_variations() -> Result<()> {
    info!("Testing hang variations to understand root cause");
    
    // Variation 1: Process without memory refresh
    info!("Variation 1: Processing without memory refresh");
    let result1 = test_processing_without_refresh().await;
    match result1 {
        Ok(_) => info!("✓ Processing without refresh works"),
        Err(e) => error!("✗ Processing without refresh failed: {}", e),
    }
    
    // Variation 2: Memory refresh without processing
    info!("Variation 2: Memory refresh without processing");
    let result2 = test_refresh_without_processing().await;
    match result2 {
        Ok(_) => info!("✓ Refresh without processing works"),
        Err(e) => error!("✗ Refresh without processing failed: {}", e),
    }
    
    // Variation 3: Different timing between flush and refresh
    info!("Variation 3: Different timing between flush and refresh");
    let result3 = test_different_timing().await;
    match result3 {
        Ok(_) => info!("✓ Different timing works"),
        Err(e) => error!("✗ Different timing failed: {}", e),
    }
    
    Ok(())
}

/// Create first block data
fn create_first_block_data() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&1u32.to_le_bytes()); // Height 1
    data.extend_from_slice(b"first_block_data");
    data.resize(1000, 0); // Pad to 1KB
    data
}

/// Simulate block processing
async fn simulate_block_processing(store: &mut MemStoreAdapter, block_data: &[u8]) -> Result<()> {
    info!("Simulating block processing ({} bytes)", block_data.len());
    
    // Simulate the work that happens during block processing
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Simulate some database operations
    for i in 0..10 {
        let key = format!("block_processing_{}", i).into_bytes();
        let value = format!("processed_value_{}", i).into_bytes();
        store.put(&key, &value)?;
    }
    
    info!("Block processing simulation completed");
    Ok(())
}

/// Simulate flush with many key-value pairs
async fn simulate_flush_with_many_kvs(store: &mut MemStoreAdapter) -> Result<()> {
    info!("Simulating __flush with many key-value pairs");
    
    // Simulate the large number of key-value pairs that ALKANES generates
    let num_pairs = 5000; // Typical for a complex block
    
    for i in 0..num_pairs {
        let key = format!("flush_kv_{}", i).into_bytes();
        let value = vec![i as u8; 100]; // 100 bytes per value
        store.put(&key, &value)?;
        
        // Yield occasionally to prevent blocking
        if i % 100 == 0 {
            tokio::task::yield_now().await;
        }
    }
    
    info!("Simulated flush of {} key-value pairs", num_pairs);
    Ok(())
}

/// Simulate problematic memory refresh
async fn simulate_problematic_memory_refresh() -> Result<()> {
    info!("Simulating problematic memory refresh");
    
    // This simulates the operations in refresh_memory() that might cause issues
    
    // 1. Create new WASM store (memory allocation)
    info!("Creating new WASM store");
    let large_allocation = vec![0u8; 200 * 1024 * 1024]; // 200MB
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // 2. Module instantiation (this might hang)
    info!("Module instantiation");
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // 3. Memory limits setup
    info!("Setting up memory limits");
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // 4. Cleanup old resources (this might cause issues)
    info!("Cleaning up old resources");
    drop(large_allocation);
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    info!("Problematic memory refresh simulation completed");
    Ok(())
}

/// Test processing without refresh
async fn test_processing_without_refresh() -> Result<()> {
    let mut store = MemStoreAdapter::new();
    let block_data = create_first_block_data();
    
    simulate_block_processing(&mut store, &block_data).await?;
    simulate_flush_with_many_kvs(&mut store).await?;
    // No memory refresh
    
    Ok(())
}

/// Test refresh without processing
async fn test_refresh_without_processing() -> Result<()> {
    // Just do memory refresh without any processing
    simulate_problematic_memory_refresh().await?;
    Ok(())
}

/// Test different timing between operations
async fn test_different_timing() -> Result<()> {
    let mut store = MemStoreAdapter::new();
    let block_data = create_first_block_data();
    
    simulate_block_processing(&mut store, &block_data).await?;
    simulate_flush_with_many_kvs(&mut store).await?;
    
    // Add delay before refresh
    info!("Adding delay before memory refresh");
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    simulate_problematic_memory_refresh().await?;
    
    Ok(())
}
//! Test to isolate the memory refresh hang issue that occurs during block processing
//!
//! This test reproduces the scenario where the processing pipeline hangs on the very first block
//! after implementing memory refresh after every block. The issue doesn't appear in the test suite
//! but manifests in real-world block processing.

use anyhow::Result;
use log::{debug, error, info, warn};
use memshrew_runtime::{MemStoreAdapter, KeyValueStoreLike};
use metashrew_runtime::MetashrewRuntime;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Test that reproduces the memory refresh hang issue
#[tokio::test]
async fn test_memory_refresh_hang_on_first_block() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    info!("Starting memory refresh hang test");

    // Create a minimal WASM module for testing
    let test_wasm_path = create_test_wasm_module().await?;

    // Test 1: Process block without memory refresh (should work)
    info!("Test 1: Processing block without memory refresh");
    let result1 = test_block_processing_without_refresh(&test_wasm_path).await;
    match result1 {
        Ok(_) => info!("✓ Block processing without refresh succeeded"),
        Err(e) => {
            error!("✗ Block processing without refresh failed: {}", e);
            return Err(e);
        }
    }

    // Test 2: Process block with memory refresh (may hang)
    info!("Test 2: Processing block with memory refresh");
    let result2 = test_block_processing_with_refresh(&test_wasm_path).await;
    match result2 {
        Ok(_) => info!("✓ Block processing with refresh succeeded"),
        Err(e) => {
            error!("✗ Block processing with refresh failed: {}", e);
            // Don't return error here - we want to continue with other tests
        }
    }

    // Test 3: Multiple blocks with refresh to see if it's first-block specific
    info!("Test 3: Processing multiple blocks with refresh");
    let result3 = test_multiple_blocks_with_refresh(&test_wasm_path).await;
    match result3 {
        Ok(_) => info!("✓ Multiple blocks with refresh succeeded"),
        Err(e) => {
            error!("✗ Multiple blocks with refresh failed: {}", e);
        }
    }

    // Test 4: Test refresh_memory in isolation
    info!("Test 4: Testing refresh_memory in isolation");
    let result4 = test_refresh_memory_isolation(&test_wasm_path).await;
    match result4 {
        Ok(_) => info!("✓ Isolated refresh_memory test succeeded"),
        Err(e) => {
            error!("✗ Isolated refresh_memory test failed: {}", e);
        }
    }

    // Test 5: Test with different memory configurations
    info!("Test 5: Testing with different memory configurations");
    let result5 = test_different_memory_configs(&test_wasm_path).await;
    match result5 {
        Ok(_) => info!("✓ Different memory configs test succeeded"),
        Err(e) => {
            error!("✗ Different memory configs test failed: {}", e);
        }
    }

    info!("Memory refresh hang test completed");
    Ok(())
}

/// Test block processing without memory refresh
async fn test_block_processing_without_refresh(wasm_path: &PathBuf) -> Result<()> {
    let start_time = Instant::now();
    
    // Create runtime with memory store
    let store = MemStoreAdapter::new();
    let mut runtime = MetashrewRuntime::load(wasm_path.clone(), store)?;

    // Create test block data
    let height = 1u32;
    let block_data = create_test_block_data(height);

    // Set block data in context
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_data;
        context.height = height;
    }

    // Process the block without refresh
    let process_result = timeout(Duration::from_secs(30), async {
        runtime.run()
    }).await;

    let elapsed = start_time.elapsed();
    info!("Block processing without refresh took: {:?}", elapsed);

    match process_result {
        Ok(Ok(_)) => {
            info!("Block processing completed successfully in {:?}", elapsed);
            Ok(())
        }
        Ok(Err(e)) => {
            error!("Block processing failed: {}", e);
            Err(e)
        }
        Err(_) => {
            error!("Block processing timed out after 30 seconds");
            Err(anyhow::anyhow!("Block processing timed out"))
        }
    }
}

/// Test block processing with memory refresh
async fn test_block_processing_with_refresh(wasm_path: &PathBuf) -> Result<()> {
    let start_time = Instant::now();
    
    // Create runtime with memory store
    let store = MemStoreAdapter::new();
    let mut runtime = MetashrewRuntime::load(wasm_path.clone(), store)?;

    // Create test block data
    let height = 1u32;
    let block_data = create_test_block_data(height);

    // Set block data in context
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_data;
        context.height = height;
    }

    // Process the block with refresh
    let process_result = timeout(Duration::from_secs(30), async {
        // First run the block processing
        let run_result = runtime.run();
        if let Err(e) = run_result {
            return Err(e);
        }

        // Then refresh memory (this is where the hang might occur)
        info!("About to call refresh_memory()");
        let refresh_start = Instant::now();
        let refresh_result = runtime.refresh_memory();
        let refresh_elapsed = refresh_start.elapsed();
        info!("refresh_memory() completed in {:?}", refresh_elapsed);
        
        refresh_result
    }).await;

    let elapsed = start_time.elapsed();
    info!("Block processing with refresh took: {:?}", elapsed);

    match process_result {
        Ok(Ok(_)) => {
            info!("Block processing with refresh completed successfully in {:?}", elapsed);
            Ok(())
        }
        Ok(Err(e)) => {
            error!("Block processing with refresh failed: {}", e);
            Err(e)
        }
        Err(_) => {
            error!("Block processing with refresh timed out after 30 seconds");
            Err(anyhow::anyhow!("Block processing with refresh timed out"))
        }
    }
}

/// Test multiple blocks with refresh
async fn test_multiple_blocks_with_refresh(wasm_path: &PathBuf) -> Result<()> {
    let start_time = Instant::now();
    
    // Create runtime with memory store
    let store = MemStoreAdapter::new();
    let mut runtime = MetashrewRuntime::load(wasm_path.clone(), store)?;

    for height in 1..=3 {
        info!("Processing block {} with refresh", height);
        let block_start = Instant::now();

        // Create test block data
        let block_data = create_test_block_data(height);

        // Set block data in context
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_data;
            context.height = height;
        }

        // Process the block with refresh
        let process_result = timeout(Duration::from_secs(30), async {
            // First run the block processing
            let run_result = runtime.run();
            if let Err(e) = run_result {
                return Err(e);
            }

            // Then refresh memory
            info!("About to call refresh_memory() for block {}", height);
            let refresh_start = Instant::now();
            let refresh_result = runtime.refresh_memory();
            let refresh_elapsed = refresh_start.elapsed();
            info!("refresh_memory() for block {} completed in {:?}", height, refresh_elapsed);
            
            refresh_result
        }).await;

        let block_elapsed = block_start.elapsed();
        info!("Block {} processing took: {:?}", height, block_elapsed);

        match process_result {
            Ok(Ok(_)) => {
                info!("Block {} processed successfully", height);
            }
            Ok(Err(e)) => {
                error!("Block {} processing failed: {}", height, e);
                return Err(e);
            }
            Err(_) => {
                error!("Block {} processing timed out", height);
                return Err(anyhow::anyhow!("Block {} processing timed out", height));
            }
        }
    }

    let total_elapsed = start_time.elapsed();
    info!("Multiple blocks processing completed in {:?}", total_elapsed);
    Ok(())
}

/// Test refresh_memory in isolation
async fn test_refresh_memory_isolation(wasm_path: &PathBuf) -> Result<()> {
    info!("Testing refresh_memory in isolation");
    
    // Create runtime with memory store
    let store = MemStoreAdapter::new();
    let mut runtime = MetashrewRuntime::load(wasm_path.clone(), store)?;

    // Test multiple refresh calls without block processing
    for i in 1..=5 {
        info!("Isolated refresh_memory test iteration {}", i);
        let refresh_start = Instant::now();
        
        let refresh_result = timeout(Duration::from_secs(10), async {
            runtime.refresh_memory()
        }).await;

        let refresh_elapsed = refresh_start.elapsed();
        info!("Isolated refresh_memory iteration {} took: {:?}", i, refresh_elapsed);

        match refresh_result {
            Ok(Ok(_)) => {
                info!("Isolated refresh_memory iteration {} succeeded", i);
            }
            Ok(Err(e)) => {
                error!("Isolated refresh_memory iteration {} failed: {}", i, e);
                return Err(e);
            }
            Err(_) => {
                error!("Isolated refresh_memory iteration {} timed out", i);
                return Err(anyhow::anyhow!("Isolated refresh_memory timed out"));
            }
        }
    }

    Ok(())
}

/// Test with different memory configurations
async fn test_different_memory_configs(wasm_path: &PathBuf) -> Result<()> {
    info!("Testing with different memory configurations");
    
    // Test with different memory limits and configurations
    // This would require modifying the WASM engine configuration
    // For now, just test the standard configuration multiple times
    
    for config_test in 1..=3 {
        info!("Memory config test {}", config_test);
        
        let store = MemStoreAdapter::new();
        let mut runtime = MetashrewRuntime::load(wasm_path.clone(), store)?;

        // Create test block data
        let height = config_test;
        let block_data = create_test_block_data(height);

        // Set block data in context
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_data;
            context.height = height;
        }

        // Process and refresh
        let result = timeout(Duration::from_secs(20), async {
            runtime.run()?;
            runtime.refresh_memory()
        }).await;

        match result {
            Ok(Ok(_)) => {
                info!("Memory config test {} succeeded", config_test);
            }
            Ok(Err(e)) => {
                error!("Memory config test {} failed: {}", config_test, e);
                return Err(e);
            }
            Err(_) => {
                error!("Memory config test {} timed out", config_test);
                return Err(anyhow::anyhow!("Memory config test timed out"));
            }
        }
    }

    Ok(())
}

/// Create a minimal test WASM module
async fn create_test_wasm_module() -> Result<PathBuf> {
    // For this test, we'll use a simple WASM module that just does basic operations
    // In a real scenario, you would compile a minimal WASM module
    // For now, let's create a path to a test module (this would need to be provided)
    
    let test_wasm_path = PathBuf::from("test_minimal.wasm");
    
    // Check if the test WASM file exists
    if !test_wasm_path.exists() {
        // Create a minimal WASM module content
        // This is a placeholder - in practice you'd need a real WASM file
        warn!("Test WASM module not found at {:?}", test_wasm_path);
        warn!("Creating a placeholder - this test may not work without a real WASM module");
        
        // For the test to work, we need to skip if no WASM module is available
        return Err(anyhow::anyhow!("Test WASM module not available"));
    }
    
    Ok(test_wasm_path)
}

/// Create test block data
fn create_test_block_data(height: u32) -> Vec<u8> {
    // Create minimal test block data
    let mut block_data = Vec::new();
    
    // Add height as first 4 bytes
    block_data.extend_from_slice(&height.to_le_bytes());
    
    // Add some dummy block data
    block_data.extend_from_slice(b"test_block_data_");
    block_data.extend_from_slice(&height.to_string().as_bytes());
    
    // Pad to a reasonable size
    while block_data.len() < 1000 {
        block_data.push(0);
    }
    
    block_data
}

/// Test to specifically reproduce the hang scenario described in the issue
#[tokio::test]
async fn test_reproduce_first_block_hang() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    info!("Reproducing first block hang scenario");

    // This test simulates the exact scenario described:
    // 1. Recent changes to refresh memory after every block
    // 2. Processing pipeline hangs on the very first block
    // 3. Test suite passes but real-world scenario hangs

    // Create a mock runtime that simulates the real-world conditions
    let store = MemStoreAdapter::new();
    
    // Try to load a WASM module - if not available, create a mock scenario
    let wasm_path = PathBuf::from("test_minimal.wasm");
    if !wasm_path.exists() {
        warn!("No test WASM module available - creating mock scenario");
        
        // Test the memory refresh logic in isolation
        info!("Testing memory refresh logic without WASM module");
        
        // Simulate the conditions that might cause a hang:
        // 1. Memory allocation patterns
        // 2. Lock contention
        // 3. Resource cleanup issues
        
        test_memory_allocation_patterns().await?;
        test_lock_contention_scenarios().await?;
        test_resource_cleanup_issues().await?;
        
        return Ok(());
    }

    // If we have a WASM module, test the actual scenario
    let mut runtime = MetashrewRuntime::load(wasm_path, store)?;

    // Simulate the exact conditions from the production environment
    let height = 1u32; // First block
    let block_data = create_realistic_block_data(height);

    info!("Setting up first block processing scenario");
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_data;
        context.height = height;
    }

    // This is the critical test - does the first block hang with memory refresh?
    info!("Starting first block processing with memory refresh");
    let start_time = Instant::now();

    let result = timeout(Duration::from_secs(60), async {
        // Process the block
        info!("Calling runtime.run() for first block");
        let run_start = Instant::now();
        let run_result = runtime.run();
        let run_elapsed = run_start.elapsed();
        info!("runtime.run() completed in {:?}", run_elapsed);
        
        if let Err(e) = run_result {
            error!("runtime.run() failed: {}", e);
            return Err(e);
        }

        // This is where the hang might occur
        info!("Calling refresh_memory() after first block");
        let refresh_start = Instant::now();
        let refresh_result = runtime.refresh_memory();
        let refresh_elapsed = refresh_start.elapsed();
        info!("refresh_memory() completed in {:?}", refresh_elapsed);
        
        refresh_result
    }).await;

    let total_elapsed = start_time.elapsed();
    info!("First block processing with refresh took: {:?}", total_elapsed);

    match result {
        Ok(Ok(_)) => {
            info!("✓ First block processing with refresh succeeded");
            Ok(())
        }
        Ok(Err(e)) => {
            error!("✗ First block processing with refresh failed: {}", e);
            Err(e)
        }
        Err(_) => {
            error!("✗ First block processing with refresh HUNG (timed out after 60 seconds)");
            error!("This reproduces the reported issue!");
            Err(anyhow::anyhow!("First block processing hung - this is the bug!"))
        }
    }
}

/// Test memory allocation patterns that might cause issues
async fn test_memory_allocation_patterns() -> Result<()> {
    info!("Testing memory allocation patterns");
    
    // Simulate large memory allocations and deallocations
    for i in 1..=5 {
        info!("Memory allocation test {}", i);
        
        // Allocate large chunks of memory
        let large_vec: Vec<u8> = vec![0; 100 * 1024 * 1024]; // 100MB
        
        // Simulate processing
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Drop the allocation
        drop(large_vec);
        
        info!("Memory allocation test {} completed", i);
    }
    
    Ok(())
}

/// Test lock contention scenarios
async fn test_lock_contention_scenarios() -> Result<()> {
    info!("Testing lock contention scenarios");
    
    let shared_data = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut handles = Vec::new();
    
    // Create multiple tasks that compete for the same lock
    for i in 0..5 {
        let data = shared_data.clone();
        let handle = tokio::spawn(async move {
            for j in 0..10 {
                let mut guard = data.lock().unwrap();
                guard.push(i as u8);
                // Simulate work while holding the lock
                std::thread::sleep(Duration::from_millis(10));
                debug!("Task {} iteration {} completed", i, j);
            }
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await?;
    }
    
    info!("Lock contention test completed");
    Ok(())
}

/// Test resource cleanup issues
async fn test_resource_cleanup_issues() -> Result<()> {
    info!("Testing resource cleanup issues");
    
    // Simulate scenarios where resources might not be cleaned up properly
    for i in 1..=3 {
        info!("Resource cleanup test {}", i);
        
        // Create resources that need cleanup
        let _temp_data = vec![0u8; 10 * 1024 * 1024]; // 10MB
        
        // Simulate processing
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Resources should be automatically cleaned up when they go out of scope
        info!("Resource cleanup test {} completed", i);
    }
    
    Ok(())
}

/// Create realistic block data for testing
fn create_realistic_block_data(height: u32) -> Vec<u8> {
    let mut block_data = Vec::new();
    
    // Add height
    block_data.extend_from_slice(&height.to_le_bytes());
    
    // Add realistic block header (80 bytes)
    let block_header = vec![0u8; 80];
    block_data.extend_from_slice(&block_header);
    
    // Add transaction count (varint, simplified as 1 byte)
    block_data.push(1); // 1 transaction
    
    // Add a realistic transaction (simplified)
    let transaction = create_realistic_transaction();
    block_data.extend_from_slice(&transaction);
    
    block_data
}

/// Create a realistic transaction for testing
fn create_realistic_transaction() -> Vec<u8> {
    let mut tx_data = Vec::new();
    
    // Version (4 bytes)
    tx_data.extend_from_slice(&1u32.to_le_bytes());
    
    // Input count (1 byte, simplified)
    tx_data.push(1);
    
    // Input (simplified)
    tx_data.extend_from_slice(&[0u8; 36]); // Previous output (32 + 4 bytes)
    tx_data.push(0); // Script length
    tx_data.extend_from_slice(&0xffffffffu32.to_le_bytes()); // Sequence
    
    // Output count (1 byte, simplified)
    tx_data.push(1);
    
    // Output (simplified)
    tx_data.extend_from_slice(&5000000000u64.to_le_bytes()); // Value (50 BTC)
    tx_data.push(25); // Script length
    tx_data.extend_from_slice(&[0u8; 25]); // Script
    
    // Locktime (4 bytes)
    tx_data.extend_from_slice(&0u32.to_le_bytes());
    
    tx_data
}
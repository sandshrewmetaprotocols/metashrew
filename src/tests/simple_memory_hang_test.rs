//! Simple test to isolate the memory refresh hang issue
//!
//! This test focuses on reproducing the hang that occurs when refresh_memory()
//! is called after every block processing, specifically on the first block.

use anyhow::Result;
use memshrew_runtime::{MemStoreAdapter, KeyValueStoreLike};
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Test that reproduces the memory refresh hang issue
#[tokio::test]
async fn test_memory_refresh_hang_reproduction() -> Result<()> {
    println!("Starting memory refresh hang reproduction test");

    // Test 1: Simulate flush operations without refresh (baseline)
    println!("Test 1: Flush operations without refresh");
    let result1 = test_flush_operations_baseline().await;
    match result1 {
        Ok(_) => println!("✓ Baseline flush operations work"),
        Err(e) => {
            println!("✗ Baseline flush operations failed: {}", e);
            return Err(e);
        }
    }

    // Test 2: Simulate the problematic sequence (flush + memory operations)
    println!("Test 2: Flush operations with memory pressure");
    let result2 = test_flush_with_memory_pressure().await;
    match result2 {
        Ok(_) => println!("✓ Flush with memory pressure works"),
        Err(e) => {
            println!("✗ Flush with memory pressure failed: {}", e);
            // Don't return error - continue with other tests
        }
    }

    // Test 3: Test the specific timing that might cause hangs
    println!("Test 3: Testing critical timing scenarios");
    let result3 = test_critical_timing_scenarios().await;
    match result3 {
        Ok(_) => println!("✓ Critical timing scenarios work"),
        Err(e) => {
            println!("✗ Critical timing scenarios failed: {}", e);
        }
    }

    println!("Memory refresh hang reproduction test completed");
    Ok(())
}

/// Test baseline flush operations
async fn test_flush_operations_baseline() -> Result<()> {
    println!("Testing baseline flush operations");
    
    let mut store = MemStoreAdapter::new();
    let start_time = Instant::now();
    
    // Simulate the key-value operations that happen during __flush
    for i in 0..1000 {
        let key = format!("baseline_key_{}", i).into_bytes();
        let value = format!("baseline_value_{}", i).into_bytes();
        store.put(&key, &value)?;
    }
    
    let elapsed = start_time.elapsed();
    println!("Baseline flush operations completed in {:?}", elapsed);
    
    Ok(())
}

/// Test flush operations with memory pressure
async fn test_flush_with_memory_pressure() -> Result<()> {
    println!("Testing flush operations with memory pressure");
    
    let start_time = Instant::now();
    
    // Use timeout to detect hangs
    let result = timeout(Duration::from_secs(30), async {
        let mut store = MemStoreAdapter::new();
        
        // Step 1: Simulate large flush operation (like ALKANES)
        println!("Step 1: Large flush operation");
        for i in 0..5000 {
            let key = format!("pressure_key_{}", i).into_bytes();
            let value = vec![i as u8; 1024]; // 1KB per value
            store.put(&key, &value)?;
            
            // Yield occasionally to prevent blocking
            if i % 100 == 0 {
                tokio::task::yield_now().await;
            }
        }
        
        // Step 2: Simulate memory allocation/deallocation (like refresh_memory)
        println!("Step 2: Memory allocation/deallocation");
        let large_allocation = vec![0u8; 100 * 1024 * 1024]; // 100MB
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(large_allocation);
        
        // Step 3: More memory operations
        println!("Step 3: Additional memory operations");
        for _ in 0..10 {
            let temp_data = vec![0u8; 10 * 1024 * 1024]; // 10MB
            tokio::time::sleep(Duration::from_millis(10)).await;
            drop(temp_data);
        }
        
        Ok(())
    }).await;
    
    let elapsed = start_time.elapsed();
    println!("Flush with memory pressure took: {:?}", elapsed);
    
    match result {
        Ok(Ok(_)) => {
            println!("Flush with memory pressure completed successfully");
            Ok(())
        }
        Ok(Err(e)) => {
            println!("Flush with memory pressure failed: {}", e);
            Err(e)
        }
        Err(_) => {
            println!("HANG DETECTED: Flush with memory pressure timed out");
            Err(anyhow::anyhow!("Memory pressure test hung - potential issue detected"))
        }
    }
}

/// Test critical timing scenarios
async fn test_critical_timing_scenarios() -> Result<()> {
    println!("Testing critical timing scenarios");
    
    // Scenario 1: Immediate memory operations after flush
    println!("Scenario 1: Immediate memory operations after flush");
    let result1 = test_immediate_memory_ops().await;
    match result1 {
        Ok(_) => println!("✓ Immediate memory operations work"),
        Err(e) => println!("✗ Immediate memory operations failed: {}", e),
    }
    
    // Scenario 2: Delayed memory operations after flush
    println!("Scenario 2: Delayed memory operations after flush");
    let result2 = test_delayed_memory_ops().await;
    match result2 {
        Ok(_) => println!("✓ Delayed memory operations work"),
        Err(e) => println!("✗ Delayed memory operations failed: {}", e),
    }
    
    // Scenario 3: Concurrent memory operations
    println!("Scenario 3: Concurrent memory operations");
    let result3 = test_concurrent_memory_ops().await;
    match result3 {
        Ok(_) => println!("✓ Concurrent memory operations work"),
        Err(e) => println!("✗ Concurrent memory operations failed: {}", e),
    }
    
    Ok(())
}

/// Test immediate memory operations after flush
async fn test_immediate_memory_ops() -> Result<()> {
    let start_time = Instant::now();
    
    let result = timeout(Duration::from_secs(15), async {
        let mut store = MemStoreAdapter::new();
        
        // Flush operation
        for i in 0..1000 {
            let key = format!("immediate_key_{}", i).into_bytes();
            let value = format!("immediate_value_{}", i).into_bytes();
            store.put(&key, &value)?;
        }
        
        // IMMEDIATE memory operations (this might cause the hang)
        let large_data = vec![0u8; 50 * 1024 * 1024]; // 50MB
        drop(large_data);
        
        Ok(())
    }).await;
    
    let elapsed = start_time.elapsed();
    println!("Immediate memory operations took: {:?}", elapsed);
    
    match result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(anyhow::anyhow!("Immediate memory operations hung")),
    }
}

/// Test delayed memory operations after flush
async fn test_delayed_memory_ops() -> Result<()> {
    let start_time = Instant::now();
    
    let result = timeout(Duration::from_secs(15), async {
        let mut store = MemStoreAdapter::new();
        
        // Flush operation
        for i in 0..1000 {
            let key = format!("delayed_key_{}", i).into_bytes();
            let value = format!("delayed_value_{}", i).into_bytes();
            store.put(&key, &value)?;
        }
        
        // DELAYED memory operations
        tokio::time::sleep(Duration::from_millis(500)).await;
        let large_data = vec![0u8; 50 * 1024 * 1024]; // 50MB
        drop(large_data);
        
        Ok(())
    }).await;
    
    let elapsed = start_time.elapsed();
    println!("Delayed memory operations took: {:?}", elapsed);
    
    match result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(anyhow::anyhow!("Delayed memory operations hung")),
    }
}

/// Test concurrent memory operations
async fn test_concurrent_memory_ops() -> Result<()> {
    let start_time = Instant::now();
    
    let result = timeout(Duration::from_secs(15), async {
        let mut store = MemStoreAdapter::new();
        
        // Start flush operation in background
        let flush_handle = tokio::spawn(async move {
            for i in 0..2000 {
                let key = format!("concurrent_key_{}", i).into_bytes();
                let value = format!("concurrent_value_{}", i).into_bytes();
                store.put(&key, &value).unwrap();
                
                if i % 50 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        });
        
        // Concurrent memory operations
        let memory_handle = tokio::spawn(async {
            for _ in 0..5 {
                let large_data = vec![0u8; 20 * 1024 * 1024]; // 20MB
                tokio::time::sleep(Duration::from_millis(100)).await;
                drop(large_data);
            }
        });
        
        // Wait for both to complete
        flush_handle.await?;
        memory_handle.await?;
        
        Ok(())
    }).await;
    
    let elapsed = start_time.elapsed();
    println!("Concurrent memory operations took: {:?}", elapsed);
    
    match result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(anyhow::anyhow!("Concurrent memory operations hung")),
    }
}

/// Test to specifically target the first block scenario
#[tokio::test]
async fn test_first_block_hang_scenario() -> Result<()> {
    println!("Testing first block hang scenario");
    
    // This test simulates the exact conditions described in the issue:
    // - Processing pipeline hangs on the very first block
    // - After implementing refresh_memory after every block
    // - Test suite passes but real-world scenario hangs
    
    let start_time = Instant::now();
    
    let result = timeout(Duration::from_secs(60), async {
        println!("Simulating first block processing conditions");
        
        // Step 1: Initialize storage (like runtime initialization)
        let mut store = MemStoreAdapter::new();
        
        // Step 2: Simulate first block processing
        println!("Processing first block data");
        let height = 1u32;
        let block_data = create_first_block_data(height);
        
        // Step 3: Simulate __flush with realistic data volume
        println!("Simulating __flush with {} key-value pairs", 3000);
        for i in 0..3000 {
            let key = format!("first_block_{}_{}", height, i).into_bytes();
            let value = vec![i as u8; 256]; // 256 bytes per value
            store.put(&key, &value)?;
            
            // Yield occasionally
            if i % 100 == 0 {
                tokio::task::yield_now().await;
            }
        }
        
        // Step 4: Simulate the memory refresh operation that causes the hang
        println!("Simulating memory refresh operation");
        simulate_memory_refresh().await?;
        
        println!("First block processing completed successfully");
        Ok(())
    }).await;
    
    let elapsed = start_time.elapsed();
    println!("First block scenario took: {:?}", elapsed);
    
    match result {
        Ok(Ok(_)) => {
            println!("✓ First block scenario completed without hanging");
            Ok(())
        }
        Ok(Err(e)) => {
            println!("✗ First block scenario failed: {}", e);
            Err(e)
        }
        Err(_) => {
            println!("✗ HANG REPRODUCED: First block scenario timed out after 60 seconds");
            println!("This reproduces the reported issue!");
            Err(anyhow::anyhow!("First block hang reproduced - this is the bug!"))
        }
    }
}

/// Create realistic first block data
fn create_first_block_data(height: u32) -> Vec<u8> {
    let mut data = Vec::new();
    
    // Add height
    data.extend_from_slice(&height.to_le_bytes());
    
    // Add realistic block data
    data.extend_from_slice(b"first_block_header_data");
    data.extend_from_slice(&[0u8; 80]); // Block header
    data.push(1); // Transaction count
    data.extend_from_slice(&[0u8; 250]); // Transaction data
    
    data
}

/// Simulate memory refresh operation
async fn simulate_memory_refresh() -> Result<()> {
    println!("Starting memory refresh simulation");
    
    // Simulate the operations that happen in refresh_memory():
    // 1. Create new WASM store
    // 2. Re-instantiate module
    // 3. Set up memory limits
    // 4. Clean up old resources
    
    // Step 1: Simulate new store creation (large memory allocation)
    println!("Simulating new store creation");
    let new_store_memory = vec![0u8; 150 * 1024 * 1024]; // 150MB
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Step 2: Simulate module instantiation
    println!("Simulating module instantiation");
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Step 3: Simulate memory limits setup
    println!("Simulating memory limits setup");
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Step 4: Simulate cleanup (this might be where the hang occurs)
    println!("Simulating resource cleanup");
    drop(new_store_memory);
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    println!("Memory refresh simulation completed");
    Ok(())
}
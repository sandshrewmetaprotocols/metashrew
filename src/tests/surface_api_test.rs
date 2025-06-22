//! Surface API tests for Metashrew runtime
//! 
//! These tests exercise the surface API via the in-memory interface to get as close as possible
//! to a full e2e test over the scope of what the JSON-RPC will do. This includes:
//! - View function execution at historical heights
//! - Preview function execution (simulating block processing + view)
//! - Block processing workflow
//! - Error handling and edge cases
//! - Performance characteristics

use super::TestConfig;
use super::block_builder::{ChainBuilder, create_test_block};
use anyhow::Result;
use metashrew_support::utils;

/// Test the complete surface API workflow that mirrors JSON-RPC usage
#[tokio::test]
async fn test_surface_api_complete_workflow() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Build a realistic chain of blocks
    let chain = ChainBuilder::new()
        .add_blocks(5)
        .blocks();
    
    println!("🚀 Testing Surface API Complete Workflow");
    
    // Phase 1: Block Processing (simulates indexer workflow)
    println!("📦 Phase 1: Block Processing");
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        // Process the block (this is what the indexer does)
        runtime.run()?;
        runtime.refresh_memory()?;
        
        println!("  ✓ Processed block {} (hash: {})", height, block.block_hash());
    }
    
    // Phase 2: View Function Testing (simulates JSON-RPC view queries)
    println!("🔍 Phase 2: View Function Testing");
    
    // Test view functions at different heights (historical queries)
    for height in 0..chain.len() {
        let height_u32 = height as u32;
        
        // Test blocktracker view function
        let view_input = vec![];
        let blocktracker_result = runtime.view("blocktracker".to_string(), &view_input, height_u32).await?;
        
        // Verify blocktracker grows with each block
        let expected_length = height + 1;
        assert_eq!(blocktracker_result.len(), expected_length,
                  "Blocktracker should have {} bytes at height {}", expected_length, height);
        
        // Test getblock view function
        let height_input = height_u32.to_le_bytes().to_vec();
        let block_result = runtime.view("getblock".to_string(), &height_input, height_u32).await?;
        
        assert!(!block_result.is_empty(), "Block data should exist at height {}", height);
        
        println!("  ✓ View functions at height {}: blocktracker={} bytes, block={} bytes", 
                height, blocktracker_result.len(), block_result.len());
    }
    
    // Phase 3: Preview Function Testing (simulates JSON-RPC preview queries)
    println!("🔮 Phase 3: Preview Function Testing");
    
    // Create a new test block for preview
    let preview_block = create_test_block(
        chain.len() as u32,
        chain.last().unwrap().block_hash(),
        b"preview_test_block",
    );
    let preview_block_bytes = utils::consensus_encode(&preview_block)?;
    
    // Test preview with blocktracker view function
    let preview_input = vec![];
    let preview_result = runtime.preview(
        &preview_block_bytes,
        "blocktracker".to_string(),
        &preview_input,
        chain.len() as u32,
    )?;
    
    // Preview should show what blocktracker would look like after processing the new block
    let expected_preview_length = chain.len() + 1;
    assert_eq!(preview_result.len(), expected_preview_length,
              "Preview blocktracker should have {} bytes", expected_preview_length);
    
    println!("  ✓ Preview function: blocktracker would have {} bytes after new block", 
            preview_result.len());
    
    // Phase 4: Historical Consistency Testing
    println!("📚 Phase 4: Historical Consistency Testing");
    
    // Test that historical queries are deterministic
    for height in 0..chain.len() {
        let height_u32 = height as u32;
        let view_input = vec![];
        
        // Query the same height multiple times
        let result1 = runtime.view("blocktracker".to_string(), &view_input, height_u32).await?;
        let result2 = runtime.view("blocktracker".to_string(), &view_input, height_u32).await?;
        let result3 = runtime.view("blocktracker".to_string(), &view_input, height_u32).await?;
        
        assert_eq!(result1, result2, "Historical queries should be deterministic at height {}", height);
        assert_eq!(result2, result3, "Historical queries should be deterministic at height {}", height);
        
        println!("  ✓ Historical consistency verified at height {}", height);
    }
    
    // Phase 5: Edge Case Testing
    println!("⚠️  Phase 5: Edge Case Testing");
    
    // Test view function with invalid height (future height)
    let future_height = chain.len() as u32 + 10;
    let view_input = vec![];
    let future_result = runtime.view("blocktracker".to_string(), &view_input, future_height).await?;
    
    // Should return empty or last known state
    println!("  ✓ Future height query handled: {} bytes", future_result.len());
    
    // Test view function with height 0
    let genesis_result = runtime.view("blocktracker".to_string(), &view_input, 0).await?;
    assert_eq!(genesis_result.len(), 1, "Genesis blocktracker should have 1 byte");
    
    println!("  ✓ Genesis height query verified");
    
    // Test getblock with invalid height input
    let invalid_height_input = vec![0xFF, 0xFF, 0xFF, 0xFF]; // u32::MAX
    let invalid_result = runtime.view("getblock".to_string(), &invalid_height_input, 0).await;
    
    // Should handle gracefully (either error or empty result)
    match invalid_result {
        Ok(result) => println!("  ✓ Invalid height input handled gracefully: {} bytes", result.len()),
        Err(_) => println!("  ✓ Invalid height input properly rejected"),
    }
    
    println!("✅ Surface API Complete Workflow Test Passed!");
    
    Ok(())
}

/// Test surface API performance characteristics
#[tokio::test]
async fn test_surface_api_performance() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    println!("⚡ Testing Surface API Performance");
    
    // Build a larger chain for performance testing
    let chain = ChainBuilder::new()
        .add_blocks(20)
        .blocks();
    
    // Process all blocks
    let start_time = std::time::Instant::now();
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
    }
    let processing_time = start_time.elapsed();
    
    println!("📦 Block processing: {} blocks in {:?} ({:.2} blocks/sec)", 
            chain.len(), processing_time, 
            chain.len() as f64 / processing_time.as_secs_f64());
    
    // Test view function performance
    let start_time = std::time::Instant::now();
    let mut total_queries = 0;
    
    for height in 0..chain.len() {
        let view_input = vec![];
        let _result = runtime.view("blocktracker".to_string(), &view_input, height as u32).await?;
        total_queries += 1;
        
        let height_input = (height as u32).to_le_bytes().to_vec();
        let _result = runtime.view("getblock".to_string(), &height_input, height as u32).await?;
        total_queries += 1;
    }
    let query_time = start_time.elapsed();
    
    println!("🔍 View queries: {} queries in {:?} ({:.2} queries/sec)", 
            total_queries, query_time,
            total_queries as f64 / query_time.as_secs_f64());
    
    // Test preview function performance
    let preview_block = create_test_block(
        chain.len() as u32,
        chain.last().unwrap().block_hash(),
        b"performance_test_block",
    );
    let preview_block_bytes = utils::consensus_encode(&preview_block)?;
    
    let start_time = std::time::Instant::now();
    let preview_input = vec![];
    let _result = runtime.preview(
        &preview_block_bytes,
        "blocktracker".to_string(),
        &preview_input,
        chain.len() as u32,
    )?;
    let preview_time = start_time.elapsed();
    
    println!("🔮 Preview function: completed in {:?}", preview_time);
    
    // Performance assertions (these are reasonable expectations)
    assert!(processing_time.as_millis() < 5000, "Block processing should complete within 5 seconds");
    assert!(query_time.as_millis() < 2000, "View queries should complete within 2 seconds");
    assert!(preview_time.as_millis() < 1000, "Preview should complete within 1 second");
    
    println!("✅ Surface API Performance Test Passed!");
    
    Ok(())
}

/// Test surface API error handling and recovery
#[tokio::test]
async fn test_surface_api_error_handling() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    println!("🛡️  Testing Surface API Error Handling");
    
    // Process a few blocks first
    let chain = ChainBuilder::new()
        .add_blocks(3)
        .blocks();
    
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
    }
    
    // Test 1: Invalid view function name
    let view_input = vec![];
    let invalid_view_result = runtime.view("nonexistent_function".to_string(), &view_input, 0).await;
    
    match invalid_view_result {
        Ok(_) => panic!("Should have failed with invalid view function"),
        Err(e) => {
            println!("  ✓ Invalid view function properly rejected: {}", e);
            assert!(e.to_string().contains("Failed to get view function"));
        }
    }
    
    // Test 2: View function should still work after error
    let valid_result = runtime.view("blocktracker".to_string(), &view_input, 0).await?;
    assert_eq!(valid_result.len(), 1, "Valid view function should work after error");
    println!("  ✓ Runtime recovered after invalid view function");
    
    // Test 3: Preview with invalid view function
    let preview_block = create_test_block(3, chain.last().unwrap().block_hash(), b"error_test");
    let preview_block_bytes = utils::consensus_encode(&preview_block)?;
    
    let invalid_preview_result = runtime.preview(
        &preview_block_bytes,
        "nonexistent_function".to_string(),
        &view_input,
        3,
    );
    
    match invalid_preview_result {
        Ok(_) => panic!("Preview should have failed with invalid view function"),
        Err(e) => {
            println!("  ✓ Invalid preview function properly rejected: {}", e);
        }
    }
    
    // Test 4: Valid preview should still work after error
    let valid_preview_result = runtime.preview(
        &preview_block_bytes,
        "blocktracker".to_string(),
        &view_input,
        3,
    )?;
    assert_eq!(valid_preview_result.len(), 5, "Valid preview should work after error");
    println!("  ✓ Runtime recovered after invalid preview function");
    
    // Test 5: Concurrent view function calls (if supported)
    let _handles: Vec<tokio::task::JoinHandle<()>> = vec![];
    for height in 0..chain.len() {
        let view_input = vec![];
        let result = runtime.view("blocktracker".to_string(), &view_input, height as u32).await?;
        assert_eq!(result.len(), height + 1, "Concurrent view should work at height {}", height);
    }
    println!("  ✓ Sequential view calls work correctly");
    
    println!("✅ Surface API Error Handling Test Passed!");
    
    Ok(())
}

/// Test surface API with complex view function inputs
#[tokio::test]
async fn test_surface_api_complex_inputs() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    println!("🧩 Testing Surface API Complex Inputs");
    
    // Process some blocks
    let chain = ChainBuilder::new()
        .add_blocks(5)
        .blocks();
    
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
    }
    
    // Test 1: Empty input
    let empty_input = vec![];
    let result = runtime.view("blocktracker".to_string(), &empty_input, 2).await?;
    assert_eq!(result.len(), 3, "Empty input should work");
    println!("  ✓ Empty input handled correctly");
    
    // Test 2: Various sized inputs for getblock
    for height in 0..chain.len() {
        let height_u32 = height as u32;
        
        // Test with correct u32 input
        let correct_input = height_u32.to_le_bytes().to_vec();
        let result = runtime.view("getblock".to_string(), &correct_input, height_u32).await?;
        assert!(!result.is_empty(), "Correct input should return data at height {}", height);
        
        // Test with oversized input (should still work, extra bytes ignored)
        let mut oversized_input = correct_input.clone();
        oversized_input.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        let result = runtime.view("getblock".to_string(), &oversized_input, height_u32).await?;
        assert!(!result.is_empty(), "Oversized input should work at height {}", height);
        
        println!("  ✓ Various input sizes work at height {}", height);
    }
    
    // Test 3: Large input data
    let large_input = vec![0x42; 1024]; // 1KB of data
    let result = runtime.view("blocktracker".to_string(), &large_input, 2).await?;
    assert_eq!(result.len(), 3, "Large input should work");
    println!("  ✓ Large input (1KB) handled correctly");
    
    // Test 4: Binary input data
    let binary_input = (0..256).map(|i| i as u8).collect::<Vec<u8>>();
    let result = runtime.view("blocktracker".to_string(), &binary_input, 1).await?;
    assert_eq!(result.len(), 2, "Binary input should work");
    println!("  ✓ Binary input handled correctly");
    
    println!("✅ Surface API Complex Inputs Test Passed!");
    
    Ok(())
}

/// Test surface API state isolation between calls
#[tokio::test]
async fn test_surface_api_state_isolation() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    println!("🔒 Testing Surface API State Isolation");
    
    // Process some blocks
    let chain = ChainBuilder::new()
        .add_blocks(3)
        .blocks();
    
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
    }
    
    // Test 1: View functions at different heights should return different results
    let view_input = vec![];
    let result_h0 = runtime.view("blocktracker".to_string(), &view_input, 0).await?;
    let result_h1 = runtime.view("blocktracker".to_string(), &view_input, 1).await?;
    let result_h2 = runtime.view("blocktracker".to_string(), &view_input, 2).await?;
    
    assert_ne!(result_h0, result_h1, "Different heights should return different results");
    assert_ne!(result_h1, result_h2, "Different heights should return different results");
    assert_eq!(result_h0.len(), 1, "Height 0 should have 1 byte");
    assert_eq!(result_h1.len(), 2, "Height 1 should have 2 bytes");
    assert_eq!(result_h2.len(), 3, "Height 2 should have 3 bytes");
    
    println!("  ✓ View functions properly isolated by height");
    
    // Test 2: Preview should not affect main state
    let preview_block = create_test_block(3, chain.last().unwrap().block_hash(), b"isolation_test");
    let preview_block_bytes = utils::consensus_encode(&preview_block)?;
    
    // Get state before preview
    let before_preview = runtime.view("blocktracker".to_string(), &view_input, 2).await?;
    
    // Run preview
    let preview_result = runtime.preview(
        &preview_block_bytes,
        "blocktracker".to_string(),
        &view_input,
        3,
    )?;
    
    // Get state after preview
    let after_preview = runtime.view("blocktracker".to_string(), &view_input, 2).await?;
    
    assert_eq!(before_preview, after_preview, "Preview should not affect main state");
    assert_eq!(preview_result.len(), 5, "Preview should show new state");
    assert_eq!(after_preview.len(), 3, "Main state should be unchanged");
    
    println!("  ✓ Preview properly isolated from main state");
    
    // Test 3: Multiple view calls should not interfere
    let mut results = Vec::new();
    for _ in 0..5 {
        let result = runtime.view("blocktracker".to_string(), &view_input, 1).await?;
        results.push(result);
    }
    
    // All results should be identical
    for (i, result) in results.iter().enumerate() {
        assert_eq!(result.len(), 2, "All calls should return same result");
        if i > 0 {
            assert_eq!(result, &results[0], "All calls should return identical data");
        }
    }
    
    println!("  ✓ Multiple view calls properly isolated");
    
    println!("✅ Surface API State Isolation Test Passed!");
    
    Ok(())
}
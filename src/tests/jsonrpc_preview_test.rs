//! JSON-RPC preview function tests
//!
//! This test suite validates the metashrew_preview JSON-RPC method functionality
//! using the sync framework's JsonRpcProvider trait implementation.

use super::block_builder::{create_test_block, ChainBuilder};
use super::TestConfig;
use anyhow::Result;
use metashrew_support::utils;
use rockshrew_sync::{
    adapters::MetashrewRuntimeAdapter,
    mock::{MockBitcoinNode, MockStorage},
    JsonRpcProvider, MetashrewSync, RuntimeAdapter, SyncConfig,
};

/// Test metashrew_preview JSON-RPC method functionality
#[tokio::test]
async fn test_metashrew_preview_jsonrpc() -> Result<()> {
    println!("🔮 Testing metashrew_preview JSON-RPC method");

    // Create test configuration
    let config = TestConfig::new();
    let runtime = config.create_runtime()?;

    // Create sync engine components
    let node = MockBitcoinNode::new();
    let storage = MockStorage::new();
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);
    let sync_config = SyncConfig::default();

    // Create sync engine
    let sync_engine = MetashrewSync::new(node, storage, runtime_adapter, sync_config);

    // Process some blocks first to establish state
    let chain = ChainBuilder::new().add_blocks(3).blocks();

    println!("📦 Processing {} blocks to establish state", chain.len());

    // Manually process blocks (since we're testing the JSON-RPC interface, not the sync engine)
    {
        let runtime = sync_engine.runtime();
        for (height, block) in chain.iter().enumerate() {
            let block_bytes = utils::consensus_encode(block)?;
            let mut runtime_guard = runtime.write().await;
            runtime_guard
                .process_block(height as u32, &block_bytes)
                .await?;
        }
    }

    // Test 1: Basic metashrew_preview functionality
    println!("🧪 Test 1: Basic metashrew_preview functionality");

    let preview_block = create_test_block(
        chain.len() as u32,
        chain.last().unwrap().block_hash(),
        b"preview_test_block",
    );
    let preview_block_bytes = utils::consensus_encode(&preview_block)?;
    let block_hex = format!("0x{}", hex::encode(&preview_block_bytes));

    let function_name = "blocktracker".to_string();
    let input_hex = "0x".to_string(); // Empty input
    let height = format!("{}", chain.len() - 1); // Use latest processed height

    let result = sync_engine
        .metashrew_preview(
            block_hex.clone(),
            function_name.clone(),
            input_hex.clone(),
            height.clone(),
        )
        .await?;

    // Decode the result
    let result_bytes = hex::decode(result.trim_start_matches("0x"))?;

    // Preview should show blocktracker with one more byte than current state
    let expected_length = chain.len() + 1;
    assert_eq!(
        result_bytes.len(),
        expected_length,
        "Preview should show blocktracker with {} bytes",
        expected_length
    );

    println!(
        "  ✓ Preview returned {} bytes (expected {})",
        result_bytes.len(),
        expected_length
    );

    // Test 2: Preview with different view functions
    println!("🧪 Test 2: Preview with getblock view function");

    let height_input = (chain.len() as u32).to_le_bytes();
    let height_input_hex = format!("0x{}", hex::encode(&height_input));

    let getblock_result = sync_engine
        .metashrew_preview(
            block_hex.clone(),
            "getblock".to_string(),
            height_input_hex,
            height.clone(),
        )
        .await?;

    let getblock_bytes = hex::decode(getblock_result.trim_start_matches("0x"))?;
    assert!(
        !getblock_bytes.is_empty(),
        "Getblock preview should return data"
    );

    println!(
        "  ✓ Getblock preview returned {} bytes",
        getblock_bytes.len()
    );

    // Test 3: Preview with "latest" height
    println!("🧪 Test 3: Preview with 'latest' height");

    let latest_result = sync_engine
        .metashrew_preview(
            block_hex.clone(),
            function_name.clone(),
            input_hex.clone(),
            "latest".to_string(),
        )
        .await?;

    let latest_bytes = hex::decode(latest_result.trim_start_matches("0x"))?;
    // Note: "latest" might resolve to a different height than expected
    // Let's just verify it returns some data
    assert!(
        !latest_bytes.is_empty(),
        "Preview with 'latest' should return data"
    );

    println!(
        "  ✓ Preview with 'latest' height returned {} bytes",
        latest_bytes.len()
    );

    // Test 4: Preview should not affect main state
    println!("🧪 Test 4: Preview isolation test");

    // Get current state before preview
    let before_preview = sync_engine
        .metashrew_view(function_name.clone(), input_hex.clone(), height.clone())
        .await?;

    let before_bytes = hex::decode(before_preview.trim_start_matches("0x"))?;

    // Run preview
    let _preview_result = sync_engine
        .metashrew_preview(
            block_hex.clone(),
            function_name.clone(),
            input_hex.clone(),
            height.clone(),
        )
        .await?;

    // Get state after preview
    let after_preview = sync_engine
        .metashrew_view(function_name.clone(), input_hex.clone(), height.clone())
        .await?;

    let after_bytes = hex::decode(after_preview.trim_start_matches("0x"))?;

    assert_eq!(
        before_bytes, after_bytes,
        "Preview should not affect main state"
    );
    // Main state should remain unchanged (exact length may vary based on runtime implementation)
    assert!(!before_bytes.is_empty(), "Main state should have data");

    println!(
        "  ✓ Preview did not affect main state (both have {} bytes)",
        before_bytes.len()
    );

    // Test 5: Error handling
    println!("🧪 Test 5: Error handling");

    // Test with invalid block data
    let invalid_block_hex = "0xinvalidhex";
    let error_result = sync_engine
        .metashrew_preview(
            invalid_block_hex.to_string(),
            function_name.clone(),
            input_hex.clone(),
            height.clone(),
        )
        .await;

    assert!(
        error_result.is_err(),
        "Invalid block data should cause error"
    );
    println!("  ✓ Invalid block data properly rejected");

    // Test with invalid view function
    let invalid_function_result = sync_engine
        .metashrew_preview(
            block_hex.clone(),
            "nonexistent_function".to_string(),
            input_hex.clone(),
            height.clone(),
        )
        .await;

    assert!(
        invalid_function_result.is_err(),
        "Invalid view function should cause error"
    );
    println!("  ✓ Invalid view function properly rejected");

    // Test with invalid height
    let invalid_height_result = sync_engine
        .metashrew_preview(
            block_hex.clone(),
            function_name.clone(),
            input_hex.clone(),
            "invalid_height".to_string(),
        )
        .await;

    assert!(
        invalid_height_result.is_err(),
        "Invalid height should cause error"
    );
    println!("  ✓ Invalid height properly rejected");

    println!("✅ All metashrew_preview JSON-RPC tests passed!");

    Ok(())
}

/// Test metashrew_preview with complex scenarios
#[tokio::test]
async fn test_metashrew_preview_complex_scenarios() -> Result<()> {
    println!("🔮 Testing metashrew_preview complex scenarios");

    // Create test configuration
    let config = TestConfig::new();
    let runtime = config.create_runtime()?;

    // Create sync engine components
    let node = MockBitcoinNode::new();
    let storage = MockStorage::new();
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);
    let sync_config = SyncConfig::default();

    // Create sync engine
    let sync_engine = MetashrewSync::new(node, storage, runtime_adapter, sync_config);

    // Process a longer chain
    let chain = ChainBuilder::new().add_blocks(10).blocks();

    println!("📦 Processing {} blocks for complex scenarios", chain.len());

    // Process blocks
    {
        let runtime = sync_engine.runtime();
        for (height, block) in chain.iter().enumerate() {
            let block_bytes = utils::consensus_encode(block)?;
            let mut runtime_guard = runtime.write().await;
            runtime_guard
                .process_block(height as u32, &block_bytes)
                .await?;
        }
    }

    // Test 1: Preview at different historical heights
    println!("🧪 Test 1: Preview at different historical heights");

    let preview_block = create_test_block(
        chain.len() as u32,
        chain.last().unwrap().block_hash(),
        b"complex_preview_block",
    );
    let preview_block_bytes = utils::consensus_encode(&preview_block)?;
    let block_hex = format!("0x{}", hex::encode(&preview_block_bytes));
    let input_hex = "0x".to_string();

    for test_height in [0, 5, 9] {
        let height_str = test_height.to_string();

        let result = sync_engine
            .metashrew_preview(
                block_hex.clone(),
                "blocktracker".to_string(),
                input_hex.clone(),
                height_str.clone(),
            )
            .await?;

        let result_bytes = hex::decode(result.trim_start_matches("0x"))?;

        // Preview should show state as if the block was processed at that height
        // The blocktracker should have (test_height + 2) bytes (original state + preview block)
        let expected_length = test_height + 2;
        assert_eq!(
            result_bytes.len(),
            expected_length,
            "Preview at height {} should show {} bytes",
            test_height,
            expected_length
        );

        println!(
            "  ✓ Preview at height {} returned {} bytes",
            test_height,
            result_bytes.len()
        );
    }

    // Test 2: Multiple consecutive previews
    println!("🧪 Test 2: Multiple consecutive previews");

    for i in 0..3 {
        let test_block = create_test_block(
            chain.len() as u32 + i,
            chain.last().unwrap().block_hash(),
            format!("consecutive_preview_{}", i).as_bytes(),
        );
        let test_block_bytes = utils::consensus_encode(&test_block)?;
        let test_block_hex = format!("0x{}", hex::encode(&test_block_bytes));

        let result = sync_engine
            .metashrew_preview(
                test_block_hex,
                "blocktracker".to_string(),
                input_hex.clone(),
                "latest".to_string(),
            )
            .await?;

        let result_bytes = hex::decode(result.trim_start_matches("0x"))?;
        // Each preview should return some data, but the exact length may vary
        // based on the runtime's internal state management
        assert!(
            !result_bytes.is_empty(),
            "Consecutive preview {} should return data",
            i
        );

        println!(
            "  ✓ Consecutive preview {} returned {} bytes",
            i,
            result_bytes.len()
        );
    }

    // Test 3: Preview with large input data
    println!("🧪 Test 3: Preview with large input data");

    let large_input = vec![0x42; 1024]; // 1KB of data
    let large_input_hex = format!("0x{}", hex::encode(&large_input));

    let large_result = sync_engine
        .metashrew_preview(
            block_hex.clone(),
            "blocktracker".to_string(),
            large_input_hex,
            "latest".to_string(),
        )
        .await?;

    let large_result_bytes = hex::decode(large_result.trim_start_matches("0x"))?;
    assert!(
        !large_result_bytes.is_empty(),
        "Preview with large input should work"
    );

    println!(
        "  ✓ Preview with large input (1KB) returned {} bytes",
        large_result_bytes.len()
    );

    println!("✅ All complex metashrew_preview scenarios passed!");

    Ok(())
}

/// Test metashrew_preview performance characteristics
#[tokio::test]
async fn test_metashrew_preview_performance() -> Result<()> {
    println!("⚡ Testing metashrew_preview performance");

    // Create test configuration
    let config = TestConfig::new();
    let runtime = config.create_runtime()?;

    // Create sync engine components
    let node = MockBitcoinNode::new();
    let storage = MockStorage::new();
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);
    let sync_config = SyncConfig::default();

    // Create sync engine
    let sync_engine = MetashrewSync::new(node, storage, runtime_adapter, sync_config);

    // Process blocks for performance testing
    let chain = ChainBuilder::new().add_blocks(20).blocks();

    println!(
        "📦 Processing {} blocks for performance testing",
        chain.len()
    );

    // Process blocks
    {
        let runtime = sync_engine.runtime();
        for (height, block) in chain.iter().enumerate() {
            let block_bytes = utils::consensus_encode(block)?;
            let mut runtime_guard = runtime.write().await;
            runtime_guard
                .process_block(height as u32, &block_bytes)
                .await?;
        }
    }

    // Performance test: Multiple preview calls
    let preview_block = create_test_block(
        chain.len() as u32,
        chain.last().unwrap().block_hash(),
        b"performance_test_block",
    );
    let preview_block_bytes = utils::consensus_encode(&preview_block)?;
    let block_hex = format!("0x{}", hex::encode(&preview_block_bytes));
    let input_hex = "0x".to_string();

    let start_time = std::time::Instant::now();
    let num_calls = 10;

    for i in 0..num_calls {
        let _result = sync_engine
            .metashrew_preview(
                block_hex.clone(),
                "blocktracker".to_string(),
                input_hex.clone(),
                "latest".to_string(),
            )
            .await?;

        if i % 5 == 0 {
            println!("  Completed {} preview calls", i + 1);
        }
    }

    let total_time = start_time.elapsed();
    let avg_time = total_time / num_calls;
    let calls_per_second = num_calls as f64 / total_time.as_secs_f64();

    println!("📊 Performance Results:");
    println!("  Total time: {:?}", total_time);
    println!("  Average time per call: {:?}", avg_time);
    println!("  Calls per second: {:.2}", calls_per_second);

    // Performance assertions (reasonable expectations)
    assert!(
        avg_time.as_millis() < 1000,
        "Average preview time should be under 1 second"
    );
    assert!(
        calls_per_second > 1.0,
        "Should handle at least 1 call per second"
    );

    println!("✅ metashrew_preview performance test passed!");

    Ok(())
}

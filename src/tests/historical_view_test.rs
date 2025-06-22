use super::block_builder::create_test_block;
use bitcoin::{hashes::Hash, BlockHash};
use memshrew_runtime::{MemStoreAdapter, MemStoreRuntime};
use metashrew_support::utils;
use std::path::PathBuf;

#[tokio::test]
async fn test_historical_view_queries() {
    // Create runtime with in-memory storage
    let adapter = MemStoreAdapter::new();
    let mut runtime = MemStoreRuntime::load(
        PathBuf::from("./target/wasm32-unknown-unknown/release/metashrew_minimal.wasm"),
        adapter.clone(),
    )
    .expect("Failed to load runtime");

    // Process multiple blocks to create historical data
    let mut prev_hash = BlockHash::all_zeros();
    let mut blocks = Vec::new();

    for height in 0..5 {
        let block = create_test_block(height, prev_hash, &[height as u8]);
        prev_hash = block.block_hash();
        let encoded_block = utils::consensus_encode(&block).expect("Failed to encode block");
        blocks.push(encoded_block);
    }

    // Process each block
    for (height, block) in blocks.iter().enumerate() {
        runtime.context.lock().unwrap().height = height as u32;
        runtime.context.lock().unwrap().block = block.clone();
        runtime
            .run()
            .expect(&format!("Failed to process block {}", height));
        runtime.refresh_memory().expect("Failed to refresh memory");
    }

    println!("Processed {} blocks", blocks.len());

    // Test historical queries at different heights
    for query_height in 0..blocks.len() {
        let result = runtime
            .view("blocktracker".to_string(), &vec![], query_height as u32)
            .await
            .expect(&format!("Failed to query at height {}", query_height));

        println!(
            "Height {}: {} bytes: {:?}",
            query_height,
            result.len(),
            result
        );

        // At each height, we should have (height + 1) bytes
        // because we track the first byte of each block hash
        let expected_length = query_height + 1;
        assert_eq!(
            result.len(),
            expected_length,
            "At height {}, expected {} bytes but got {}",
            query_height,
            expected_length,
            result.len()
        );
    }

    // Test querying at current height (should have all 5 bytes)
    let current_result = runtime
        .view(
            "blocktracker".to_string(),
            &vec![],
            4, // Current height
        )
        .await
        .expect("Failed to query at current height");

    println!(
        "Current (height 4): {} bytes: {:?}",
        current_result.len(),
        current_result
    );
    assert_eq!(
        current_result.len(),
        5,
        "Current height should have 5 bytes"
    );

    // Test querying beyond current height (should still return current state)
    let future_result = runtime
        .view(
            "blocktracker".to_string(),
            &vec![],
            10, // Future height
        )
        .await
        .expect("Failed to query at future height");

    println!(
        "Future (height 10): {} bytes: {:?}",
        future_result.len(),
        future_result
    );
    assert_eq!(
        future_result.len(),
        5,
        "Future height should return current state (5 bytes)"
    );
}

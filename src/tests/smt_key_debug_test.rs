//! Debug test to investigate SMT key format and storage issues

use super::TestConfig;
use anyhow::Result;
use memshrew_runtime::KeyValueStoreLike;
use metashrew_runtime::smt::SMTHelper;
use metashrew_support::utils;

#[tokio::test]
async fn test_smt_key_storage_and_retrieval() -> Result<()> {
    println!("=== SMT KEY STORAGE AND RETRIEVAL DEBUG TEST ===");

    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;

    // Process a single block
    let genesis = super::TestUtils::create_genesis_block();
    let block_bytes = utils::consensus_encode(&genesis)?;
    let height = 0u32;

    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes;
        context.height = height;
    }

    runtime.run()?;

    // Get direct access to the adapter
    let adapter = &mut runtime.context.lock().unwrap().db;

    println!("=== SCANNING ALL KEYS IN DATABASE ===");

    // Scan all keys to see what's actually stored
    let all_data = adapter.get_all_data();
    for (key, value) in &all_data {
        let key_str = String::from_utf8_lossy(key);
        if key_str.contains("smt:root") {
            println!(
                "Found SMT root key: '{}' -> {} bytes: 0x{}",
                key_str,
                value.len(),
                hex::encode(value)
            );
        }
    }

    println!("=== TESTING SMT HELPER RETRIEVAL ===");

    // Test SMTHelper retrieval
    let smt_helper = SMTHelper::new(adapter.clone());

    // Try to get state root using SMTHelper
    match smt_helper.get_smt_root_at_height(height) {
        Ok(root) => {
            println!(
                "SMTHelper.get_smt_root_at_height({}) returned: 0x{}",
                height,
                hex::encode(&root)
            );

            // Check if it's all zeros
            let zero_root = [0u8; 32];
            if root == zero_root {
                println!("❌ SMTHelper returned all zeros!");
            } else {
                println!("✅ SMTHelper returned non-zero root");
            }
        }
        Err(e) => {
            println!(
                "❌ SMTHelper.get_smt_root_at_height({}) failed: {}",
                height, e
            );
        }
    }

    println!("=== TESTING DIRECT KEY ACCESS ===");

    // Test direct key access with different formats
    let formats = vec![
        format!("smt:root:{}", height),
        format!("smt:root::{}", height),
        format!("smt:root:0"),
        format!("smt:root::0"),
    ];

    for key_format in formats {
        match adapter.get(key_format.as_bytes()) {
            Ok(Some(value)) => {
                println!(
                    "✅ Found value with key '{}': {} bytes: 0x{}",
                    key_format,
                    value.len(),
                    hex::encode(&value)
                );
            }
            Ok(None) => {
                println!("❌ No value found with key '{}'", key_format);
            }
            Err(e) => {
                println!("❌ Error accessing key '{}': {}", key_format, e);
            }
        }
    }

    println!("=== TESTING PREFIX SCAN ===");

    // Test prefix scanning
    let prefixes = vec!["smt:", "smt:root", "smt:root:"];

    for prefix in prefixes {
        match adapter.scan_prefix(prefix.as_bytes()) {
            Ok(results) => {
                println!("Prefix '{}' found {} results:", prefix, results.len());
                for (key, value) in results {
                    let key_str = String::from_utf8_lossy(&key);
                    println!("  - '{}' -> {} bytes", key_str, value.len());
                }
            }
            Err(e) => {
                println!("❌ Error scanning prefix '{}': {}", prefix, e);
            }
        }
    }

    println!("=== SMT KEY DEBUG TEST COMPLETE ===");
    Ok(())
}

#[tokio::test]
async fn test_smt_helper_storage_mechanism() -> Result<()> {
    println!("=== SMT HELPER STORAGE MECHANISM TEST ===");

    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;

    // Process a single block
    let genesis = super::TestUtils::create_genesis_block();
    let block_bytes = utils::consensus_encode(&genesis)?;
    let height = 0u32;

    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes;
        context.height = height;
    }

    runtime.run()?;

    // Get direct access to the adapter
    let adapter = &mut runtime.context.lock().unwrap().db;

    println!("=== TESTING MANUAL SMT HELPER OPERATIONS ===");

    // Create a fresh SMTHelper and test its operations
    let mut smt_helper = SMTHelper::new(adapter.clone());

    // Test storing a state root manually
    let test_root = [
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
        0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
        0xde, 0xf0,
    ];

    let test_height = 999u32;
    let root_key = format!("smt:root:{}", test_height).into_bytes();

    println!(
        "Manually storing test root at height {} with key '{}'",
        test_height,
        String::from_utf8_lossy(&root_key)
    );

    match smt_helper.storage.put(&root_key, &test_root) {
        Ok(_) => {
            println!("✅ Successfully stored test root");

            // Try to retrieve it
            match smt_helper.get_smt_root_at_height(test_height) {
                Ok(retrieved_root) => {
                    println!("✅ Retrieved root: 0x{}", hex::encode(&retrieved_root));
                    if retrieved_root == test_root {
                        println!("✅ Retrieved root matches stored root!");
                    } else {
                        println!("❌ Retrieved root does not match stored root");
                        println!("   Expected: 0x{}", hex::encode(&test_root));
                        println!("   Got:      0x{}", hex::encode(&retrieved_root));
                    }
                }
                Err(e) => {
                    println!("❌ Failed to retrieve test root: {}", e);
                }
            }
        }
        Err(e) => {
            println!("❌ Failed to store test root: {}", e);
        }
    }

    println!("=== SMT HELPER STORAGE MECHANISM TEST COMPLETE ===");
    Ok(())
}

//! Test to inspect what data is actually being stored by metashrew-minimal

use super::{TestConfig, TestUtils, block_builder::*};
use anyhow::Result;
use memshrew_runtime::{MemStoreRuntime, MemStoreAdapter, KeyValueStoreLike};
use metashrew_support::utils;

#[tokio::test]
async fn test_inspect_stored_data() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create and process a few blocks
    let chain = ChainBuilder::new()
        .add_blocks(2)
        .blocks();
    
    for (height, block) in chain.iter().enumerate() {
        let block_bytes = utils::consensus_encode(block)?;
        println!("Processing block {} with {} bytes", height, block_bytes.len());
        
        {
            let mut context = runtime.context.lock().unwrap();
            context.block = block_bytes;
            context.height = height as u32;
        }
        
        runtime.run()?;
        runtime.refresh_memory()?;
        
        // Inspect database after each block
        let adapter = &runtime.context.lock().unwrap().db;
        let all_data = adapter.get_all_data();
        
        println!("After block {}, database contains {} keys:", height, all_data.len());
        for (key, value) in &all_data {
            let key_str = String::from_utf8_lossy(key);
            println!("  Key: '{}' ({} bytes) -> Value: {} bytes", key_str, key.len(), value.len());
            
            // Show first few bytes of value
            let preview = if value.len() > 10 {
                format!("{:?}...", &value[..10])
            } else {
                format!("{:?}", value)
            };
            println!("    Value preview: {}", preview);
        }
        println!();
    }
    
    Ok(())
}

#[tokio::test]
async fn test_inspect_blocktracker() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Process a single block
    let genesis = TestUtils::create_genesis_block();
    let block_bytes = utils::consensus_encode(&genesis)?;
    
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes;
        context.height = 0;
    }
    
    runtime.run()?;
    
    // Inspect blocktracker specifically
    let adapter = &runtime.context.lock().unwrap().db;
    let blocktracker_key = b"/blocktracker".to_vec();
    
    if let Some(blocktracker_data) = adapter.get_immutable(&blocktracker_key)? {
        println!("Blocktracker data: {} bytes", blocktracker_data.len());
        println!("Raw bytes: {:?}", blocktracker_data);
        
        // Try to understand the structure
        if blocktracker_data.len() >= 32 {
            println!("First 32 bytes (possible hash): {:02x?}", &blocktracker_data[..32]);
        }
        if blocktracker_data.len() > 32 {
            println!("Remaining bytes: {:?}", &blocktracker_data[32..]);
        }
    } else {
        println!("No blocktracker data found");
    }
    
    Ok(())
}
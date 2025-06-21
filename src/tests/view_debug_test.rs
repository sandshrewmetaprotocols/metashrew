//! Debug test to understand what the view functions are returning

use super::TestConfig;
use anyhow::Result;
use memshrew_runtime::{MemStoreRuntime, KeyValueStoreLike};
use metashrew_support::utils;
use metashrew_support::byte_view::ByteView;
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::Network;

#[tokio::test]
async fn test_view_function_output() -> Result<()> {
    println!("=== Testing view function output ===");
    
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Use genesis block
    let block = genesis_block(Network::Testnet);
    let block_bytes = utils::consensus_encode(&block)?;
    let height = 0u32;
    
    println!("Block hash: {}", block.block_hash());
    println!("Block bytes length: {}", block_bytes.len());
    
    // Set up runtime context
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes.clone();
        context.height = height;
    }
    
    // Run the indexer to process the block
    runtime.run()?;
    runtime.refresh_memory()?;
    
    println!("Successfully processed block");
    
    // Test blocktracker view
    let view_input = Vec::new();
    let blocktracker_result = runtime.view("blocktracker".to_string(), &view_input, height).await?;
    
    println!("Blocktracker raw result length: {}", blocktracker_result.len());
    println!("Blocktracker raw result (first 20 bytes): {:?}", 
             &blocktracker_result[..std::cmp::min(20, blocktracker_result.len())]);
    
    // Check if it starts with a length prefix
    if blocktracker_result.len() >= 4 {
        let length_prefix = u32::from_le_bytes([
            blocktracker_result[0],
            blocktracker_result[1], 
            blocktracker_result[2],
            blocktracker_result[3]
        ]);
        println!("Length prefix: {}", length_prefix);
        
        if length_prefix as usize + 4 == blocktracker_result.len() {
            println!("Length prefix matches total length - this is correctly formatted");
            let actual_data = &blocktracker_result[4..];
            println!("Actual data length: {}", actual_data.len());
            println!("Actual data: {:?}", actual_data);
        } else {
            println!("Length prefix doesn't match - this might not be a length-prefixed result");
        }
    }
    
    // Test getblock view
    let height_input = height.to_bytes();
    let getblock_result = runtime.view("getblock".to_string(), &height_input, height).await?;
    
    println!("Getblock raw result length: {}", getblock_result.len());
    println!("Getblock raw result (first 20 bytes): {:?}", 
             &getblock_result[..std::cmp::min(20, getblock_result.len())]);
    
    // Check if it starts with a length prefix
    if getblock_result.len() >= 4 {
        let length_prefix = u32::from_le_bytes([
            getblock_result[0],
            getblock_result[1], 
            getblock_result[2],
            getblock_result[3]
        ]);
        println!("Getblock length prefix: {}", length_prefix);
        
        if length_prefix as usize + 4 == getblock_result.len() {
            println!("Getblock length prefix matches total length");
            let actual_data = &getblock_result[4..];
            println!("Getblock actual data length: {}", actual_data.len());
            
            // Try to decode the block
            if actual_data.len() > 0 {
                let mut cursor = std::io::Cursor::new(actual_data.to_vec());
                match utils::consensus_decode::<bitcoin::Block>(&mut cursor) {
                    Ok(decoded_block) => {
                        println!("Successfully decoded block: {}", decoded_block.block_hash());
                        println!("Original block hash: {}", block.block_hash());
                        assert_eq!(decoded_block.block_hash(), block.block_hash());
                    }
                    Err(e) => {
                        println!("Failed to decode block: {}", e);
                    }
                }
            }
        }
    }
    
    Ok(())
}
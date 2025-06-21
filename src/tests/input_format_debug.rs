//! Debug test to understand the exact input format expected by metashrew-minimal

use super::TestConfig;
use anyhow::Result;
use memshrew_runtime::{MemStoreRuntime, KeyValueStoreLike};
use metashrew_support::utils;
use metashrew_support::byte_view::ByteView;
use bitcoin::{Block, BlockHash, Transaction, TxIn, TxOut, OutPoint, Txid, ScriptBuf};
use bitcoin::blockdata::block::Header as BlockHeader;
use bitcoin::hashes::Hash;
use std::io::Cursor;

#[tokio::test]
async fn test_input_format_step_by_step() -> Result<()> {
    println!("=== Testing input format step by step ===");
    
    // Create a simple block
    let height: u32 = 0;
    let block = create_simple_block();
    
    println!("Block hash: {}", block.block_hash());
    println!("Block header: {:?}", block.header);
    println!("Block transactions: {}", block.txdata.len());
    
    // Test encoding the block
    let block_bytes = utils::consensus_encode(&block)?;
    println!("Block encoded length: {} bytes", block_bytes.len());
    
    // Test decoding the block back
    let mut cursor = Cursor::new(block_bytes.clone());
    let decoded_block: Block = utils::consensus_decode(&mut cursor)?;
    println!("Block decoded successfully, hash: {}", decoded_block.block_hash());
    
    // Test height encoding
    let height_bytes = height.to_bytes();
    println!("Height {} encoded as: {:?} (length: {})", height, height_bytes, height_bytes.len());
    
    println!("=== Testing runtime input format ===");
    
    // The runtime expects only block bytes in context.block
    // It will automatically prepend the height when the WASM calls input()
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Set only block bytes in the runtime context (runtime will add height)
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes.clone();  // Only block bytes
        context.height = height;
    }
    
    println!("Set block bytes length: {} in runtime context", block_bytes.len());
    println!("Set height: {} in runtime context", height);
    
    // Try to run the WASM module
    match runtime.run() {
        Ok(_) => {
            println!("Runtime execution successful!");
            runtime.refresh_memory()?;
        }
        Err(e) => {
            println!("Runtime execution failed: {}", e);
            return Err(e);
        }
    }
    
    Ok(())
}

fn create_simple_block() -> Block {
    use bitcoin::blockdata::constants::genesis_block;
    use bitcoin::Network;
    
    // Start with the Bitcoin testnet genesis block as a template
    let mut genesis = genesis_block(Network::Testnet);
    
    // Ensure it has a proper coinbase transaction
    if genesis.txdata.is_empty() {
        // Create a simple coinbase transaction
        let coinbase_tx = Transaction {
            version: bitcoin::transaction::Version::ONE,
            lock_time: bitcoin::locktime::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::from_hex("4d04ffff001d0104455468652054696d65732030332f4a616e2f32303039204368616e63656c6c6f72206f6e206272696e6b206f66207365636f6e64206261696c6f757420666f722062616e6b73").unwrap(),
                sequence: bitcoin::Sequence::MAX,
                witness: bitcoin::Witness::new(),
            }],
            output: vec![TxOut {
                value: bitcoin::Amount::from_sat(5000000000),
                script_pubkey: ScriptBuf::new(),
            }],
        };
        genesis.txdata = vec![coinbase_tx];
    }
    
    genesis
}

#[tokio::test]
async fn test_minimal_input() -> Result<()> {
    println!("=== Testing with minimal valid input ===");
    
    // Create the absolute minimal valid input
    let height: u32 = 0;
    let height_bytes = height.to_bytes();
    
    // Create a minimal valid block (just use genesis)
    use bitcoin::blockdata::constants::genesis_block;
    use bitcoin::Network;
    let block = genesis_block(Network::Testnet);
    let block_bytes = utils::consensus_encode(&block)?;
    
    println!("Using genesis block with hash: {}", block.block_hash());
    println!("Height bytes: {:?}", height_bytes);
    println!("Block bytes length: {}", block_bytes.len());
    
    // Test the runtime with only block bytes (runtime adds height)
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes;  // Only block bytes
        context.height = height;
    }
    
    runtime.run()?;
    runtime.refresh_memory()?;
    
    println!("Minimal input test passed!");
    Ok(())
}
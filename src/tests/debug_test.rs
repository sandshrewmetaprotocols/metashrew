//! Debug test to isolate the WASM module issue

use super::{TestConfig};
use anyhow::Result;
use memshrew_runtime::{MemStoreRuntime, MemStoreAdapter, KeyValueStoreLike};
use metashrew_support::utils;
use bitcoin::{Block, BlockHash, Transaction, TxIn, TxOut, OutPoint, Txid, ScriptBuf};
use bitcoin::blockdata::block::Header as BlockHeader;
use bitcoin::hashes::Hash;

#[tokio::test]
async fn test_debug_simple_block() -> Result<()> {
    let config = TestConfig::new();
    let mut runtime = config.create_runtime()?;
    
    // Create the simplest possible valid block
    let prev_blockhash = BlockHash::all_zeros();
    
    // Create a minimal coinbase transaction
    let coinbase_tx = Transaction {
        version: bitcoin::transaction::Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_hex("00").unwrap(), // Minimal script
            sequence: bitcoin::Sequence::MAX,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: bitcoin::Amount::from_sat(5000000000),
            script_pubkey: ScriptBuf::new(), // Empty script
        }],
    };
    
    let header = BlockHeader {
        version: bitcoin::blockdata::block::Version::ONE,
        prev_blockhash,
        merkle_root: coinbase_tx.compute_txid().into(),
        time: 1231006505,
        bits: bitcoin::CompactTarget::from_consensus(0x1d00ffff),
        nonce: 2083236893,
    };
    
    let block = Block {
        header,
        txdata: vec![coinbase_tx],
    };
    
    // Encode the block using metashrew's consensus encoding
    let block_bytes = utils::consensus_encode(&block)?;
    println!("Block bytes length: {}", block_bytes.len());
    
    // Set the input in the runtime context (only block bytes, runtime handles height)
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes;
        context.height = 0;
    }
    
    // Try to run the indexer
    match runtime.run() {
        Ok(_) => {
            println!("Block processing succeeded!");
            
            // Verify the block was stored
            let adapter = &runtime.context.lock().unwrap().db;
            let block_key = b"/blocks/0".to_vec();
            let stored_block = adapter.get_immutable(&block_key)?;
            assert!(stored_block.is_some(), "Block should be stored");
            
            Ok(())
        }
        Err(e) => {
            println!("Block processing failed: {:?}", e);
            Err(e)
        }
    }
}

#[tokio::test]
async fn test_debug_input_format() -> Result<()> {
    // Test that our input format matches what metashrew-minimal expects
    let height = 0u32;
    let block_data = vec![1, 2, 3, 4]; // Dummy block data
    
    // Create input as height + block data
    let mut input = Vec::new();
    input.extend_from_slice(&height.to_le_bytes());
    input.extend_from_slice(&block_data);
    
    // Verify we can parse it back
    let mut cursor = std::io::Cursor::new(input);
    let parsed_height = utils::consume_sized_int::<u32>(&mut cursor)?;
    let parsed_block = utils::consume_to_end(&mut cursor)?;
    
    assert_eq!(parsed_height, height);
    assert_eq!(parsed_block, block_data);
    
    println!("Input format test passed");
    Ok(())
}
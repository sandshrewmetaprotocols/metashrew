//! Test to validate our block encoding/decoding works correctly

use anyhow::Result;
use metashrew_support::utils;
use bitcoin::{Block, BlockHash, Transaction, TxIn, TxOut, OutPoint, ScriptBuf};
use bitcoin::blockdata::block::Header as BlockHeader;
use bitcoin::hashes::Hash;
use std::io::Cursor;

#[tokio::test]
async fn test_block_encode_decode_cycle() -> Result<()> {
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
    
    let original_block = Block {
        header,
        txdata: vec![coinbase_tx],
    };
    
    println!("Original block created successfully");
    
    // Encode the block using metashrew's consensus encoding
    let encoded_bytes = utils::consensus_encode(&original_block)?;
    println!("Block encoded successfully, {} bytes", encoded_bytes.len());
    
    // Try to decode it back using metashrew's consensus decoding
    let mut cursor = Cursor::new(encoded_bytes);
    let decoded_block: Block = utils::consensus_decode(&mut cursor)?;
    println!("Block decoded successfully");
    
    // Verify they match
    assert_eq!(original_block.header.prev_blockhash, decoded_block.header.prev_blockhash);
    assert_eq!(original_block.header.merkle_root, decoded_block.header.merkle_root);
    assert_eq!(original_block.txdata.len(), decoded_block.txdata.len());
    
    println!("Block encode/decode cycle successful!");
    Ok(())
}

#[tokio::test]
async fn test_input_parsing_like_wasm() -> Result<()> {
    // Simulate exactly what the WASM module does
    let height = 0u32;
    
    // Create a simple block
    let prev_blockhash = BlockHash::all_zeros();
    let coinbase_tx = Transaction {
        version: bitcoin::transaction::Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_hex("00").unwrap(),
            sequence: bitcoin::Sequence::MAX,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: bitcoin::Amount::from_sat(5000000000),
            script_pubkey: ScriptBuf::new(),
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
    
    // Encode the block
    let block_bytes = utils::consensus_encode(&block)?;
    
    // Create input exactly like our runtime does
    let mut input_data = Vec::new();
    input_data.extend_from_slice(&height.to_le_bytes());
    input_data.extend_from_slice(&block_bytes);
    
    println!("Created input data: {} bytes total", input_data.len());
    
    // Now parse it exactly like the WASM module does
    let mut input_cursor = Cursor::new(input_data);
    
    // Step 1: Parse height (line 11 in metashrew-minimal)
    let parsed_height = utils::consume_sized_int::<u32>(&mut input_cursor)?;
    println!("Parsed height: {}", parsed_height);
    assert_eq!(parsed_height, height);
    
    // Step 2: Get remaining block bytes (line 12 in metashrew-minimal)
    let parsed_block_bytes = utils::consume_to_end(&mut input_cursor)?;
    println!("Parsed block bytes: {} bytes", parsed_block_bytes.len());
    assert_eq!(parsed_block_bytes, block_bytes);
    
    // Step 3: Decode the block (line 14 in metashrew-minimal)
    let mut block_cursor = Cursor::new(parsed_block_bytes);
    let decoded_block: Block = utils::consensus_decode(&mut block_cursor)?;
    println!("Successfully decoded block in WASM-like parsing");
    
    // Verify the block hash matches
    assert_eq!(decoded_block.block_hash(), block.block_hash());
    
    println!("All WASM-like parsing steps successful!");
    Ok(())
}
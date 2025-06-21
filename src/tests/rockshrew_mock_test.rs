//! Mock tests for the complete rockshrew program using in-memory database

use anyhow::{anyhow, Result};
use memshrew_store::MemStore;
use rockshrew_smt::{SMTOperations, BSTOperations, StateManager};
use metashrew_runtime::{KeyValueStoreLike, BatchLike};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

// Mock Bitcoin block data structure
#[derive(Clone, Debug)]
struct MockBlock {
    height: u32,
    hash: String,
    transactions: Vec<MockTransaction>,
}

// Mock Bitcoin transaction data structure
#[derive(Clone, Debug)]
struct MockTransaction {
    txid: String,
    inputs: Vec<MockInput>,
    outputs: Vec<MockOutput>,
}

// Mock transaction input
#[derive(Clone, Debug)]
struct MockInput {
    prev_txid: String,
    prev_vout: u32,
    script_sig: Vec<u8>,
}

// Mock transaction output
#[derive(Clone, Debug)]
struct MockOutput {
    value: u64,
    script_pubkey: Vec<u8>,
}

// Mock WASM module that processes blocks
struct MockWasmModule {
    store: MemStore,
    processed_blocks: Arc<Mutex<HashMap<u32, String>>>, // height -> hash
}

impl MockWasmModule {
    fn new() -> Self {
        Self {
            store: MemStore::new(),
            processed_blocks: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    // Process a block and update the state
    fn process_block(&self, block: &MockBlock) -> Result<()> {
        log::info!("Processing block {} at height {}", block.hash, block.height);
        
        // Store block hash by height
        let key = format!("block:height:{}", block.height).into_bytes();
        let value = block.hash.clone().into_bytes();
        self.store.bst_put(&key, &value, block.height)?;
        
        // Store block data
        let block_key = format!("block:hash:{}", block.hash).into_bytes();
        let block_value = format!("Block at height {}", block.height).into_bytes();
        self.store.bst_put(&block_key, &block_value, block.height)?;
        
        // Process each transaction
        for tx in &block.transactions {
            self.process_transaction(tx, block.height)?;
        }
        
        // Calculate and store state root
        let root = self.store.calculate_and_store_state_root(block.height)?;
        log::info!("Calculated state root for block {}: {:?}", block.height, hex::encode(root));
        
        // Record that we've processed this block
        if let Ok(mut processed_blocks) = self.processed_blocks.lock() {
            processed_blocks.insert(block.height, block.hash.clone());
        }
        
        Ok(())
    }
    
    // Process a transaction and update the state
    fn process_transaction(&self, tx: &MockTransaction, height: u32) -> Result<()> {
        // Store transaction by txid
        let tx_key = format!("tx:{}", tx.txid).into_bytes();
        let tx_value = format!("Transaction at height {}", height).into_bytes();
        self.store.bst_put(&tx_key, &tx_value, height)?;
        
        // Process outputs (create UTXOs)
        for (vout, output) in tx.outputs.iter().enumerate() {
            let utxo_key = format!("utxo:{}:{}", tx.txid, vout).into_bytes();
            let utxo_value = format!("{}:{}", output.value, hex::encode(&output.script_pubkey)).into_bytes();
            self.store.bst_put(&utxo_key, &utxo_value, height)?;
            
            // Index by script_pubkey (address)
            if !output.script_pubkey.is_empty() {
                let addr_key = format!("addr:{}:{}:{}", 
                    hex::encode(&output.script_pubkey), 
                    tx.txid, 
                    vout
                ).into_bytes();
                let addr_value = output.value.to_le_bytes().to_vec();
                self.store.bst_put(&addr_key, &addr_value, height)?;
            }
        }
        
        // Process inputs (spend UTXOs)
        for input in &tx.inputs {
            if input.prev_txid != "0000000000000000000000000000000000000000000000000000000000000000" {
                // Mark the UTXO as spent
                let spent_key = format!("spent:{}:{}", input.prev_txid, input.prev_vout).into_bytes();
                let spent_value = format!("{}:{}", tx.txid, height).into_bytes();
                self.store.bst_put(&spent_key, &spent_value, height)?;
            }
        }
        
        Ok(())
    }
    
    // Query the balance for an address at a specific height
    fn get_address_balance(&self, script_pubkey: &[u8], height: u32) -> Result<u64> {
        let prefix = format!("addr:{}", hex::encode(script_pubkey));
        let mut balance = 0u64;
        
        // List all UTXOs for this address
        let keys = self.store.list_keys_at_height(height)?;
        for key in keys {
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with(&prefix) {
                // Extract txid and vout
                let parts: Vec<&str> = key_str.split(':').collect();
                if parts.len() >= 4 {
                    let txid = parts[2];
                    let vout: u32 = parts[3].parse().unwrap_or(0);
                    
                    // Check if this UTXO is spent
                    let spent_key = format!("spent:{}:{}", txid, vout).into_bytes();
                    if let Ok(None) = self.store.bst_get_at_height(&spent_key, height) {
                        // UTXO is unspent, add to balance
                        if let Ok(Some(value_bytes)) = self.store.bst_get_at_height(&key, height) {
                            if value_bytes.len() >= 8 {
                                let mut value_arr = [0u8; 8];
                                value_arr.copy_from_slice(&value_bytes[0..8]);
                                balance += u64::from_le_bytes(value_arr);
                            }
                        }
                    }
                }
            }
        }
        
        Ok(balance)
    }
    
    // Get all processed blocks
    fn get_processed_blocks(&self) -> HashMap<u32, String> {
        if let Ok(processed_blocks) = self.processed_blocks.lock() {
            processed_blocks.clone()
        } else {
            HashMap::new()
        }
    }
}

// Create a mock blockchain with some test data
fn create_mock_blockchain() -> Vec<MockBlock> {
    let mut blocks = Vec::new();
    
    // Genesis block
    blocks.push(MockBlock {
        height: 0,
        hash: "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f".to_string(),
        transactions: vec![
            MockTransaction {
                txid: "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b".to_string(),
                inputs: vec![
                    MockInput {
                        prev_txid: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                        prev_vout: 0xFFFFFFFF,
                        script_sig: vec![0x04, 0xFF, 0xFF, 0x00, 0x1D],
                    }
                ],
                outputs: vec![
                    MockOutput {
                        value: 5000000000,
                        script_pubkey: vec![0x41, 0x04, 0x67, 0x8A],
                    }
                ],
            }
        ],
    });
    
    // Block 1
    blocks.push(MockBlock {
        height: 1,
        hash: "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048".to_string(),
        transactions: vec![
            MockTransaction {
                txid: "0e3e2357e806b6cdb1f70b54c3a3a17b6714ee1f0e68bebb44a74b1efd512098".to_string(),
                inputs: vec![
                    MockInput {
                        prev_txid: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                        prev_vout: 0xFFFFFFFF,
                        script_sig: vec![0x04, 0xFF, 0xFF, 0x00, 0x1D],
                    }
                ],
                outputs: vec![
                    MockOutput {
                        value: 5000000000,
                        script_pubkey: vec![0x41, 0x04, 0x67, 0x8B],
                    }
                ],
            }
        ],
    });
    
    // Block 2 with a transaction spending from block 0
    blocks.push(MockBlock {
        height: 2,
        hash: "000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd".to_string(),
        transactions: vec![
            // Coinbase
            MockTransaction {
                txid: "9b0fc92260312ce44e74ef369f5c66bbb85848f2eddd5a7a1cde251e54ccfdd5".to_string(),
                inputs: vec![
                    MockInput {
                        prev_txid: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                        prev_vout: 0xFFFFFFFF,
                        script_sig: vec![0x04, 0xFF, 0xFF, 0x00, 0x1D],
                    }
                ],
                outputs: vec![
                    MockOutput {
                        value: 5000000000,
                        script_pubkey: vec![0x41, 0x04, 0x67, 0x8C],
                    }
                ],
            },
            // Transaction spending from genesis block
            MockTransaction {
                txid: "999e1c837c76a1b7fbb7e57baf87b309960f5ffefbf2a9b95dd890602272f644".to_string(),
                inputs: vec![
                    MockInput {
                        prev_txid: "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b".to_string(),
                        prev_vout: 0,
                        script_sig: vec![0x48, 0x30, 0x45, 0x02],
                    }
                ],
                outputs: vec![
                    MockOutput {
                        value: 4000000000,
                        script_pubkey: vec![0x41, 0x04, 0x67, 0x8D],
                    },
                    MockOutput {
                        value: 1000000000,
                        script_pubkey: vec![0x41, 0x04, 0x67, 0x8A], // Same as genesis block output
                    }
                ],
            }
        ],
    });
    
    blocks
}

#[tokio::test]
async fn test_mock_rockshrew_program() -> Result<()> {
    // Create the mock WASM module
    let wasm_module = MockWasmModule::new();
    
    // Create a mock blockchain
    let blocks = create_mock_blockchain();
    
    // Process each block
    for block in &blocks {
        wasm_module.process_block(block)?;
    }
    
    // Verify that all blocks were processed
    let processed_blocks = wasm_module.get_processed_blocks();
    assert_eq!(processed_blocks.len(), blocks.len(), "All blocks should be processed");
    
    // Check that we can retrieve block data
    for block in &blocks {
        let key = format!("block:height:{}", block.height).into_bytes();
        let value = wasm_module.store.bst_get_at_height(&key, block.height)?;
        assert!(value.is_some(), "Block data should be retrievable");
        assert_eq!(
            String::from_utf8_lossy(&value.unwrap()),
            block.hash,
            "Block hash should match"
        );
    }
    
    // Check that state roots were calculated and can be retrieved
    for block in &blocks {
        let root = wasm_module.store.get_smt_root_at_height(block.height)?;
        assert_ne!(root, [0u8; 32], "State root should not be empty");
    }
    
    // Check address balance
    let address = vec![0x41, 0x04, 0x67, 0x8A]; // Address from genesis block
    let balance = wasm_module.get_address_balance(&address, 2)?;
    
    // Initial balance was 5000000000, then spent in block 2, but received 1000000000 back
    assert_eq!(balance, 1000000000, "Address balance should be correct");
    
    // Check that we can query at different heights
    let balance_at_height_0 = wasm_module.get_address_balance(&address, 0)?;
    assert_eq!(balance_at_height_0, 5000000000, "Balance at height 0 should be 5000000000");
    
    let balance_at_height_1 = wasm_module.get_address_balance(&address, 1)?;
    assert_eq!(balance_at_height_1, 5000000000, "Balance at height 1 should still be 5000000000");
    
    // Test chain reorganization by creating a fork and processing it
    let fork_block = MockBlock {
        height: 2,
        hash: "000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbe".to_string(), // Different hash
        transactions: vec![
            // Coinbase
            MockTransaction {
                txid: "9b0fc92260312ce44e74ef369f5c66bbb85848f2eddd5a7a1cde251e54ccfdd6".to_string(),
                inputs: vec![
                    MockInput {
                        prev_txid: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                        prev_vout: 0xFFFFFFFF,
                        script_sig: vec![0x04, 0xFF, 0xFF, 0x00, 0x1D],
                    }
                ],
                outputs: vec![
                    MockOutput {
                        value: 5000000000,
                        script_pubkey: vec![0x41, 0x04, 0x67, 0x8E], // Different address
                    }
                ],
            },
            // Different transaction spending from genesis block
            MockTransaction {
                txid: "999e1c837c76a1b7fbb7e57baf87b309960f5ffefbf2a9b95dd890602272f645".to_string(),
                inputs: vec![
                    MockInput {
                        prev_txid: "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b".to_string(),
                        prev_vout: 0,
                        script_sig: vec![0x48, 0x30, 0x45, 0x02],
                    }
                ],
                outputs: vec![
                    MockOutput {
                        value: 3000000000,
                        script_pubkey: vec![0x41, 0x04, 0x67, 0x8F],
                    },
                    MockOutput {
                        value: 2000000000, // Different amount
                        script_pubkey: vec![0x41, 0x04, 0x67, 0x8A], // Same as genesis block output
                    }
                ],
            }
        ],
    };
    
    // Process the fork block
    wasm_module.process_block(&fork_block)?;
    
    // Check that the fork block was processed
    let key = format!("block:height:{}", fork_block.height).into_bytes();
    let value = wasm_module.store.bst_get_at_height(&key, fork_block.height)?;
    assert!(value.is_some(), "Fork block data should be retrievable");
    assert_eq!(
        String::from_utf8_lossy(&value.unwrap()),
        fork_block.hash,
        "Fork block hash should match"
    );
    
    // Check that the address balance is now different due to the fork
    let balance_after_fork = wasm_module.get_address_balance(&address, fork_block.height)?;
    assert_eq!(balance_after_fork, 2000000000, "Address balance should be updated after fork");
    
    Ok(())
}
# Helper Libraries

Metashrew provides several helper libraries that simplify the development of Bitcoin indexers and metaprotocols. This document explains the key libraries, their features, and how to use them to build custom indexers.

## metashrew-core

The `metashrew-core` library provides core abstractions and macros for indexer development. It serves as the foundation for building WASM-based indexers that can run on the Metashrew platform.

### Key Features

1. **Host Interface Bindings**: Provides Rust bindings for the Metashrew host interface
2. **Macros for Indexer Development**: Simplifies common indexing patterns
3. **Type-Safe Abstractions**: Wraps low-level operations in type-safe abstractions
4. **Error Handling**: Provides robust error handling utilities

### Host Interface Bindings

The library provides bindings for all host functions:

```rust
extern "C" {
    fn __host_len() -> i32;
    fn __load_input(ptr: i32);
    fn __log(ptr: i32);
    fn __flush(ptr: i32);
    fn __get(key_ptr: i32, value_ptr: i32) -> i32;
    fn __get_len(ptr: i32) -> i32;
}
```

These are wrapped in safe Rust functions:

```rust
pub fn host_len() -> usize {
    unsafe { __host_len() as usize }
}

pub fn load_input(ptr: *mut u8) {
    unsafe { __load_input(ptr as i32) }
}

pub fn log(message: &str) {
    let bytes = message.as_bytes();
    let ptr = alloc(bytes.len());
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        __log(ptr as i32);
    }
}

// ... and so on
```

### Indexer Macro

The `indexer` macro simplifies the creation of indexers by generating the necessary boilerplate:

```rust
#[macro_export]
macro_rules! indexer {
    ($process_block:expr) => {
        static mut INPUT_BUFFER: Vec<u8> = Vec::new();
        static mut STATE: i32 = 0;

        #[no_mangle]
        pub extern "C" fn _start() {
            unsafe {
                STATE = 0;
                let len = metashrew_core::host_len();
                INPUT_BUFFER.resize(len, 0);
                metashrew_core::load_input(INPUT_BUFFER.as_mut_ptr());
                
                // Process the block
                if let Err(e) = $process_block(&INPUT_BUFFER) {
                    metashrew_core::log(&format!("Error: {}", e));
                    return;
                }
                
                STATE = 1;
            }
        }
    };
}
```

### Example Usage

Here's a simple example of using `metashrew-core` to create an indexer:

```rust
use metashrew_core::{indexer, put, get, flush};
use anyhow::Result;

fn process_block(input: &[u8]) -> Result<()> {
    // Parse block height (first 4 bytes)
    let height = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
    
    // Parse block data (remaining bytes)
    let block_data = &input[4..];
    
    // Process the block
    // ...
    
    // Update the database
    put(b"last_height", &height.to_le_bytes())?;
    flush()?;
    
    Ok(())
}

// Generate the _start function
indexer!(process_block);
```

## metashrew-support

The `metashrew-support` library provides utilities for working with Bitcoin data structures. It simplifies parsing and processing Bitcoin blocks, transactions, and scripts.

### Key Features

1. **Bitcoin Data Structures**: Types for blocks, transactions, scripts, etc.
2. **Parsing Utilities**: Functions for parsing Bitcoin data
3. **Address Handling**: Utilities for working with Bitcoin addresses
4. **Byte View**: Efficient byte slice manipulation

### Bitcoin Data Structures

The library provides Rust types for Bitcoin data structures:

```rust
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

pub struct BlockHeader {
    pub version: i32,
    pub prev_blockhash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub time: u32,
    pub bits: u32,
    pub nonce: u32,
}

pub struct Transaction {
    pub version: i32,
    pub inputs: Vec<TxIn>,
    pub outputs: Vec<TxOut>,
    pub lock_time: u32,
}

// ... and so on
```

### Parsing Utilities

Functions for parsing Bitcoin data:

```rust
pub fn parse_block(data: &[u8]) -> Result<Block> {
    // Implementation
}

pub fn parse_transaction(data: &[u8]) -> Result<Transaction> {
    // Implementation
}

pub fn parse_script(data: &[u8]) -> Result<Script> {
    // Implementation
}
```

### Address Handling

Utilities for working with Bitcoin addresses:

```rust
pub fn script_to_address(script: &Script, network: Network) -> Option<String> {
    // Implementation
}

pub fn address_to_script(address: &str) -> Result<Script> {
    // Implementation
}
```

### Example Usage

Here's an example of using `metashrew-support` to process a Bitcoin block:

```rust
use metashrew_core::{indexer, put, flush};
use metashrew_support::{parse_block, script_to_address};
use anyhow::Result;

fn process_block(input: &[u8]) -> Result<()> {
    // Parse block height (first 4 bytes)
    let height = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
    
    // Parse block data (remaining bytes)
    let block_data = &input[4..];
    let block = parse_block(block_data)?;
    
    // Process each transaction
    for tx in &block.transactions {
        // Process outputs
        for (i, output) in tx.outputs.iter().enumerate() {
            if let Some(address) = script_to_address(&output.script_pubkey, Network::Bitcoin) {
                // Index address balance
                let key = format!("address:{}:balance", address).into_bytes();
                let value = output.value.to_le_bytes();
                put(&key, &value)?;
            }
        }
    }
    
    flush()?;
    Ok(())
}

// Generate the _start function
indexer!(process_block);
```

## Building Indexers

Using the helper libraries, you can build custom indexers for various use cases. This section provides guidance on common indexing patterns and best practices.

### Basic Indexer Structure

A typical indexer follows this structure:

1. **Parse Input**: Extract block height and block data
2. **Process Block**: Parse and process the block
3. **Update Database**: Store indexed data
4. **Flush Changes**: Commit changes to the database

```rust
fn process_block(input: &[u8]) -> Result<()> {
    // 1. Parse Input
    let height = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
    let block_data = &input[4..];
    let block = parse_block(block_data)?;
    
    // 2. Process Block
    // ... custom processing logic ...
    
    // 3. Update Database
    // ... store indexed data ...
    
    // 4. Flush Changes
    flush()?;
    
    Ok(())
}
```

### Common Indexing Patterns

#### 1. Address Balance Indexing

```rust
fn index_address_balances(block: &Block, height: u32) -> Result<()> {
    for tx in &block.transactions {
        // Process inputs (spending UTXOs)
        for input in &tx.inputs {
            if input.prev_output.is_coinbase() {
                continue;
            }
            
            // Get the previous output
            let prev_tx_key = format!("tx:{}", hex::encode(&input.prev_output.txid)).into_bytes();
            if let Some(prev_tx_data) = get(&prev_tx_key)? {
                let prev_tx = parse_transaction(&prev_tx_data)?;
                let prev_output = &prev_tx.outputs[input.prev_output.vout as usize];
                
                if let Some(address) = script_to_address(&prev_output.script_pubkey, Network::Bitcoin) {
                    // Deduct from address balance
                    let key = format!("address:{}:balance", address).into_bytes();
                    if let Some(balance_data) = get(&key)? {
                        let mut balance = u64::from_le_bytes(balance_data.try_into()?);
                        balance = balance.saturating_sub(prev_output.value);
                        put(&key, &balance.to_le_bytes())?;
                    }
                }
            }
        }
        
        // Process outputs (creating UTXOs)
        for (i, output) in tx.outputs.iter().enumerate() {
            if let Some(address) = script_to_address(&output.script_pubkey, Network::Bitcoin) {
                // Add to address balance
                let key = format!("address:{}:balance", address).into_bytes();
                let balance = if let Some(balance_data) = get(&key)? {
                    let mut balance = u64::from_le_bytes(balance_data.try_into()?);
                    balance = balance.saturating_add(output.value);
                    balance
                } else {
                    output.value
                };
                
                put(&key, &balance.to_le_bytes())?;
            }
        }
    }
    
    Ok(())
}
```

#### 2. Transaction Indexing

```rust
fn index_transactions(block: &Block, height: u32) -> Result<()> {
    for tx in &block.transactions {
        // Compute transaction hash
        let tx_hash = compute_tx_hash(tx);
        
        // Store transaction data
        let tx_key = format!("tx:{}", hex::encode(&tx_hash)).into_bytes();
        let tx_data = serialize_transaction(tx);
        put(&tx_key, &tx_data)?;
        
        // Store transaction height
        let tx_height_key = format!("tx:{}:height", hex::encode(&tx_hash)).into_bytes();
        put(&tx_height_key, &height.to_le_bytes())?;
    }
    
    Ok(())
}
```

#### 3. Custom Token Indexing

```rust
fn index_tokens(block: &Block, height: u32) -> Result<()> {
    for tx in &block.transactions {
        for (i, output) in tx.outputs.iter().enumerate() {
            // Check if the output contains token data
            if let Some(token_data) = extract_token_data(&output.script_pubkey) {
                // Store token data
                let token_id = compute_token_id(&tx.txid, i as u32);
                let token_key = format!("token:{}", hex::encode(&token_id)).into_bytes();
                put(&token_key, &token_data)?;
                
                // Update token supply
                update_token_supply(&token_data.token_type, token_data.amount)?;
            }
        }
    }
    
    Ok(())
}
```

### View Functions

In addition to the block processing logic, indexers can provide view functions for querying the indexed data:

```rust
#[no_mangle]
pub extern "C" fn get_address_balance(input_ptr: i32, input_len: i32) -> i32 {
    let input = unsafe { std::slice::from_raw_parts(input_ptr as *const u8, input_len as usize) };
    let address = std::str::from_utf8(input).unwrap();
    
    let key = format!("address:{}:balance", address).into_bytes();
    let result = match get(&key).unwrap() {
        Some(balance_data) => balance_data,
        None => vec![0, 0, 0, 0, 0, 0, 0, 0], // Zero balance
    };
    
    // Allocate memory for the result
    let result_ptr = alloc(result.len());
    unsafe {
        std::ptr::copy_nonoverlapping(result.as_ptr(), result_ptr, result.len());
    }
    
    result_ptr as i32
}
```

### Best Practices

1. **Efficient Data Structures**: Use appropriate data structures for your indexing needs
2. **Batch Updates**: Group database updates to minimize I/O operations
3. **Error Handling**: Implement robust error handling to prevent crashes
4. **Memory Management**: Be mindful of memory usage, especially for large blocks
5. **Deterministic Processing**: Ensure your indexer produces the same results for the same input
6. **Testing**: Test your indexer with various block types and edge cases
7. **Documentation**: Document your indexer's functionality and data schema

## Example Indexer

Here's a complete example of a simple address balance indexer:

```rust
use metashrew_core::{indexer, put, get, flush, alloc};
use metashrew_support::{parse_block, script_to_address, Network};
use anyhow::Result;

fn process_block(input: &[u8]) -> Result<()> {
    // Parse block height (first 4 bytes)
    let height = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
    
    // Parse block data (remaining bytes)
    let block_data = &input[4..];
    let block = parse_block(block_data)?;
    
    // Process each transaction
    for tx in &block.transactions {
        // Process inputs (spending UTXOs)
        for input in &tx.inputs {
            if input.prev_output.is_coinbase() {
                continue;
            }
            
            // Get the previous output
            let prev_tx_key = format!("tx:{}", hex::encode(&input.prev_output.txid)).into_bytes();
            if let Some(prev_tx_data) = get(&prev_tx_key)? {
                let prev_tx = parse_transaction(&prev_tx_data)?;
                let prev_output = &prev_tx.outputs[input.prev_output.vout as usize];
                
                if let Some(address) = script_to_address(&prev_output.script_pubkey, Network::Bitcoin) {
                    // Deduct from address balance
                    let key = format!("address:{}:balance", address).into_bytes();
                    if let Some(balance_data) = get(&key)? {
                        let mut balance = u64::from_le_bytes(balance_data.try_into()?);
                        balance = balance.saturating_sub(prev_output.value);
                        put(&key, &balance.to_le_bytes())?;
                    }
                }
            }
        }
        
        // Process outputs (creating UTXOs)
        for (i, output) in tx.outputs.iter().enumerate() {
            if let Some(address) = script_to_address(&output.script_pubkey, Network::Bitcoin) {
                // Add to address balance
                let key = format!("address:{}:balance", address).into_bytes();
                let balance = if let Some(balance_data) = get(&key)? {
                    let mut balance = u64::from_le_bytes(balance_data.try_into()?);
                    balance = balance.saturating_add(output.value);
                    balance
                } else {
                    output.value
                };
                
                put(&key, &balance.to_le_bytes())?;
            }
        }
    }
    
    // Store the last processed height
    put(b"last_height", &height.to_le_bytes())?;
    
    // Commit changes
    flush()?;
    
    Ok(())
}

// Generate the _start function
indexer!(process_block);

// View function to get address balance
#[no_mangle]
pub extern "C" fn get_address_balance(input_ptr: i32, input_len: i32) -> i32 {
    let input = unsafe { std::slice::from_raw_parts(input_ptr as *const u8, input_len as usize) };
    let address = std::str::from_utf8(input).unwrap();
    
    let key = format!("address:{}:balance", address).into_bytes();
    let result = match get(&key).unwrap() {
        Some(balance_data) => balance_data,
        None => vec![0, 0, 0, 0, 0, 0, 0, 0], // Zero balance
    };
    
    // Allocate memory for the result
    let result_ptr = alloc(result.len());
    unsafe {
        std::ptr::copy_nonoverlapping(result.as_ptr(), result_ptr, result.len());
    }
    
    result_ptr as i32
}
```

## Conclusion

The `metashrew-core` and `metashrew-support` libraries provide a solid foundation for building Bitcoin indexers and metaprotocols. By leveraging these libraries, developers can focus on their specific indexing logic rather than dealing with low-level details of block parsing and database operations.

Whether you're building a simple address balance tracker or a complex metaprotocol, these helper libraries can significantly accelerate your development process and ensure your indexer integrates seamlessly with the Metashrew platform.
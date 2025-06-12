# Development Guide

This guide provides instructions for setting up a development environment, creating custom indexers, testing and debugging, and following best practices when working with Metashrew.

## Setting Up Development Environment

### Prerequisites

Before you start developing with Metashrew, ensure you have the following prerequisites installed:

1. **Rust and Cargo**: Metashrew is built with Rust, so you'll need the Rust toolchain.
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **WASM Target**: Add the WebAssembly target to your Rust toolchain.
   ```bash
   rustup target add wasm32-unknown-unknown
   ```

3. **Bitcoin Core**: You'll need a Bitcoin Core node for indexing.
   ```bash
   # Example for Ubuntu
   sudo add-apt-repository ppa:bitcoin/bitcoin
   sudo apt-get update
   sudo apt-get install bitcoind
   ```

4. **RocksDB Dependencies**: Install the dependencies required for RocksDB.
   ```bash
   # Example for Ubuntu
   sudo apt-get install libclang-dev librocksdb-dev
   ```

5. **Git**: For version control and cloning repositories.
   ```bash
   sudo apt-get install git
   ```

### Cloning the Repository

Clone the Metashrew repository:

```bash
git clone https://github.com/metashrew/metashrew.git
cd metashrew
```

### Building Metashrew

Build the Metashrew binaries:

```bash
cargo build --release
```

This will build the following binaries:
- `rockshrew`: The standalone indexer
- `rockshrew-view`: The standalone view layer
- `rockshrew-mono`: The combined indexer and view layer

### Setting Up Bitcoin Core

Configure Bitcoin Core for development:

```bash
mkdir -p ~/.bitcoin
cat > ~/.bitcoin/bitcoin.conf << EOF
server=1
rpcuser=bitcoinrpc
rpcpassword=bitcoinrpc
txindex=1
rpcallowip=127.0.0.1
EOF

# Start Bitcoin Core
bitcoind -daemon
```

### Project Structure

A typical Metashrew indexer project has the following structure:

```
my-indexer/
├── Cargo.toml
├── src/
│   └── lib.rs
└── build.sh
```

#### Example Cargo.toml

```toml
[package]
name = "my-indexer"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
metashrew-core = { path = "../metashrew/metashrew-core" }
metashrew-support = { path = "../metashrew/metashrew-support" }
anyhow = "1.0"
hex = "0.4"
```

#### Example build.sh

```bash
#!/bin/bash
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/my_indexer.wasm ./my_indexer.wasm
```

## Creating a Custom Indexer

### Basic Indexer Structure

A basic indexer consists of a `lib.rs` file with the following structure:

```rust
use metashrew_core::{indexer, put, get, flush};
use metashrew_support::{parse_block, script_to_address, Network};
use anyhow::Result;

fn process_block(input: &[u8]) -> Result<()> {
    // Parse block height (first 4 bytes)
    let height = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
    
    // Parse block data (remaining bytes)
    let block_data = &input[4..];
    let block = parse_block(block_data)?;
    
    // Process the block
    // ...
    
    // Commit changes
    flush()?;
    
    Ok(())
}

// Generate the _start function
indexer!(process_block);
```

### Adding View Functions

View functions allow querying the indexed data:

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

### Example Indexers

#### Address Balance Indexer

This indexer tracks the balance of Bitcoin addresses:

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

#### Token Indexer

This indexer tracks custom tokens embedded in OP_RETURN outputs:

```rust
use metashrew_core::{indexer, put, get, flush, alloc};
use metashrew_support::{parse_block, parse_script};
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
            // Check if the output is an OP_RETURN
            if output.script_pubkey.is_op_return() {
                // Extract token data from OP_RETURN
                if let Some(token_data) = extract_token_data(&output.script_pubkey) {
                    // Store token data
                    let token_id = format!("{}:{}", hex::encode(&tx.txid), i);
                    let token_key = format!("token:{}", token_id).into_bytes();
                    put(&token_key, &token_data)?;
                    
                    // Update token supply
                    let token_type = extract_token_type(&token_data);
                    let token_amount = extract_token_amount(&token_data);
                    
                    let supply_key = format!("token:{}:supply", token_type).into_bytes();
                    let supply = if let Some(supply_data) = get(&supply_key)? {
                        let mut supply = u64::from_le_bytes(supply_data.try_into()?);
                        supply = supply.saturating_add(token_amount);
                        supply
                    } else {
                        token_amount
                    };
                    
                    put(&supply_key, &supply.to_le_bytes())?;
                }
            }
        }
    }
    
    // Store the last processed height
    put(b"last_height", &height.to_le_bytes())?;
    
    // Commit changes
    flush()?;
    
    Ok(())
}

// Helper function to extract token data from OP_RETURN
fn extract_token_data(script: &Script) -> Option<Vec<u8>> {
    // Implementation depends on the token protocol
    // ...
}

// Helper function to extract token type from token data
fn extract_token_type(token_data: &[u8]) -> String {
    // Implementation depends on the token protocol
    // ...
}

// Helper function to extract token amount from token data
fn extract_token_amount(token_data: &[u8]) -> u64 {
    // Implementation depends on the token protocol
    // ...
}

// Generate the _start function
indexer!(process_block);

// View function to get token supply
#[no_mangle]
pub extern "C" fn get_token_supply(input_ptr: i32, input_len: i32) -> i32 {
    let input = unsafe { std::slice::from_raw_parts(input_ptr as *const u8, input_len as usize) };
    let token_type = std::str::from_utf8(input).unwrap();
    
    let key = format!("token:{}:supply", token_type).into_bytes();
    let result = match get(&key).unwrap() {
        Some(supply_data) => supply_data,
        None => vec![0, 0, 0, 0, 0, 0, 0, 0], // Zero supply
    };
    
    // Allocate memory for the result
    let result_ptr = alloc(result.len());
    unsafe {
        std::ptr::copy_nonoverlapping(result.as_ptr(), result_ptr, result.len());
    }
    
    result_ptr as i32
}
```

## Testing and Debugging

### Unit Testing

You can write unit tests for your indexer using Rust's testing framework:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_extract_token_data() {
        // Create a test script
        let script = create_test_script();
        
        // Extract token data
        let token_data = extract_token_data(&script);
        
        // Assert that token data is extracted correctly
        assert!(token_data.is_some());
        assert_eq!(token_data.unwrap(), vec![1, 2, 3, 4]);
    }
    
    #[test]
    fn test_extract_token_type() {
        // Create test token data
        let token_data = vec![1, 2, 3, 4];
        
        // Extract token type
        let token_type = extract_token_type(&token_data);
        
        // Assert that token type is extracted correctly
        assert_eq!(token_type, "TEST");
    }
    
    #[test]
    fn test_extract_token_amount() {
        // Create test token data
        let token_data = vec![1, 2, 3, 4];
        
        // Extract token amount
        let token_amount = extract_token_amount(&token_data);
        
        // Assert that token amount is extracted correctly
        assert_eq!(token_amount, 1000);
    }
}
```

### Integration Testing

You can write integration tests that run your indexer against real or simulated blocks:

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    
    #[test]
    fn test_process_block() {
        // Create a test block
        let block = create_test_block();
        let height = 123456;
        
        // Serialize the block
        let mut input = Vec::new();
        input.extend_from_slice(&height.to_le_bytes());
        input.extend_from_slice(&serialize_block(&block));
        
        // Process the block
        let result = process_block(&input);
        
        // Assert that the block was processed successfully
        assert!(result.is_ok());
        
        // Assert that the expected data was stored
        let key = b"last_height";
        let value = get(key).unwrap().unwrap();
        assert_eq!(u32::from_le_bytes(value.try_into().unwrap()), height);
    }
}
```

### Debugging with Logs

You can use the `log` function to output debug information:

```rust
use metashrew_core::{indexer, put, get, flush, log};

fn process_block(input: &[u8]) -> Result<()> {
    // Parse block height (first 4 bytes)
    let height = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
    
    log(&format!("Processing block at height {}", height));
    
    // Parse block data (remaining bytes)
    let block_data = &input[4..];
    let block = parse_block(block_data)?;
    
    log(&format!("Block has {} transactions", block.transactions.len()));
    
    // Process the block
    // ...
    
    log(&format!("Finished processing block at height {}", height));
    
    // Commit changes
    flush()?;
    
    Ok(())
}
```

### Debugging with rockshrew-mono

You can run your indexer with `rockshrew-mono` in debug mode:

```bash
RUST_LOG=debug rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --start-block 500000 \
  --exit-at 500010
```

This will process blocks 500000 to 500010 and output debug information.

### Common Debugging Issues

1. **Memory Allocation**: Ensure you're allocating enough memory for your data.
2. **Error Handling**: Handle errors properly to avoid crashes.
3. **Database Operations**: Ensure you're using the correct keys for database operations.
4. **Block Parsing**: Ensure you're parsing blocks correctly.
5. **View Functions**: Ensure your view functions are returning the correct data.

## Best Practices

### Database Schema Design

1. **Consistent Key Format**: Use a consistent format for database keys.
   ```
   <namespace>:<identifier>:<attribute>
   ```
   Example: `address:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa:balance`

2. **Namespace Isolation**: Use namespaces to isolate different types of data.
   ```
   address:... for address data
   tx:... for transaction data
   token:... for token data
   ```

3. **Efficient Queries**: Design your schema for efficient queries.
   ```
   // Good: Direct lookup
   address:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa:balance
   
   // Bad: Requires scanning
   address:balance:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa
   ```

### Memory Management

1. **Minimize Allocations**: Minimize memory allocations to improve performance.
   ```rust
   // Good: Reuse buffer
   let mut buffer = Vec::with_capacity(1024);
   
   // Bad: New allocation for each iteration
   for i in 0..1000 {
       let buffer = Vec::with_capacity(1024);
       // ...
   }
   ```

2. **Release Memory**: Release memory when it's no longer needed.
   ```rust
   // Good: Drop large data structures when done
   {
       let large_data = load_large_data();
       process_data(&large_data);
   } // large_data is dropped here
   
   // Bad: Keep large data structures in memory
   let large_data = load_large_data();
   process_data(&large_data);
   // large_data is still in memory
   ```

### Error Handling

1. **Propagate Errors**: Propagate errors to the caller using the `?` operator.
   ```rust
   fn process_block(input: &[u8]) -> Result<()> {
       let height = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
       let block_data = &input[4..];
       let block = parse_block(block_data)?; // Propagate error
       
       // ...
       
       Ok(())
   }
   ```

2. **Provide Context**: Add context to errors to make them more informative.
   ```rust
   use anyhow::{Context, Result};
   
   fn process_block(input: &[u8]) -> Result<()> {
       let height = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
       let block_data = &input[4..];
       let block = parse_block(block_data)
           .context(format!("Failed to parse block at height {}", height))?;
       
       // ...
       
       Ok(())
   }
   ```

### Performance Optimization

1. **Batch Database Operations**: Group database operations to minimize I/O.
   ```rust
   // Good: Batch operations
   for tx in &block.transactions {
       // Process tx
       // ...
   }
   flush()?; // Single flush at the end
   
   // Bad: Flush after each transaction
   for tx in &block.transactions {
       // Process tx
       // ...
       flush()?; // Flush after each transaction
   }
   ```

2. **Minimize Database Lookups**: Cache frequently accessed data.
   ```rust
   // Good: Cache data
   let mut address_balances = HashMap::new();
   
   for tx in &block.transactions {
       for output in &tx.outputs {
           if let Some(address) = script_to_address(&output.script_pubkey, Network::Bitcoin) {
               let balance = address_balances.entry(address.clone())
                   .or_insert_with(|| {
                       let key = format!("address:{}:balance", address).into_bytes();
                       get(&key).unwrap().map(|data| u64::from_le_bytes(data.try_into().unwrap())).unwrap_or(0)
                   });
               
               *balance += output.value;
           }
       }
   }
   
   for (address, balance) in address_balances {
       let key = format!("address:{}:balance", address).into_bytes();
       put(&key, &balance.to_le_bytes())?;
   }
   
   // Bad: Lookup for each output
   for tx in &block.transactions {
       for output in &tx.outputs {
           if let Some(address) = script_to_address(&output.script_pubkey, Network::Bitcoin) {
               let key = format!("address:{}:balance", address).into_bytes();
               let balance = get(&key)?.map(|data| u64::from_le_bytes(data.try_into().unwrap())).unwrap_or(0);
               put(&key, &(balance + output.value).to_le_bytes())?;
           }
       }
   }
   ```

### Documentation

1. **Code Comments**: Document your code with comments.
   ```rust
   /// Process a Bitcoin block and index address balances
   ///
   /// # Arguments
   ///
   /// * `input` - Input data containing block height and block data
   ///
   /// # Returns
   ///
   /// * `Result<()>` - Ok if successful, Err otherwise
   fn process_block(input: &[u8]) -> Result<()> {
       // ...
   }
   ```

2. **README**: Create a README file for your indexer.
   ```markdown
   # Address Balance Indexer
   
   This indexer tracks the balance of Bitcoin addresses.
   
   ## Features
   
   - Track address balances
   - Query address balances
   - Historical balance queries
   
   ## Usage
   
   ```bash
   rockshrew-mono \
     --daemon-rpc-url http://localhost:8332 \
     --auth bitcoinrpc:bitcoinrpc \
     --indexer path/to/address_balance_indexer.wasm \
     --db-path ~/.metashrew
   ```
   
   ## API
   
   ### get_address_balance
   
   Returns the balance of an address.
   
   Parameters:
   - `address` (string): The Bitcoin address
   
   Returns:
   - `balance` (u64): The address balance in satoshis
   ```

### Version Control

1. **Git**: Use Git for version control.
   ```bash
   git init
   git add .
   git commit -m "Initial commit"
   ```

2. **Semantic Versioning**: Use semantic versioning for your indexer.
   ```toml
   [package]
   name = "my-indexer"
   version = "0.1.0" # Major.Minor.Patch
   ```

## Conclusion

This development guide provides a foundation for building custom indexers with Metashrew. By following these guidelines, you can create efficient, reliable, and maintainable indexers that leverage the power of the Metashrew platform.

Remember that indexer development is an iterative process. Start with a simple indexer, test it thoroughly, and gradually add more features as you become more familiar with the platform.

For more information, refer to the other documentation sections, particularly the [Helper Libraries](./helper-libraries.md) and [JSON-RPC Interface](./json-rpc.md) sections.
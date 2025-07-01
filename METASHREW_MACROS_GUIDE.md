# Metashrew Procedural Macros Guide

This guide covers the two procedural macros provided by the Metashrew framework for simplifying indexer and view function development.

## Overview

The Metashrew framework provides two key procedural macros:

1. **`#[metashrew_core::main]`** - For creating indexer main functions
2. **`#[metashrew_core::view]`** - For creating view functions

These macros eliminate boilerplate code and provide consistent input parsing and error handling.

## `#[metashrew_core::main]` Macro

### Purpose
Transforms a main indexer function into a complete WASM-compatible indexer with automatic input parsing and error handling.

### Usage

```rust
use metashrew_core::main;

#[main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    // Your indexer logic here
    Ok(())
}
```

### Generated Code

The macro generates the following `_start` function:

```rust
#[cfg(all(target_arch = "wasm32", not(test)))]
#[no_mangle]
pub fn _start() {
    let mut host_input = std::io::Cursor::new(metashrew_core::input());
    let height = metashrew_support::utils::consume_sized_int::<u32>(&mut host_input)
        .expect("failed to parse height");
    let input_vec = metashrew_support::utils::consume_to_end(&mut host_input)
        .expect("failed to parse bytearray from input after height");
    main(height, &input_vec).expect("failed to run indexer");
    metashrew_core::flush();
}
```

### Requirements

- **Function name**: Must be `main`
- **Parameters**: Exactly 2 parameters:
  - `height: u32` - The block height
  - `block: &[u8]` - The serialized block data
- **Return type**: `Result<(), Box<dyn std::error::Error>>`

### Example

```rust
use metashrew_core::{main, set, get};
use std::sync::Arc;

#[main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    // Parse the block
    let block: bitcoin::Block = metashrew_support::utils::consensus_decode(
        &mut std::io::Cursor::new(block.to_vec())
    )?;
    
    // Process transactions
    for (i, tx) in block.txdata.iter().enumerate() {
        let key = Arc::new(format!("tx_{}_{}", height, i).into_bytes());
        let value = Arc::new(tx.compute_txid().to_string().into_bytes());
        set(key, value);
    }
    
    Ok(())
}
```

## `#[metashrew_core::view]` Macro

### Purpose
Transforms a view function into a WASM-compatible export function with automatic input parsing and result serialization.

### Usage

```rust
use metashrew_core::view;

#[view]
pub fn protorunesbyaddress(input: &[u8]) -> Result<&[u8], Box<dyn std::error::Error>> {
    // Your view logic here
    Ok(b"result_data")
}
```

### Generated Code

The macro generates two functions:

1. **Internal function** (with `__` prefix):
```rust
pub fn __protorunesbyaddress(input: &[u8]) -> Result<&[u8], Box<dyn std::error::Error>> {
    // Your original function body
}
```

2. **WASM export function**:
```rust
#[cfg(not(test))]
#[no_mangle]
pub fn protorunesbyaddress() -> i32 {
    let mut host_input = std::io::Cursor::new(metashrew_core::input());
    let _height = metashrew_support::utils::consume_sized_int::<u32>(&mut host_input)
        .expect("failed to read height from host input");
    let result = __protorunesbyaddress(&metashrew_support::utils::consume_to_end(&mut host_input)
        .expect("failed to read input from host environment")).unwrap();
    metashrew_support::compat::export_bytes(result.to_vec())
}
```

### Requirements

- **Parameters**: Exactly 1 parameter:
  - `input: &[u8]` - The input data from the host
- **Return type**: `Result<&[u8], Box<dyn std::error::Error>>`

### Example

```rust
use metashrew_core::{view, get};
use std::sync::Arc;

#[view]
pub fn get_balance(input: &[u8]) -> Result<&[u8], Box<dyn std::error::Error>> {
    // Parse the address from input
    let address = std::str::from_utf8(input)?;
    
    // Query the database
    let key = Arc::new(format!("balance_{}", address).into_bytes());
    let balance = get(key);
    
    // Return the balance
    Ok(&balance)
}

#[view]
pub fn get_transaction(input: &[u8]) -> Result<&[u8], Box<dyn std::error::Error>> {
    // Parse transaction ID from input
    let txid = hex::encode(input);
    
    // Query transaction data
    let key = Arc::new(format!("tx_{}", txid).into_bytes());
    let tx_data = get(key);
    
    if tx_data.is_empty() {
        return Err("Transaction not found".into());
    }
    
    Ok(&tx_data)
}
```

## Complete Indexer Example

Here's a complete example showing both macros in use:

```rust
use metashrew_core::{main, view, set, get};
use std::sync::Arc;

// Main indexer function
#[main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    // Parse the block
    let block: bitcoin::Block = metashrew_support::utils::consensus_decode(
        &mut std::io::Cursor::new(block.to_vec())
    )?;
    
    // Index block metadata
    let block_key = Arc::new(format!("block_{}", height).into_bytes());
    let block_hash = Arc::new(block.block_hash().to_string().into_bytes());
    set(block_key, block_hash);
    
    // Index transactions
    for (i, tx) in block.txdata.iter().enumerate() {
        let tx_key = Arc::new(format!("tx_{}_{}", height, i).into_bytes());
        let tx_data = Arc::new(metashrew_support::utils::consensus_encode(tx)?);
        set(tx_key, tx_data);
        
        // Index by transaction ID
        let txid_key = Arc::new(format!("txid_{}", tx.compute_txid()).into_bytes());
        let location = Arc::new(format!("{}:{}", height, i).into_bytes());
        set(txid_key, location);
    }
    
    Ok(())
}

// View function to get block information
#[view]
pub fn get_block(input: &[u8]) -> Result<&[u8], Box<dyn std::error::Error>> {
    let height_str = std::str::from_utf8(input)?;
    let height: u32 = height_str.parse()?;
    
    let key = Arc::new(format!("block_{}", height).into_bytes());
    let block_hash = get(key);
    
    if block_hash.is_empty() {
        return Err("Block not found".into());
    }
    
    Ok(&block_hash)
}

// View function to get transaction by ID
#[view]
pub fn get_transaction_by_id(input: &[u8]) -> Result<&[u8], Box<dyn std::error::Error>> {
    let txid = std::str::from_utf8(input)?;
    
    // Get transaction location
    let location_key = Arc::new(format!("txid_{}", txid).into_bytes());
    let location = get(location_key);
    
    if location.is_empty() {
        return Err("Transaction not found".into());
    }
    
    let location_str = std::str::from_utf8(&location)?;
    let parts: Vec<&str> = location_str.split(':').collect();
    let height: u32 = parts[0].parse()?;
    let index: usize = parts[1].parse()?;
    
    // Get transaction data
    let tx_key = Arc::new(format!("tx_{}_{}", height, index).into_bytes());
    let tx_data = get(tx_key);
    
    Ok(&tx_data)
}
```

## Testing

Both macros preserve the original functions for testing:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indexer() {
        let height = 12345;
        let block_data = b"test_block_data";
        
        let result = main(height, block_data);
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_view_function() {
        let input = b"test_input";
        
        let result = __get_block(input);
        assert!(result.is_ok());
    }
}
```

## Migration from Manual Implementation

### Before (Manual)

```rust
#[cfg(all(target_arch = "wasm32", not(test)))]
#[no_mangle]
pub fn _start() {
    let data = metashrew_core::input();
    let height = u32::from_le_bytes((&data[0..4]).try_into().unwrap());
    let reader = &data[4..];
    index_block(height, reader).unwrap();
    metashrew_core::flush();
}

#[cfg(not(test))]
#[no_mangle]
pub fn get_balance() -> i32 {
    let data = metashrew_core::input();
    let _height = u32::from_le_bytes((&data[0..4]).try_into().unwrap());
    let reader = &data[4..];
    let result = query_balance(reader).unwrap();
    metashrew_support::compat::export_bytes(result)
}
```

### After (With Macros)

```rust
#[metashrew_core::main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    index_block(height, block)?;
    Ok(())
}

#[metashrew_core::view]
pub fn get_balance(input: &[u8]) -> Result<&[u8], Box<dyn std::error::Error>> {
    query_balance(input)
}
```

## Benefits

1. **Reduced Boilerplate**: Eliminates repetitive input parsing and export code
2. **Consistent Error Handling**: Standardized error propagation and reporting
3. **Type Safety**: Compile-time validation of function signatures
4. **Testing Support**: Preserves original functions for unit testing
5. **WASM Compatibility**: Automatic handling of WASM-specific compilation flags
6. **Maintainability**: Centralized input/output handling logic

## Error Handling

Both macros provide automatic error handling:

- **Input parsing errors** are caught with descriptive messages
- **Function execution errors** are propagated with context
- **Type validation** occurs at compile time

## Best Practices

1. **Use descriptive function names** for view functions (they become WASM exports)
2. **Handle errors gracefully** in your function implementations
3. **Keep view functions focused** on single queries
4. **Use the internal functions** (with `__` prefix) for unit testing
5. **Document your functions** as they become part of the public API
6. **Validate input data** in view functions before processing

The macros provide a clean, type-safe way to build Metashrew indexers while maintaining full compatibility with the existing ecosystem.
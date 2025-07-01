# Using the #[metashrew_core::main] Procedural Macro

The `#[metashrew_core::main]` procedural macro simplifies the creation of Metashrew indexers by automatically generating the required `_start` function and input parsing logic.

## Overview

The macro transforms a function with the signature `fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>>` into a complete Metashrew indexer with the required `_start` function.

## Usage

### Basic Example

```rust
use metashrew_core::main;

#[main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    // Your indexer logic here
    
    // Example: Process the block data
    println!("Processing block at height: {}", height);
    println!("Block size: {} bytes", block.len());
    
    // Example: Store some data
    use metashrew_core::{set, get};
    use std::sync::Arc;
    
    let key = Arc::new(format!("block_{}", height).into_bytes());
    let value = Arc::new(format!("processed_at_{}", height).into_bytes());
    
    set(key, value);
    
    Ok(())
}
```

### What the Macro Generates

The macro automatically generates the following `_start` function:

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

## Function Signature Requirements

The macro validates that your function has the correct signature:

- **First parameter**: `height: u32` - The block height
- **Second parameter**: `block: &[u8]` - The serialized block data
- **Return type**: `Result<(), Box<dyn std::error::Error>>` - For error handling

### Invalid Examples

These will cause compilation errors:

```rust
// Wrong parameter types
#[main]
pub fn main(height: String, block: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
    // ERROR: height must be u32, block must be &[u8]
    Ok(())
}

// Wrong number of parameters
#[main]
pub fn main(height: u32) -> Result<(), Box<dyn std::error::Error>> {
    // ERROR: Must have exactly 2 parameters
    Ok(())
}
```

## Integration with Existing Patterns

The macro works seamlessly with existing Metashrew patterns:

### With View Functions

```rust
use metashrew_core::main;

#[main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    // Index the block
    index_block(height, block)?;
    Ok(())
}

// View functions remain unchanged
#[cfg(not(test))]
#[no_mangle]
pub fn get_balance() -> i32 {
    // View function implementation
    0
}
```

### With Error Handling

```rust
use metashrew_core::main;
use anyhow::Result;

#[main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    // Parse the block
    let block = parse_block(block)
        .map_err(|e| format!("Failed to parse block: {}", e))?;
    
    // Process transactions
    for tx in &block.txdata {
        process_transaction(height, tx)
            .map_err(|e| format!("Failed to process transaction: {}", e))?;
    }
    
    Ok(())
}

fn parse_block(data: &[u8]) -> Result<bitcoin::Block> {
    // Block parsing logic
    todo!()
}

fn process_transaction(height: u32, tx: &bitcoin::Transaction) -> Result<()> {
    // Transaction processing logic
    todo!()
}
```

## Comparison with Manual Implementation

### Before (Manual)

```rust
#[cfg(all(target_arch = "wasm32", not(test)))]
#[no_mangle]
pub fn _start() {
    let data = metashrew_core::input();
    let height = u32::from_le_bytes((&data[0..4]).try_into().unwrap());
    let reader = &data[4..];
    let block: bitcoin::Block = 
        metashrew_support::utils::consensus_decode(&mut std::io::Cursor::new(reader.to_vec())).unwrap();
    
    index_block(&block, height).unwrap();
    metashrew_core::flush();
}

fn index_block(block: &bitcoin::Block, height: u32) -> Result<(), Box<dyn std::error::Error>> {
    // Indexer logic
    Ok(())
}
```

### After (With Macro)

```rust
use metashrew_core::main;

#[main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    // Parse block if needed
    let block: bitcoin::Block = 
        metashrew_support::utils::consensus_decode(&mut std::io::Cursor::new(block.to_vec()))?;
    
    // Indexer logic
    index_block(&block, height)?;
    Ok(())
}

fn index_block(block: &bitcoin::Block, height: u32) -> Result<(), Box<dyn std::error::Error>> {
    // Indexer logic
    Ok(())
}
```

## Benefits

1. **Reduced Boilerplate**: Eliminates the need to manually write `_start` function
2. **Consistent Input Parsing**: Standardizes how height and block data are extracted
3. **Error Handling**: Automatic error propagation with clear error messages
4. **Type Safety**: Compile-time validation of function signatures
5. **WASM Compatibility**: Automatically handles WASM-specific compilation flags

## Testing

The macro preserves your original function, so you can test it directly:

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
}
```

## Migration Guide

To migrate existing indexers to use the macro:

1. **Identify your main indexing function** (usually called from `_start`)
2. **Rename it to `main`** and add the `#[main]` attribute
3. **Update the signature** to match `fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>>`
4. **Remove your manual `_start` function**
5. **Add error handling** using `?` operator or explicit error conversion

The macro will handle all the input parsing and error management automatically.
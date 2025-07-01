# Height-Partitioned Caching System

This document describes the enhanced LRU caching system that supports height-partitioned caching for view functions and proper cache management for indexer vs view functions.

## Overview

The enhanced caching system provides:

1. **Height-Partitioned Caching**: View functions cache entries partitioned by block height
2. **Cache Isolation**: View functions only see cache entries for their specific height
3. **Proper Cache Management**: Different cache behavior for indexer vs view functions
4. **No Side Effects**: View functions don't affect the main cache or database state

## Architecture

### Three-Tier Caching System

```text
Indexer Mode:
get() -> CACHE -> LRU_CACHE -> __get/__get_len (host calls)

View Mode (height-partitioned):
get() -> CACHE -> HEIGHT_PARTITIONED_CACHE[height] -> __get/__get_len (host calls)
```

### Cache Types

1. **CACHE**: Immediate cache, cleared after each function call
2. **LRU_CACHE**: Persistent cache for indexer functions, survives across blocks
3. **HEIGHT_PARTITIONED_CACHE**: Partitioned cache for view functions by height
4. **API_CACHE**: User-defined caching for arbitrary data

## Key Functions

### Cache Management

#### `flush_to_lru()`
- Called by `#[metashrew_core::main]` before `flush()`
- Moves all CACHE entries to LRU_CACHE
- Clears CACHE after transfer
- Only operates in indexer mode (not view functions)

#### `set_view_for_height(height: u32)`
- Called by `#[metashrew_core::view]` at start
- Sets current view height for partitioned caching
- Subsequent `get()` calls use height-partitioned cache

#### `clear_view_cache()`
- Called by `#[metashrew_core::view]` at end
- Clears view height setting
- Clears immediate CACHE
- Ensures no side effects from view functions

### Height-Partitioned Operations

#### `get_height_partitioned_cache(height: u32, key: &Arc<Vec<u8>>)`
- Retrieves value from height-partitioned cache
- Only returns values cached for the specific height
- Updates LRU ordering on access

#### `set_height_partitioned_cache(height: u32, key: Arc<Vec<u8>>, value: Arc<Vec<u8>>)`
- Stores value in height-partitioned cache
- Associates value with specific height
- Automatic eviction when memory limit reached

## Macro Integration

### `#[metashrew_core::main]` Macro

**Generated Code:**
```rust
#[no_mangle]
pub fn _start() {
    let mut host_input = std::io::Cursor::new(metashrew_core::input());
    let height = metashrew_support::utils::consume_sized_int::<u32>(&mut host_input)
        .expect("failed to parse height");
    let input_vec = metashrew_support::utils::consume_to_end(&mut host_input)
        .expect("failed to parse bytearray from input after height");
    main(height, &input_vec).expect("failed to run indexer");
    metashrew_core::flush_to_lru();  // NEW: Transfer CACHE to LRU_CACHE
    metashrew_core::flush();
}
```

**Key Changes:**
- Calls `flush_to_lru()` before `flush()`
- Ensures CACHE contents persist in LRU_CACHE
- Provides consistent performance across blocks

### `#[metashrew_core::view]` Macro

**Generated Code:**
```rust
#[no_mangle]
pub fn protorunesbyaddress() -> i32 {
    let mut host_input = std::io::Cursor::new(metashrew_core::input());
    let height = metashrew_support::utils::consume_sized_int::<u32>(&mut host_input)
        .expect("failed to read height from host input");
    
    // NEW: Set view height for partitioned caching
    metashrew_core::set_view_for_height(height);
    
    let result = __protorunesbyaddress(&metashrew_support::utils::consume_to_end(&mut host_input)
        .expect("failed to read input from host environment")).unwrap();
    
    // NEW: Clear view height and immediate cache
    metashrew_core::clear_view_cache();
    
    metashrew_support::compat::export_bytes(result.to_vec())
}
```

**Key Changes:**
- Sets view height at start for partitioned caching
- Clears view state at end to prevent side effects
- Ensures cache isolation by height

## Cache Behavior

### Indexer Functions (`#[metashrew_core::main]`)

1. **During Execution:**
   - `get()` checks: CACHE → LRU_CACHE → host calls
   - `set()` populates: CACHE (immediate)
   - Values accumulate in CACHE during block processing

2. **At End (flush_to_lru()):**
   - All CACHE entries transferred to LRU_CACHE
   - CACHE cleared for next block
   - LRU_CACHE persists across blocks

3. **Benefits:**
   - Consistent performance across blocks
   - No memory growth in immediate cache
   - Persistent caching of frequently accessed data

### View Functions (`#[metashrew_core::view]`)

1. **During Execution:**
   - `get()` checks: CACHE → HEIGHT_PARTITIONED_CACHE[height] → host calls
   - Values cached only for specific height
   - No cross-contamination between heights

2. **At End (clear_view_cache()):**
   - CACHE cleared completely
   - View height cleared
   - No side effects on main caches

3. **Benefits:**
   - Proper archival state queries
   - Cache isolation by height
   - No side effects on indexer state

## Memory Management

### Memory Distribution
- **LRU_CACHE**: 1/3 of total memory limit (≈333MB)
- **HEIGHT_PARTITIONED_CACHE**: 1/3 of total memory limit (≈333MB)
- **API_CACHE**: 1/3 of total memory limit (≈333MB)
- **Total Limit**: 1GB hard limit

### Eviction Policy
- LRU (Least Recently Used) eviction when memory limit reached
- Automatic eviction maintains memory bounds
- Statistics tracking for monitoring

## Usage Examples

### Indexer Function
```rust
use metashrew_core::{main, set, get};
use std::sync::Arc;

#[main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    // Process block data
    let key = Arc::new(format!("block_{}", height).into_bytes());
    let value = Arc::new(b"processed_data".to_vec());
    
    // This goes to CACHE, then transferred to LRU_CACHE at end
    set(key, value);
    
    Ok(())
}
// flush_to_lru() and flush() called automatically
```

### View Function
```rust
use metashrew_core::{view, get};
use std::sync::Arc;

#[view]
pub fn get_block_data(input: &[u8]) -> Result<&[u8], Box<dyn std::error::Error>> {
    // Height is automatically set for partitioned caching
    let key = Arc::new(b"some_key".to_vec());
    
    // This checks HEIGHT_PARTITIONED_CACHE[height] only
    let value = get(key);
    
    Ok(&value)
}
// clear_view_cache() called automatically
```

## Benefits

### For Indexers
1. **Consistent Performance**: LRU cache persists across blocks
2. **Memory Efficiency**: Automatic transfer from immediate to persistent cache
3. **No Memory Leaks**: CACHE cleared after each block

### For View Functions
1. **Archival Correctness**: Only see data for specific height
2. **No Side Effects**: Don't affect main indexer state
3. **Cache Isolation**: Height-partitioned prevents cross-contamination
4. **Performance**: Cached lookups for repeated queries at same height

### For System
1. **Memory Bounded**: Hard 1GB limit prevents OOM crashes
2. **Automatic Management**: No manual cache management required
3. **Statistics**: Monitoring and debugging capabilities
4. **Thread Safe**: Concurrent access support

## Migration Guide

### Existing Code
No changes required for existing indexers using the macros. The enhanced caching is automatic and transparent.

### New Features Available
1. **API Cache**: Use `cache_set()`, `cache_get()`, `cache_remove()` for custom caching
2. **Statistics**: Use `lru_cache_stats()` for monitoring
3. **Memory Usage**: Use `lru_cache_memory_usage()` for tracking

## Technical Implementation

### Key Types
```rust
// Height-partitioned cache key
pub struct HeightPartitionedKey {
    pub height: u32,
    pub key: Arc<Vec<u8>>,
}

// Cache statistics
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub items: usize,
    pub memory_usage: usize,
    pub evictions: u64,
}
```

### Global State
```rust
// Main LRU cache for indexer functions
static LRU_CACHE: RwLock<Option<LruCache<CacheKey, CacheValue>>>;

// Height-partitioned cache for view functions
static HEIGHT_PARTITIONED_CACHE: RwLock<Option<LruCache<HeightPartitionedKey, CacheValue>>>;

// Current view height for partitioning
static CURRENT_VIEW_HEIGHT: RwLock<Option<u32>>;
```

The system provides a robust, memory-bounded caching solution that ensures correct behavior for both indexer and view functions while maintaining high performance and preventing side effects.
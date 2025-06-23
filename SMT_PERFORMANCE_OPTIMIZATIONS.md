# SMT Performance Optimizations

## Overview

This document describes the performance optimizations implemented for Metashrew's Sparse Merkle Tree (SMT) operations to address the user's concern that "Performance is not great" while maintaining the constraint of not persisting any cache or state between different blocks.

## Problem Statement

The original SMT implementation had performance issues due to:
1. Individual database operations for each SMT node update
2. Repeated key hashing computations
3. No batching of database writes
4. Lack of in-memory caching during block processing

## Solution: Batch Operations and In-Memory Caching

### 1. BatchedSMTHelper

A new `BatchedSMTHelper` struct that provides optimized SMT operations with:

- **In-memory node caching**: Caches SMT nodes during block processing to avoid repeated database reads
- **Key hash caching**: Pre-computes and caches key hashes to avoid repeated SHA256 operations
- **Batch database operations**: Groups all database writes into a single atomic batch operation
- **Cache clearing**: Automatically clears all caches after each block (no persistent state)

```rust
pub struct BatchedSMTHelper<T: KeyValueStoreLike> {
    pub storage: T,
    // In-memory cache for the current block processing (cleared after each block)
    node_cache: HashMap<[u8; 32], SMTNode>,
    // Pre-computed key hashes to avoid repeated hashing
    key_hash_cache: HashMap<Vec<u8>, [u8; 32]>,
}
```

### 2. Batch Operations in Regular SMTHelper

Added batch-optimized methods to the regular `SMTHelper`:

- `calculate_and_store_state_root_batched()`: Batch version of state root calculation
- `compute_batched_smt_root()`: Process multiple keys in a single batch
- `update_smt_for_key_batched()`: Update SMT for individual keys using batch operations
- `compute_path_updates_batched()`: Compute path updates with batch writes
- `create_separating_internals_batched()`: Create internal nodes using batch operations

### 3. Key Optimizations

#### Database I/O Reduction
- **Before**: Each SMT node update required individual database writes
- **After**: All operations are batched and written atomically at the end

#### Caching Strategy
- **Node Cache**: Stores recently accessed SMT nodes in memory during block processing
- **Key Hash Cache**: Avoids repeated SHA256 computations for the same keys
- **Cache Lifecycle**: Caches are cleared after each block to maintain stateless operation

#### Binary Search for BST Lookups
- Implemented `bst_get_at_height_fast()` using binary search instead of linear scan
- Reduces BST lookup complexity from O(n) to O(log n)

## Performance Results

Based on test results with the mock storage implementation:

### Batch vs Individual Operations

| Keys | Individual Time | Batch Time | Time Improvement | DB Ops (Individual) | DB Ops (Batch) | Ops Improvement |
|------|----------------|------------|------------------|---------------------|-----------------|-----------------|
| 10   | 793.473µs      | 252.689µs  | **3.14x faster** | 86                  | 40              | **2.15x fewer** |
| 50   | 5.003888ms     | 995.726µs  | **5.03x faster** | 679                 | 200             | **3.40x fewer** |
| 100  | 10.509446ms    | 1.962697ms | **5.35x faster** | 1545                | 400             | **3.86x fewer** |

### Key Metrics

- **Time Performance**: 3-5x faster processing times
- **Database Operations**: 2-4x fewer database operations
- **Scalability**: Performance improvement increases with more keys
- **Memory Usage**: Minimal memory overhead with automatic cache clearing

## Implementation Details

### Cache Management

```rust
/// Clear caches after block processing (no persistent state between blocks)
pub fn clear_caches(&mut self) {
    self.node_cache.clear();
    self.key_hash_cache.clear();
}
```

### Batch Processing Flow

1. **Initialize**: Clear caches at start of block processing
2. **Pre-compute**: Generate key hashes for all updated keys
3. **Batch Operations**: Group all database writes into a single batch
4. **Process**: Update SMT using cached nodes and batched writes
5. **Commit**: Write entire batch atomically
6. **Cleanup**: Clear all caches after block completion

### Binary Search BST Optimization

```rust
/// Fast BST lookup using binary search on heights
fn bst_get_at_height_fast(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>> {
    let mut low = 0;
    let mut high = height;
    let mut result = None;

    while low <= high {
        let mid = (low + high) / 2;
        // Binary search logic...
    }
    
    Ok(result)
}
```

## Usage

### BatchedSMTHelper

```rust
use metashrew_runtime::{BatchedSMTHelper, SMTHelper};

// Create batched helper
let mut batched_helper = BatchedSMTHelper::new(storage);

// Process block with multiple keys
let updated_keys = vec![key1, key2, key3];
let root = batched_helper.calculate_and_store_state_root_batched(height, &updated_keys)?;

// Caches are automatically cleared after processing
assert!(batched_helper.caches_are_empty());
```

### Regular SMTHelper with Batch Operations

```rust
// Use batch operations with regular helper
let mut smt_helper = SMTHelper::new(storage);
let updated_keys = vec![key1, key2, key3];
let root = smt_helper.calculate_and_store_state_root_batched(height, &updated_keys)?;
```

## Constraints Satisfied

✅ **No persistent caching**: All caches are cleared after each block  
✅ **Stateless operation**: No state persists between different blocks  
✅ **Backward compatibility**: Original methods still available  
✅ **Atomic operations**: All database writes are batched and atomic  
✅ **Memory efficiency**: Minimal memory overhead with automatic cleanup  

## Testing

Comprehensive test suite includes:

- **Performance comparison tests**: Batch vs individual operations
- **Correctness verification**: Ensures batch operations produce identical results
- **Cache management tests**: Verifies caches are properly cleared
- **Edge case handling**: Empty batches, single keys, large datasets

Run tests with:
```bash
cargo test batch_smt_performance_test --lib -- --nocapture
cargo test incremental_smt_test --lib -- --nocapture
```

## Conclusion

The implemented optimizations provide significant performance improvements (3-5x faster) while maintaining the constraint of no persistent state between blocks. The solution uses in-memory caching and batch database operations to minimize I/O overhead while ensuring all caches are cleared after each block processing cycle.
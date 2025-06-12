# Database Structure

Metashrew uses a specialized database structure designed for blockchain data indexing. This document explains the key aspects of this database design, including its append-only nature, height indexing, state root integrity, and chain reorganization handling.

## Append-Only Design

At the core of Metashrew's database is an append-only design pattern. This means that data is never overwritten or deleted; instead, new versions are appended when values change.

### Key Characteristics

1. **Immutable History**: All historical values are preserved, enabling point-in-time queries
2. **Height Annotation**: Values are annotated with the block height at which they were created
3. **Versioned Keys**: Each key maintains a list of values, indexed by position
4. **Atomic Updates**: Changes are applied in atomic batches, ensuring consistency

### Key Format

In the basic append-only structure, keys follow this format:

```
<namespace>:<key>:<index>
```

Where:
- `namespace`: Optional label for organizing keys (e.g., "tokens", "balances")
- `key`: The actual key identifier
- `index`: Sequential index for versioning

Values are stored with metadata:

```
{
  height: <block_height>,
  value: <actual_value>
}
```

### Benefits of Append-Only Design

1. **Historical Queries**: Enables querying the state at any historical block height
2. **Audit Trail**: Provides a complete history of state changes
3. **Chain Reorganization Handling**: Simplifies rolling back state changes during reorgs
4. **Consistency**: Ensures consistent state across parallel indexers

## Height-Indexed Binary Search Tree (BST)

While the append-only design preserves historical data, it can be inefficient for historical queries that require scanning through all versions of a key. To address this, Metashrew implements a Height-Indexed Binary Search Tree structure.

### Key Structure

The Height-Indexed BST uses three types of keys:

1. **Height-indexed values**:
   ```
   height_tree:<key_hex>:<height>
   ```

2. **Height lists** for each key:
   ```
   <key_hex>:heights
   ```

3. **Modified keys tracking**:
   ```
   modified_keys:<height>
   ```

### Binary Search Algorithm

The Height-Indexed BST enables efficient historical queries using binary search:

```rust
// Binary search for the closest height less than or equal to target_height
let closest_height = match heights.binary_search_by(|&h| {
    if h <= target_height {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}) {
    Ok(idx) => heights[idx],
    Err(idx) => {
        if idx == 0 {
            return Ok(None); // No value before target_height
        }
        heights[idx - 1] // Get the closest height less than target_height
    }
};
```

This reduces query complexity from O(n) to O(log n), where n is the number of versions of a key.

### Implementation Details

1. **Height Lists**: For each key, a sorted list of heights at which the key was modified is maintained
2. **Direct Access**: Values can be accessed directly using the height-indexed key
3. **Modified Keys Tracking**: For each height, a list of modified keys is maintained for efficient reorg handling

## State Root Integrity

Metashrew uses a Sparse Merkle Tree (SMT) to maintain state root integrity. The state root is a cryptographic commitment to the entire state at a particular height.

### Sparse Merkle Tree

A Sparse Merkle Tree is a binary tree where:

1. Each leaf node represents a key-value pair
2. Internal nodes are hashes of their children
3. The root node is a single hash representing the entire state

```
                  Root Hash
                 /        \
                /          \
               /            \
          Hash_0_127      Hash_128_255
           /    \            /    \
          /      \          /      \
     Hash_0_63  Hash_64_127 ...    ...
        / \       / \
       /   \     /   \
      ...  ...  ...  ...
```

### State Root Calculation

The state root is calculated as follows:

1. For each key-value pair, compute a leaf node hash
2. Update the Sparse Merkle Tree with the leaf node
3. Compute the new root hash

The state root is stored for each height:

```
smt:root:<height> -> <root_hash>
```

### Benefits of State Root Integrity

1. **Verification**: Enables cryptographic verification of the state
2. **Consistency Checking**: Allows checking for consistency between different instances
3. **Proof Generation**: Enables generating Merkle proofs for specific keys
4. **Tamper Detection**: Makes it possible to detect unauthorized changes to the database

## Chain Reorganization Handling

One of the key challenges in blockchain indexing is handling chain reorganizations (reorgs). Metashrew's database structure is specifically designed to handle reorgs efficiently.

### Reorg Detection

Reorgs are detected by comparing block hashes:

```rust
pub fn detect_reorg(&self, new_block_height: u32, new_block_hash: &[u8]) -> Result<Option<u32>> {
    // Check if we have a block at this height
    let height_key = format!("block:{}:hash", new_block_height).into_bytes();
    
    if let Some(stored_hash) = self.db.get(&height_key)? {
        // If the hash doesn't match, we have a reorg
        if stored_hash != new_block_hash {
            // Find the fork point (last common block)
            let mut fork_height = new_block_height - 1;
            
            while fork_height > 0 {
                let prev_height_key = format!("block:{}:hash", fork_height).into_bytes();
                
                if let Some(prev_stored_hash) = self.db.get(&prev_height_key)? {
                    let prev_block_hash = self.get_block_hash_at_height(fork_height)?;
                    
                    if prev_stored_hash == prev_block_hash {
                        // Found the fork point
                        return Ok(Some(fork_height));
                    }
                }
                
                fork_height -= 1;
            }
        }
    }
    
    // No reorg detected
    Ok(None)
}
```

### Reorg Handling Process

When a reorg is detected, Metashrew follows these steps:

1. **Identify Fork Point**: Find the last common block between the old and new chains
2. **Identify Affected Keys**: Determine which keys were modified after the fork point
3. **Roll Back State**: Restore the state of affected keys to their values at the fork point
4. **Reprocess Blocks**: Process blocks from the new chain starting from the fork point

### Efficient Rollback with Height-Indexed BST

The Height-Indexed BST structure makes rollback operations efficient:

1. **Quick Identification**: Modified keys at each height are tracked, making it easy to identify affected keys
2. **Efficient Retrieval**: Binary search enables efficient retrieval of values at the fork height
3. **Atomic Rollback**: All changes are rolled back in a single atomic operation

```rust
pub fn rollback_to_height(&self, fork_height: u32, current_height: u32) -> Result<()> {
    // Get all keys affected by the reorg
    let affected_keys = self.get_keys_affected_by_reorg(fork_height, current_height)?;
    
    let mut batch = WriteBatch::default();
    
    // For each affected key
    for key in &affected_keys {
        // Get the value at the fork height
        let fork_value = match self.get_value_at_height(key, fork_height)? {
            Some(value) => value,
            None => Vec::new(), // Key didn't exist at fork height
        };
        
        // Update the height list to mark this key as not modified after fork_height
        let heights_key = format!("{}:{}", hex::encode(key), HEIGHT_LIST_SUFFIX).into_bytes();
        
        if let Some(heights_data) = self.db.get(&heights_key)? {
            // Process height list...
        }
    }
    
    // Delete height index entries for heights > fork_height
    for height in (fork_height + 1)..=current_height {
        let modified_keys_key = format!("{}:{}", MODIFIED_KEYS_PREFIX, height).into_bytes();
        batch.delete(&modified_keys_key);
        
        // Delete the stateroot for this height
        let root_key = format!("{}:{}", SMT_ROOT_PREFIX, height).into_bytes();
        batch.delete(&root_key);
    }
    
    // Apply all changes atomically
    self.db.write(batch)?;
    
    Ok(())
}
```

## Storage Backends

Metashrew supports multiple storage backends through its `KeyValueStoreLike` trait:

```rust
pub trait KeyValueStoreLike {
    type Error: std::fmt::Debug;
    type Batch: BatchLike;
    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error>;
    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error>;
    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error>;
    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>;
}
```

The primary implementation uses RocksDB, but the architecture supports other backends like DynamoDB.

### RocksDB Configuration

For optimal performance, RocksDB is configured with:

- **Multi-threaded Column Families**: For parallel processing
- **Large Write Buffers**: To handle high write throughput
- **Optimized Compaction**: To manage the append-only growth pattern
- **Bloom Filters**: To speed up key lookups

## Conclusion

Metashrew's database structure combines an append-only design with height indexing and state root integrity to create a robust foundation for blockchain data indexing. The Height-Indexed BST structure enables efficient historical queries, while the append-only nature ensures data integrity and simplifies chain reorganization handling.

This design is particularly well-suited for blockchain applications, where historical data access, state verification, and handling chain reorganizations are critical requirements.
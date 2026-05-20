# Reorg Rollback Bug Fix

## Summary

Fixed a critical bug in blockchain reorganization (reorg) handling where SMT (Sparse Merkle Tree) data from old blocks was not properly cleaned up during rollbacks. This caused indexer instances to potentially diverge when processing reorgs.

## The Bug

### What Was Happening (BEFORE)

When a blockchain reorganization occurred, both `RocksDBStorageAdapter` and `MemStoreAdapter` implemented `rollback_to_height()` with **incomplete cleanup**:

```rust
// OLD BUGGY IMPLEMENTATION (rockshrew-runtime/src/storage_adapter.rs)
async fn rollback_to_height(&mut self, height: u32) -> SyncResult<()> {
    // Only deleted metadata keys:
    for h in (height + 1)..=current_height {
        self.db.delete(&format!("/__INTERNAL/height-to-hash/{}", h))?;  // ✓ Deleted
        self.db.delete(&format!("smt:root:{}", h))?;                    // ✓ Deleted
    }

    // ❌ BUG: Did NOT delete actual SMT data written by WASM indexers:
    //   - /blocks/{height}
    //   - /block-hashes/{height}
    //   - /blocktracker updates
    //   - Any other keys written via IndexPointer
}
```

### Impact

During a reorg from Chain A to Chain B:
1. ✓ Metadata was rolled back (block hashes, state roots)
2. ❌ **Actual indexed data was NOT rolled back**
3. When indexing Chain B's blocks, old Chain A data was still present
4. This could cause:
   - Incorrect query results (mixing Chain A and Chain B data)
   - State divergence between indexer instances
   - Potential panics if WASM code checks for duplicate entries

## The Fix

### Shared SMT Rollback Implementation

Created a new module `metashrew-runtime/src/rollback.rs` with:

1. **`SmtRollback` Trait**: Defines operations needed for proper rollback
   ```rust
   pub trait SmtRollback {
       fn get_all_keys(&self) -> Result<Vec<Vec<u8>>>;
       fn delete_key(&mut self, key: &[u8]) -> Result<()>;
       fn put_key(&mut self, key: &[u8], value: &[u8]) -> Result<()>;
       fn get_value(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
   }
   ```

2. **`rollback_smt_data()` Function**: Proper rollback algorithm
   ```rust
   pub fn rollback_smt_data<S: SmtRollback>(
       storage: &mut S,
       rollback_height: u32,
       current_height: u32,
   ) -> Result<()>
   ```

### Rollback Algorithm

The correct rollback process:

1. **Delete metadata keys** for heights > rollback_height
   - `block_hash_{h}`
   - `state_root_{h}`
   - `smt:root:{h}`

2. **Roll back append-only SMT structures** (keys with `/length` suffix)
   - Parse each update's height from its value (`height:data` format)
   - Keep only updates where `height <= rollback_height`
   - Compact the remaining updates with sequential indices
   - Update or remove the `/length` key accordingly

3. This ensures ALL WASM-indexed data is properly cleaned up!

### Implementation for Both Adapters

**MemStoreAdapter** (`memshrew-runtime/src/adapter.rs`):
```rust
impl SmtRollback for MemStoreAdapter {
    // Implemented all four methods using HashMap operations
}

async fn rollback_to_height(&mut self, height: u32) -> SyncResult<()> {
    rollback_smt_data(self, height, current_height)?;
    self.set_height(height);
    Ok(())
}
```

**RocksDBStorageAdapter** (`rockshrew-runtime/src/storage_adapter.rs`):
```rust
impl SmtRollback for RocksDBStorageAdapter {
    // Implemented all four methods using RocksDB operations
}

async fn rollback_to_height(&mut self, height: u32) -> SyncResult<()> {
    rollback_smt_data(self, height, current_height)?;
    self.set_indexed_height(height).await?;
    Ok(())
}
```

## Testing

### Test Files

1. **`src/tests/comprehensive_e2e_test.rs`**
   - Modified to verify rollback behavior
   - Tests that stored block hashes match Chain B after reorg (not Chain A)

2. **`src/tests/reorg_rollback_bug_test.rs`** (NEW)
   - `test_reorg_with_proper_smt_rollback()`: Comprehensive reorg test
   - `test_reorg_demonstrates_proper_cleanup()`: Verifies data replacement

3. **`crates/metashrew-minimal/src/lib.rs`**
   - Updated WASM indexer to store `/block-hashes/{height}` for verification
   - Helps tests validate which chain's data is actually stored

### Test Results

```
running 2 tests
test tests::reorg_rollback_bug_test::test_reorg_demonstrates_proper_cleanup ... ok
test tests::reorg_rollback_bug_test::test_reorg_with_proper_smt_rollback ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured
```

All tests demonstrate that:
- ✅ Old Chain A data is properly rolled back
- ✅ New Chain B data is successfully indexed
- ✅ Stored data matches Chain B, not Chain A
- ✅ No mixing of data from different chains

## Files Changed

### Core Implementation
- `crates/metashrew-runtime/src/rollback.rs` (NEW) - Shared rollback logic
- `crates/metashrew-runtime/src/lib.rs` - Added rollback module
- `crates/memshrew-runtime/src/adapter.rs` - Implemented SmtRollback, updated rollback
- `crates/rockshrew-runtime/src/storage_adapter.rs` - Implemented SmtRollback, updated rollback

### Tests
- `src/tests/reorg_rollback_bug_test.rs` (NEW) - Comprehensive rollback tests
- `src/tests/mod.rs` - Added new test module
- `src/tests/comprehensive_e2e_test.rs` - Updated to verify rollback
- `crates/metashrew-minimal/src/lib.rs` - Added block hash storage for tests

## Benefits

1. **Correctness**: Reorgs now properly clean up old data
2. **Consistency**: All indexer instances handle reorgs identically
3. **Testability**: MemStore tests prove RocksDB behavior
4. **Maintainability**: Shared implementation reduces duplication
5. **Debuggability**: Better logging during rollback operations

## Migration Notes

- **No breaking changes** to public APIs
- Existing indexer databases will work correctly going forward
- The fix is transparent to WASM indexers
- No configuration changes needed

## Future Improvements

Potential enhancements (not required immediately):
1. Add metrics for rollback operations
2. Optimize rollback for large datasets (batching)
3. Add rollback validation checks
4. Create integration tests with actual bitcoind reorgs

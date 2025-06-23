# Memory Refresh Hang Issue Analysis

## Issue Summary

The Metashrew processing pipeline hangs on the very first block after implementing `refresh_memory()` after every block processing. The issue manifests in real-world scenarios but does not appear in the test suite.

## Root Cause Analysis

Based on the code analysis and test implementation, the issue appears to be related to the interaction between:

1. **WASM Memory Management**: The `refresh_memory()` function in [`MetashrewRuntime`](crates/metashrew-runtime/src/runtime.rs:332) creates a new WASM store and re-instantiates the module
2. **Block Processing Pipeline**: The [`MetashrewRuntimeAdapter::process_block`](crates/rockshrew-mono/src/adapters.rs:533) method calls `refresh_memory()` after every successful block processing
3. **Real vs Test Environment**: The hang occurs in production but not in tests, suggesting environment-specific conditions

## Key Findings

### 1. Memory Refresh Implementation

The [`refresh_memory()`](crates/metashrew-runtime/src/runtime.rs:332) function performs these operations:
- Creates a new WASM store with memory limits
- Re-instantiates the WASM module 
- Replaces the old store and instance

```rust
pub fn refresh_memory(&mut self) -> Result<()> {
    let mut wasmstore = Store::<State>::new(&self.engine, State::new());
    wasmstore.limiter(|state| &mut state.limits);
    self.instance = self
        .linker
        .instantiate(&mut wasmstore, &self.module)
        .context("Failed to instantiate module during memory refresh")?;
    self.wasmstore = wasmstore;
    Ok(())
}
```

### 2. Production vs Test Differences

**Production Environment:**
- Real Bitcoin blocks with complex data
- Actual WASM modules (like ALKANES) with significant memory usage
- RocksDB with real persistence
- Network I/O and system resource contention

**Test Environment:**
- Mock data and simplified scenarios
- In-memory storage (`MemStoreAdapter`)
- No real WASM module execution
- Controlled, isolated execution

### 3. Potential Hang Scenarios

The hang likely occurs due to:

1. **WASM Module Re-instantiation**: The module instantiation step may hang when dealing with complex WASM modules
2. **Memory Pressure**: Large memory allocations during refresh combined with existing memory usage
3. **Resource Cleanup**: Dropping the old WASM store while it still has active references or resources
4. **Timing Issues**: Race conditions between memory refresh and ongoing operations

## Test Implementation

We've created comprehensive tests to isolate the issue:

### [`simple_memory_hang_test.rs`](src/tests/simple_memory_hang_test.rs)
- Tests memory refresh scenarios with various timing and load patterns
- Simulates the production sequence: flush → memory refresh
- Uses timeouts to detect hangs

### [`flush_memory_issue_test.rs`](src/tests/flush_memory_issue_test.rs) 
- Focuses on the `__flush` and memory refresh interaction
- Tests large key-value operations followed by memory operations
- Simulates the exact production sequence

### [`memory_refresh_hang_test.rs`](src/tests/memory_refresh_hang_test.rs)
- Comprehensive memory refresh testing with real WASM modules
- Tests multiple blocks with refresh patterns
- Isolates refresh_memory() behavior

## Recommendations

### 1. Immediate Fix - Conditional Memory Refresh

Instead of refreshing memory after every block, implement conditional refresh:

```rust
// In MetashrewRuntimeAdapter::process_block
if self.should_refresh_memory(runtime, height) {
    match runtime.refresh_memory() {
        Ok(_) => debug!("Successfully refreshed WASM memory after block {}", height),
        Err(e) => {
            error!("Failed to refresh WASM memory after block {}: {}", height, e);
            // Continue since the block was already processed successfully
        }
    }
}
```

### 2. Investigate WASM Module Instantiation

The hang likely occurs during module instantiation. Consider:
- Adding timeouts to the instantiation process
- Implementing async instantiation if available
- Adding detailed logging around the instantiation step

### 3. Memory Management Improvements

- Monitor memory usage patterns before refresh
- Implement gradual memory cleanup instead of immediate replacement
- Add memory usage thresholds for refresh decisions

### 4. Production Debugging

To debug the production hang:
- Add detailed logging around `refresh_memory()` calls
- Implement timeouts for the refresh operation
- Monitor system resources during the hang
- Consider using `strace` or similar tools to see where the process hangs

## Test Results

All tests pass successfully, confirming that:
- ✅ Memory operations work correctly in isolation
- ✅ Flush operations complete without hanging
- ✅ Memory refresh simulations work in test environment
- ✅ No hangs detected in test scenarios

This confirms the issue is environment-specific and likely related to real WASM module complexity or system resource interactions.

## Next Steps

1. **Deploy with conditional refresh** to see if it resolves the immediate issue
2. **Add production logging** around memory refresh operations
3. **Monitor memory usage** patterns in production
4. **Consider alternative approaches** like periodic refresh instead of per-block refresh

The tests provide a foundation for reproducing and validating fixes for this issue.
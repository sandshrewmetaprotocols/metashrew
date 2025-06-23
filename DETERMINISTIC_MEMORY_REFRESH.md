# Deterministic Memory Refresh Implementation

## Overview

This document describes the implementation of deterministic memory refresh in Metashrew, ensuring that WASM memory is ALWAYS refreshed after every block execution, eliminating conditional logic and centralizing memory management.

## Problem Statement

Previously, Metashrew had conditional memory refresh logic scattered across multiple adapter layers:

1. **`rockshrew-mono/src/adapters.rs`** - Conditional refresh based on memory threshold (1.75GB)
2. **`rockshrew-sync/src/adapters.rs`** - Conditional refresh on execution failure
3. **`rockshrew-diff/src/main.rs`** - Manual refresh on execution failure

This approach had several issues:
- **Non-deterministic behavior**: Memory refresh depended on conditions
- **Code duplication**: Logic scattered across multiple files
- **Potential state leakage**: WASM state could persist between blocks
- **Inconsistent execution**: Different behavior in different scenarios

## Solution

### Core Principle
**ALWAYS refresh memory after block execution, without exception.**

### Implementation Strategy

#### 1. Centralized Memory Management in `metashrew-runtime`

**File**: [`crates/metashrew-runtime/src/runtime.rs`](crates/metashrew-runtime/src/runtime.rs)

**Changes Made**:

##### A. Modified `run()` method (lines 343-378)
```rust
pub fn run(&mut self) -> Result<(), anyhow::Error> {
    // ... existing execution logic ...
    
    let execution_result = match start.call(&mut self.wasmstore, ()) {
        // ... execution logic ...
    };

    // ALWAYS refresh memory after block execution for deterministic behavior
    // This ensures no WASM state persists between blocks
    if let Err(refresh_err) = self.refresh_memory() {
        log::error!("Failed to refresh memory after block execution: {}", refresh_err);
        // Return the refresh error as it's critical for deterministic execution
        return Err(refresh_err).context("Memory refresh failed after block execution");
    }

    log::debug!("Memory refreshed after block execution for deterministic state isolation");
    execution_result
}
```

##### B. Modified `process_block_atomic()` method (lines 1370-1449)
```rust
pub async fn process_block_atomic(&mut self, ...) -> Result<AtomicBlockResult> {
    // ... execution logic ...
    
    // Calculate state root and batch data before memory refresh
    let (state_root, batch_data) = match execution_result {
        Ok(_) => {
            let state_root = self.calculate_state_root()?;
            let batch_data = self.get_accumulated_batch()?;
            (state_root, batch_data)
        }
        Err(e) => {
            // ALWAYS refresh memory even on execution failure
            if let Err(refresh_err) = self.refresh_memory() {
                log::error!("Failed to refresh memory after failed atomic block execution: {}", refresh_err);
            }
            return Err(e);
        }
    };

    // ALWAYS refresh memory after block execution for deterministic behavior
    if let Err(refresh_err) = self.refresh_memory() {
        log::error!("Failed to refresh memory after atomic block execution: {}", refresh_err);
        return Err(refresh_err).context("Memory refresh failed after atomic block execution");
    }

    log::debug!("Memory refreshed after atomic block execution for deterministic state isolation");
    
    Ok(AtomicBlockResult { state_root, batch_data, height, block_hash: block_hash.to_vec() })
}
```

#### 2. Removed Conditional Logic from Adapters

##### A. `rockshrew-mono/src/adapters.rs`

**Removed**:
- `should_refresh_memory()` function (lines 434-470)
- Conditional memory refresh logic in `process_block()` (lines 500-588)
- Manual refresh calls in error handling

**Simplified `process_block()`**:
```rust
async fn process_block(&mut self, height: u32, block_data: &[u8]) -> SyncResult<()> {
    // ... context setup ...
    
    // Execute the runtime - memory refresh is now handled automatically by metashrew-runtime
    match runtime.run() {
        Ok(_) => {
            info!("RUNTIME_RUN: Successfully executed WASM for block {} (size: {} bytes)", height, block_size);
            Ok(())
        }
        Err(run_err) => {
            error!("Failed to process block {}: {}", height, run_err);
            Err(SyncError::Runtime(format!("Runtime execution failed: {}", run_err)))
        }
    }
}
```

**Updated `refresh_memory()`**:
```rust
async fn refresh_memory(&mut self) -> SyncResult<()> {
    // Memory refresh is now handled automatically by metashrew-runtime after each block execution
    // This method is kept for API compatibility but no longer performs manual refresh
    info!("Manual memory refresh requested - note that memory is now refreshed automatically after each block");
    Ok(())
}
```

##### B. `rockshrew-sync/src/adapters.rs`

**Removed**:
- Conditional refresh logic in `process_block()` (lines 58-69)
- Conditional refresh logic in `process_block_atomic()` (lines 95-105)

**Simplified `process_block()`**:
```rust
async fn process_block(&mut self, height: u32, block_data: &[u8]) -> SyncResult<()> {
    // ... context setup ...
    
    // Execute the runtime - memory refresh is now handled automatically by metashrew-runtime
    runtime.run().map_err(|e| {
        SyncError::Runtime(format!("Runtime execution failed: {}", e))
    })?;

    Ok(())
}
```

**Simplified `process_block_atomic()`**:
```rust
async fn process_block_atomic(&mut self, ...) -> SyncResult<AtomicBlockResult> {
    let mut runtime = self.runtime.lock().await;

    // Use the built-in atomic processing method from metashrew-runtime
    // This handles memory refresh automatically
    match runtime.process_block_atomic(height, block_data, block_hash).await {
        Ok(result) => {
            // Convert result types
            Ok(AtomicBlockResult { ... })
        }
        Err(e) => Err(SyncError::Runtime(format!("Atomic block processing failed: {}", e)))
    }
}
```

##### C. `rockshrew-diff/src/main.rs`

**Removed**:
- Manual refresh calls for primary runtime (lines 346-362)
- Manual refresh calls for compare runtime (lines 381-397)

**Simplified execution**:
```rust
// Run primary runtime - memory refresh is now handled automatically
self.primary_runtime.run().map_err(|e| {
    error!("Error running primary WASM module for block {}: {}", height, e);
    anyhow!("Error running primary WASM module: {}", e)
})?;

// Run compare runtime - memory refresh is now handled automatically  
self.compare_runtime.run().map_err(|e| {
    error!("Error running compare WASM module for block {}: {}", height, e);
    anyhow!("Error running compare WASM module: {}", e)
})?;
```

## Benefits

### 1. **Deterministic Execution**
- **Guaranteed Fresh State**: Every block starts with completely clean WASM memory
- **No State Leakage**: Previous block execution cannot affect current block
- **Consistent Results**: Same block data produces same results regardless of execution history

### 2. **Simplified Architecture**
- **Single Responsibility**: Memory management centralized in `metashrew-runtime`
- **Reduced Complexity**: Eliminated conditional logic across multiple files
- **Easier Maintenance**: One place to modify memory management behavior

### 3. **Improved Reliability**
- **No Memory Threshold Dependencies**: No risk of missing refresh due to threshold logic
- **Consistent Error Handling**: Memory refresh failures are always caught and reported
- **Predictable Behavior**: Same memory management regardless of execution path

### 4. **Better Debugging**
- **Clear Logging**: Every memory refresh is logged for debugging
- **Consistent State**: Easier to reason about WASM state between blocks
- **Reproducible Issues**: Problems are more likely to be reproducible

## Memory Refresh Lifecycle

```
Block Processing Flow:
1. Set block context (height, data)
2. Execute WASM (_start function)
3. Handle execution result
4. ALWAYS call refresh_memory() ← NEW: Deterministic step
5. Log memory refresh completion
6. Return execution result
```

## Logging Changes

### New Log Messages

**Debug Level**:
```
"Memory refreshed after block execution for deterministic state isolation"
"Memory refreshed after atomic block execution for deterministic state isolation"
```

**Error Level**:
```
"Failed to refresh memory after block execution: {error}"
"Failed to refresh memory after atomic block execution: {error}"
"Failed to refresh memory after failed atomic block execution: {error}"
```

**Info Level** (for manual refresh calls):
```
"Manual memory refresh requested - note that memory is now refreshed automatically after each block"
```

## API Compatibility

### Maintained Interfaces
- `refresh_memory()` methods in adapters are kept for API compatibility
- They now log a message and return success without performing actual refresh
- This ensures existing code doesn't break while indicating the new behavior

### Breaking Changes
- **None**: All existing APIs maintained
- **Behavioral Change**: Memory is now refreshed deterministically instead of conditionally

## Performance Implications

### Positive Impacts
- **Eliminated Conditional Checks**: No more memory size monitoring overhead
- **Simplified Code Paths**: Fewer branches in execution logic
- **Predictable Performance**: Consistent memory refresh timing

### Considerations
- **Slightly Increased Memory Operations**: Refresh happens after every block instead of conditionally
- **Deterministic Overhead**: Small, consistent cost for guaranteed deterministic behavior

## Testing Implications

### Improved Testability
- **Deterministic Test Results**: Tests will be more reliable and reproducible
- **Consistent State**: Each test block starts with clean WASM state
- **Easier Debugging**: Memory-related issues easier to isolate and fix

### Test Updates Required
- Tests expecting conditional memory refresh behavior may need updates
- Performance tests should account for deterministic refresh overhead

## Migration Guide

### For Developers
1. **Remove Manual Refresh Calls**: Any manual `refresh_memory()` calls can be removed
2. **Update Error Handling**: Memory refresh errors are now handled automatically
3. **Review Logging**: New debug logs available for memory refresh tracking

### For Operations
1. **Monitor Logs**: Watch for memory refresh error messages
2. **Performance Monitoring**: Slight increase in consistent memory operations
3. **Debugging**: Use new debug logs to track memory refresh behavior

## Conclusion

This implementation ensures **100% deterministic WASM memory management** in Metashrew by:

1. **Centralizing** memory refresh logic in `metashrew-runtime`
2. **Eliminating** all conditional refresh logic
3. **Guaranteeing** memory refresh after every block execution
4. **Maintaining** API compatibility while improving reliability

The result is a more predictable, maintainable, and reliable system that ensures complete isolation between block processing cycles.
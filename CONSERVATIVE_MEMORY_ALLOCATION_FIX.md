# Conservative Memory Allocation Fix for WASM Environments

## Problem Summary

The original memory allocation issue was caused by attempting to preallocate too much memory (1GB) in WASM environments during the lazy static initialization of the LRU cache. This resulted in `alloc::raw_vec::capacity_overflow` panics during the `ensure_preallocated_memory()` function call.

## Root Cause Analysis

The error stack trace showed:
```
7: 0x207185 - alkanes.wasm!alloc::raw_vec::capacity_overflow::h8ced4ea51260dc20
13: 0x1d63f9 - alkanes.wasm!metashrew_support::lru_cache::ensure_preallocated_memory::h825393b114330aae
14: 0x1a4437 - alkanes.wasm!metashrew_core::initialize::h2cc4aea40aad200e
```

The issue occurred during:
1. **WASM module initialization** - `metashrew_core::initialize()`
2. **Memory preallocation** - `ensure_preallocated_memory()`
3. **Lazy static initialization** - First access to `PREALLOCATED_CACHE_MEMORY`
4. **Capacity overflow** - Attempting to allocate 1GB in a constrained WASM environment

## Conservative Solution Implemented

### 1. Progressive Memory Detection

**Before:**
```rust
let test_sizes = [
    LRU_CACHE_MEMORY_LIMIT,           // 1GB - too aggressive for WASM
    LRU_CACHE_MEMORY_LIMIT / 2,       // 512MB
    MIN_LRU_CACHE_MEMORY_LIMIT,       // 256MB
    // ...
];
```

**After:**
```rust
let test_sizes = [
    256 * 1024 * 1024,  // 256MB (start conservative)
    128 * 1024 * 1024,  // 128MB
    64 * 1024 * 1024,   // 64MB
    32 * 1024 * 1024,   // 32MB
    16 * 1024 * 1024,   // 16MB
    8 * 1024 * 1024,    // 8MB
    4 * 1024 * 1024,    // 4MB (absolute minimum)
];
```

### 2. Safe Allocation with `try_reserve_exact()`

**Before:**
```rust
let mut test_vec = Vec::with_capacity(size); // Could panic with capacity overflow
test_vec.resize(test_allocation_size, 0);
```

**After:**
```rust
let mut test_vec = Vec::new();
match test_vec.try_reserve_exact(size) {  // Safe allocation attempt
    Ok(()) => {
        test_vec.resize(test_allocation_size, 0);
        true
    }
    Err(_) => false
}
```

### 3. Progressive Preallocation with Fallbacks

**Before:**
```rust
// Single attempt at full allocation
memory.resize(actual_limit, 0); // Could panic
```

**After:**
```rust
let allocation_attempts = [
    actual_limit,           // Try the detected limit first
    actual_limit / 2,       // Try half
    actual_limit / 4,       // Try quarter
    16 * 1024 * 1024,      // 16MB fallback
    8 * 1024 * 1024,       // 8MB fallback
    4 * 1024 * 1024,       // 4MB fallback
    1024 * 1024,           // 1MB minimal
];

for &size in &allocation_attempts {
    match memory.try_reserve_exact(size) {
        Ok(()) => {
            memory.resize(size, 0);
            return memory; // Success
        }
        Err(_) => continue, // Try smaller size
    }
}

// If all fail, return empty vector to avoid panics
Vec::new()
```

### 4. Safe Cache Initialization

**Before:**
```rust
*cache = Some(LruCache::new(actual_memory_limit)); // Could fail if limit too high
```

**After:**
```rust
let safe_memory_limit = if preallocated_size > 0 {
    actual_memory_limit.min(preallocated_size) // Use smaller of detected vs preallocated
} else {
    4 * 1024 * 1024 // 4MB fallback if preallocation failed
};
*cache = Some(LruCache::new(safe_memory_limit));
```

## Key Improvements

### 1. **WASM-Friendly Memory Limits**
- Start with 256MB maximum instead of 1GB
- Progressive fallback to 4MB absolute minimum
- Realistic limits for WASM execution environments

### 2. **Graceful Failure Handling**
- Use `try_reserve_exact()` to avoid capacity overflow panics
- Multiple fallback allocation attempts
- Empty vector fallback instead of panics

### 3. **Conservative Preallocation**
- Test multiple allocation sizes progressively
- Handle preallocation failures gracefully
- Maintain consistent memory layout when possible

### 4. **Safe Cache Initialization**
- Use actually preallocated memory size as limit
- Fallback to minimal cache if preallocation failed
- Never attempt to allocate more than what was successfully preallocated

## Testing

The fix includes comprehensive tests in `conservative_memory_test.rs`:

```rust
#[test]
fn test_conservative_memory_detection() {
    let detected_memory = detect_available_memory();
    assert!(detected_memory >= 4 * 1024 * 1024); // At least 4MB
    assert!(detected_memory <= 256 * 1024 * 1024); // At most 256MB
}

#[test]
fn test_safe_preallocation() {
    ensure_preallocated_memory(); // Should not panic
}

#[test]
fn test_safe_lru_initialization() {
    initialize_lru_cache(); // Should not panic
}
```

All tests pass, confirming the fix works correctly.

## Performance Impact

### Memory Usage
- **Before**: Attempted 1GB allocation (failed in WASM)
- **After**: 4MB-256MB based on environment capabilities
- **Result**: Successful allocation with reasonable memory usage

### Cache Performance
- **Before**: No cache due to initialization failure
- **After**: Working cache with appropriate size for environment
- **Result**: Improved performance despite smaller cache size

## Deployment Considerations

### 1. **WASM Memory Constraints**
- The fix is specifically designed for WASM environments
- Handles memory-constrained execution contexts
- Maintains functionality even with minimal memory

### 2. **Backward Compatibility**
- All existing APIs remain unchanged
- Graceful degradation in constrained environments
- No breaking changes to user code

### 3. **Monitoring**
- Added logging for memory allocation decisions
- Clear warnings when operating below recommended minimums
- Debug information for troubleshooting

## Conclusion

The conservative memory allocation fix resolves the capacity overflow panics by:

1. **Starting with realistic memory limits** for WASM environments
2. **Using safe allocation methods** that don't panic on failure
3. **Implementing progressive fallbacks** to find working memory sizes
4. **Gracefully handling allocation failures** instead of crashing

This ensures the indexer can start successfully in WASM environments while maintaining optimal performance within available memory constraints.
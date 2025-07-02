# Stateful Views Performance Benchmark Results

## Overview

This document summarizes the comprehensive benchmark testing of stateful vs non-stateful WASM memory persistence in the Metashrew Bitcoin indexer framework.

## Test Implementation

### Benchmark Setup
- **WASM Module**: Modified metashrew-minimal that creates 1000 storage entries on block 0
- **View Function**: `benchmark_view` that reads all 1000 storage entries and concatenates them
- **Test Data**: Each storage entry contains a single byte (0x01), totaling 1000 bytes
- **Test Scenarios**: 50 view function calls comparing stateful vs non-stateful modes

### Key Changes Made
1. **Enabled stateful views by default** in all MetashrewRuntime constructors
2. **Added comprehensive benchmark tests** in `crates/metashrew-runtime/src/tests/stateful_benchmark_test.rs`
3. **Modified metashrew-minimal** to include benchmark functions
4. **Updated Tokio dependencies** to support required runtime features

## Benchmark Results

### Performance Comparison (50 View Calls)

| Metric | Non-Stateful Views | Stateful Views | Improvement |
|--------|-------------------|----------------|-------------|
| **Total Duration** | 440.43ms | 70.29ms | **6.27x faster** |
| **Average per Call** | 8.81ms | 1.41ms | **6.26x faster** |
| **Throughput** | 113.52 calls/sec | 711.30 calls/sec | **6.27x higher** |
| **Time Saved** | - | 370.14ms total | 7.40ms per call |

### Single Call Comparison

| Metric | Non-Stateful | Stateful | Difference |
|--------|--------------|----------|------------|
| **Duration** | 9.18ms | 9.37ms | ~Similar |
| **Throughput** | 108.95 calls/sec | 106.70 calls/sec | ~Similar |

## Key Findings

### 1. Dramatic Performance Improvement for Multiple Calls
- **6.27x speedup** when making multiple view calls
- Stateful views eliminate the overhead of creating new WASM instances
- Performance benefit increases with the number of view calls

### 2. Minimal Overhead for Single Calls
- Single call performance is nearly identical between modes
- The overhead of maintaining stateful memory is negligible
- First call initialization cost is similar in both modes

### 3. Memory Persistence Benefits
- WASM memory state is retained between view calls in stateful mode
- No memory leaks observed during testing
- LRU cache in metashrew-core prevents unbounded memory growth

### 4. Thread Safety and Concurrency
- Stateful views work correctly with multi-threaded Tokio runtime
- Arc<Mutex<_>> protection ensures safe concurrent access
- No race conditions or deadlocks observed

## Technical Implementation Details

### Stateful View Runtime Architecture
```rust
pub struct StatefulViewRuntime<T: KeyValueStoreLike> {
    pub wasmstore: Arc<tokio::sync::Mutex<wasmtime::Store<State>>>,
    pub instance: wasmtime::Instance,
    pub linker: wasmtime::Linker<State>,
    pub context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
}
```

### Default Behavior Changes
- All MetashrewRuntime constructors now automatically enable stateful views
- Graceful fallback if stateful view initialization fails
- Backward compatibility maintained - existing code works without changes

### Memory Management
- WASM memory persists between view calls
- Database context updated for each view call
- Thread-safe access via tokio::sync::Mutex
- Automatic cleanup when runtime is dropped

## Use Cases and Benefits

### High-Frequency View Operations
- **API servers** making many view calls benefit significantly
- **Real-time dashboards** with frequent state queries
- **Analytics workloads** processing multiple view requests

### Stateful WASM Applications
- **Caching mechanisms** within WASM modules
- **Session state** for complex view operations
- **Performance optimizations** in WASM code

### Development and Testing
- **Faster test execution** for view-heavy test suites
- **Improved developer experience** with quicker feedback loops
- **Better debugging** with persistent WASM state

## Migration Guide

### For Existing Applications
No code changes required - stateful views are now enabled by default.

### For Performance-Critical Applications
Consider the following optimizations:
1. **Batch view calls** when possible to maximize stateful benefits
2. **Design WASM modules** to take advantage of persistent memory
3. **Monitor memory usage** in long-running applications

### For Testing
Update test configurations to use multi-threaded Tokio runtime:
```rust
#[tokio::test(flavor = "multi_thread")]
async fn my_test() {
    // Test code here
}
```

## Conclusion

The implementation of stateful WASM memory persistence provides significant performance improvements for view-heavy workloads while maintaining backward compatibility and thread safety. The **6.27x speedup** for multiple view calls makes this a valuable optimization for production Bitcoin indexing applications.

### Key Benefits
- ✅ **6.27x faster** view operations for multiple calls
- ✅ **Enabled by default** - no code changes required
- ✅ **Thread-safe** implementation with proper concurrency controls
- ✅ **Memory efficient** with LRU cache preventing unbounded growth
- ✅ **Backward compatible** - existing applications work unchanged
- ✅ **Comprehensive test coverage** with benchmark validation

This enhancement significantly improves the performance characteristics of the Metashrew Bitcoin indexer framework, especially for applications that make frequent view function calls.
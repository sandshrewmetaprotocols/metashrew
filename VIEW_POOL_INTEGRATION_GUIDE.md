# View Pool Integration Guide for Rockshrew-Mono

## Overview

The parallelized view runtime pool has been successfully integrated into the `rockshrew-mono` process, enabling concurrent execution of view functions for improved performance and scalability.

## Architecture

The view pool integration consists of several key components:

### 1. MetashrewRuntimeAdapter Enhancement
- **Location**: `crates/rockshrew-mono/src/adapters.rs`
- **New Fields**:
  - `view_pool: Arc<RwLock<Option<ViewRuntimePool<RocksDBKeyValueAdapter>>>>`
- **New Methods**:
  - `initialize_view_pool(config: ViewPoolConfig)` - Initialize the view pool
  - `get_view_pool_stats()` - Get pool statistics for monitoring

### 2. Command Line Arguments
- **Location**: `crates/rockshrew-mono/src/lib.rs`
- **New Arguments**:
  - `--enable-view-pool` - Enable view pool for parallel view execution
  - `--view-pool-size <SIZE>` - Number of view runtimes in the pool (default: CPU cores)
  - `--view-pool-max-concurrent <COUNT>` - Maximum concurrent view requests (default: pool_size * 2)
  - `--view-pool-logging` - Enable view pool logging for debugging

### 3. Execution Modes
- **View Pool Enabled**: Uses stateful view runtimes in a parallelized pool for concurrent execution
- **View Pool Disabled**: Uses non-stateful async wasmtime with fresh WASM instances per view call
- This ensures optimal performance for both scenarios while maintaining backward compatibility

## Usage

### Basic Usage

Enable the view pool with default settings:
```bash
rockshrew-mono --enable-view-pool \
  --daemon-rpc-url http://localhost:8332 \
  --indexer ./indexer.wasm \
  --db-path ./data
```

### Advanced Configuration

Configure view pool with custom settings:
```bash
rockshrew-mono --enable-view-pool \
  --view-pool-size 4 \
  --view-pool-max-concurrent 8 \
  --view-pool-logging \
  --daemon-rpc-url http://localhost:8332 \
  --indexer ./indexer.wasm \
  --db-path ./data
```

### Production Recommendations

For production deployments:
```bash
rockshrew-mono --enable-view-pool \
  --view-pool-size 8 \
  --view-pool-max-concurrent 16 \
  --daemon-rpc-url http://localhost:8332 \
  --indexer ./indexer.wasm \
  --db-path ./data \
  --host 0.0.0.0 \
  --port 8080
```

## Configuration Guidelines

### Pool Size
- **Default**: Number of CPU cores
- **Recommendation**: Start with CPU cores, adjust based on workload
- **Considerations**: Each runtime maintains its own WASM memory and cache

### Max Concurrent Requests
- **Default**: pool_size * 2
- **Recommendation**: 2-4x pool size for I/O bound workloads
- **Considerations**: Higher values increase memory usage but improve throughput

### Logging
- **Development**: Enable with `--view-pool-logging`
- **Production**: Disable for performance (default)

## Performance Benefits

### Concurrent Execution
- Multiple view functions can execute simultaneously
- Eliminates blocking on single runtime mutex
- Improved response times under load

### Height-Partitioned Caching
- Each runtime maintains its own cache partitioned by block height
- Reduces cache contention between concurrent requests
- Ensures correctness during blockchain reorganizations

### Load Balancing
- Round-robin distribution of requests across pool members
- Even utilization of available runtimes
- Automatic scaling with pool size

## Monitoring

### View Pool Statistics
The system provides comprehensive statistics for monitoring:

```rust
pub struct ViewPoolStats {
    pub pool_size: usize,
    pub available_permits: usize,
    pub max_concurrent_requests: Option<usize>,
    pub total_requests_processed: u64,
    pub config: ViewPoolConfig,
}
```

### Accessing Statistics
Statistics can be accessed programmatically through the adapter:
```rust
if let Some(stats) = adapter.get_view_pool_stats().await {
    println!("Pool size: {}", stats.pool_size);
    println!("Active requests: {}", stats.active_requests());
    println!("Total processed: {}", stats.total_requests_processed);
}
```

## JSON-RPC Integration

The view pool is automatically used for all JSON-RPC view calls:
- `metashrew_view` - Execute view functions with parallel processing
- `metashrew_preview` - Preview functions (uses direct runtime)
- Transparent to clients - no API changes required

## Testing

### Integration Tests
Comprehensive test suite validates:
- View pool initialization
- Concurrent execution
- Fallback behavior
- Performance characteristics

### Running Tests
```bash
cargo test --package rockshrew-mono view_pool_integration --release
```

## Troubleshooting

### Common Issues

1. **"can call blocking only when running on the multi-threaded runtime"**
   - Ensure tests use `#[tokio::test(flavor = "multi_thread")]`
   - Production runtime is automatically multi-threaded

2. **High memory usage**
   - Reduce `--view-pool-size`
   - Adjust `--view-pool-max-concurrent`
   - Monitor with view pool statistics

3. **Poor performance**
   - Increase `--view-pool-size` for CPU-bound workloads
   - Increase `--view-pool-max-concurrent` for I/O-bound workloads
   - Enable logging to debug bottlenecks

### Debug Logging
Enable detailed logging:
```bash
RUST_LOG=debug rockshrew-mono --enable-view-pool --view-pool-logging ...
```

## Migration Guide

### From Single Runtime
1. Add `--enable-view-pool` to command line
2. Optionally configure pool size and concurrency
3. Monitor performance and adjust as needed

### Backward Compatibility
- Existing deployments continue to work without changes
- View pool is opt-in via command line flag
- No breaking changes to JSON-RPC API

## Implementation Details

### Thread Safety
- All pool operations are thread-safe using Arc<RwLock<_>>
- Semaphore-based concurrency control
- Lock-free round-robin load balancing

### Memory Management
- Each runtime maintains independent WASM memory
- Height-partitioned caches prevent memory leaks
- Automatic cleanup of old cache entries

### Error Handling
- Graceful degradation on pool initialization failure
- Automatic fallback to direct runtime execution
- Comprehensive error reporting and logging

## Future Enhancements

### Planned Features
- Dynamic pool resizing based on load
- Advanced load balancing algorithms
- Metrics integration (Prometheus, etc.)
- Health checks and runtime replacement

### Performance Optimizations
- WASM module sharing between runtimes
- Optimized cache eviction policies
- Connection pooling for database access

## Conclusion

The view pool integration provides significant performance improvements for concurrent view function execution while maintaining full backward compatibility. The modular design allows for easy configuration and monitoring, making it suitable for both development and production deployments.

For questions or issues, refer to the test suite in `crates/rockshrew-mono/src/tests/view_pool_integration_test.rs` for working examples.
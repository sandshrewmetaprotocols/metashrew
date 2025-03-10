# Metashrew Technical Context

## Technologies Used

### Programming Languages

- **Rust**: The primary language used throughout the project, chosen for its performance, memory safety, and strong type system.
- **WebAssembly (WASM)**: The compilation target for both the indexer and client modules, enabling portable and sandboxed execution.
- **AssemblyScript** (supported client language): A TypeScript-like language that compiles to WebAssembly, making it accessible for web developers.
- **C/C++** (supported client language): Can be compiled to WebAssembly using Emscripten.

### Runtime Environments

- **wasmtime**: WebAssembly runtime used for executing WASM modules.
- **tokio**: Asynchronous runtime for handling concurrent operations.
- **actix-web**: Web framework for the JSON-RPC server.

### Storage

- **RocksDB**: High-performance embedded key-value store used for persistent storage.
  - Replaced KeyDB in previous versions for better performance and reliability.
  - Configured for optimal performance with Bitcoin indexing workloads.

### Serialization

- **Protocol Buffers**: Used for data serialization in the runtime.
- **serde/serde_json**: Used for JSON serialization and deserialization in the RPC server.

### Bitcoin Integration

- **bitcoin**: Rust library for Bitcoin data structures and utilities.
- **bitcoin_slices**: Efficient Bitcoin data parsing.
- **bitcoincore-rpc**: Client for communicating with Bitcoin Core nodes.

### Utilities

- **anyhow**: Error handling library.
- **clap**: Command-line argument parsing.
- **hex**: Hexadecimal encoding/decoding.
- **log/env_logger**: Logging infrastructure.

## RocksDB Integration

Metashrew uses RocksDB as its primary storage backend, replacing the previously used KeyDB for improved performance and reliability.

### RocksDB Adapter

The `RocksDBRuntimeAdapter` implements the `KeyValueStoreLike` trait, providing a consistent interface for database operations:

```rust
#[derive(Clone)]
pub struct RocksDBRuntimeAdapter {
    pub db: Arc<DB>,
    pub height: u32,
}

impl KeyValueStoreLike for RocksDBRuntimeAdapter {
    type Batch = RocksDBBatch;
    type Error = rocksdb::Error;

    fn write(&mut self, batch: RocksDBBatch) -> Result<(), Self::Error> {
        // Implementation...
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        // Implementation...
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        // Implementation...
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]> {
        // Implementation...
    }
}
```

### RocksDB Configuration

RocksDB is configured for optimal performance with Bitcoin indexing workloads:

```rust
let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_max_open_files(10000);
opts.set_use_fsync(false);
opts.set_bytes_per_sync(8388608); // 8MB
opts.optimize_for_point_lookup(1024);
opts.set_table_cache_num_shard_bits(6);
opts.set_max_write_buffer_number(6);
opts.set_write_buffer_size(256 * 1024 * 1024);
opts.set_target_file_size_base(256 * 1024 * 1024);
opts.set_min_write_buffer_number_to_merge(2);
opts.set_level_zero_file_num_compaction_trigger(4);
opts.set_level_zero_slowdown_writes_trigger(20);
opts.set_level_zero_stop_writes_trigger(30);
opts.set_max_background_jobs(4);
opts.set_max_background_compactions(4);
opts.set_disable_auto_compactions(false);
```

These settings are optimized for:
- High write throughput during initial synchronization
- Efficient point lookups for view function queries
- Balanced memory usage and disk I/O
- Effective background compaction

### Secondary Instance Support

RocksDB supports secondary instances, which are used by the view layer to provide read-only access to the database without interfering with the indexer:

```rust
pub fn open_secondary(
    primary_path: String,
    secondary_path: String, 
    opts: rocksdb::Options
) -> Result<Self, rocksdb::Error> {
    let db = rocksdb::DB::open_as_secondary(&opts, &primary_path, &secondary_path)?;
    Ok(RocksDBRuntimeAdapter {
        db: Arc::new(db),
        height: 0
    })
}
```

Secondary instances periodically catch up with the primary instance to ensure they have the latest data.

### Namespace Support

RocksDB keys can be prefixed with a namespace label to isolate different applications:

```rust
pub fn to_labeled_key(key: &Vec<u8>) -> Vec<u8> {
    if has_label() {
        let mut result: Vec<u8> = vec![];
        result.extend(get_label().as_str().as_bytes());
        result.extend(key);
        result
    } else {
        key.clone()
    }
}
```

This allows multiple applications to share the same RocksDB instance without key collisions.

## Development Setup

### Prerequisites

- **Rust Toolchain**: Latest stable version
- **Bitcoin Core**: v22.0+ for blockchain data
- **RocksDB Dependencies**: Development libraries for RocksDB
- **WebAssembly Tools**: For WASM module development

### Building Metashrew

```sh
# Clone the repository
git clone https://github.com/sandshrewmetaprotocols/metashrew
cd metashrew

# Build the combined binary (recommended)
cargo build --release -p rockshrew-mono

# Or build individual components
cargo build --release -p rockshrew
cargo build --release -p rockshrew-view
```

### Running Metashrew

```sh
# Combined binary (recommended)
./target/release/rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:password \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --host 0.0.0.0 \
  --port 8080

# Or separate processes
./target/release/rockshrew \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:password \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew

./target/release/rockshrew-view \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --secondary-path ~/.metashrew-secondary \
  --host 0.0.0.0 \
  --port 8080
```

### Docker Deployment

```sh
# Build the Docker images
docker build -f docker/Dockerfile.indexer -t metashrew-indexer .
docker build -f docker/Dockerfile.view -t metashrew-view .

# Run with Docker Compose
docker-compose up -d
```

### Developing WASM Modules

#### AssemblyScript Example

```typescript
// indexer.ts
export function _start(): void {
  // Get input (height + block)
  const data = input();
  const height = u32(data.slice(0, 4));
  const block = data.slice(4);
  
  // Process block...
  
  // Write to database
  flush();
}

// Optional view function
export function getBalance(address: string): u64 {
  const data = input();
  // Query database state...
  return balance;
}
```

#### Rust Example

```rust
#[no_mangle]
pub extern "C" fn _start() {
    // Get input (height + block)
    let input = host::load_input();
    let height = u32::from_le_bytes(input[0..4].try_into().unwrap());
    let block = &input[4..];
    
    // Process block...
    
    // Write to database
    host::flush(&key_value_pairs);
}

// Optional view function
#[no_mangle]
pub extern "C" fn get_balance() -> i32 {
    // Query database state...
    // Return pointer to result
}
```

## Technical Constraints

### RocksDB Constraints

- **Disk Space**: The append-only database design requires more storage than traditional databases.
- **Memory Usage**: RocksDB can use significant memory, especially with the optimized settings.
- **File Handles**: RocksDB opens many files, requiring appropriate system limits.
- **Compaction**: Background compaction can impact performance during heavy write loads.

### WebAssembly Constraints

- **Memory Limit**: WebAssembly modules have a limited memory space (currently 4GB maximum).
- **No Direct System Access**: WASM modules cannot directly access the file system or network.
- **Limited Standard Library**: Some standard library functions may not be available in WASM.
- **Performance Overhead**: There's some overhead compared to native code, though it's minimal.

### Bitcoin Node Constraints

- **Disk Space**: A full Bitcoin node requires significant disk space (300GB+).
- **Network Bandwidth**: Initial synchronization requires substantial bandwidth.
- **RPC Limitations**: Bitcoin Core's RPC interface has rate limits and performance constraints.

### Concurrency Constraints

- **Shared State**: Access to shared state must be carefully managed to avoid race conditions.
- **Lock Contention**: Heavy concurrent access can lead to lock contention.
- **Resource Limits**: The system must balance resources between indexing and serving queries.

## Performance Considerations

### RocksDB Optimization

- **Write Buffer Size**: Larger write buffers improve write performance but use more memory.
- **Block Cache Size**: Larger block caches improve read performance but use more memory.
- **Compression**: Compression reduces disk usage but increases CPU usage.
- **File System**: The underlying file system can significantly impact performance.

### Indexing Performance

- **Block Processing**: The time to process each block depends on the complexity of the WASM module.
- **Initial Synchronization**: Initial synchronization can take days or weeks for the full Bitcoin blockchain.
- **Incremental Updates**: Once synchronized, keeping up with new blocks is typically fast.

### Query Performance

- **Point Lookups**: RocksDB is optimized for point lookups, which are typically fast.
- **Range Queries**: Range queries can be slower, especially for large ranges.
- **Historical Queries**: Querying historical state may require scanning multiple values.
- **View Function Complexity**: Complex view functions can impact query performance.

### Memory Management

- **RocksDB Cache**: RocksDB uses a block cache for frequently accessed data.
- **WASM Memory**: WASM modules have their own memory space, separate from the host.
- **Shared Data**: Data shared between components should be minimized to reduce copying.

### Optimizing for ALKANES

ALKANES is a complex metaprotocol that can benefit from specific optimizations:

- **Increased Write Buffer**: ALKANES generates many state changes, benefiting from larger write buffers.
- **Optimized Point Lookups**: ALKANES view functions rely heavily on point lookups.
- **Combined Binary**: Using rockshrew-mono is strongly recommended for ALKANES to avoid stability issues.
- **Memory Tuning**: ALKANES may require more memory for its complex state management.

## Security Considerations

### WebAssembly Sandboxing

- **Memory Isolation**: WASM modules have isolated memory spaces.
- **Limited Capabilities**: WASM modules can only access host functions explicitly provided.
- **Resource Limits**: The host can limit resources available to WASM modules.

### Database Security

- **No Authentication**: RocksDB has no built-in authentication or encryption.
- **File Permissions**: Database files should have appropriate permissions.
- **Backup Strategy**: Regular backups are recommended to prevent data loss.

### API Security

- **No Built-in Authentication**: The JSON-RPC API has no built-in authentication.
- **CORS Configuration**: The API has CORS configured to allow localhost origins only by default.
- **Input Validation**: All API inputs are validated to prevent injection attacks.

### Bitcoin Node Security

- **RPC Authentication**: Bitcoin Core RPC requires authentication.
- **Network Isolation**: The Bitcoin node should be properly isolated from the public internet.
- **Resource Limits**: Appropriate resource limits should be set to prevent DoS attacks.

## Deployment Considerations

### Resource Requirements

- **CPU**: 4+ cores recommended, especially for initial synchronization.
- **Memory**: 8GB+ recommended, more for complex metaprotocols like ALKANES.
- **Disk**: 500GB+ for a full Bitcoin node and Metashrew database.
- **Network**: High-bandwidth connection for initial synchronization.

### Monitoring

- **Logging**: Configure appropriate log levels using the `RUST_LOG` environment variable.
- **Metrics**: Monitor system resources (CPU, memory, disk) and application metrics.
- **Alerts**: Set up alerts for critical conditions like synchronization issues.

### Backup Strategy

- **Database Backups**: Regular backups of the RocksDB database.
- **Configuration Backups**: Backup of configuration files and WASM modules.
- **Recovery Testing**: Periodically test recovery from backups.

### Scaling

- **Vertical Scaling**: Increase resources (CPU, memory, disk) for better performance.
- **Horizontal Scaling**: Deploy multiple instances for read scaling (using secondary instances).
- **Sharding**: For very large datasets, consider sharding by block ranges or other criteria.

## Integration with ALKANES

### ALKANES-RS Requirements

- **Memory**: ALKANES-RS requires more memory due to its complex state management.
- **Disk Space**: ALKANES-RS generates more state changes, requiring more disk space.
- **CPU**: ALKANES-RS performs more complex computations, requiring more CPU resources.

### Optimizing RocksDB for ALKANES

```rust
// Recommended RocksDB settings for ALKANES
let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_max_open_files(10000);
opts.set_use_fsync(false);
opts.set_bytes_per_sync(8388608); // 8MB
opts.optimize_for_point_lookup(2048); // Increased for ALKANES
opts.set_table_cache_num_shard_bits(6);
opts.set_max_write_buffer_number(8); // Increased for ALKANES
opts.set_write_buffer_size(512 * 1024 * 1024); // Increased for ALKANES
opts.set_target_file_size_base(256 * 1024 * 1024);
opts.set_min_write_buffer_number_to_merge(2);
opts.set_level_zero_file_num_compaction_trigger(4);
opts.set_level_zero_slowdown_writes_trigger(20);
opts.set_level_zero_stop_writes_trigger(30);
opts.set_max_background_jobs(8); // Increased for ALKANES
opts.set_max_background_compactions(4);
opts.set_disable_auto_compactions(false);
```

### ALKANES View Functions

ALKANES-RS exposes several view functions that require specific optimizations:

- **protorunes_by_address**: Requires efficient access to the `OUTPOINTS_FOR_ADDRESS` and `OUTPOINT_TO_RUNES` tables.
- **runes_by_address**: Similar to `protorunes_by_address` but without protocol tag filtering.
- **protorunes_by_outpoint**: Requires efficient access to the `OUTPOINT_TO_RUNES` table.

### Deployment Recommendations for ALKANES

- **Use rockshrew-mono**: Always use the combined binary for ALKANES to avoid stability issues.
- **Increase Memory**: Allocate more memory than the default recommendations.
- **SSD Storage**: Use SSD storage for better performance with ALKANES's high write load.
- **Regular Maintenance**: Schedule regular maintenance for database compaction.

## Conclusion

Metashrew's technical architecture, centered around RocksDB and WebAssembly, provides a flexible and powerful platform for Bitcoin indexing and metaprotocol development. The system is designed to handle the demands of complex applications like ALKANES while maintaining flexibility for a wide range of use cases.

By understanding the technical constraints and performance considerations, developers can optimize their deployments for specific workloads and ensure reliable operation in production environments.
# Advanced Topics

This document covers advanced topics for Metashrew users and developers, including the snapshot system, SSH tunneling, custom storage backends, and performance optimization.

## Snapshot System

Metashrew includes a snapshot system that allows you to create and restore database snapshots, which is useful for backup, recovery, and sharing indexed data without requiring reindexing from scratch.

### How Snapshots Work

The snapshot system works by creating a consistent point-in-time copy of the RocksDB database. This includes:

1. The key-value pairs in the database
2. The current block height
3. Metadata about the snapshot

Snapshots are created using RocksDB's native snapshot mechanism, which ensures consistency without blocking writes.

### Creating Snapshots

You can create a snapshot using the `rockshrew-mono` CLI:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --create-snapshot my-snapshot
```

This will create a snapshot in the `~/.metashrew/snapshots/my-snapshot` directory.

You can also create a snapshot programmatically:

```rust
use rockshrew_mono::snapshot::{create_snapshot, SnapshotOptions};

let options = SnapshotOptions {
    name: "my-snapshot".to_string(),
    db_path: "~/.metashrew".to_string(),
    compress: true,
};

create_snapshot(&options)?;
```

### Restoring Snapshots

You can restore a snapshot using the `rockshrew-mono` CLI:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --restore-snapshot my-snapshot
```

This will restore the database from the snapshot in the `~/.metashrew/snapshots/my-snapshot` directory.

You can also restore a snapshot programmatically:

```rust
use rockshrew_mono::snapshot::{restore_snapshot, SnapshotOptions};

let options = SnapshotOptions {
    name: "my-snapshot".to_string(),
    db_path: "~/.metashrew".to_string(),
    compress: true,
};

restore_snapshot(&options)?;
```

### Exporting and Importing Snapshots

You can export a snapshot to a file:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --export-snapshot my-snapshot \
  --output my-snapshot.tar.gz
```

And import a snapshot from a file:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --import-snapshot my-snapshot.tar.gz
```

### Snapshot Verification

You can verify a snapshot's integrity:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --verify-snapshot my-snapshot
```

This will check the snapshot's integrity by verifying the state root against the block hash.

### Snapshot Pruning

You can prune old snapshots to save disk space:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --prune-snapshots --keep-last 5
```

This will keep the 5 most recent snapshots and delete the rest.

## SSH Tunneling

Metashrew supports SSH tunneling for secure remote access to Bitcoin Core nodes and RocksDB databases.

### Why Use SSH Tunneling?

SSH tunneling provides several benefits:

1. **Security**: Encrypt communication between Metashrew and remote services
2. **Access Control**: Access remote services without exposing them to the public internet
3. **Firewall Traversal**: Access services behind firewalls or NAT

### Setting Up SSH Tunneling for Bitcoin Core

You can set up SSH tunneling for Bitcoin Core using the `rockshrew-mono` CLI:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --ssh-tunnel user@remote-host:22 \
  --ssh-key ~/.ssh/id_rsa \
  --ssh-remote-port 8332 \
  --ssh-local-port 8333
```

This will create an SSH tunnel from `localhost:8333` to `remote-host:8332` and use `localhost:8333` as the daemon RPC URL.

### SSH Tunneling Configuration

The SSH tunneling feature supports the following options:

- `--ssh-tunnel`: The SSH server to connect to in the format `user@host:port`
- `--ssh-key`: The path to the SSH private key
- `--ssh-remote-port`: The remote port to forward
- `--ssh-local-port`: The local port to forward to
- `--ssh-password`: The SSH password (not recommended, use key-based authentication instead)
- `--ssh-known-hosts`: The path to the known_hosts file

### SSH Tunneling for Remote Databases

You can also use SSH tunneling to access remote RocksDB databases:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --ssh-tunnel user@remote-host:22 \
  --ssh-key ~/.ssh/id_rsa \
  --ssh-remote-db-path /path/to/remote/db \
  --ssh-local-db-path ~/.metashrew/remote-db
```

This will mount the remote database at `/path/to/remote/db` to the local path `~/.metashrew/remote-db` using SSHFS.

### Programmatic SSH Tunneling

You can also set up SSH tunneling programmatically:

```rust
use rockshrew_mono::ssh_tunnel::{SshTunnel, SshTunnelOptions};

let options = SshTunnelOptions {
    host: "remote-host".to_string(),
    port: 22,
    username: "user".to_string(),
    key_path: "~/.ssh/id_rsa".to_string(),
    remote_port: 8332,
    local_port: 8333,
    ..Default::default()
};

let tunnel = SshTunnel::new(options)?;
tunnel.start()?;

// Use the tunnel
// ...

tunnel.stop()?;
```

## Custom Storage Backends

Metashrew supports custom storage backends, allowing you to use different key-value stores or even remote databases.

### Built-in Storage Backends

Metashrew includes the following built-in storage backends:

1. **RocksDB**: The default storage backend, optimized for high-performance local storage
2. **Memory**: An in-memory storage backend for testing and development
3. **Remote**: A storage backend that connects to a remote Metashrew instance via JSON-RPC

### Using Different Storage Backends

You can specify the storage backend using the `rockshrew-mono` CLI:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --storage-backend rocksdb \
  --db-path ~/.metashrew
```

### Implementing a Custom Storage Backend

You can implement a custom storage backend by implementing the `KeyValueStoreLike` trait:

```rust
use rockshrew_runtime::storage::{KeyValueStoreLike, KeyValueStoreError};

struct MyCustomStorage {
    // Your storage implementation
}

impl KeyValueStoreLike for MyCustomStorage {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, KeyValueStoreError> {
        // Implement get
    }

    fn put(&self, key: &[u8], value: &[u8]) -> Result<(), KeyValueStoreError> {
        // Implement put
    }

    fn delete(&self, key: &[u8]) -> Result<(), KeyValueStoreError> {
        // Implement delete
    }

    fn flush(&self) -> Result<(), KeyValueStoreError> {
        // Implement flush
    }

    fn get_at_height(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>, KeyValueStoreError> {
        // Implement get_at_height
    }

    fn get_with_proof(&self, key: &[u8]) -> Result<Option<(Vec<u8>, Vec<u8>)>, KeyValueStoreError> {
        // Implement get_with_proof
    }

    fn get_with_proof_at_height(&self, key: &[u8], height: u32) -> Result<Option<(Vec<u8>, Vec<u8>)>, KeyValueStoreError> {
        // Implement get_with_proof_at_height
    }

    fn get_state_root(&self) -> Result<Vec<u8>, KeyValueStoreError> {
        // Implement get_state_root
    }

    fn get_state_root_at_height(&self, height: u32) -> Result<Vec<u8>, KeyValueStoreError> {
        // Implement get_state_root_at_height
    }
}
```

### Example: Redis Storage Backend

Here's an example of a Redis storage backend:

```rust
use rockshrew_runtime::storage::{KeyValueStoreLike, KeyValueStoreError};
use redis::{Client, Commands};

struct RedisStorage {
    client: Client,
}

impl RedisStorage {
    fn new(url: &str) -> Result<Self, redis::RedisError> {
        let client = Client::open(url)?;
        Ok(Self { client })
    }
}

impl KeyValueStoreLike for RedisStorage {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, KeyValueStoreError> {
        let mut conn = self.client.get_connection()
            .map_err(|e| KeyValueStoreError::Other(format!("Redis connection error: {}", e)))?;
        
        let result: Option<Vec<u8>> = conn.get(key)
            .map_err(|e| KeyValueStoreError::Other(format!("Redis get error: {}", e)))?;
        
        Ok(result)
    }

    fn put(&self, key: &[u8], value: &[u8]) -> Result<(), KeyValueStoreError> {
        let mut conn = self.client.get_connection()
            .map_err(|e| KeyValueStoreError::Other(format!("Redis connection error: {}", e)))?;
        
        conn.set(key, value)
            .map_err(|e| KeyValueStoreError::Other(format!("Redis set error: {}", e)))?;
        
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<(), KeyValueStoreError> {
        let mut conn = self.client.get_connection()
            .map_err(|e| KeyValueStoreError::Other(format!("Redis connection error: {}", e)))?;
        
        conn.del(key)
            .map_err(|e| KeyValueStoreError::Other(format!("Redis del error: {}", e)))?;
        
        Ok(())
    }

    fn flush(&self) -> Result<(), KeyValueStoreError> {
        // Redis doesn't have a direct equivalent to flush
        // You might implement this differently based on your needs
        Ok(())
    }

    // Implement other methods...
}
```

### Registering a Custom Storage Backend

You can register a custom storage backend with Metashrew:

```rust
use rockshrew_mono::storage::{register_storage_backend, StorageBackendFactory};

struct RedisStorageFactory;

impl StorageBackendFactory for RedisStorageFactory {
    fn create(&self, options: &str) -> Result<Box<dyn KeyValueStoreLike>, KeyValueStoreError> {
        let storage = RedisStorage::new(options)
            .map_err(|e| KeyValueStoreError::Other(format!("Redis error: {}", e)))?;
        
        Ok(Box::new(storage))
    }
}

fn main() {
    register_storage_backend("redis", Box::new(RedisStorageFactory));
    
    // Now you can use the redis storage backend:
    // rockshrew-mono --storage-backend redis --storage-options "redis://localhost:6379"
}
```

## Performance Optimization

Optimizing Metashrew's performance is crucial for efficient indexing and querying of blockchain data.

### RocksDB Optimization

RocksDB is highly configurable and can be optimized for different workloads:

#### Block Cache Size

The block cache size determines how much memory RocksDB uses for caching data blocks:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --rocksdb-block-cache-size 1073741824  # 1GB
```

#### Write Buffer Size

The write buffer size determines how much data is accumulated before flushing to disk:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --rocksdb-write-buffer-size 67108864  # 64MB
```

#### Max Open Files

The max open files setting determines how many files RocksDB can keep open at once:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --rocksdb-max-open-files 1000
```

#### Compression

RocksDB supports different compression algorithms:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --rocksdb-compression lz4
```

Options include: `none`, `snappy`, `zlib`, `bzip2`, `lz4`, `lz4hc`, `zstd`

#### Parallelism

You can configure the number of threads RocksDB uses for background operations:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --rocksdb-max-background-jobs 4
```

### Indexer Optimization

Optimizing your indexer code can significantly improve performance:

#### Batch Operations

Group database operations to minimize I/O:

```rust
// Process all transactions in a block
for tx in &block.transactions {
    // Process tx
    // ...
}

// Single flush at the end
flush()?;
```

#### Minimize Memory Allocations

Reuse buffers to minimize memory allocations:

```rust
// Preallocate a buffer
let mut key_buffer = Vec::with_capacity(64);
let mut value_buffer = Vec::with_capacity(64);

for tx in &block.transactions {
    // Reuse the buffers
    key_buffer.clear();
    key_buffer.extend_from_slice(b"tx:");
    key_buffer.extend_from_slice(&tx.txid);
    
    value_buffer.clear();
    // Serialize tx to value_buffer
    
    put(&key_buffer, &value_buffer)?;
}
```

#### Use Efficient Data Structures

Choose appropriate data structures for your use case:

```rust
// Use a HashMap for caching
let mut address_balances = HashMap::new();

for tx in &block.transactions {
    for output in &tx.outputs {
        if let Some(address) = script_to_address(&output.script_pubkey, Network::Bitcoin) {
            let balance = address_balances.entry(address.clone())
                .or_insert_with(|| {
                    let key = format!("address:{}:balance", address).into_bytes();
                    get(&key).unwrap().map(|data| u64::from_le_bytes(data.try_into().unwrap())).unwrap_or(0)
                });
            
            *balance += output.value;
        }
    }
}

// Write all balances at once
for (address, balance) in address_balances {
    let key = format!("address:{}:balance", address).into_bytes();
    put(&key, &balance.to_le_bytes())?;
}
```

### System Optimization

Optimizing your system can also improve Metashrew's performance:

#### Disk I/O

Use a fast SSD for the database:

```bash
# Mount an SSD to ~/.metashrew
sudo mount -o discard,noatime /dev/nvme0n1p1 ~/.metashrew

# Run rockshrew-mono with the SSD as the database path
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew
```

#### Memory

Allocate enough memory to Metashrew:

```bash
# Set the maximum heap size for the JVM (used by RocksDB's JNI bindings)
export JAVA_OPTS="-Xmx4g"

# Run rockshrew-mono with the JVM options
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew
```

#### CPU

Use multiple cores for indexing:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --threads 4
```

### Monitoring and Profiling

Monitoring and profiling can help identify performance bottlenecks:

#### Logging

Enable debug logging to see what's happening:

```bash
RUST_LOG=debug rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew
```

#### Metrics

Enable metrics to collect performance data:

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --metrics \
  --metrics-port 9090
```

You can then view the metrics at `http://localhost:9090/metrics`.

#### Profiling

Use a profiler to identify performance bottlenecks:

```bash
# Install perf
sudo apt-get install linux-tools-common linux-tools-generic

# Run rockshrew-mono with perf
perf record -g rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew

# Analyze the results
perf report
```

### Performance Tuning Recommendations

Based on common usage patterns, here are some recommended settings:

#### Small Dataset (< 10GB)

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --rocksdb-block-cache-size 268435456 \  # 256MB
  --rocksdb-write-buffer-size 33554432 \  # 32MB
  --rocksdb-max-open-files 500 \
  --rocksdb-compression snappy \
  --rocksdb-max-background-jobs 2 \
  --threads 2
```

#### Medium Dataset (10GB - 100GB)

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --rocksdb-block-cache-size 1073741824 \  # 1GB
  --rocksdb-write-buffer-size 67108864 \  # 64MB
  --rocksdb-max-open-files 1000 \
  --rocksdb-compression lz4 \
  --rocksdb-max-background-jobs 4 \
  --threads 4
```

#### Large Dataset (> 100GB)

```bash
rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --rocksdb-block-cache-size 4294967296 \  # 4GB
  --rocksdb-write-buffer-size 134217728 \  # 128MB
  --rocksdb-max-open-files 2000 \
  --rocksdb-compression zstd \
  --rocksdb-max-background-jobs 8 \
  --threads 8
```

## Conclusion

This document covered advanced topics for Metashrew users and developers, including the snapshot system, SSH tunneling, custom storage backends, and performance optimization. By leveraging these advanced features, you can build more powerful and efficient blockchain indexers and applications.

For more information, refer to the other documentation sections, particularly the [Development Guide](./development.md) and [RocksDB Configuration](./rockshrew-mono.md#rocksdb-configuration) sections.
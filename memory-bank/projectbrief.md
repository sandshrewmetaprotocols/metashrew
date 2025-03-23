# Metashrew Project Brief

## Project Overview

Metashrew is a Bitcoin indexer framework with a WebAssembly (WASM) virtual machine. It's designed to simplify the development of Bitcoin metaprotocols and indexers by allowing developers to focus solely on the logic of processing individual blocks. The framework provides a portable calling convention compatible with AssemblyScript, C, Rust, or any language that can compile to WASM.

The project aims to create an extensible platform where custom indexers can be loaded as WASM modules to process Bitcoin blockchain data. This approach allows for flexible and modular development of blockchain data processing applications without modifying the core indexing engine.

## Technical Context

### Programming Languages and Technologies

- **Primary Language**: Rust
- **Secondary Technologies**: WebAssembly (WASM), Bitcoin Core RPC
- **Database**: RocksDB (key-value store)
- **API**: JSON-RPC
- **Containerization**: Docker

### Dependencies

#### Core Dependencies
- **wasmtime**: WebAssembly runtime (v15.0.1)
- **rocksdb**: Persistent key-value store (v0.21.0)
- **bitcoin**: Bitcoin library for Rust (v0.31.0/v0.32.1)
- **bitcoin_slices**: Efficient Bitcoin data parsing (v0.7)
- **bitcoincore-rpc**: Bitcoin Core RPC client (v0.18)
- **actix-web**: Web server framework (v4.5.1/v4.9.0)
- **tokio**: Asynchronous runtime (v1.43.0)
- **clap**: Command-line argument parsing (v4.5.13/v4.5.26)
- **serde/serde_json**: Serialization/deserialization (v1.0.x)
- **anyhow**: Error handling (v1.0.x)
- **rlp**: Recursive Length Prefix encoding (v0.5.2)

#### Compression Libraries
- **zstd**: Compression library (v0.13.0)
- **snap**: Snappy compression (v1.1.0)

### System Architecture

The Metashrew architecture consists of several key components:

1. **Core Runtime (metashrew-runtime)**:
   - Provides the WASM execution environment
   - Implements the host functions for WASM modules
   - Manages the key-value store interface

2. **RocksDB Adapter (rockshrew-runtime)**:
   - Implements the key-value store interface for RocksDB
   - Handles database operations with labeled namespaces

3. **Indexer (rockshrew)**:
   - Fetches blocks from Bitcoin Core
   - Loads and executes WASM modules for each block
   - Maintains the indexed data in RocksDB

4. **View Layer (rockshrew-view)**:
   - Exposes a JSON-RPC API for querying indexed data
   - Executes view functions from WASM modules
   - Provides read-only access to the database

5. **Combined Binary (rockshrew-mono)**:
   - Combines indexer and view layer in a single process
   - Recommended for stability, especially with large databases

### Design Patterns

1. **WebAssembly Host Interface**:
   - Defined host functions (`__log`, `__load_input`, `__host_len`, `__flush`, `__get`, `__get_len`)
   - Memory management conventions for passing data between host and WASM

2. **Append-Only Database**:
   - Key-value pairs are never overwritten
   - Values are annotated with block height
   - Enables historical queries and rollbacks during chain reorganizations

3. **Actor Model**:
   - Uses Tokio for asynchronous processing
   - Mutex-protected shared state for thread safety

4. **Dependency Injection**:
   - Generic traits for database backends (`KeyValueStoreLike`, `BatchLike`)
   - Allows for different storage implementations

## Source Code Modules

### Core Modules

1. **runtime**:
   - `runtime.rs`: Core WASM runtime implementation
   - `proto/`: Protocol buffer definitions
   - Implements the MetashrewRuntime for executing WASM modules

2. **rockshrew-runtime**:
   - `lib.rs`: RocksDB adapter implementation
   - Provides RocksDBRuntimeAdapter for database operations

3. **rockshrew**:
   - `main.rs`: Indexer implementation
   - Fetches blocks and processes them with WASM modules

4. **rockshrew-view**:
   - `main.rs`: View layer implementation
   - JSON-RPC server for querying indexed data

5. **rockshrew-mono**:
   - `main.rs`: Combined indexer and view layer
   - Single process for both indexing and serving queries

6. **mempool**:
   - Placeholder for mempool-related functionality (currently minimal)

7. **dynamodb/dynamodb-runtime/dynamodb-view**:
   - Alternative implementations using DynamoDB

### Docker and Deployment

1. **docker/**:
   - Dockerfile.indexer: Container for the indexer
   - Dockerfile.view: Container for the view layer
   - Docker entrypoint scripts

2. **docker-compose.yaml**:
   - Defines services for running Metashrew
   - Environment variable configuration
   - Volume mounting for data persistence

## Additional Context

### Database Structure

The RocksDB database is structured as an append-only store where:
- Keys are annotated with indices for list-like structures
- Values are annotated with block heights
- Special internal keys track metadata like tip height
- Namespaces can be created using labels

### WASM Integration

WASM modules must implement:
1. A `_start` function that processes a block
2. Optional view functions for querying indexed data

The host provides functions for:
- Logging (`__log`)
- Loading input data (`__load_input`, `__host_len`)
- Database operations (`__get`, `__get_len`, `__flush`)

### Chain Reorganization Handling

The system handles Bitcoin chain reorganizations by:
1. Detecting divergence in the blockchain
2. Rolling back affected keys to the state before the divergence
3. Reprocessing blocks from the new chain

### Deployment Options

1. **Single Process (rockshrew-mono)**:
   - Recommended for stability
   - Combines indexing and view functionality
   - Command: `rockshrew-mono --daemon-rpc-url http://localhost:8332 --auth bitcoinrpc:bitcoinrpc --indexer path/to/indexer.wasm --db-path ~/.metashrew --host 0.0.0.0 --port 8080`

2. **Separate Processes**:
   - `rockshrew` for indexing
   - `rockshrew-view` for serving queries
   - Allows for distributed deployment

3. **Docker Containers**:
   - Pre-built containers available
   - Configurable via environment variables
   - Docker Compose setup provided in the repository

### Metaprotocol Development

Developers can create custom metaprotocols by:
1. Writing code in AssemblyScript, Rust, or C that compiles to WASM
2. Implementing the required interface functions
3. Loading the compiled WASM module into Metashrew

Example metaprotocols are available in the [sandshrewmetaprotocols](https://github.com/sandshrewmetaprotocols) GitHub organization, including ALKANES which is mentioned in the documentation.

### View Functions

The system extends the electrs JSON-RPC with `metashrew.view` for querying indexed data:
- View functions are exported from the WASM module
- They use the same runtime as the indexer but in read-only mode
- Parameters follow the format: `[functionName, inputAsHex, blockTag]`
- `blockTag` can be "latest" or a block number in hex

## Debugging and Maintenance

### Common Issues

- Stability problems may occur when running rockshrew-view as a secondary process on top of a mainnet database for certain indexers (like ALKANES)
- The recommended solution is to use rockshrew-mono which combines the view layer and indexer process

### Logging

- Environment variable `RUST_LOG` controls log levels
- `env_logger` is used for logging implementation
- Default log level is INFO for indexer and DEBUG for view layer

### Performance Considerations

- RocksDB is configured with multi-threaded column families
- The system is designed to handle chain reorganizations efficiently
- The append-only database design may require more storage but enables historical queries

## Conclusion

Metashrew provides a flexible framework for building Bitcoin indexers and metaprotocols using WebAssembly. Its modular design allows developers to focus on block processing logic while the framework handles the complexities of blockchain data retrieval, storage, and querying. The append-only database design ensures data integrity during chain reorganizations and enables historical queries. 
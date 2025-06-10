# metashrew

Bitcoin indexer framework powered by WebAssembly (WASM).

## Overview

Metashrew was architected to reduce the problem of building a metaprotocol, or even a lower-level indexer, to architecting an executable that handles solely the logic of processing a single block. The executables follow a portable calling convention and are compatible with AssemblyScript, but can also, themselves, be written in C or Rust, or anything that builds to a WASM target.

Rather than dealing with complex Bitcoin node interactions, developers only need to implement block processing logic in their preferred language that compiles to WASM (AssemblyScript, Rust, C, etc.).

Repositories are hosted within [https://github.com/sandshrewmetaprotocols](https://github.com/sandshrewmetaprotocols) with a number of metaprotocols already built to WASM, which can be loaded into metashrew.

The framework handles:
- Block synchronization with Bitcoin Core
- Database management and rollbacks
- JSON-RPC interface for queries
- View function execution for historical state

## Prerequisites

- Rust toolchain (latest stable)
- Bitcoin Core node (v22.0+)
- For WASM development:
  - AssemblyScript toolchain (optional)
  - Rust wasm32 target (optional)
  - C/C++ WASM toolchain (optional)

## Installation

1. Clone the repository:
```sh
git clone https://github.com/sandshrewmetaprotocols/metashrew
cd metashrew
```

2. Build the indexer:
```sh
cargo build --release -p rockshrew-mono
```

This produces the `rockshrew-mono` binary that combines both indexing and view capabilities.

## Basic Usage

Start rockshrew-mono with your WASM indexer:

```sh
./target/release/rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:password \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --host 0.0.0.0 \
  --port 8080
```

Configuration options:
- `--daemon-rpc-url`: Bitcoin Core RPC URL
- `--auth`: RPC credentials (username:password)
- `--indexer`: Path to your WASM indexer
- `--db-path`: Database directory
- `--start-block`: Optional starting block height
- `--host`: JSON-RPC bind address
- `--port`: JSON-RPC port
- `--label`: Optional database label
- `--exit-at`: Optional block height to stop at
- `--snapshot-interval`: Block interval between automatic snapshots
- `--snapshot-directory`: Directory to store snapshots
- `--repo`: URL of snapshot repository to sync from
- `--pipeline-size`: Size of the processing pipeline (default: auto-determined)
- `--cors`: CORS allowed origins (e.g., '*' for all origins)

## Comparing Indexers with rockshrew-diff

The `rockshrew-diff` tool allows you to compare the output of two different WASM modules processing the same blockchain data. This is particularly useful for:

- Validating changes to metaprotocol implementations
- Ensuring upgrades don't introduce financial side effects
- Debugging differences between implementations
- Testing compatibility between versions

### How It Works

1. `rockshrew-diff` processes blocks with both WASM modules
2. It compares key-value pairs with a specified prefix
3. When differences are found, it prints a detailed report and exits
4. If no differences are found, it continues to the next block

### Example Usage

Compare balance changes indexed by two versions of the ALKANES metaprotocol:

```sh
./target/release/rockshrew-diff \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:bitcoinrpc \
  --indexer /home/ubuntu/primary.wasm \
  --compare /home/ubuntu/alkanes-rs/target/wasm32-unknown-unknown/release/alkanes.wasm \
  --db-path /home/ubuntu/.rockshrew-diff \
  --prefix 0x2f72756e65732f70726f746f2f312f62796f7574706f696e742f \
  --start-block 880000
```

This example compares how two versions of the ALKANES metaprotocol index balance changes at the key prefix `/runes/proto/1/byoutpoint/`. This ensures that protocol upgrades don't introduce destructive financial side effects - a major benefit of using a metashrew-based index for metaprotocol development.

### Configuration Options

- `--daemon-rpc-url`: Bitcoin Core RPC URL
- `--auth`: RPC credentials (username:password)
- `--indexer`: Path to primary WASM module
- `--compare`: Path to comparison WASM module
- `--db-path`: Database directory for both modules
- `--prefix`: Hex-encoded key prefix to compare (must start with 0x)
- `--start-block`: Block height to start comparison
- `--exit-at`: Optional block height to stop at
- `--pipeline-size`: Optional pipeline size for parallel processing (default: 5)

## WASM Runtime Environment

### Host Functions

Your WASM program has access to these key host functions:

```typescript
// Get input data length (block height + serialized block)
__host_len(): i32

// Load input data into WASM memory
__load_input(ptr: i32): void

// Write to stdout (UTF-8 encoded)
__log(ptr: i32): void

// Commit key-value pairs to database
__flush(ptr: i32): void

// Get value length for a key
__get_len(ptr: i32): i32

// Read value for a key
__get(key_ptr: i32, value_ptr: i32): void
```

### Memory Layout

Pointers passed to host functions must follow AssemblyScript's ArrayBuffer memory layout:
- 4 bytes for length (little-endian u32)
- Followed by actual data bytes

### Required Entry Points

Your WASM program must export:

1. `_start()` - Main indexing function
   - Receives: block height (u32) + serialized block
   - Processes block and updates database

2. View functions (optional)
   - Custom named exports
   - Receive: function-specific input
   - Return: function-specific output
   - Read-only access to database

## Building an Indexer

Here's a minimal example using AssemblyScript:

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

For a complete example, see the [alkanes-rs](https://github.com/kungfuflex/alkanes-rs) indexer.

## Database Architecture

Metashrew uses RocksDB with an append-only architecture:

- Keys are versioned by position
- Values are annotated with block height
- Maintains list of touched keys per block
- Enables automatic rollbacks on reorgs
- Supports historical state queries

The database structure allows:
- Consistent state across parallel indexers
- Easy rollbacks during reorgs
- Historical state queries
- High performance reads/writes

## State Root and Snapshot System

Metashrew implements a robust state verification and snapshot system that enables fast syncing, integrity verification, and static file serving capabilities.

### Sparse Merkle Tree (SMT) for State Roots

Metashrew uses a Sparse Merkle Tree (SMT) implementation to generate cryptographic state roots:

- **State Root Generation**: Each block height has an associated state root hash that represents the entire database state
- **Hierarchical Structure**: The SMT organizes data in a binary tree with 256-bit paths
- **Efficient Verification**: Allows proving inclusion/exclusion of specific keys without downloading the entire database
- **Height-Annotated Values**: Values are annotated with block heights for historical queries
- **Rollback Support**: The tree structure facilitates chain reorganization handling

The state root can be queried via the `metashrew_stateroot` JSON-RPC method:

```sh
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"metashrew_stateroot","params":["latest"]}'
```

### Snapshot Mode

Snapshot mode creates point-in-time captures of the database state:

- **Automatic Snapshots**: Created at configurable block intervals
- **Complete State Capture**: Includes database files, state root, and WASM module hash
- **Integrity Verification**: Each snapshot includes checksums and state root for verification
- **Efficient Storage**: Stores only the necessary SST files from RocksDB
- **WASM Versioning**: Tracks which WASM module was used to build each snapshot

Enable snapshot mode with:

```sh
./target/release/rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:password \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --snapshot-interval 10000 \
  --snapshot-directory ~/.metashrew/snapshots
```

### Repo Mode

Repo mode enables fast syncing from a remote snapshot repository:

- **Fast Initial Sync**: Download pre-built database snapshots instead of processing blocks from scratch
- **Incremental Updates**: Apply diffs between snapshots to catch up to the latest state
- **WASM Consistency**: Ensures the correct WASM module is used for each block range
- **Static File Serving**: Snapshots can be served from static file servers like nginx
- **Integrity Verification**: Validates state roots and file checksums during sync

Sync from a snapshot repository with:

```sh
./target/release/rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:password \
  --db-path ~/.metashrew \
  --repo https://snapshots.example.com/metashrew
```

### Benefits of the Snapshot System

- **Fast Bootstrapping**: New nodes can sync in minutes instead of days
- **Verifiable Integrity**: Cryptographic verification of database state
- **Reduced Resource Usage**: Minimizes CPU, network, and disk I/O during initial sync
- **Deployment Flexibility**: Snapshots can be hosted on any static file server
- **WASM Trajectory Tracking**: Maintains history of which WASM modules processed which blocks

### Snapshot Repository Structure

A snapshot repository is a static file structure that can be served by any web server (like nginx). The structure is organized as follows:

```
snapshot-repo/
├── metadata.json             # Global metadata about the repository
├── wasm/                     # WASM module files
│   ├── <hash1>.wasm          # WASM module identified by hash
│   └── <hash2>.wasm          # Another WASM module version
├── snapshots/                # Full database snapshots
│   ├── <height>-<blockhash>/ # Snapshot directory for specific height
│   │   ├── metadata.json     # Snapshot metadata (height, state root, etc.)
│   │   ├── stateroot.json    # State root information
│   │   └── sst/              # RocksDB SST files
│   │       ├── <file1>.sst   # Database file
│   │       └── <file2>.sst   # Database file
│   └── ...
└── diffs/                    # Incremental updates between snapshots
    ├── <start>-<end>/        # Diff from start height to end height
    │   ├── metadata.json     # Diff metadata
    │   ├── stateroot.json    # End state root information
    │   └── diff.bin          # Binary diff data
    └── ...
```

The `metadata.json` file at the repository root contains:
- Index name and version
- Start block height and hash
- Latest snapshot height and hash
- WASM module history with height ranges
- Current WASM module hash

To create your own snapshot repository:

1. Enable snapshot mode on your indexer
2. Copy the generated snapshot directory to your web server
3. Ensure proper MIME types are configured for `.wasm` and `.sst` files
4. Configure your web server to allow CORS if needed

Example nginx configuration:
```nginx
server {
    listen 80;
    server_name snapshots.example.com;
    root /var/www/snapshot-repo;

    location / {
        autoindex on;
        add_header Access-Control-Allow-Origin *;
    }

    types {
        application/wasm wasm;
        application/octet-stream sst;
    }
}
```

## Development Guide

1. Choose your WASM development environment:
   - AssemblyScript (TypeScript-like)
   - Rust + wasm32-unknown-unknown
   - C/C++ with Emscripten
   
2. Implement required functions:
   - `_start()` for indexing
   - View functions as needed

3. Build WASM binary:
   ```sh
   # AssemblyScript
   asc indexer.ts -o indexer.wasm
   
   # Rust
   cargo build --target wasm32-unknown-unknown
   ```

4. Test locally:
   ```sh
   ./rockshrew-mono --daemon-rpc-url ... --indexer ./indexer.wasm
   ```

5. Query indexed data:
    ```sh
    curl -X POST http://localhost:8080 \
      -H "Content-Type: application/json" \
      -d '{"jsonrpc":"2.0","method":"metashrew_view","params":["viewFunction","inputHex","latest"]}'
    ```

## JSON-RPC API

Metashrew provides a JSON-RPC API for interacting with the indexer:

### Core Methods

- `metashrew_view`: Execute a view function at a specific height
  ```json
  {"jsonrpc":"2.0","method":"metashrew_view","params":["viewFunction","inputHex","height"]}
  ```

- `metashrew_preview`: Execute a view function against a specific block without indexing it
  ```json
  {"jsonrpc":"2.0","method":"metashrew_preview","params":["blockHex","viewFunction","inputHex","height"]}
  ```

- `metashrew_height`: Get the current indexed height
  ```json
  {"jsonrpc":"2.0","method":"metashrew_height","params":[]}
  ```

- `metashrew_getblockhash`: Get the block hash at a specific height
  ```json
  {"jsonrpc":"2.0","method":"metashrew_getblockhash","params":[height]}
  ```

### State Root and Snapshot Methods

- `metashrew_stateroot`: Get the SMT state root at a specific height
  ```json
  {"jsonrpc":"2.0","method":"metashrew_stateroot","params":[height]}
  ```
  
  The state root is a 32-byte hash that cryptographically represents the entire database state at the specified height. It can be used to verify the integrity of the database and to validate snapshots.

  Example response:
  ```json
  {
    "id": 1,
    "result": "0x7a9c5375e7c6916f525d8d091f9e084e9dd6b1506f1a41d2178de0d53f43c30a",
    "jsonrpc": "2.0"
  }
  ```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

For additional tools and metaprotocols, visit [sandshrewmetaprotocols](https://github.com/sandshrewmetaprotocols).

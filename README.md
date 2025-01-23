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

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

For additional tools and metaprotocols, visit [sandshrewmetaprotocols](https://github.com/sandshrewmetaprotocols).

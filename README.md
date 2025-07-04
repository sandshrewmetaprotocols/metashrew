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
- `--prefixroot`: Defines a named, hex-encoded key prefix for which to track a separate SMT root (e.g., `balances:0x...`). Can be specified multiple times or as a comma-separated list.

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
## Prefix Root State Commitments

Metashrew supports tracking separate Sparse Merkle Tree (SMT) roots for specific key prefixes. This allows for creating state commitments for subsets of the total state, which is useful for protocols that need to verify the integrity of their own data independently.

### Configuration

Use the `--prefixroot` argument to define a named prefix. You can provide multiple arguments or a single comma-separated list.

**Format**: `name:0x<hex_prefix>`

- `name`: A human-readable identifier for the prefix.
- `hex_prefix`: The key prefix, hex-encoded.

**Example**:

```sh
./target/release/rockshrew-mono \
  # ... other args
  --prefixroot "balances:0x62616c616e6365733a" \
  --prefixroot "sequence:0x73657175656e63653a"
```

Or, using a comma-separated list:

```sh
./target/release/rockshrew-mono \
  # ... other args
  --prefixroot "balances:0x62616c616e6365733a,sequence:0x73657175656e63653a"
```

### Querying Prefix Roots

A new JSON-RPC method, `metashrew_prefixroot`, is available to query the SMT root for a configured prefix at a specific block height.

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "metashrew_prefixroot",
  "params": ["<name>", "<height>"]
}
```

- `<name>`: The name of the prefix (e.g., `"balances"`).
- `<height>`: The block height or `"latest"`.

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": "0x<root_hash>"
}
```

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

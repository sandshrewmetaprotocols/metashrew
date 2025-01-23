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
# metashrew

Bitcoin indexer framework with WASM VM.

Metashrew was architected to reduce the problem of building a metaprotocol, or even a lower-level indexer, to architecting an executable that handles solely the logic of processing a single block. The executables follow a portable calling convention and are compatible with AssemblyScript, but can also, themselves, be written in C or Rust, or anything that builds to a WASM target.

Repositories are hosted within [https://github.com/sandshrewmetaprotocols](https://github.com/sandshrewmetaprotocols) with a number of metaprotocols already built to WASM, which can be loaded into metashrew.

## Install

```sh
git clone https://github.com/sandshrewmetaprotocols/metashrew
cd metashrew
cargo build --release -p rockshrew-mono
```

## Usage

The metashrew build will produce a rockshrew-mono binary which can be used alone. Alternatively a rockshrew and rockshrew-view binary can be used separately to multiprocess the view layer as a secondary RPC. As of this writing there are some stability problems when running rockshrew-view as a secondary process on top of a mainnet database for the ALKANES indexer, so it is advised to use rockshrew-mono exclusively which combines the view layer and the indexer process into a single process, which could be parallelized if needed since it will build a consistent database if the same WASM is loaded into the indexer to process blocks.

Usage of rockshrew-mono is given below:

```sh
cd metashrew
./target/release/rockshrew-mono --daemon-rpc-url http://localhost:8332 --auth bitcoinrpc:bitcoinrpc --indexer ~/alkanes-rs/target/wasm32-unknown-unknown/release/alkanes.wasm --db-path ~/.metashrew --host 0.0.0.0 --port 8080
```

This will expose a JSON-RPC on port 8080 and also begin the indexer process against a Bitcoin Core instance running locally on port 8332.

## Runtime

Host functions are made available to the WASM program via the `env` module.

To implement a runtime compatible with metashrew, if the ones within this Rust project, metashrew-view, or metashrew-test are not suitable for your use case, it is required to implement the functions listed below.

- `__log(i32)`
- `__load_input(i32)`
- `__host_len()`
- `__flush(i32)`
- `__get(i32, i32)`
- `__get_len(i32): i32`

All i32 parameters mentioned here MUST represent a pointer to the location in the WebAssembly memory region where memory should be written or read from. All pointers follow the same memory layout as an `ArrayBuffer` type from the AssemblyScript, though it is possible to implement a runtime that can run WASM programs without AssemblyScript. The only requirement is that the byte size of the region that the pointer points to is stored as a `u32` (little-endian) in the 4-byte region immediately preceding the location in memory that the pointer references.

The host function `__host_len(): i32` must return an i32 representing the length of the region that the WASM program should allocate, via any means, in memory, to store the block height (as a little-endian u32) + the serialized block. This function is called, ideally, at the start of the WASM run for each block, immediately before calling `__load_input(i32)` which must be called with a pointer to the region that has been allocated by the WASM program.

For a `__load_input(i32)` implementation, it is required to copy the bytes representing either the view function input (for use with metashrew-view read-only functions for the index) or otherwise a byte vector composed of a little-endian u32 representing the block height of the block being processed, followed by the serialized block.

An example implementation within a WASM program that makes use of the convention to load the program input is available in metashrew-as, but is reproduced below for reference:

```js
function input(): ArrayBuffer {
  const data = new ArrayBuffer(__host_len());
  __load_input(data);
  return data;
}
```

A `__log(i32)` implementation MUST use the host environment to print the bytes (which must be encoded as UTF-8) to stdout. Input is a pointer to an `ArrayBuffer` compatible data structure in WASM memory.

A `__flush(i32)` implementation MUST decode its `ArrayBuffer` compatible input with an RLP decoder and handle a 1-D list of byte vectors, flattened from a list of `[key, value]` pairs to a single `[key, value, key, value, key, value, ...]` list. The host environment MUST store key-value pairs in its database backend such that they can be accessed historically as the WASM program is run again for each new block.

A `__get_len(i32)` implementation MUST return the size of the memory region needed to store the result of a key-value lookup for the key supplied as an argument (as an `ArrayBuffer`)

A `__get(i32, i32)` implementation MUST use the byte vector supplied as its first parameter (as an `ArrayBuffer`) to do a key-value lookup, and the result of the lookup MUST be copied to the region of memory pointed to by the second parameter.

An example implementation of a general purpose `get(key: ArrayBuffer)` function can be seen in the metashrew-as repository, but a trivial implementation that does not perform any caching is reproduced below, to aid understanding of the workflow for a key-value lookup from the WASM program:

```js
function get(k: ArrayBuffer): ArrayBuffer {
  let result = new ArrayBuffer(__get_len(k));
  __get(k, result);
  return result;
}
```

#### AssemblyScript compatibility

For running an AssemblyScript program within a custom runtime, the AssemblyScript runtime can be included in your build but it will be necessary to implement a trivial

`abort(): void`

As a host function. In the Rust implementation of metashrew, it simply calls `panic!("abort");`

If this host function is not included, a runtime will not run the AssemblyScript WASM, which expects it.

## Database

The rocksdb database built by a metashrew process is designed to be append-only, but this detail is abstracted for developers designing a program for metashrew. Under the hood, instead of overwriting a key-value pair, when the same key is written to by a host call from the WASM instance to the `__set` function, a list structure is appended to in the underlying database where the key is annotated by the position in the list maintained for that key, and the value is annotated with the block height at which the value was updated. In this way, it is trivial to rollback updates to the key-value store, by maintaining a list of touched keys for each new block, then in the event of a reorg, those keys can be procedurally rolled back to the block prior to the divergence in the chain, and index updates can be applied for the blocks which are now accepted as valid.

Additionally, this append-only structure makes it possible to write view functions for the index that can run archived state, to check the state of the index at the desired block height.

For specific implementation details, refer to `src/index.rs`

## View Functions

metashrew extends the electrs JSON-RPC (available over a TCP socket) with `metashrew.view`

View functions can be included in the WASM program and exported under any name. It is expected that a view function implementation will use the same convention to read the host input as the `_start` function is expected to when it reads the block height and serialized block.

View functions have the exact same runtime, except the `__flush` function MUST be a no-op, to ensure that the WASM program can be run in read-only mode without side-effects on the database.

The parameters to the RPC call to invoke view functions follow the format:

```js
[functionName, inputAsHex, blockTag]
```

`blockTag` must be either the string `latest` or a block number, represented as hex.

## Author

flex

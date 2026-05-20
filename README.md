# metashrew

Bitcoin indexer framework powered by WebAssembly.

Write a single WASM module that decodes a block and emits key/value writes; metashrew handles the rest — Bitcoin Core block fetch, atomic RocksDB writes, reorg-safe rollbacks, historical state queries at any height, and a JSON-RPC server for view functions.

```
                                   ┌────────────────────────────────┐
   Bitcoin Core ──getblock──▶  fetcher  ──block──▶  block-processor │
                                   │                                │
                                   │                                ▼
                                   │                           wasmi/wasmtime
                                   │                           runs YOUR
                                   │                           indexer.wasm
                                   │                                │
                                   │                          __flush(K/V)
                                   │                                │
                                   ▼                                ▼
                              metashrew_view                   RocksDB
                              JSON-RPC ◀──read history──▶  (versioned chains
                                                            per key, height-tagged)
```

The runtime is a thin shim: it loads a wasm binary, calls `_start()` for each block, traps host calls (`__host_len`, `__load_input`, `__get`, `__get_len`, `__flush`, `__log`) into Rust, and commits the emitted batch atomically. View functions exported by the wasm module are callable over JSON-RPC and read the same chain-versioned KV store at any historical height.

The reference metaprotocol built on metashrew is [alkanes-rs](https://github.com/kungfuflex/alkanes-rs) (ALKANES).

## Table of contents

| Document | What it covers |
|----------|----------------|
| [docs/SPECIFICATION.md](docs/SPECIFICATION.md) | Full technical spec — WASM ABI, host functions, KV storage model, view-function semantics, sync framework |
| [docs/REORG_ROLLBACK_FIX.md](docs/REORG_ROLLBACK_FIX.md) | How chain reorgs are detected + reverted (per-block manifest + rollback by deleting future-height chain entries) |
| [docs/MEMORY_NONDETERMINISM_BUG.md](docs/MEMORY_NONDETERMINISM_BUG.md) | Consensus-critical: why WASM memory limits must be identical across nodes (and how to verify) |
| [docs/WASMTIME_UPGRADE.md](docs/WASMTIME_UPGRADE.md) | Background on the wasmtime 18 → 43 upgrade path (relevant if integrating WASIP2 component-model programs alongside metashrew) |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Build setup, project structure, PR process |

## Versions

- **v10.x.y** (current): binary versioned-chain entry format (`[u32 LE height | value_bytes]`), chunked outpoint storage API on `KeyValuePointer`, WAL-off gating during initial sync (`bitcoind_tip - indexer_tip > SYNC_WAL_OFF_THRESHOLD`), RocksDB CF tuning (L0–L2 uncompressed), view-syscall infrastructure (`__flush` dispatcher for view-side LRU cache + thread spawn/join). Default branch: `kungfuflex/v10.0.0-alpha.1`.
- **v9.x.y**: previous stable; UTF-8 hex-string K/V entry format. Not forward-compatible — v10 indexers re-sync from genesis with a clean RocksDB.

## Prerequisites

- Rust stable toolchain
- Bitcoin Core (v22.0+) with `rpcuser`/`rpcpassword` configured
- For WASM indexer development: `rustup target add wasm32-unknown-unknown`

## Install

```sh
git clone https://github.com/kungfuflex/metashrew
cd metashrew
cargo build --release -p rockshrew-mono
```

Produces `target/release/rockshrew-mono` — combined indexer + JSON-RPC view server.

## Run

```sh
./target/release/rockshrew-mono \
  --daemon-rpc-url http://localhost:8332 \
  --auth bitcoinrpc:password \
  --indexer path/to/indexer.wasm \
  --db-path ~/.metashrew \
  --host 0.0.0.0 \
  --port 8080
```

Flags (full list via `--help`):

| Flag | Purpose |
|------|---------|
| `--daemon-rpc-url` | Bitcoin Core RPC endpoint |
| `--auth` | RPC credentials `user:password` |
| `--indexer` | Path to your `.wasm` indexer |
| `--db-path` | RocksDB directory |
| `--start-block` | Override starting block height |
| `--exit-at` | Stop at a block height (testing) |
| `--host` / `--port` | JSON-RPC bind |
| `--label` | Multi-instance namespacing on the same disk |

## Query indexed state

```sh
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"metashrew_view",
       "params":["viewFunctionName","0xhexInput","latest"]}'
```

Built-in methods: `metashrew_view`, `metashrew_height`. View functions are arbitrary exports from the indexer wasm — `params[0]` is the export name, `params[1]` is the input bytes (hex), `params[2]` is the height (`"latest"` or a decimal string).

## rockshrew-diff: side-by-side indexer comparison

Run two WASM modules over the same block range and diff their KV writes by prefix. Useful for verifying upgrades don't introduce state divergence:

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

Differences print a per-key diff and exit non-zero. No diff → continues to the next block. The example prefix `0x2f72756e65732f70726f746f2f312f62796f7574706f696e742f` is `/runes/proto/1/byoutpoint/` — the ALKANES outpoint-indexed balance subtree.

Flags:

| Flag | Purpose |
|------|---------|
| `--indexer` | Primary WASM module |
| `--compare` | Comparison WASM module |
| `--prefix` | Hex-encoded key prefix to diff (must start with `0x`) |
| `--start-block` | Block height to start at |
| `--pipeline-size` | Parallel block prefetch depth (default 5) |

## WASM indexer ABI

A minimal indexer needs to handle two callbacks: `_start` (called once per block) plus zero or more view functions.

### Host functions imported by the wasm

```text
__host_len(): i32                      // length of input (height-prefixed serialized block)
__load_input(ptr: i32): void           // copy input into wasm memory at ptr
__log(ptr: i32): void                  // utf-8 logging
__get_len(key_ptr: i32): i32           // length of value for key
__get(key_ptr: i32, value_ptr: i32)    // copy value bytes into wasm memory
__flush(ptr: i32): void                // commit a serialized KV batch
```

Pointers follow AssemblyScript ArrayBuffer layout: 4 bytes of little-endian u32 length immediately followed by data bytes.

### Required wasm exports

- `_start()` — main indexing function. Reads `__host_len()` bytes via `__load_input`. The input is `[u32 LE: block_height | serialized_block_bytes]`. Process the block, accumulate KV writes in a `KeyValueFlush` protobuf, and call `__flush(ptr_to_serialized_flush)` exactly once.
- View functions (any name) — receive function-specific input via `__load_input`, return output by calling `__flush` with the response wrapped in a `KeyValueFlush` where `list[0]` is the response bytes (convention; specific view ABIs vary by indexer).

### Minimal Rust skeleton

```rust
use metashrew_core::{input, flush, get, set};

#[no_mangle]
pub extern "C" fn _start() {
    let raw = input();
    let height = u32::from_le_bytes(raw[..4].try_into().unwrap());
    let block = &raw[4..];
    // ... decode block, update KV via set(key, value) ...
    flush();
}

#[no_mangle]
pub extern "C" fn my_view() {
    // ... read state via get(key), serialize response ...
    flush_response(response_bytes);
}
```

See [alkanes-rs](https://github.com/kungfuflex/alkanes-rs) for a complete production indexer.

## Storage model

Every KV write the indexer emits at height `H` for key `K` becomes a new entry in a per-key versioned chain:

```text
{K}/length         -> u32 LE (count of entries in K's chain)
{K}/0              -> [u32 LE: H_0 | value_at_H_0]
{K}/1              -> [u32 LE: H_1 | value_at_H_1]
...
```

Reads at height `H` binary-search the chain for the largest stored height `≤ H` and return that value. This gives O(log N) historical reads with no separate state-at-height snapshot cost.

Rollbacks on reorg are O(keys touched in the reorged range) — read the per-height manifest at `/__INTERNAL/keys-at-height/{height}`, walk each key's chain, drop entries with height in the reorged range. Full details in [docs/REORG_ROLLBACK_FIX.md](docs/REORG_ROLLBACK_FIX.md).

## v10 changes worth knowing

- **Binary K/V entries**: replaces v9's `"{height}:{hex_value}"` UTF-8 strings. No 2× hex expansion on every stored value, no string parsing on read. Same chain structure as v9 (length key + indexed entry keys), so binary search semantics are unchanged.
- **Chunked outpoint API**: `KeyValuePointer::set_chunk` / `get_chunk` / `get_chunk_at_height` lets indexers store a whole multi-entry record under one chain key (the alkanes v3 OUTPOINT_TO_RUNES uses this for balance sheets with `spent_at_height` markers).
- **WAL-off during catch-up**: when `bitcoind_tip - indexer_tip > SYNC_WAL_OFF_THRESHOLD`, `commit_atomic` switches to `no_wal_write_options()` and force-flushes every `SYNC_WAL_OFF_FLUSH_INTERVAL` blocks. Atomicity is preserved (WriteBatch is still all-or-nothing within a single `write_opt`); only durability is relaxed during the bulk-sync window. Auto re-enables when close to tip.
- **View syscalls via `__flush` dispatcher** (feature `view-syscalls`): the `__flush` host function in view mode is now a discriminator-style dispatcher for `ViewSyscall` protobuf payloads — `CacheGet`/`CachePut` against a host-side LRU cache, and `ThreadSpawn`/`ThreadJoin` for parallel-view fanout. No wasm ABI change; legacy `__flush` callers still work. Indexer engine has `wasm_threads(false)` (consensus-critical); view engine has `wasm_threads(true)`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT

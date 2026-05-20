# metashrew docs

Long-form documentation. The repo [README](../README.md) is the entry point; this directory has the deeper material.

## Index

### Core reference

- **[SPECIFICATION.md](SPECIFICATION.md)** — full technical specification. WASM ABI, host functions, KV storage model (versioned per-key chains), view function semantics, sync framework (`StorageAdapter` / `NodeAdapter` traits), RocksDB layout. Start here if you're building a new indexer or integrating metashrew into a different storage backend.

### Operational notes

- **[REORG_ROLLBACK_FIX.md](REORG_ROLLBACK_FIX.md)** — how chain reorgs are detected and reverted. Per-height manifest at `/__INTERNAL/keys-at-height/{height}` + rollback by deleting future-height chain entries. Read this if you're touching reorg paths or debugging rollback divergence.

- **[MEMORY_NONDETERMINISM_BUG.md](MEMORY_NONDETERMINISM_BUG.md)** — **consensus-critical**. WASM memory limits MUST be identical across all nodes running the same indexer; mismatched limits produce different execution outcomes for the same input, breaking consensus. Read before changing `--memory-limit` or running mixed-config pools.

- **[WASMTIME_UPGRADE.md](WASMTIME_UPGRADE.md)** — background on the wasmtime 18 → 43 upgrade path. Relevant if integrating WASIP2 component-model programs alongside the metashrew runtime (the two need compatible wasmtime versions in the same process).

## Other places to look

- [`CONTRIBUTING.md`](../CONTRIBUTING.md) — build setup, project structure (`crates/`), PR process
- [`memory-bank/`](../memory-bank/) — internal architecture/context notes (older, AI-generated, may lag the v10 changes)
- [`crates/rockshrew-mono/ROLLBACK.md`](../crates/rockshrew-mono/ROLLBACK.md) — rockshrew-specific rollback semantics (per-binary state, label namespacing)
- [alkanes-rs](https://github.com/kungfuflex/alkanes-rs) — reference indexer built on metashrew (the ALKANES metaprotocol)

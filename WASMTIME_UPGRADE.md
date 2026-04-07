# Wasmtime Upgrade: 18.0.4 → 43.0.0

## Why

The subzero-rs workspace uses wasmtime 43 for WASIP2 component model support (signal programs). metashrew-runtime uses wasmtime 18. When both are in the same dependency tree (subzero-devnet's `indexer` feature), two incompatible wasmtime versions coexist and `Engine` types can't cross the boundary.

This blocks the fully in-process e2e test harness where signal programs + alkanes indexer run in the same process.

## Current State

| Crate | wasmtime version |
|-------|-----------------|
| `metashrew` workspace | 18.0.4 |
| `metashrew-runtime` | 18.0.4 |
| `subzero-rs` workspace | 43.0.0 |
| Latest on crates.io | 43.0.0 |

## Breaking API Changes: 18 → 43

### 1. Memory Configuration (Config)

```rust
// OLD (wasmtime 18)
config.static_memory_maximum_size(0x100000000); // 4GB
config.static_memory_guard_size(0x10000);       // 64KB

// NEW (wasmtime 43)
config.memory_reservation(0x100000000);         // 4GB
config.memory_guard_size(0x10000);              // 64KB
```

Same semantics, just renamed. `static_memory_maximum_size` → `memory_reservation`, `static_memory_guard_size` → `memory_guard_size`.

### 2. Everything Else — No Changes Required

These APIs are **stable** between 18 and 43 (same signatures):

| API | Status |
|-----|--------|
| `Config::consume_fuel(bool)` | ✅ Same |
| `Config::cranelift_nan_canonicalization(bool)` | ✅ Same |
| `Config::memory_init_cow(bool)` | ✅ Same |
| `Config::async_support(bool)` | ✅ Same |
| `Config::relaxed_simd_deterministic(bool)` | ✅ Same |
| `Engine::new(&Config)` | ✅ Same |
| `Module::new(&Engine, &[u8])` | ✅ Same |
| `Module::from_file(&Engine, path)` | ✅ Same |
| `Store::new(&Engine, state)` | ✅ Same |
| `Store::limiter(fn)` | ✅ Same |
| `Linker::new(&Engine)` | ✅ Same |
| `Linker::func_wrap(module, name, fn)` | ✅ Same |
| `Linker::define_unknown_imports_as_traps(&Module)` | ✅ Same |
| `Linker::instantiate_async(&mut Store, &Module)` | ✅ Same |
| `Caller<'_, T>` | ✅ Same |
| `Memory` / `Memory::data()` / `Memory::grow()` | ✅ Same |
| `StoreLimits` / `StoreLimitsBuilder` | ✅ Same |

### 3. Summary of Required Changes

In `metashrew-runtime/src/runtime.rs`, change these two lines (appears twice — in `load()` and `new()`):

```diff
- config.static_memory_maximum_size(0x100000000);
+ config.memory_reservation(0x100000000);

- config.static_memory_guard_size(0x10000);
+ config.memory_guard_size(0x10000);
```

In `Cargo.toml` (workspace root):
```diff
- wasmtime = "18.0.4"
+ wasmtime = "43"
```

In `metashrew-runtime/Cargo.toml`:
```diff
- wasmtime = "18.0.4"
+ wasmtime = "43"
```

## Verification

After upgrading, run:
```bash
cargo test
cargo test -p metashrew-runtime
```

The view functions (`view`, `preview`, `preview_async`), block processing (`process_block`), and host function linking should all work without changes. Only the two `Config` method renames need updating.

## Notes

- wasmtime 43 is the current latest stable (as of April 2026)
- The upgrade adds WASIP2 component model support which metashrew doesn't need but doesn't hurt
- No WASM binary format changes — existing compiled `.wasm` indexers work as-is
- If you want to pin to a specific minor: `wasmtime = "43.0.0"`

# Memory Pre-allocation Test Findings

## Summary

**CRITICAL FINDING**: The metashrew-runtime does NOT actually pre-allocate the full 4GB WASM32 memory space on instantiation. It only sets a maximum limit, allowing memory to grow on-demand.

## Test Results

### Test 1: Full Memory Stress Test (Dockerfile.final)

**High memory environment (4GB)**:
- ✅ Runtime instantiates successfully
- ✅ Test executes and completes
- Result: WASM stores "SUCCESS:50MB"

**Low memory environment (512MB)**:
- ✅ Runtime instantiates successfully (!)
- ❌ Process killed during execution (exit code 137 - OOM killer)
- Result: Process terminated at "Starting block processing for height 1"

### Test 2: Minimal Instantiation Test (Dockerfile.preallocate)

**High memory environment (4GB)**:
- ✅ Runtime instantiates successfully
- ✅ Test completes: "Memory pre-allocation succeeded"

**Low memory environment (512MB)**:
- ✅ Runtime instantiates successfully (!)
- ✅ Test completes: "Memory pre-allocation succeeded"
- **This should NOT have succeeded if pre-allocation was working**

## Analysis

### Current Configuration

In `/data/metashrew/crates/metashrew-runtime/src/runtime.rs:494-498`:

```rust
// Allocate memory at maximum size to avoid non-deterministic memory growth
config.static_memory_maximum_size(0x100000000); // 4GB max memory
config.static_memory_guard_size(0x10000); // 64KB guard
// Pre-allocate memory to maximum size
config.memory_init_cow(false); // Disable copy-on-write
```

**What the comments claim**: "Pre-allocate memory to maximum size"
**What actually happens**: Sets a maximum limit, but does NOT pre-allocate

### Wasmtime Behavior

`static_memory_maximum_size(0x100000000)` does the following:
- Sets the **maximum** size that WASM memory can grow to
- Uses virtual memory with demand paging (pages are only committed when accessed)
- Does NOT allocate physical memory upfront

This is why:
1. The runtime can instantiate successfully in a 512MB environment
2. The process only gets killed when it actually tries to USE more memory than available
3. Memory allocation is non-deterministic (depends on host availability at runtime)

## Implications

### Non-Deterministic Execution Risk

Since memory is not pre-allocated:
1. Two identical WASM executions can have different memory availability
2. A WASM module might succeed on one host and fail on another
3. This violates the determinism requirement for blockchain indexers

### Current Failure Mode

- **When**: During execution, when memory is actually accessed
- **How**: OS OOM killer terminates the process (exit code 137)
- **Problem**: This is late detection - the runtime has already started processing

### Required Behavior

- **When**: During runtime instantiation, before any WASM execution
- **How**: Runtime should panic/error if it cannot secure 4GB upfront
- **Benefit**: Early detection - fail fast before processing begins

## Solutions

### Option 1: Force Memory Commit After Instantiation

Add code to touch every page of the 4GB memory space immediately after instantiation:

```rust
// After instantiating the WASM module
// Touch every page to force OS to commit physical memory
let memory = instance.get_memory(&mut store, "memory")?;
let page_size = 65536; // 64KB pages
for page in 0..(0x100000000 / page_size) {
    let offset = page * page_size;
    memory.write(&mut store, offset, &[0])?;
}
```

**Pros**:
- Simple to implement
- Works with existing WASM modules

**Cons**:
- Adds startup time (need to write 64K times)
- May not work on all OS configurations

### Option 2: Configure WASM Module Memory Requirements

Modify WASM compilation to set `minimum = maximum = 65536 pages` (4GB):

In Cargo.toml or build configuration:
```toml
[target.wasm32-unknown-unknown]
rustflags = ["-C", "link-args=--initial-memory=4294967296 --max-memory=4294967296"]
```

**Pros**:
- Memory requirement declared in WASM module itself
- More portable/standard

**Cons**:
- Requires recompiling all WASM modules
- May not be supported by all toolchains

### Option 3: Use Memory Guard/Allocation Test

Add a function that attempts to allocate a large chunk immediately after instantiation:

```rust
// Call a test function that allocates memory
let test_fn = instance.get_typed_func::<(), ()>(&mut store, "memory_test")?;
test_fn.call(&mut store, ())?;
```

Where `memory_test` in WASM:
```rust
#[no_mangle]
pub extern "C" fn memory_test() {
    // Attempt to allocate and touch a large memory region
    // This will fail if host doesn't have 4GB available
}
```

**Pros**:
- Explicit test of memory availability
- Can provide clear error messages

**Cons**:
- Requires adding test function to all WASM modules
- May not catch all memory allocation failures

## Recommendation

Implement **Option 1** (force memory commit) as it:
1. Requires no changes to WASM modules
2. Provides immediate verification at startup
3. Fails fast in memory-constrained environments
4. Ensures deterministic memory availability

The implementation should:
- Happen immediately after WASM instantiation
- Touch memory in a way that forces OS page commits
- Provide clear error messages when it fails
- Be documented as a determinism requirement

## Test Files Created

1. `/data/metashrew/tests/memory/Dockerfile.final` - Full stress test
2. `/data/metashrew/tests/memory/Dockerfile.preallocate` - Minimal instantiation test
3. `/data/metashrew/src/tests/memory_determinism_test.rs` - Memory stress test suite
4. `/data/metashrew/src/tests/memory_preallocate_test.rs` - Instantiation test suite
5. `/data/metashrew/crates/metashrew-memory-stress/` - WASM stress test module

## Commands to Reproduce

### Build and run full stress test:
```bash
cd /data/metashrew/tests/memory
docker build -f Dockerfile.final -t metashrew-memory-test:final ../..

# High memory (should succeed)
docker run --rm --memory=4g metashrew-memory-test:final

# Low memory (gets killed during execution)
docker run --rm --memory=512m --memory-swap=512m metashrew-memory-test:final
```

### Build and run instantiation test:
```bash
cd /data/metashrew/tests/memory
docker build -f Dockerfile.preallocate -t metashrew-preallocate-test ../..

# High memory (succeeds)
docker run --rm --memory=4g metashrew-preallocate-test

# Low memory (also succeeds - proves no pre-allocation!)
docker run --rm --memory=512m --memory-swap=512m metashrew-preallocate-test
```

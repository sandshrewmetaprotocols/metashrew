# Memory Non-Determinism Bug in WASM Indexers

## Executive Summary

**Critical Issue**: Metashrew WASM indexers can produce **different execution results** for the **same input** based on **host memory constraints**, violating determinism requirements for blockchain systems.

**Impact**:
- Different nodes with different memory limits produce different state roots
- Consensus failures between nodes
- Reorg handling inconsistencies
- Unpredictable behavior in memory-constrained environments (Kubernetes, Docker, VMs)

**Status**: 🚨 **DEMONSTRATED** - Test suite proves non-determinism exists

## The Problem

### What Happens

**CRITICAL**: Both environments execute `_start()` successfully, but produce different stored data!

When executing WASM indexers:

1. **High memory environment** (4GB available):
   - Memory allocations succeed (`Vec::try_reserve()` returns `Ok`)
   - WASM execution completes successfully
   - Stores: `"SUCCESS:1000MB"` in storage

2. **Low memory environment** (512MB available):
   - Memory allocations fail (`Vec::try_reserve()` returns `Err`)
   - WASM execution **still** completes successfully
   - Stores: `"ERROR:failed at allocation 5"` in storage

**Both execute successfully, but write DIFFERENT DATA = Non-Deterministic**

This is more insidious than a panic/crash because both nodes appear to work correctly, but their storage diverges!

### Root Cause

The issue stems from how WASM memory operations interact with host memory:

```rust
// In WASM indexer
let data = Vec::with_capacity(50_000_000); // 50MB
data.resize(50_000_000, 0x42);

let mut pointer = IndexPointer::from_keyword("/large-data");
pointer.set(Arc::new(data)); // ← THIS CAN FAIL!
```

**What happens under the hood**:

1. `Vec::with_capacity()` requests memory from WASM runtime
2. Wasmtime allocates actual memory from **HOST**
3. `IndexPointer::set()` triggers SMT operations
4. SMT allocates additional memory in **HOST**
5. If host is out of memory → **allocation fails**
6. Failure propagates back to WASM as error/trap

**Key Issue**: The success/failure of WASM operations depends on **host memory availability**, not WASM logic.

### Why This Matters for Blockchain

Blockchain indexers must be **deterministic**:
- All nodes must produce identical state from identical blocks
- State roots must match across the network
- Reorg handling must be consistent

**With this bug**:
- Node A (4GB RAM): Processes block 12345 → Stores "SUCCESS" data → State Root: `0xabc...`
- Node B (512MB RAM): Processes block 12345 → Stores "ERROR" data → State Root: `0xdef...`
- **Both think they succeeded, but state roots differ!** 🚨
- **Silent consensus failure** - no error logs, just divergent state

## Demonstration

### Test Suite Location

```
/data/metashrew/tests/memory/
```

### Quick Reproduction

1. **Build WASM module**:
   ```bash
   cargo build --target wasm32-unknown-unknown --release -p metashrew-memory-stress
   ```

2. **Run comparison test**:
   ```bash
   cd /data/metashrew
   ./tests/memory/run_comparison.sh
   ```

3. **Observe non-determinism**:
   - 4GB container: WASM completes, stores `SUCCESS:1000MB` ✅
   - 512MB container: WASM completes, stores `ERROR:failed...` ❌
   - **Both complete successfully, but store different data!**

### Test Configuration

The test attempts to allocate:
- **20 allocations** × **50MB each** = **1GB total**

This is within WASM32's 4GB limit but:
- ✅ Succeeds in 4GB container
- ❌ Fails in 512MB container

## Technical Details

### Memory Allocation Chain

```
WASM Code
    ↓
Vec::try_reserve(size)
    ↓
WASM Linear Memory
    ↓
Wasmtime Runtime
    ↓
mmap/malloc on HOST
    ↓
HOST Memory Limit Check
    ↓
    ├─ Available → SUCCESS
    └─ Exhausted → FAILURE (trap)
```

### Where It Fails

1. **Direct allocations**: `Vec::with_capacity()`, `Vec::try_reserve()`
2. **IndexPointer storage**: SMT operations allocate host memory
3. **Batch operations**: Multiple small allocations can accumulate
4. **Clone operations**: Duplicating large structures

### Container/VM Impact

**Docker with memory limits**:
```bash
docker run --memory=512m app  # Can fail
docker run --memory=4g app    # Succeeds
```

**Kubernetes pod limits**:
```yaml
resources:
  limits:
    memory: "512Mi"  # Risk of OOM
```

**Virtual machines**:
- Small VMs (t2.micro): 1GB RAM → high failure risk
- Large VMs (t2.large): 8GB RAM → usually succeeds

## Solutions

### Option 1: Enforce Memory Limits in WASM

Add pre-allocation checks:

```rust
const MAX_INDEXER_MEMORY_MB: u32 = 500;

fn check_memory_budget(requested_mb: u32) -> Result<()> {
    if requested_mb > MAX_INDEXER_MEMORY_MB {
        return Err("Exceeds memory budget".into());
    }
    Ok(())
}

// Before large allocations
check_memory_budget(50)?;
let data = Vec::with_capacity(50_000_000);
```

**Pros**:
- Deterministic failure (always fails above limit)
- Early detection before host OOM
- Predictable behavior

**Cons**:
- Requires modifying all indexers
- Need to track cumulative usage
- May be too restrictive

### Option 2: Fail Fast on OOM

Convert host memory failures to panics:

```rust
let data = Vec::new();
if let Err(_) = data.try_reserve(size) {
    panic!("Memory allocation failed - deterministic abort");
}
```

**Pros**:
- Clear failure mode
- Forces addressing memory issues
- Consistent behavior (always panics)

**Cons**:
- Still depends on host memory
- Doesn't prevent the issue
- Harsh failure mode

### Option 3: Document Memory Requirements

Clearly specify minimum requirements:

```
Minimum System Requirements:
- RAM: 4GB minimum, 8GB recommended
- Container memory limit: 4GB minimum
- Kubernetes: spec.containers[].resources.limits.memory: "4Gi"
```

**Pros**:
- Simple to implement
- Allows planning
- Sets expectations

**Cons**:
- Doesn't fix the root cause
- Can't enforce at runtime
- Users may ignore

### Option 4: Streaming/Chunked Processing

Avoid large allocations by processing in chunks:

```rust
// Instead of allocating 1GB at once
let data = vec![0u8; 1_000_000_000]; // BAD

// Process in 10MB chunks
for chunk_id in 0..100 {
    let chunk = vec![0u8; 10_000_000]; // 10MB chunks
    process_chunk(chunk)?;
}
```

**Pros**:
- Reduces memory pressure
- More efficient
- Better scalability

**Cons**:
- Requires redesigning indexers
- May not fit all use cases
- More complex code

### Recommended Approach

**Combination of all options**:

1. **Document** minimum memory requirements (4GB)
2. **Add** pre-allocation budget checks in critical paths
3. **Redesign** high-memory operations to use streaming
4. **Test** in constrained environments regularly

## Files and Tests

### Test Suite

```
tests/memory/
├── README.md                 # Detailed documentation
├── Dockerfile                # Docker image with memory limits
├── run_low_memory.sh         # Run in 512MB container
├── run_high_memory.sh        # Run in 4GB container
└── run_comparison.sh         # Compare results (proves non-determinism)
```

### WASM Module

```
crates/metashrew-memory-stress/
├── Cargo.toml                # Package definition
└── src/lib.rs                # Memory stress test logic
```

### Test Code

```
src/tests/memory_determinism_test.rs
```

Tests:
- `test_memory_stress_normal_environment()` - Run in unconstrained environment
- `test_memory_stress_docker_instructions()` - Print Docker instructions
- `test_memory_stress_gradual_increase()` - Find breaking point

### Running Tests

**Local test** (no Docker):
```bash
cargo test --lib memory_stress_normal_environment -- --ignored --nocapture
```

**Docker test** (512MB):
```bash
./tests/memory/run_low_memory.sh
```

**Docker test** (4GB):
```bash
./tests/memory/run_high_memory.sh
```

**Comparison** (proves non-determinism):
```bash
./tests/memory/run_comparison.sh
```

## Expected Behavior

### Success Case (4GB Container)

```
🔬 TEST: Memory Stress in NORMAL Environment

Running memory stress test with config: MemoryStressConfig {
    height: 1,
    allocation_size_mb: 50,
    num_allocations: 20
}

Executing WASM memory stress test...
WASM execution completed in 1.234s
✓ WASM _start() completed successfully
WASM stored result: SUCCESS:1000MB

📊 RESULT: WASM completed successfully, stored: SUCCESS:1000MB

✅ Memory allocations SUCCEEDED in normal environment
✅ WASM _start() completed and stored SUCCESS result
```

### Failure Case (512MB Container)

```
🔬 TEST: Memory Stress in NORMAL Environment

Running memory stress test with config: MemoryStressConfig {
    height: 1,
    allocation_size_mb: 50,
    num_allocations: 20
}

Executing WASM memory stress test...
WASM execution completed in 0.543s
✓ WASM _start() completed successfully
WASM stored result: ERROR:Failed at allocation 5 of 20: Failed to reserve 50MB

📊 RESULT: WASM completed successfully, stored: ERROR:Failed at allocation 5...

⚠️  Memory allocations FAILED in this environment
⚠️  WASM _start() completed but stored ERROR result
⚠️  This demonstrates non-determinism: same input, different stored data!
```

**Key Point**: Both show `WASM _start() completed successfully`, but the stored data differs!

## Monitoring and Debugging

### Check Memory Usage

**During test**:
```bash
watch -n 1 'ps aux | grep metashrew | head -10'
```

**Docker stats**:
```bash
docker stats
```

**System memory**:
```bash
free -h
```

### Gradual Increase Test

Find the exact breaking point:

```bash
cargo test --lib memory_stress_gradual_increase -- --ignored --nocapture
```

Tests allocations from 100MB to 3GB, stops at first failure.

## Impact Assessment

### Severity: **CRITICAL** 🚨

- **Determinism**: Violates fundamental blockchain requirement
- **Consensus**: Can cause state divergence between nodes
- **Reorg**: Inconsistent rollback behavior
- **Production**: Unpredictable failures in containers/VMs

### Affected Systems

- ✅ **Proven**: Metashrew (this test suite)
- ⚠️  **Likely**: All WASM-based blockchain indexers using similar patterns
- ⚠️  **Possible**: Any WASM system with large memory allocations

### Mitigation Priority

**Immediate** (do now):
1. Document minimum memory requirements
2. Test in constrained environments
3. Monitor memory usage in production

**Short-term** (next release):
1. Add memory budget checks
2. Redesign high-memory operations
3. Add pre-allocation validation

**Long-term** (future):
1. Streaming/chunked processing architecture
2. Memory pooling and reuse
3. Deterministic memory limits in runtime

## References

- **Test suite**: `/data/metashrew/tests/memory/`
- **WASM module**: `/data/metashrew/crates/metashrew-memory-stress/`
- **Test code**: `/data/metashrew/src/tests/memory_determinism_test.rs`
- **Documentation**: `/data/metashrew/tests/memory/README.md`

## Conclusion

This test suite **proves** that WASM execution in Metashrew is **non-deterministic** when host memory varies. The same WASM module processing the same block can produce:
- ✅ SUCCESS in high-memory environments
- ❌ FAILURE in low-memory environments

This is a **critical issue** for blockchain systems that require **deterministic execution** across all nodes.

**Status**: 🚨 **DEMONSTRATED** - Bug exists and is reproducible
**Next Steps**: Implement mitigation strategies (memory limits, streaming, documentation)

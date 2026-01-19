# Memory Non-Determinism Test Suite

## Overview

This test suite demonstrates a **critical non-determinism bug** in WASM-based blockchain indexers: execution can produce different **stored data** based on **host memory constraints**.

### The Problem

**CRITICAL**: Both environments execute `_start()` successfully, but write different data!

When running WASM indexers in memory-constrained environments (like Kubernetes pods with memory limits), the **same WASM module with the same input can store different results**:

- **High memory environment (4GB)**: Allocations succeed → stores `"SUCCESS:1000MB"`
- **Low memory environment (512MB)**: Allocations fail → stores `"ERROR:failed at allocation 5"`

**Both WASM executions complete successfully**, but they produce **different storage state**.

This is more insidious than a crash because both nodes appear healthy, but their data diverges! This violates a fundamental requirement for blockchain systems: **deterministic execution**.

## How It Works

### Test Components

1. **WASM Module**: `metashrew-memory-stress` (`crates/metashrew-memory-stress/`)
   - Attempts to allocate large amounts of memory (configurable)
   - Stores allocations via `IndexPointer` (triggers host memory allocation)
   - Reports success or failure based on allocation results

2. **Test Suite**: `memory_determinism_test.rs` (`src/tests/`)
   - Loads and executes the WASM module
   - Tests with different memory allocation configurations
   - Documents whether allocations succeed or fail

3. **Docker Infrastructure**: `tests/memory/`
   - Dockerfile for reproducible test environment
   - Scripts to run tests with different memory limits
   - Comparison scripts to demonstrate non-determinism

### Memory Allocation Flow

```
WASM Module
    ↓
Attempts allocation: Vec::try_reserve(50MB)
    ↓
WASM runtime (Wasmtime)
    ↓
Requests memory from HOST
    ↓
HOST checks available memory
    ↓
    ├─ If sufficient: SUCCESS
    └─ If exhausted: FAILURE
```

The key issue: **HOST memory availability** determines execution outcome, not WASM logic.

## Running the Tests

### Prerequisites

- Docker installed and running
- Sufficient disk space (~2GB for Docker image)
- At least 4GB of RAM for high-memory tests

### Quick Start: Compare Results

Run both high and low memory tests to see non-determinism:

```bash
cd /data/metashrew
./tests/memory/run_comparison.sh
```

This will:
1. Build the Docker image
2. Run test in 4GB container (should succeed)
3. Run test in 512MB container (should fail)
4. Compare results and report non-determinism

### Individual Tests

#### High Memory Test (4GB)

Expected: **SUCCESS** - Memory allocations complete

```bash
cd /data/metashrew
./tests/memory/run_high_memory.sh
```

Look for output like:
```
📊 RESULT: Host: SUCCESS (WASM succeeded), SUCCESS:1000MB
✅ Memory allocations SUCCEEDED in normal environment
```

#### Low Memory Test (512MB)

Expected: **FAILURE** - Host memory exhausted

```bash
cd /data/metashrew
./tests/memory/run_low_memory.sh
```

Look for output like:
```
📊 RESULT: Host: FAILURE (host memory exhausted), error: ...
❌ Memory allocations FAILED even in normal environment
⚠️  This may indicate the host has very limited memory
```

### Local Testing (Without Docker)

Run the test directly on your machine:

```bash
# Build WASM module
cargo build --target wasm32-unknown-unknown --release -p metashrew-memory-stress

# Run test
cargo test --lib memory_stress_normal_environment -- --ignored --nocapture
```

For gradual memory increase test:

```bash
cargo test --lib memory_stress_gradual_increase -- --ignored --nocapture
```

## Test Configurations

### Default Configuration

- **Allocation size**: 50MB per allocation
- **Number of allocations**: 20
- **Total memory**: 1GB

Adjust in the test by modifying `MemoryStressConfig`:

```rust
let config = MemoryStressConfig {
    height: 1,
    allocation_size_mb: 50,  // MB per allocation
    num_allocations: 20,      // Number of allocations
};
```

### Gradual Increase Test

The `test_memory_stress_gradual_increase` test finds the breaking point for your environment:

```bash
cargo test --lib memory_stress_gradual_increase -- --ignored --nocapture
```

It tests:
- 100MB total (10 × 10MB)
- 500MB total (50 × 10MB)
- 1GB total (100 × 10MB)
- 2GB total (100 × 20MB)
- 3GB total (100 × 30MB)

Stops at first failure.

## Understanding the Results

### Success Output (High Memory)

```
🔬 TEST: Memory Stress in NORMAL Environment

Running memory stress test with config: MemoryStressConfig { height: 1, allocation_size_mb: 50, num_allocations: 20 }
Executing WASM memory stress test...
WASM execution completed in 1.234s
✓ WASM _start() completed successfully
WASM stored result: SUCCESS:1000MB

📊 RESULT: WASM completed successfully, stored: SUCCESS:1000MB

✅ Memory allocations SUCCEEDED in normal environment
✅ WASM _start() completed and stored SUCCESS result
```

**Interpretation**: Host had sufficient memory, all allocations completed, SUCCESS data stored.

### Failure Output (Low Memory)

```
🔬 TEST: Memory Stress in NORMAL Environment

Running memory stress test with config: MemoryStressConfig { height: 1, allocation_size_mb: 50, num_allocations: 20 }
Executing WASM memory stress test...
WASM execution completed in 0.543s
✓ WASM _start() completed successfully
WASM stored result: ERROR:Failed at allocation 5 of 20: Failed to reserve 50MB

📊 RESULT: WASM completed successfully, stored: ERROR:Failed at allocation 5...

⚠️  Memory allocations FAILED in this environment
⚠️  WASM _start() completed but stored ERROR result
⚠️  This demonstrates non-determinism: same input, different stored data!
```

**Interpretation**: Host ran out of memory, allocations failed, but WASM still completed successfully and stored ERROR data.

### Non-Determinism Evidence

When comparing results from different environments:

| Environment | WASM Execution | Stored Data | Deterministic? |
|-------------|----------------|-------------|----------------|
| 4GB container | ✅ Completed | `SUCCESS:1000MB` | ❌ No - depends on host |
| 512MB container | ✅ Completed | `ERROR:failed...` | ❌ No - depends on host |
| **Same input** | **Both succeed** | **DIFFERENT** | **🚨 NON-DETERMINISTIC** |

**The Bug**: Both executions succeed, but storage state diverges!

## Technical Details

### Why This Happens

1. **WASM Memory Model**: WASM32 has a 4GB address space limit
2. **Host Allocation**: Wasmtime allocates actual memory from the HOST
3. **Memory Pressure**: Host memory limits (cgroups, containers) affect allocation success
4. **IndexPointer Storage**: Writing to IndexPointer triggers SMT operations that allocate host memory
5. **Failure Propagation**: Host memory exhaustion causes WASM trap/error

### Root Cause

The issue is in how `IndexPointer::set()` works:

```rust
// In WASM module
let mut pointer = IndexPointer::from_keyword(&key);
pointer.set(Arc::new(large_data)); // Allocates in HOST memory via SMT
```

This triggers:
1. SMT update in host memory
2. Memory allocation for tree nodes
3. Potential host memory exhaustion
4. Failure returned to WASM

### Why It Matters

For blockchain indexers:
- **Same block** processed in different environments
- **Different stored data** due to memory constraints (not just success/failure)
- **Silent consensus failure**: Both nodes appear healthy but have divergent state
- **State root mismatch**: Identical blocks produce different state roots
- **Reorg issues**: Nodes with different memory limits handle reorgs differently

## Solutions

### Option 1: Memory Limits in WASM

Add memory limit checks **before** allocation:

```rust
fn check_memory_limit(required_mb: u32) -> Result<(), String> {
    const MAX_ALLOWED_MB: u32 = 500; // Conservative limit
    if required_mb > MAX_ALLOWED_MB {
        return Err("Exceeds memory limit".to_string());
    }
    Ok(())
}
```

### Option 2: Fail Fast

Detect memory pressure early and abort:

```rust
if let Err(_) = data.try_reserve(size) {
    panic!("Memory allocation failed - deterministic failure");
}
```

### Option 3: External Validation

Document minimum memory requirements:
- **Minimum**: 2GB RAM for WASM execution
- **Recommended**: 4GB+ RAM for production
- **Kubernetes**: Set memory limits appropriately

## Files

```
tests/memory/
├── README.md              # This file
├── Dockerfile             # Docker image for testing
├── run_low_memory.sh      # Run in 512MB container
├── run_high_memory.sh     # Run in 4GB container
└── run_comparison.sh      # Compare both results

crates/metashrew-memory-stress/
├── Cargo.toml             # WASM module package
└── src/lib.rs             # Memory stress test logic

src/tests/
└── memory_determinism_test.rs  # Test suite
```

## Troubleshooting

### Docker build fails

```bash
# Clear Docker cache
docker system prune -a
```

### Out of memory errors even in high-memory test

Your host may not have 4GB available. Check:

```bash
free -h
docker info | grep Memory
```

### WASM module not found

Build it first:

```bash
cargo build --target wasm32-unknown-unknown --release -p metashrew-memory-stress
```

## Further Investigation

### Monitor Memory Usage (Linux)

While test is running:

```bash
watch -n 1 'ps aux | grep metashrew | head -10'
```

### Docker Memory Stats

In another terminal:

```bash
docker stats
```

### Kubernetes Testing

Deploy with different memory limits:

```yaml
resources:
  limits:
    memory: "512Mi"  # Low memory
  # vs
  limits:
    memory: "4Gi"    # High memory
```

## ⚠️ CRITICAL UPDATE: Memory Pre-allocation Issue

**NEW FINDING**: The runtime does NOT actually pre-allocate the full 4GB WASM32 memory space!

### Test Results (January 2026)

We created additional tests to verify memory pre-allocation behavior:

#### Test: Runtime Instantiation in 512MB Environment

```bash
docker run --rm --memory=512m metashrew-preallocate-test
```

**Expected**: ❌ Should fail - cannot pre-allocate 4GB in 512MB environment
**Actual**: ✅ Succeeds - runtime instantiates successfully

**This proves**: `static_memory_maximum_size(0x100000000)` only sets a MAXIMUM, it does NOT pre-allocate!

### Root Cause

In `/data/metashrew/crates/metashrew-runtime/src/runtime.rs:494-498`:

```rust
config.static_memory_maximum_size(0x100000000); // 4GB max memory
config.static_memory_guard_size(0x10000); // 64KB guard
config.memory_init_cow(false); // Disable copy-on-write
```

**What the comments claim**: "Pre-allocate memory to maximum size"
**What actually happens**: Sets maximum limit, uses demand paging

Wasmtime uses virtual memory - pages are only committed when accessed, not upfront.

### Impact

1. **Late failure**: Process crashes during execution (OOM killer), not at startup
2. **Non-deterministic**: Same WASM succeeds on high-memory hosts, fails on low-memory hosts
3. **Silent divergence**: Both complete `_start()` but store different data

### Solution

See [FINDINGS.md](./FINDINGS.md) and [force_memory_commit_poc.rs](./force_memory_commit_poc.rs) for:
- Detailed analysis of the issue
- Proof-of-concept fix that forces memory commit
- Implementation guide for the fix

**Fix summary**: After instantiation, touch every page (65,536 writes) to force OS to commit physical memory. Runtime fails fast if 4GB not available.

## Conclusion

This test suite **proves** that WASM execution in Metashrew is **non-deterministic** when host memory constraints vary. This is a critical issue for blockchain indexers that must produce identical results across all nodes.

The non-determinism stems from:
1. WASM memory operations depending on host memory availability
2. IndexPointer storage allocating in host memory
3. Different environments (containers, VMs, bare metal) having different memory limits
4. **Lack of actual 4GB pre-allocation** (allows runtime to start when it shouldn't)

**Impact**: Nodes with different memory configurations can produce different state roots, leading to consensus failures and reorg issues.

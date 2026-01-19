# Corrected Memory Non-Determinism Bug Description

## The REAL Bug (Corrected Understanding)

### What We Thought The Bug Was (WRONG ❌)

- High memory: WASM execution succeeds → `runtime.run()` returns `Ok()`
- Low memory: WASM execution fails/panics → `runtime.run()` returns `Err()`
- **This would be easy to detect** - one node crashes, the other doesn't

### What The Bug ACTUALLY Is (CORRECT ✅)

**Both WASM executions complete successfully**, but they store **different data**:

- **High memory (4GB)**:
  - `Vec::try_reserve(50MB)` returns `Ok(())`
  - WASM stores: `"SUCCESS:1000MB"` in IndexPointer
  - `_start()` completes successfully
  - `runtime.run()` returns `Ok(())`

- **Low memory (512MB)**:
  - `Vec::try_reserve(50MB)` returns `Err(...)`
  - WASM stores: `"ERROR:Failed at allocation 5"` in IndexPointer
  - `_start()` **still** completes successfully
  - `runtime.run()` returns `Ok(())`

**Critical**: Both return `Ok()`, both nodes think they succeeded, but **storage state has diverged**.

## Why This Is More Dangerous

### Silent Failure

```rust
// Node A (4GB RAM)
let result = runtime.run().await?;  // ✅ Ok(())
let stored = storage.get("/result")?;  // "SUCCESS:1000MB"
let state_root = compute_state_root();  // 0xabc...

// Node B (512MB RAM)
let result = runtime.run().await?;  // ✅ Ok(()) - ALSO succeeds!
let stored = storage.get("/result")?;  // "ERROR:failed..." - DIFFERENT!
let state_root = compute_state_root();  // 0xdef... - DIFFERENT!
```

**Both nodes report success**, but:
- Storage contents differ
- State roots differ
- No error logs indicate a problem
- Silent consensus failure

### In Real Blockchain Scenarios

Processing block 12345:

```
Node A (high memory):
  ✅ Block processed successfully
  📝 Stored: { tx_data, utxo_set, balances } = SUCCESS data
  🔐 State root: 0xabc123...

Node B (low memory):
  ✅ Block processed successfully  <- APPEARS FINE!
  📝 Stored: { error_message } = ERROR data  <- BUT DATA DIFFERS!
  🔐 State root: 0xdef456...  <- STATE ROOT DIFFERS!
```

Both nodes:
- Don't crash
- Don't log errors
- Appear to be syncing correctly
- But have **divergent state**

This only shows up when comparing state roots across nodes, which may happen:
- During network consensus
- When new nodes bootstrap
- During chain reorgs
- When comparing with other implementations

## The Fix In Our Test

### Before (Wrong Expectation)

```rust
match runtime.run().await {
    Ok(_) => "Host: SUCCESS",
    Err(e) => "Host: FAILURE (host memory exhausted)",  // Expected this
}
```

We expected low memory to cause `runtime.run()` to return `Err()`.

### After (Correct Expectation)

```rust
match runtime.run().await {
    Ok(_) => {
        info!("✓ WASM _start() completed successfully");

        // Check WHAT WAS STORED
        let stored = storage.get("/memory-stress-result/1")?;

        if stored.starts_with("SUCCESS") {
            "WASM completed successfully, stored: SUCCESS:1000MB"
        } else if stored.starts_with("ERROR") {
            "WASM completed successfully, stored: ERROR:failed..."
        }
    }
    Err(e) => {
        error!("❌ WASM PANICKED - this is NOT the bug we're looking for!");
        Err(e)
    }
}
```

We now expect **both** to return `Ok()`, and check the **stored data** to see the divergence.

## Test Results

### High Memory Container (4GB)

```
✓ WASM _start() completed successfully
WASM stored result: SUCCESS:1000MB
📊 RESULT: WASM completed successfully, stored: SUCCESS:1000MB
✅ Memory allocations SUCCEEDED
```

### Low Memory Container (512MB)

```
✓ WASM _start() completed successfully  <- SAME! Both succeed!
WASM stored result: ERROR:Failed at allocation 5 of 20...
📊 RESULT: WASM completed successfully, stored: ERROR:Failed...  <- DIFFERENT DATA!
⚠️  This demonstrates non-determinism: same input, different stored data!
```

## Comparison

| Aspect | High Memory | Low Memory | Same? |
|--------|-------------|------------|-------|
| `_start()` execution | ✅ Completed | ✅ Completed | ✅ YES |
| `runtime.run()` result | `Ok(())` | `Ok(())` | ✅ YES |
| Error logs | None | None | ✅ YES |
| **Stored data** | `SUCCESS:1000MB` | `ERROR:failed...` | ❌ **NO!** |
| **State root** | `0xabc...` | `0xdef...` | ❌ **NO!** |

## Why This Matters

### If It Was A Panic (What We Thought)

- **Easy to detect**: One node crashes, logs show errors
- **Clear failure mode**: Restart with more memory
- **No silent corruption**: Failed nodes don't contribute bad data

### As It Actually Is (Silent Divergence)

- **Hard to detect**: Both nodes appear healthy
- **Subtle failure mode**: Only shows up in consensus checks
- **Silent corruption**: Nodes serve different data to clients
- **Consensus breakdown**: Network can't agree on state root
- **Reorg issues**: Different nodes have different views of chain history

## The WASM Code That Causes This

```rust
// In metashrew-memory-stress/src/lib.rs
fn attempt_memory_stress(config: &MemoryConfig) -> Result<u32, String> {
    for i in 0..config.num_allocations {
        // This is where the non-determinism happens:
        let data = match try_allocate(allocation_size) {
            Ok(d) => d,        // High memory: succeeds
            Err(e) => {        // Low memory: fails
                return Err(format!("Failed at allocation {}", i));
            }
        };

        pointer.set(Arc::new(data));
    }
    Ok(total_allocated)  // High memory: returns Ok
}

// In _start()
match attempt_memory_stress(&config) {
    Ok(total) => {
        // HIGH MEMORY PATH
        result_pointer.set(Arc::new(format!("SUCCESS:{}", total).into_bytes()));
    }
    Err(err) => {
        // LOW MEMORY PATH
        result_pointer.set(Arc::new(format!("ERROR:{}", err).into_bytes()));
    }
}

flush();  // Both paths flush successfully!
// _start() returns successfully in BOTH cases!
```

The key insight: `try_allocate()` returns `Result`, so the code **handles both cases gracefully** and stores different data, rather than panicking.

## Conclusion

This bug is **more dangerous** than a simple crash because:

1. ✅ Both nodes execute successfully
2. ✅ Both nodes report no errors
3. ❌ But they store different data
4. ❌ Leading to different state roots
5. ❌ Causing silent consensus failure

The test suite now correctly demonstrates this by:
1. Verifying both WASM executions return `Ok()`
2. Comparing the stored data (which differs)
3. Showing that same input → different stored state
4. Proving non-determinism without requiring a panic

This is the **actual bug** that needs to be fixed in production blockchain indexers.

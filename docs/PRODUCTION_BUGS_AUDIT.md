# Production bugs audit + failing-test inventory

Status: living document. Last update 2026-05-22.

This file captures four bug classes surfaced in production (FROST
Batallion 6 chat, 2026-05-21; g vs h drift at h=950299; mork1e's
node wedged at block 892936) and the deterministic test repros we
need to write before attempting fixes. The methodology is failing-
test-first: we want to *prove* the bug exists before we touch the
code, and then prove the fix lands by flipping the same test from
red to green. We also need to verify that NONE of the existing
patches landed during the rc.* line accidentally made things worse.

---

## Bug 1 — `commit_atomic` infinite retry on persistent failure

### Production evidence

mork1e's v10 node, 2026-05-21 23:15-23:20Z:

```
[INFO ] processed block 892936 atomically (8813203 batch bytes)
[ERROR] snapshot-path atomic commit STILL failing for height 892936
        (attempt 590): Storage error: commit_atomic write failed at
        height 892936: IO error: While open a file for random read:
        /data/.metashrew/v10-v3/006117.sst: Too many open files —
        block 892936 atomic apply has been retrying for 590 attempts
... (5 minutes later)
[ERROR] (attempt 600) ... retrying for 600 attempts
```

The pod was wedged for hours retrying the same block against an
exhausted file-descriptor table (ulimit -n default ≈ 1024, RocksDB
needs ~1 FD per active SST file, the v10 DB has thousands of SSTs).
No progress, no escape, no visible error to the operator. mork1e's
`metashrew-bouncer` workaround sidesteps this by gating JSON-RPC
during indexing, which reduces FD pressure from view-path SST
opens.

### Root cause

`SnapshotMetashrewSync::process_block` and
`SnapshotMetashrewSync::process_block_with_snapshots` both run an
unbounded `loop` with only two exit conditions:
  1. Commit succeeds → return Ok.
  2. `current_height` already moved past `height` → return Ok
     (the rc.9 staleness guard).

Persistent commit failure has no third exit. The line in
`snapshot_sync.rs:546` explicitly acknowledges this:
"same infinite-retry-with-exponential-backoff policy as
process_block".

### Repro

`crates/metashrew-sync/tests/commit_atomic_persistent_failure_should_fail_visibly.rs`
— **landed, RED today**.

Fails with: `process_block did not return within 60s — the retry
loop has no max-attempts bound and spun forever.`

### Proposed fix

Add a `MAX_COMMIT_RETRIES` constant (suggest 20-30). When the retry
loop crosses that bound, return
`SyncError::Storage("commit_atomic exceeded max retries at height
{N}: last error: ...")`. At the 30 s backoff cap, 20-30 attempts is
10-15 minutes — enough to ride out transient FD pressure (compaction
catching up) but short enough that the operator gets paged before
hours of silent spin.

### Patch-history audit

- rc.3 (`150abef`): introduced the "never exit on atomic-write
  failure" policy. THIS IS THE BUG. It was a deliberate decision to
  prevent spurious exits, but the policy never grew a sanity-bound.
- rc.5 (`02a769d`): added the strict in-order check inside
  commit_atomic. Interacts with rc.3's infinite retry to wedge on
  stale heights (the previous test:
  `snapshot_path_stale_retry_test.rs` patched that interaction with
  the rc.9 staleness guard).
- rc.9 (the staleness guard): partial fix — only covers the
  height-already-advanced case. Persistent-commit-failure case is
  still wedged.

Conclusion: rc.3 introduced the regression. The fix is bounded retries
matching the original pre-rc.3 fail-fast intent, but keeping
exponential backoff for transient cases.

---

## Bug 2 — Indexer engine missing deterministic flags

### Code evidence

`crates/rockshrew-mono/src/lib.rs:1322-1325` and `:1349-1352`
build the indexer engine with ONLY:

```rust
let mut config_engine = wasmtime::Config::default();
config_engine.async_support(true);
let engine = wasmtime::Engine::new(&config_engine)?;
let runtime = MetashrewRuntime::load(args.indexer, adapter, engine).await?;
```

But `MetashrewRuntime::load` (`runtime.rs:498-526`) builds its
*async* engine (used for views) with:

```rust
let mut config = wasmtime::Config::default();
config.cranelift_nan_canonicalization(true);
config.relaxed_simd_deterministic(true);
config.memory_reservation(0x100000000);    // 4 GiB pre-alloc
config.memory_guard_size(0x10000);
config.memory_init_cow(false);
let mut async_config = config.clone();
async_config.async_support(true);
async_config.wasm_threads(true);
let async_engine = wasmtime::Engine::new(&async_config)?;
```

The `config` variable's deterministic flags are only inherited into
`async_config`. They are NEVER applied to the indexer engine. The
caller (rockshrew-mono) built that with bare defaults.

### Consensus impact

Without the deterministic flags on the indexer engine:
- **NaN canonicalization off**: floating-point NaN payloads can
  differ across hardware (x86-64 vs ARM, different SSE/AVX
  variants). Alkanes doesn't currently use float ops, but if it
  ever does (or if wasmi sub-instances inside alkanes do), state
  drift.
- **SIMD non-deterministic**: relaxed SIMD ops can produce
  different results across CPUs. Same risk.
- **No memory pre-allocation**: `memory.grow` happens lazily as the
  wasm allocates. Page-fault timing is non-deterministic across
  hosts, but the resulting memory contents shouldn't be (memory.grow
  produces deterministic zero-init pages). HOWEVER — without
  pre-allocation, a host under memory pressure can fail
  `memory.grow` mid-execution, producing a different runtime error
  path than a host with headroom. flex flagged this in the chat as
  "the fix that resolved most of our problems" (pre-allocating max
  wasm32).
- **memory_init_cow not disabled**: COW can share pages across
  instances. If two instances exist concurrently and one writes,
  the other's pages get unshared — read latency varies by host.
  Not directly a determinism violation but contributes to view-
  during-indexing race surface.

This is the strongest candidate I've found for the g vs h drift
(907.78 DIESEL apart at h=950299, same image, same wasm).

### Repro

Not yet written. The structural test would assert that the engine
passed to `MetashrewRuntime::load` has all the deterministic flags
set. But wasmtime doesn't expose Config getters from an Engine, so
the test has to be at the call-site level: a unit test that builds
the production engine, runs a stress wasm that depends on the
deterministic flags (e.g. emits NaN payloads), and asserts the
output matches a reference impl.

**Alternative test design**: refactor the engine construction into a
`metashrew_runtime::indexer_engine_config()` helper, then assert
both rockshrew-mono call sites use it. Determinism is then
guaranteed-by-construction.

### Proposed fix

Centralize the indexer engine config:

```rust
// metashrew-runtime/src/engine.rs (new)
pub fn indexer_config() -> wasmtime::Config {
    let mut config = wasmtime::Config::default();
    config.cranelift_nan_canonicalization(true);
    config.relaxed_simd_deterministic(true);
    config.memory_reservation(0x100000000);
    config.memory_guard_size(0x10000);
    config.memory_init_cow(false);
    config.consume_fuel(true);
    config.async_support(true);
    // NOTE: wasm_threads(true) is for the VIEW engine only —
    // consensus-critical NOT to enable for indexer.
    config
}
```

Both call sites in `rockshrew-mono` and the
`MetashrewRuntime::{load,new}` internals consume this. The
duplication goes away.

### Patch-history audit

The `runtime.rs` deterministic config block has been there since
v9.x at least. The bug is the rockshrew-mono integration:
`MetashrewRuntime::load` accepts an `engine` parameter built by
the caller, so the deterministic settings inside `load` only
flow into the `async_engine` it builds internally. The original
intent was probably "give the caller control over the engine"
but the consequence was "skip determinism on the indexer path".

### Verification

After the refactor, write an integration test that runs an
alkanes block 10x in parallel against the same DB and asserts the
post-state hashes are byte-identical. Without the determinism
flags this test could exhibit drift on a NaN-emitting workload.
Actually constructing such a workload is non-trivial because
alkanes-rs doesn't emit floats — would need a synthetic wasm
indexer.

---

## Bug 3 — Concurrent view-during-indexing supply drift (HYPOTHESIS)

### Production evidence

- g vs h on `mainnet-v905rc9-v220rc4`, same image, same wasm,
  same start time (2026-05-18T17:28:15Z): 907.78 DIESEL apart at
  h=950299 (g=644,918.98, h=644,011.20).
- flex (FROST chat): "It only surfaces when the jsonrpc is
  servicing an extreme amount of requests."
- mork1e's metashrew-bouncer workaround GATES JSON-RPC during
  indexing → drift stops on his node.
- Existing fix `preview_isolation_bug_test.rs` already addresses
  one variant (`metashrew_preview` writes leaking to production
  DB). Both tests in that file PASS today (`cargo test -p
  rockshrew-runtime --test preview_isolation_bug_test`).

So the preview-shadow leak is already plugged. The g/h drift must
have a different mechanism. Bug 2 above (missing determinism
flags) is the leading hypothesis.

### Auxiliary hypotheses to rule out

1. **HashMap iteration order** in the indexer path. Audit needed:
   `alkanes-rs` `__flush` payload is `KeyValueFlush` protobuf
   built from a `StorageMap` — iteration order matters for
   binary serialization. `StorageMap` in `alkanes-support` is
   backed by a HashMap; if its `serialize()` doesn't sort keys,
   the batch bytes are non-deterministic per process.
2. **RocksDB write barrier ordering**: independent (no shared
   write paths between view and indexer; rocksdb serializes its
   own writes).
3. **AtomicPointer's `IndexCheckpointStack`**: shared via
   `Arc<Mutex<Vec<IndexCheckpoint>>>` per
   `metashrew-core/src/index_pointer.rs:53`. If two tasks share
   the same AtomicPointer instance (NOT just clones), they
   contend on the lock. Audit needed: does the view path ever
   share an AtomicPointer with the indexer path?

### Repro

Not yet written. Proposed test: spawn the rockshrew-mono binary
locally with a deterministic test indexer, hammer it with `N=1000`
concurrent view requests while indexing a block range, then diff
the resulting RocksDB against a reference run with no concurrent
views. Heavyweight but matches the production trigger conditions.

### Patch-history audit (pending)

- Look at every change since the g/h pods were started
  (2026-05-18) that touched the indexer write path. Specifically:
  the v10 LengthCache (commit `0e32f48`) added a process-wide
  `RwLock<HashMap<Vec<u8>, u32>>`; reads from views could race
  with writes from `commit_atomic`'s post-commit walk. Need to
  verify the read path doesn't observe partially-applied
  updates.

---

## Bug 4 — `StorageMap::serialize()` iteration order (CONFIRMED NEEDS AUDIT)

### Hypothesis

If `StorageMap::serialize()` in `alkanes-support` iterates its
backing HashMap without sorting, the serialized `KeyValueFlush`
protobuf bytes vary per process. Different bytes → different
content hash → arguably different state when comparing snapshots
across nodes, even though the KV writes themselves are equivalent.

This wouldn't cause supply drift (state is the same logical set
of writes), but it would cause snapshot hash drift across nodes.
That's a separate concern from g vs h's 907 DIESEL drift but worth
documenting.

### Repro

`crates/alkanes-support/tests/storage_map_serialize_deterministic.rs`
(not yet written): build a StorageMap with the same KVs in two
different orders, call `serialize()`, assert byte-equal output.

---

## Bug 5 — `__flush` building the batch from wasm linear memory under load

### Hypothesis

The `__flush` host function reads a protobuf-serialized
`KeyValueFlush` from wasm linear memory at `ptr`. If the wasm
linear memory is being concurrently modified (it shouldn't be,
because the wasm is single-threaded), or if the host function
panics partway through reading, the batch is incomplete.

flex's "I already allocate the exact amount of memory to a block"
fix was about preventing this class of failure — pre-allocating
the max wasm32 region so wasm allocations inside the indexer
never need to grow memory during a `__flush` call.

This connects to Bug 2 (missing determinism flags) — without
`memory_reservation(4GiB)` on the indexer engine, this whole class
of failure is back in the picture.

### Repro

Subsumed by Bug 2 fix. After centralizing the indexer engine
config, run alkanes mass-mint blocks against the fixed engine and
assert no `__flush` partial-batch errors.

---

## Test-coverage roadmap

| Bug | Test path | Status |
|-----|-----------|--------|
| 1: infinite retry on commit failure | `crates/metashrew-sync/tests/commit_atomic_persistent_failure_should_fail_visibly.rs` | RED ✗ |
| 2: indexer engine missing determinism flags | TBD: `crates/metashrew-runtime/tests/indexer_engine_determinism.rs` | NOT WRITTEN |
| 3: concurrent view-during-indexing drift | TBD: `crates/rockshrew-mono/tests/concurrent_view_index_determinism.rs` | NOT WRITTEN — heavyweight |
| 4: StorageMap serialize order | TBD: in alkanes-rs `alkanes-support` crate | NOT WRITTEN — different repo |
| 5: __flush partial-batch under memory pressure | subsumed by Bug 2 | — |

---

## Existing patches to audit for regressions

| Patch | What it changed | Did it make things worse? |
|-------|------------------|---------------------------|
| rc.3 (`150abef`) "never exit on atomic-write failure" | retry loop became unbounded | YES — Bug 1 |
| rc.5 (`02a769d`) single-batch atomic commit | strict in-order rejection | Combined with rc.3 → wedge on stale heights (separate fix: rc.9 staleness guard) |
| rc.9 (snapshot_path_stale_retry_test.rs guard) | staleness guard | NO — fix for an interaction the previous rcs created |
| v10 `LengthCache` (`0e32f48`) | process-wide chain-length cache | UNKNOWN — needs audit. Could interact with concurrent reads from views. |
| v10 SMT removal (`c23b14f`) | removed dead SMT state-root code | NO — confirmed neutral perf-wise, removed footgun |
| v10 multi_get optimization (`7f9a7bc` superseded by `0e32f48`) | bulk `/length` lookups via RocksDB MultiGet | Superseded; not in current code |
| v10 view-syscall (`5b41ac9..00c8830`) | `__flush` dispatcher + ThreadSpawn/Join | View-only, NOT consensus. Should not affect indexer determinism. |
| preview_isolation fix | `create_isolated_copy` actually isolates | NO — fixed a real bug, tested |

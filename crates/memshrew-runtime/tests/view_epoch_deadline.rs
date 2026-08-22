//! Integration test for the view epoch deadline (2026-08-22
//! `protorunesbyoutpoint` hang incident).
//!
//! Views run with `set_fuel(u64::MAX)` + a cooperative yield interval — the
//! yield makes a runaway view *cancellable* but nothing ever *capped* it, so
//! a poisoned record that drives the view WASM into an unbounded loop spun
//! until the JSON-RPC layer's 60s method timeout while holding a
//! view-runtime permit, on every request. This test pins the new hard bound:
//! a view that loops forever must TRAP with an epoch-deadline error within
//! the configured budget, not hang.
//!
//! Uses a synthetic spin view rather than any real indexer WASM so the test
//! is self-contained and fast; the deadline is dropped to 2s via
//! `METASHREW_VIEW_EPOCH_DEADLINE_SECS` (read once per process — this file
//! is its own test binary, so the override cannot race another test).

use memshrew_runtime::{MemStoreAdapter, MetashrewRuntime};
use std::time::{Duration, Instant};

/// Minimal module: exports the linear memory the view path expects plus a
/// `spin` view that never returns.
const SPIN_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "spin") (result i32)
    (loop $forever
      br $forever)
    i32.const 0))
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn epoch_deadline_traps_runaway_view() {
    std::env::set_var("METASHREW_VIEW_EPOCH_DEADLINE_SECS", "2");

    let wasm = wat::parse_str(SPIN_WAT).expect("valid wat");
    let mut config = wasmtime::Config::default();
    config.async_support(true);
    let engine = wasmtime::Engine::new(&config).expect("engine");

    let runtime = MetashrewRuntime::new(&wasm, MemStoreAdapter::new(), engine)
        .await
        .expect("runtime");

    let started = Instant::now();
    let err = runtime
        .view("spin".to_string(), &vec![], 0)
        .await
        .expect_err("a forever-looping view must not succeed");
    let elapsed = started.elapsed();

    // Bounded: well under the old behavior (spin to the 60s method timeout).
    // 2s deadline + 1s tick granularity + slack for slow CI.
    assert!(
        elapsed < Duration::from_secs(15),
        "runaway view took {elapsed:?} — epoch deadline did not fire"
    );

    // The failure must be the epoch trap, not some other error.
    let msg = format!("{err:#}").to_lowercase();
    assert!(
        msg.contains("epoch") || msg.contains("interrupt") || msg.contains("deadline"),
        "expected an epoch-deadline trap, got: {msg}"
    );
}

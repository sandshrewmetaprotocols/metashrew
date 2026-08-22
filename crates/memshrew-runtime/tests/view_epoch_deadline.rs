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

/// Minimal module: exports the linear memory the view path expects, a
/// `spin` view that never returns, and an `ok` view that returns "abc" via
/// the metashrew arraybuffer convention (u32 LE length at ptr-4, data at ptr
/// — see `try_read_arraybuffer_as_vec`).
const SPIN_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "spin") (result i32)
    (loop $forever
      br $forever)
    i32.const 0)
  (func (export "ok") (result i32)
    (i32.store (i32.const 96) (i32.const 3))
    (i32.store8 (i32.const 100) (i32.const 97))
    (i32.store8 (i32.const 101) (i32.const 98))
    (i32.store8 (i32.const 102) (i32.const 99))
    i32.const 100))
"#;

/// Both tests set the override BEFORE constructing a runtime: the value is
/// read once per process (OnceLock), and vitest-style parallel test threads
/// would otherwise race which test initializes it. Setting the same value in
/// both makes initialization order irrelevant.
fn set_short_deadline() {
    std::env::set_var("METASHREW_VIEW_EPOCH_DEADLINE_SECS", "2");
}

async fn build_runtime() -> MetashrewRuntime<MemStoreAdapter> {
    let wasm = wat::parse_str(SPIN_WAT).expect("valid wat");
    let mut config = wasmtime::Config::default();
    config.async_support(true);
    let engine = wasmtime::Engine::new(&config).expect("engine");
    MetashrewRuntime::new(&wasm, MemStoreAdapter::new(), engine)
        .await
        .expect("runtime")
}

/// Regression guard for the epoch-interruption rollout itself: a HEALTHY
/// view must still instantiate and succeed on the epoch-enabled engine with
/// an armed deadline. (A store on an epoch engine with the default deadline
/// of 0 traps instantly — this pins that instantiation and the call are both
/// armed correctly.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn healthy_view_still_succeeds_on_epoch_engine() {
    set_short_deadline();
    let runtime = build_runtime().await;
    let out = runtime
        .view("ok".to_string(), &vec![], 0)
        .await
        .expect("healthy view must succeed under the epoch deadline");
    assert_eq!(out, b"abc".to_vec());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn epoch_deadline_traps_runaway_view() {
    set_short_deadline();
    let runtime = build_runtime().await;

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

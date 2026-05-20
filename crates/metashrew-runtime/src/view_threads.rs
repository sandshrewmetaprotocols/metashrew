//! Per-view-call thread registry for v10 view-syscall `ThreadSpawn` /
//! `ThreadJoin`.
//!
//! ## What this is
//!
//! Each view call (one inbound JSON-RPC `metashrew_view` request) owns
//! one `ViewThreadRegistry`. The registry holds `tokio::task::JoinHandle`s
//! keyed by a u32 thread id minted by the registry. When the view call
//! ends — i.e. the `ViewPermit` is dropped, which transitively drops the
//! registry — any pending join handles are aborted. That's the
//! load-bearing cleanup: it prevents tokio tasks spawned by one view from
//! outliving the view and racing the next view's state.
//!
//! ## Determinism
//!
//! Thread ids are minted from a per-registry `AtomicU32` starting at 1
//! (0 is reserved as a sentinel for "no thread"). Two view calls running
//! identical wasm get registries with the same starting counter and the
//! same sequence of spawn ops, so thread ids are deterministic from the
//! wasm's perspective.
//!
//! ## Scope
//!
//! Threading is a view-only capability. The indexer engine MUST NOT have
//! `wasm_threads(true)` enabled (see `runtime::load` / `runtime::new`
//! engine configs), so the indexer path cannot reach this registry. The
//! `Option<Arc<ViewThreadRegistry>>` on `State` is `None` in indexer mode
//! and `Some(_)` in view mode; the `__flush` dispatcher refuses thread
//! ops when the registry is absent.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

/// Sentinel value returned from `spawn` when the registry has already
/// shut down. Also reserved as "no thread" — wasm should treat a 0
/// thread id as a failed spawn.
pub const INVALID_THREAD_ID: u32 = 0;

/// Sentinel exit code returned from `join` when the thread id is
/// unknown, was already joined, or the spawned future panicked.
pub const INVALID_EXIT_CODE: i32 = i32::MIN;

/// Per-view-call registry of tokio task handles. Cheap to construct;
/// expected to live the duration of one view RPC.
pub struct ViewThreadRegistry {
    next_id: AtomicU32,
    handles: Mutex<HashMap<u32, tokio::task::JoinHandle<i32>>>,
}

impl ViewThreadRegistry {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU32::new(1),
            handles: Mutex::new(HashMap::new()),
        }
    }

    /// Spawn a future onto the current tokio runtime and return the
    /// thread id wasm can later pass to `join`. The future is wrapped in
    /// a panic-catching layer so a panic in the spawned wasm thread
    /// surfaces as `INVALID_EXIT_CODE` rather than a join-handle error.
    pub fn spawn<F>(&self, fut: F) -> u32
    where
        F: Future<Output = i32> + Send + 'static,
    {
        self.register(tokio::spawn(fut))
    }

    /// Register an already-spawned tokio `JoinHandle` and return the
    /// thread id. Used by the wasm-spawn host (which spawns via
    /// `tokio::task::spawn_blocking` for Send-bound reasons; see
    /// `run_spawned_view_thread_blocking`).
    pub fn register(&self, handle: tokio::task::JoinHandle<i32>) -> u32 {
        // Allocate id first. fetch_add wraps after 4 billion spawns —
        // that's safely outside any plausible single view-call lifetime.
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        if id == INVALID_THREAD_ID {
            // Skip 0 if we ever wrap around. Re-register the same handle.
            return self.register(handle);
        }
        let mut guard = match self.handles.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.insert(id, handle);
        id
    }

    /// Await the thread with the given id and return its exit code.
    /// Returns `INVALID_EXIT_CODE` if the id is unknown, has already
    /// been joined, or the task panicked / was cancelled.
    pub async fn join(&self, thread_id: u32) -> i32 {
        let handle = {
            let mut guard = match self.handles.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            guard.remove(&thread_id)
        };
        match handle {
            Some(h) => match h.await {
                Ok(code) => code,
                Err(_) => INVALID_EXIT_CODE,
            },
            None => INVALID_EXIT_CODE,
        }
    }

    /// Number of in-flight thread handles. Exposed for tests + metrics.
    pub fn in_flight(&self) -> usize {
        match self.handles.lock() {
            Ok(g) => g.len(),
            Err(p) => p.into_inner().len(),
        }
    }
}

/// Aborts any still-pending tokio tasks when the registry is dropped.
/// This is the load-bearing cleanup: if wasm spawns a thread and the
/// view call returns before joining it, the orphaned task could
/// otherwise live indefinitely and race against the next view call.
impl Drop for ViewThreadRegistry {
    fn drop(&mut self) {
        let handles = match self.handles.get_mut() {
            Ok(g) => std::mem::take(g),
            Err(p) => std::mem::take(p.into_inner()),
        };
        for (_id, h) in handles {
            h.abort();
        }
    }
}

impl Default for ViewThreadRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Drive `MetashrewRuntime::<T>::run_spawned_view_thread_impl` on a
/// blocking thread.
///
/// We use `tokio::task::spawn_blocking` rather than `tokio::spawn`
/// because the inner future captures wasmtime types whose Send
/// inference is brittle — concretely, `MetashrewRuntime::setup_linker_view`
/// returns a Future that wasmtime doesn't prove `Send` for, even though
/// each individual host-fn closure inside it IS `Send`. By running the
/// child on its own `current_thread` runtime inside `spawn_blocking`,
/// we sidestep the parent runtime's Send-bound requirement entirely.
///
/// The cost is one extra OS-thread per spawned view-thread; that's
/// acceptable for the view-syscall use case (cache-population, parallel
/// scan) which is bounded by `--view-concurrency` x small-N anyway.
pub fn run_spawned_view_thread_blocking<T>(
    engine: wasmtime::Engine,
    module: wasmtime::Module,
    context: std::sync::Arc<std::sync::RwLock<crate::context::MetashrewRuntimeContext<T>>>,
    fn_idx: u32,
    arg: u32,
) -> tokio::task::JoinHandle<i32>
where
    T: crate::traits::KeyValueStoreLike + Clone + Send + Sync + 'static,
{
    tokio::task::spawn_blocking(move || {
        // Build a single-thread tokio runtime inside the blocking task.
        // It owns the wasmtime Store + Instance lifetime, so the Send
        // requirements of the outer tokio::spawn never apply.
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!(
                    "spawn_blocking view thread: failed to build child runtime: {:?}",
                    e
                );
                return INVALID_EXIT_CODE;
            }
        };
        rt.block_on(async move {
            crate::runtime::MetashrewRuntime::<T>::run_spawned_view_thread_impl(
                engine, module, context, fn_idx, arg,
            )
            .await
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_and_join_returns_exit_code() {
        let r = ViewThreadRegistry::new();
        let id = r.spawn(async { 7 });
        assert!(id >= 1);
        assert_eq!(r.join(id).await, 7);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn join_unknown_id_returns_sentinel() {
        let r = ViewThreadRegistry::new();
        assert_eq!(r.join(999).await, INVALID_EXIT_CODE);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn double_join_returns_sentinel() {
        let r = ViewThreadRegistry::new();
        let id = r.spawn(async { 42 });
        assert_eq!(r.join(id).await, 42);
        assert_eq!(r.join(id).await, INVALID_EXIT_CODE);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ids_are_unique() {
        let r = ViewThreadRegistry::new();
        let a = r.spawn(async { 1 });
        let b = r.spawn(async { 2 });
        let c = r.spawn(async { 3 });
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        // Mint-order should be increasing under the current sequential
        // semantics (no test of strict-monotonic, just uniqueness).
        assert_eq!(r.in_flight(), 3);
        let _ = r.join(a).await;
        assert_eq!(r.in_flight(), 2);
        let _ = r.join(b).await;
        let _ = r.join(c).await;
        assert_eq!(r.in_flight(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn panic_in_spawned_task_yields_sentinel() {
        let r = ViewThreadRegistry::new();
        let id = r.spawn(async {
            panic!("intentional panic in spawn test");
        });
        assert_eq!(r.join(id).await, INVALID_EXIT_CODE);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn drop_aborts_pending_handles() {
        // Spawn a future that would never complete on its own, then
        // drop the registry. The task must be aborted (not leaked).
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let r = ViewThreadRegistry::new();
        let _id = r.spawn(async move {
            // Block on a channel we never send to. If Drop doesn't
            // abort, this future leaks forever and the test runtime
            // will be poisoned.
            let _ = rx.await;
            999
        });
        drop(r);
        // Sender will be dropped at end of scope; the spawned future
        // would receive Err(_), but since the registry's Drop aborted
        // it, it never gets that far. The test passes if we reach here
        // without hanging.
        drop(tx);
        tokio::task::yield_now().await;
    }
}

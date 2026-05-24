//! v10 indexer-mode thread registry for Block-STM tx-handler spawn.
//!
//! ## What this is
//!
//! Each block being indexed in parallel mode owns one
//! `IndexerThreadRegistry`. Wasm spawns tx-handlers via the
//! `IndexerThreadSpawn` opcode (carried over `__flush`); the host
//! looks up this registry on the caller's Store, spawns an OS thread
//! that runs a fresh wasmtime Store + module instance, and registers
//! the join handle keyed by a monotonically-increasing u32 thread id.
//!
//! Wasm later passes the id back through `IndexerThreadJoin`; the
//! host removes the handle and blocks for completion, returning the
//! tx-handler's i32 exit code.
//!
//! ## Why std::thread (not tokio::task)
//!
//! The indexer-side `__flush` binding is sync (`func_wrap`). To
//! block on a join inside that closure without deadlocking a
//! tokio executor, the spawned thread must be an OS thread with a
//! sync join handle. The spawned thread itself builds a
//! `current_thread` tokio runtime so the wasmtime async APIs work
//! inside — same trick `run_spawned_view_thread_blocking` uses, but
//! we expose a sync `.join()` to the parent.
//!
//! ## Determinism
//!
//! Thread ids start at 1 (0 reserved as `INVALID_THREAD_ID`) and
//! are minted via `fetch_add` from a per-registry `AtomicU32`. Two
//! blocks executing identical wasm get registries with the same
//! starting counter and the same spawn sequence — ids are
//! deterministic from the wasm's perspective.
//!
//! Block-STM correctness does NOT depend on thread id assignment;
//! validation works off `tx_seq`, which the wasm passes explicitly
//! as the spawn arg. Thread ids are just a wasm-side handle for
//! join.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

/// Returned from spawn when the registry has shut down or
/// instantiation failed. Wasm should treat 0 as a failed spawn
/// and not pass it to join.
pub const INVALID_THREAD_ID: u32 = 0;

/// Returned from join when the thread id is unknown, was already
/// joined, or the OS thread panicked. Maps to wasm-side
/// `Option::None` semantics in the eventual wrapper.
pub const INVALID_EXIT_CODE: i32 = i32::MIN;

/// Per-block thread registry for indexer-mode tx-handler spawns.
/// Wraps a `HashMap<thread_id, std::thread::JoinHandle<i32>>`.
pub struct IndexerThreadRegistry {
    next_id: AtomicU32,
    handles: Mutex<HashMap<u32, std::thread::JoinHandle<i32>>>,
}

impl IndexerThreadRegistry {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU32::new(1),
            handles: Mutex::new(HashMap::new()),
        }
    }

    /// Register an OS-thread handle and return the wasm-visible
    /// thread id. The id is unique within this registry's lifetime
    /// (4 billion spawns before wraparound; we'd hit far more
    /// pressing limits first).
    pub fn register(&self, handle: std::thread::JoinHandle<i32>) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        if id == INVALID_THREAD_ID {
            // Skip 0 on the (vanishingly unlikely) wraparound case.
            return self.register(handle);
        }
        let mut guard = match self.handles.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.insert(id, handle);
        id
    }

    /// Block until the thread with `thread_id` finishes, return its
    /// exit code. `INVALID_EXIT_CODE` if the id is unknown, has
    /// already been joined, or the OS thread panicked.
    ///
    /// Sync (no `.await`) — safe to call from inside a sync wasmtime
    /// `func_wrap` binding. The caller's task will block; multi-
    /// thread tokio runtimes (the production setup) handle this fine.
    pub fn join(&self, thread_id: u32) -> i32 {
        let handle = {
            let mut guard = match self.handles.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            guard.remove(&thread_id)
        };
        match handle {
            Some(h) => match h.join() {
                Ok(code) => code,
                Err(_panic) => INVALID_EXIT_CODE,
            },
            None => INVALID_EXIT_CODE,
        }
    }

    /// Number of registered-but-not-yet-joined handles. Exposed for
    /// tests + per-block diagnostics.
    pub fn in_flight(&self) -> usize {
        match self.handles.lock() {
            Ok(g) => g.len(),
            Err(p) => p.into_inner().len(),
        }
    }

    /// Drain and join every still-registered handle. Called from
    /// Drop and from the scheduler's end-of-block cleanup so leaked
    /// spawn-without-join doesn't leave zombie OS threads.
    pub fn join_all(&self) {
        let handles = {
            let mut guard = match self.handles.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            std::mem::take(&mut *guard)
        };
        for (_id, h) in handles {
            // Best-effort drain. We discard exit codes here because
            // the wasm never asked for them (didn't call join).
            let _ = h.join();
        }
    }
}

impl Default for IndexerThreadRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// On drop, drain pending handles so OS threads don't outlive the
/// registry. Unlike tokio tasks we can't `abort()` an std::thread,
/// so we block until they finish. Tx-handler wasm is bounded by
/// fuel + memory limits so each thread will exit in finite time.
impl Drop for IndexerThreadRegistry {
    fn drop(&mut self) {
        self.join_all();
    }
}

/// Spawn an OS thread that builds a fresh tokio runtime + calls
/// `MetashrewRuntime::run_spawned_indexer_tx_handler_impl` inside it,
/// returning the OS-thread JoinHandle to the caller. The returned
/// handle is meant to be registered with
/// [`IndexerThreadRegistry::register`] so wasm can later join via
/// thread id.
///
/// We use `std::thread::spawn` (not `tokio::task::spawn_blocking`)
/// because the indexer-side `__flush` binding is sync (`func_wrap`)
/// and needs to `.join()` blockingly. The spawned thread builds its
/// own `current_thread` tokio runtime so the wasmtime async APIs
/// work inside.
pub fn spawn_indexer_tx_handler_blocking<T>(
    engine: wasmtime::Engine,
    module: wasmtime::Module,
    context: std::sync::Arc<std::sync::RwLock<crate::context::MetashrewRuntimeContext<T>>>,
    block_stm: std::sync::Arc<crate::block_stm::BlockStmCtx>,
    tx_seq: u32,
    fn_idx: u32,
    arg: u32,
) -> std::thread::JoinHandle<i32>
where
    T: crate::traits::KeyValueStoreLike + Clone + Send + Sync + 'static,
    <T as crate::traits::KeyValueStoreLike>::Batch: Send,
{
    std::thread::spawn(move || {
        // Each spawned thread owns its own current_thread tokio
        // runtime — Send-bound issues with the wasmtime async stack
        // make a shared multi-thread runtime awkward here. The cost
        // is one runtime build per spawn; for blocks with ~10s of
        // txs and the cooperative wasmtime fuel cap, the relative
        // overhead is small.
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!(
                    "spawn_indexer_tx_handler_blocking: failed to build child runtime: {:?}",
                    e
                );
                return INVALID_EXIT_CODE;
            }
        };
        rt.block_on(async move {
            crate::runtime::MetashrewRuntime::<T>::run_spawned_indexer_tx_handler_impl(
                engine, module, context, block_stm, tx_seq, fn_idx, arg,
            )
            .await
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_join_returns_exit_code() {
        let r = IndexerThreadRegistry::new();
        let handle = std::thread::spawn(|| 7);
        let id = r.register(handle);
        assert!(id >= 1);
        assert_eq!(r.join(id), 7);
    }

    #[test]
    fn join_unknown_id_returns_sentinel() {
        let r = IndexerThreadRegistry::new();
        assert_eq!(r.join(999), INVALID_EXIT_CODE);
    }

    #[test]
    fn double_join_returns_sentinel() {
        let r = IndexerThreadRegistry::new();
        let id = r.register(std::thread::spawn(|| 42));
        assert_eq!(r.join(id), 42);
        assert_eq!(r.join(id), INVALID_EXIT_CODE);
    }

    #[test]
    fn ids_are_unique_and_in_flight_tracks_count() {
        let r = IndexerThreadRegistry::new();
        let a = r.register(std::thread::spawn(|| 1));
        let b = r.register(std::thread::spawn(|| 2));
        let c = r.register(std::thread::spawn(|| 3));
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        assert_eq!(r.in_flight(), 3);
        let _ = r.join(a);
        assert_eq!(r.in_flight(), 2);
        let _ = r.join(b);
        let _ = r.join(c);
        assert_eq!(r.in_flight(), 0);
    }

    #[test]
    fn panic_in_spawned_thread_yields_sentinel() {
        let r = IndexerThreadRegistry::new();
        let id = r.register(std::thread::spawn(|| {
            panic!("intentional panic in spawn test");
        }));
        assert_eq!(r.join(id), INVALID_EXIT_CODE);
    }

    #[test]
    fn drop_drains_pending_handles() {
        // Spawn a short-running task, drop the registry without
        // joining. The Drop impl must drain (block until done) so the
        // OS thread doesn't outlive the registry's owning State.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        {
            let r = IndexerThreadRegistry::new();
            let _id = r.register(std::thread::spawn(move || {
                let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
                999
            }));
            // Signal the spawned thread to exit, then drop the registry.
            // Drop will block until the thread finishes.
            tx.send(()).ok();
        }
        // Reached this line ⇒ Drop didn't deadlock.
    }
}

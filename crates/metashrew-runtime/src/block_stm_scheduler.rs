//! v10 Block-STM scheduler: optimistic-concurrency tx execution with
//! validate-then-re-execute.
//!
//! Wraps the [`crate::block_stm::BlockStmCtx`] data structures with
//! the actual algorithm:
//!
//!   1. Round R = 0. Mark all txs `Pending`.
//!   2. Caller's executor runs every `Pending` tx in parallel. Each
//!      run resolves reads via `handle_conflict_read` (records
//!      Version in tracker), stages writes via `handle_conflict_write`
//!      (into MvMemory at tx_seq). Returns exit code.
//!   3. Validate every executed tx IN CANONICAL ORDER. For each, if
//!      `validate_tx` returns false, mark it `Pending` for round R+1.
//!      Note: an invalidation of tx N often cascades — if tx N is
//!      re-executed and writes different keys, txs M > N reading
//!      those keys are also stale. The validator catches this on
//!      the next round.
//!   4. If no invalidations: success. The merged WriteBatch is
//!      `ctx.mv.merge()`. If round budget exhausted: divergence
//!      fallback — caller should retry the block sequentially.
//!
//! ## Determinism
//!
//! The output is byte-identical to sequential execution PROVIDED:
//!
//! - The executor calls the per-tx closure exactly once per
//!   (tx_seq, round) pair. (Concurrent re-execution of the same
//!   (tx_seq, round) would race in MvMemory.)
//! - Tx-handler reads/writes flow through ConflictRead/ConflictWrite
//!   (instrumented). Direct `__get`/`__set` from a tx-handler would
//!   bypass tracking and break determinism.
//!
//! The scheduler holds these invariants by construction: re-execution
//! is single-threaded per tx_seq, and the wasm-side wrapper
//! (separate crate) routes all tx-handler I/O through the
//! ConflictRead/Write opcodes.
//!
//! ## Why an Executor trait
//!
//! The scheduler is pure orchestration: "here are the tx_seqs that
//! need to run this round, please run them and call me back when
//! done". The actual parallelism backend (rayon, native threads,
//! sequential) is abstracted via the [`Executor`] trait so:
//!
//! - Tests can use [`SerialExecutor`] for deterministic ordering
//!   without thread non-determinism.
//! - Production uses a wasmtime-aware executor (commit #4b) that
//!   spawns a fresh Store per tx-handler call.
//! - Alternative backends (e.g. async-runtime-based) plug in
//!   without scheduler changes.

use crate::block_stm::{validate_tx, BlockStmCtx};
use std::sync::Arc;

/// Outcome of running one block through the scheduler.
#[derive(Debug, PartialEq, Eq)]
pub enum ExecOutcome {
    /// All txs validated in canonical order within the round budget.
    /// `merged_writes` is the final per-key set the caller folds into
    /// the block's WriteBatch.
    Success { rounds: u32, merged_writes: usize },
    /// At least one tx returned a non-zero exit code. The block is
    /// poisoned; caller should treat as a failure (set had_failure).
    TxFailure { tx_seq: u32, exit_code: i32, round: u32 },
    /// Hit `max_rounds` without converging. Indicates pathological
    /// conflict density (or a scheduler bug). Caller should retry
    /// the block sequentially as a fallback — sequential execution
    /// is by construction conflict-free.
    DivergedFallback { rounds: u32, pending: Vec<u32> },
}

/// Strategy for running a batch of tx-handler closures. The
/// scheduler hands a slice of tx_seqs to execute; the executor
/// runs them however it wants (sequential, rayon, async) and
/// returns the per-tx exit codes in the same order.
pub trait Executor {
    /// Run `f(tx_seq)` for each tx_seq in `txs`, returning the exit
    /// codes parallel to `txs`. Implementations are free to run
    /// concurrently as long as the per-tx closure invocations don't
    /// observe each other (the scheduler relies on each
    /// (tx_seq, round) pair running exactly once).
    fn execute<F>(&self, txs: &[u32], f: F) -> Vec<i32>
    where
        F: Fn(u32) -> i32 + Send + Sync;
}

/// Single-threaded executor. The scheduler still gets the validate/
/// re-execute correctness benefits even without parallelism — useful
/// for tests, for low-CPU-count operators, and as the sequential
/// fallback when [`ExecOutcome::DivergedFallback`] fires.
pub struct SerialExecutor;

impl Executor for SerialExecutor {
    fn execute<F>(&self, txs: &[u32], f: F) -> Vec<i32>
    where
        F: Fn(u32) -> i32 + Send + Sync,
    {
        txs.iter().map(|&seq| f(seq)).collect()
    }
}

/// The Block-STM scheduler.
pub struct BlockStmScheduler {
    n_txs: u32,
    ctx: Arc<BlockStmCtx>,
    max_rounds: u32,
}

impl BlockStmScheduler {
    pub fn new(n_txs: u32, ctx: Arc<BlockStmCtx>) -> Self {
        Self { n_txs, ctx, max_rounds: 16 }
    }

    /// Override the round budget. Default 16 is tuned for alkanes-v3:
    /// most blocks converge in 1-2 rounds; pathological blocks
    /// (many txs touching the same global state) may take more, but
    /// 16 is a hard cap before we fall back to sequential.
    pub fn with_max_rounds(mut self, max_rounds: u32) -> Self {
        self.max_rounds = max_rounds;
        self
    }

    /// Drive the block through validate/re-execute rounds until
    /// convergence or budget exhaustion. `execute_tx_fn(tx_seq)`
    /// runs one tx and returns its exit code.
    ///
    /// The executor controls parallelism; this method is the
    /// algorithm: deciding which txs need (re-)execution per round
    /// and validating in canonical order.
    pub fn execute_block<E, F>(
        &self,
        executor: &E,
        execute_tx_fn: F,
    ) -> ExecOutcome
    where
        E: Executor,
        F: Fn(u32) -> i32 + Send + Sync,
    {
        // Round 0: every tx is pending.
        let mut pending: Vec<u32> = (0..self.n_txs).collect();

        for round in 0..self.max_rounds {
            if pending.is_empty() {
                return ExecOutcome::Success {
                    rounds: round,
                    merged_writes: self.ctx.mv.len(),
                };
            }

            // Clear prior writes + read-sets for the pending set.
            // Round 0 is a no-op (nothing was written yet); later
            // rounds need this so re-execution starts from a clean
            // slate per (tx_seq, round).
            for &seq in &pending {
                self.ctx.mv.clear_writes_of(seq);
                self.ctx.tracker_for(seq).clear();
            }

            // Hand the batch to the executor. It runs each
            // execute_tx_fn(seq) and returns exit codes parallel to
            // `pending`.
            let exits = executor.execute(&pending, &execute_tx_fn);

            // Surface any non-zero exit. Pick the lowest tx_seq with
            // a failure for determinism — multiple parallel txs
            // could fail in the same round.
            for (idx, &exit) in exits.iter().enumerate() {
                if exit != 0 {
                    return ExecOutcome::TxFailure {
                        tx_seq: pending[idx],
                        exit_code: exit,
                        round,
                    };
                }
            }

            // Validate every tx (not just the just-executed set —
            // re-execution of an earlier-seq tx can invalidate a
            // later-seq tx that previously validated). Iterate in
            // canonical order so the failure-cause is deterministic.
            let mut next_pending = Vec::new();
            for tx_seq in 0..self.n_txs {
                let tracker = self.ctx.tracker_for(tx_seq);
                if tracker.is_empty() {
                    // Tx never read anything (e.g. pure-write tx).
                    // Nothing to validate against; trivially valid.
                    continue;
                }
                if !validate_tx(&self.ctx.mv, &tracker) {
                    next_pending.push(tx_seq);
                }
            }

            if next_pending.is_empty() {
                return ExecOutcome::Success {
                    rounds: round + 1,
                    merged_writes: self.ctx.mv.len(),
                };
            }
            pending = next_pending;
        }

        ExecOutcome::DivergedFallback {
            rounds: self.max_rounds,
            pending,
        }
    }
}

// Scheduler tests live in
// `crates/metashrew-runtime/tests/block_stm_scheduler.rs` because they
// use `MemStoreAdapter` from the sibling `memshrew-runtime` crate
// (only available as a dev-dep in integration-test scope, not in
// lib's #[cfg(test)] scope).


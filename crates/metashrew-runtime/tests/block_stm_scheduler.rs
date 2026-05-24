//! End-to-end tests for the v10 Block-STM scheduler against the
//! real ConflictRead/Write handlers + MemStoreAdapter. Exercises the
//! validate/re-execute loop with hand-crafted "tx-handler" closures
//! that simulate what wasm tx-handlers will do once commit #4b
//! wires the spawn machinery.

use metashrew_runtime::block_stm::BlockStmCtx;
use metashrew_runtime::block_stm_scheduler::{
    BlockStmScheduler, ExecOutcome, Executor, SerialExecutor,
};
use metashrew_runtime::indexer_syscall::{handle_conflict_read, handle_conflict_write};
use memshrew_runtime::MemStoreAdapter;
use std::sync::Arc;

/// Trait the test programs interact with — wraps the host-side
/// handle_conflict_{read,write} calls so the program-under-test
/// looks like wasm-side code calling the syscall wrappers.
trait TxIo: Send + Sync {
    fn read(&self, key: &[u8]) -> Vec<u8>;
    fn write(&self, key: &[u8], value: &[u8]);
}

struct MockTxIo<'a> {
    db: &'a MemStoreAdapter,
    ctx: Arc<BlockStmCtx>,
    target_height: u32,
    tx_seq: u32,
}

impl<'a> TxIo for MockTxIo<'a> {
    fn read(&self, key: &[u8]) -> Vec<u8> {
        handle_conflict_read(
            self.db, self.target_height, &self.ctx, self.tx_seq, key,
        )
        .expect("read")
    }
    fn write(&self, key: &[u8], value: &[u8]) {
        handle_conflict_write(
            &self.ctx, self.tx_seq, key.to_vec(), value.to_vec(),
        );
    }
}

/// Wrap a test program (closure of (tx_seq, &TxIo) -> ()) into the
/// scheduler's expected `Fn(u32) -> i32` shape.
fn mock_tx_handler<'a, P>(
    db: &'a MemStoreAdapter,
    ctx: &'a Arc<BlockStmCtx>,
    target_height: u32,
    program: P,
) -> impl Fn(u32) -> i32 + Send + Sync + 'a
where
    P: Fn(u32, &dyn TxIo) + Send + Sync + 'a,
{
    move |tx_seq: u32| {
        let io = MockTxIo {
            db,
            ctx: ctx.clone(),
            target_height,
            tx_seq,
        };
        program(tx_seq, &io);
        0
    }
}

#[test]
fn empty_block_succeeds_in_zero_rounds() {
    let ctx = Arc::new(BlockStmCtx::new());
    let s = BlockStmScheduler::new(0, ctx);
    let out = s.execute_block(&SerialExecutor, |_| panic!("not called"));
    assert_eq!(
        out,
        ExecOutcome::Success { rounds: 0, merged_writes: 0 }
    );
}

#[test]
fn two_independent_txs_converge_in_one_round() {
    let db = MemStoreAdapter::new();
    let ctx = Arc::new(BlockStmCtx::new());

    let handler = mock_tx_handler(&db, &ctx, 100, |seq, io| match seq {
        0 => io.write(b"a", b"from-tx0"),
        1 => io.write(b"b", b"from-tx1"),
        _ => unreachable!(),
    });

    let s = BlockStmScheduler::new(2, ctx.clone());
    let out = s.execute_block(&SerialExecutor, handler);

    assert_eq!(
        out,
        ExecOutcome::Success { rounds: 1, merged_writes: 2 }
    );
    let merged = ctx.mv.merge();
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0], (b"a".to_vec(), b"from-tx0".to_vec()));
    assert_eq!(merged[1], (b"b".to_vec(), b"from-tx1".to_vec()));
}

#[test]
fn read_after_write_in_canonical_order_converges() {
    let db = MemStoreAdapter::new();
    let ctx = Arc::new(BlockStmCtx::new());

    let handler = mock_tx_handler(&db, &ctx, 100, |seq, io| match seq {
        0 => io.write(b"K", &[5u8]),
        1 => {
            let v = io.read(b"K");
            let doubled = if v.is_empty() { 0 } else { v[0] * 2 };
            io.write(b"L", &[doubled]);
        }
        _ => unreachable!(),
    });

    let s = BlockStmScheduler::new(2, ctx.clone());
    let out = s.execute_block(&SerialExecutor, handler);

    match out {
        ExecOutcome::Success { rounds, merged_writes } => {
            assert!(rounds >= 1);
            assert_eq!(merged_writes, 2);
        }
        other => panic!("expected Success, got {:?}", other),
    }

    let merged = ctx.mv.merge();
    let l = merged.iter().find(|(k, _)| k == b"L").map(|(_, v)| v.clone());
    assert_eq!(l, Some(vec![10u8]), "tx 1 must see tx 0's K=5 → L=10");
}

/// Run tx_seqs in reverse order each round. Used to force the
/// scheduler into validate→re-execute on read-after-write blocks.
struct ReverseExec;
impl Executor for ReverseExec {
    fn execute<F>(&self, txs: &[u32], f: F) -> Vec<i32>
    where
        F: Fn(u32) -> i32 + Send + Sync,
    {
        let mut rev: Vec<u32> = txs.iter().copied().collect();
        rev.reverse();
        let mut results: Vec<(u32, i32)> =
            rev.iter().map(|&s| (s, f(s))).collect();
        results.sort_by_key(|x| x.0);
        results.into_iter().map(|x| x.1).collect()
    }
}

#[test]
fn read_before_write_invalidates_and_re_executes() {
    let db = MemStoreAdapter::new();
    let ctx = Arc::new(BlockStmCtx::new());

    let handler = mock_tx_handler(&db, &ctx, 100, |seq, io| match seq {
        0 => io.write(b"K", &[5u8]),
        1 => {
            let v = io.read(b"K");
            let doubled = if v.is_empty() { 0 } else { v[0] * 2 };
            io.write(b"L", &[doubled]);
        }
        _ => unreachable!(),
    });

    let s = BlockStmScheduler::new(2, ctx.clone());
    let out = s.execute_block(&ReverseExec, handler);

    match out {
        ExecOutcome::Success { rounds, .. } => {
            assert!(
                rounds >= 2,
                "tx 1 ran first → read returned Storage → tx 0 wrote → \
                 validation must invalidate tx 1 → rounds >= 2 (got {})",
                rounds
            );
        }
        other => panic!("expected Success, got {:?}", other),
    }

    let merged = ctx.mv.merge();
    let l_value = merged
        .iter()
        .find(|(k, _)| k == b"L")
        .map(|(_, v)| v.clone())
        .expect("L must exist after re-execution");
    assert_eq!(
        l_value,
        vec![10u8],
        "final L value must equal sequential execution (tx 1 sees tx 0's K=5)"
    );
}

#[test]
fn tx_failure_aborts_with_lowest_seq() {
    struct FailingExecutor;
    impl Executor for FailingExecutor {
        fn execute<F>(&self, txs: &[u32], f: F) -> Vec<i32>
        where
            F: Fn(u32) -> i32 + Send + Sync,
        {
            txs.iter()
                .map(|&seq| if seq == 1 { -7 } else { f(seq) })
                .collect()
        }
    }

    let db = MemStoreAdapter::new();
    let ctx = Arc::new(BlockStmCtx::new());
    let handler = mock_tx_handler(&db, &ctx, 100, |_, _| {});

    let s = BlockStmScheduler::new(3, ctx.clone());
    let out = s.execute_block(&FailingExecutor, handler);
    assert_eq!(
        out,
        ExecOutcome::TxFailure { tx_seq: 1, exit_code: -7, round: 0 }
    );
}

#[test]
fn diverged_fallback_after_max_rounds() {
    // 5 txs each read K then write K=seq. Adversarial reverse order
    // means each round only converges the most-recent tx. With
    // max_rounds = 2, the block can't possibly converge → fallback.
    let db = MemStoreAdapter::new();
    let ctx = Arc::new(BlockStmCtx::new());

    let handler = mock_tx_handler(&db, &ctx, 100, |seq, io| {
        let _ = io.read(b"K");
        io.write(b"K", &[seq as u8]);
    });

    let s = BlockStmScheduler::new(5, ctx.clone()).with_max_rounds(2);
    let out = s.execute_block(&ReverseExec, handler);

    match out {
        ExecOutcome::DivergedFallback { rounds, pending } => {
            assert_eq!(rounds, 2);
            assert!(!pending.is_empty(), "some tx must remain pending");
        }
        other => panic!("expected DivergedFallback, got {:?}", other),
    }
}

#[test]
fn deterministic_merge_regardless_of_executor_order() {
    let db = MemStoreAdapter::new();

    let program = |seq: u32, io: &dyn TxIo| {
        let v = io.read(b"K");
        let prev = if v.is_empty() { 0u8 } else { v[0] };
        io.write(b"K", &[prev.wrapping_add(seq as u8 + 1)]);
    };

    // Run 1: serial executor.
    let ctx1 = Arc::new(BlockStmCtx::new());
    let handler1 = mock_tx_handler(&db, &ctx1, 100, program);
    let s1 = BlockStmScheduler::new(4, ctx1.clone());
    assert!(matches!(
        s1.execute_block(&SerialExecutor, handler1),
        ExecOutcome::Success { .. }
    ));
    let merge1 = ctx1.mv.merge();

    // Run 2: reverse executor with generous budget.
    let ctx2 = Arc::new(BlockStmCtx::new());
    let handler2 = mock_tx_handler(&db, &ctx2, 100, program);
    let s2 = BlockStmScheduler::new(4, ctx2.clone()).with_max_rounds(64);
    assert!(matches!(
        s2.execute_block(&ReverseExec, handler2),
        ExecOutcome::Success { .. }
    ));
    let merge2 = ctx2.mv.merge();

    assert_eq!(
        merge1, merge2,
        "scheduler output must be byte-identical regardless of \
         executor parallelism / order"
    );

    // Sequential expected: K starts 0; tx 0: 0+1=1, tx 1: 1+2=3,
    // tx 2: 3+3=6, tx 3: 6+4=10.
    assert_eq!(merge1[0].0, b"K");
    assert_eq!(merge1[0].1, vec![10u8]);
}

#[test]
fn pure_write_txs_skip_validation() {
    let db = MemStoreAdapter::new();
    let ctx = Arc::new(BlockStmCtx::new());

    let handler = mock_tx_handler(&db, &ctx, 100, |seq, io| {
        io.write(format!("k{}", seq).as_bytes(), &[seq as u8]);
    });

    let s = BlockStmScheduler::new(10, ctx.clone());
    let out = s.execute_block(&SerialExecutor, handler);

    assert_eq!(
        out,
        ExecOutcome::Success { rounds: 1, merged_writes: 10 },
        "no reads → no validation → 1 round"
    );
}

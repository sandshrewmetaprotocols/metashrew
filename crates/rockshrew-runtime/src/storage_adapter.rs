//! RocksDB-specific implementation of the `StorageAdapter` trait.

use async_trait::async_trait;
use log::{info, warn};
use metashrew_runtime::{
    rollback::{RollbackOp, SmtRollback},
    KeyValueStoreLike,
};
use metashrew_sync::{StorageAdapter, StorageStats, SyncError, SyncResult};
use rocksdb::{WriteBatch, WriteOptions, DB};
use std::sync::Arc;

use crate::adapter::RocksDBRuntimeAdapter;

/// Build `WriteOptions` with `set_sync(true)` so the RocksDB WAL is fsynced
/// before the write returns. This is what makes `commit_atomic` actually
/// durable — without it, RocksDB can return Ok to the caller and then lose
/// the write across a host crash or OOM kill before the kernel page cache
/// is flushed. See the comment block on `StorageAdapter::commit_atomic` for
/// the determinism invariant this protects.
fn sync_write_options() -> WriteOptions {
    let mut wo = WriteOptions::default();
    wo.set_sync(true);
    wo
}

/// RocksDB storage adapter for persistent storage.
#[derive(Clone)]
pub struct RocksDBStorageAdapter {
    db: Arc<DB>,
}

impl RocksDBStorageAdapter {
    pub fn new(db: Arc<DB>) -> Self {
        Self { db }
    }
}

// Implement SmtRollback trait for proper reorg handling
impl SmtRollback for RocksDBStorageAdapter {
    fn iter_keys<F>(&self, mut callback: F) -> anyhow::Result<()>
    where
        F: FnMut(&[u8]) -> anyhow::Result<()>,
    {
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        for item in iter {
            match item {
                Ok((key, _)) => callback(&key)?,
                Err(e) => return Err(anyhow::anyhow!("Failed to iterate keys: {}", e)),
            }
        }
        Ok(())
    }

    fn delete_key(&mut self, key: &[u8]) -> anyhow::Result<()> {
        self.db.delete(key)
            .map_err(|e| anyhow::anyhow!("Failed to delete key: {}", e))
    }

    fn put_key(&mut self, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        self.db.put(key, value)
            .map_err(|e| anyhow::anyhow!("Failed to put key: {}", e))
    }

    fn get_value(&self, key: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        self.db.get(key)
            .map_err(|e| anyhow::anyhow!("Failed to get value: {}", e))
    }

    fn apply_atomic(&mut self, ops: &[RollbackOp]) -> anyhow::Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        let mut batch = WriteBatch::default();
        for op in ops {
            match op {
                RollbackOp::Put(k, v) => batch.put(k, v),
                RollbackOp::Delete(k) => batch.delete(k),
            }
        }
        self.db
            .write(batch)
            .map_err(|e| anyhow::anyhow!("Atomic rollback batch write failed: {}", e))
    }
}

#[async_trait]
impl StorageAdapter for RocksDBStorageAdapter {
    async fn get_indexed_height(&self) -> SyncResult<u32> {
        let height_key = b"__INTERNAL/height".to_vec();
        match self.db.get(&height_key) {
            Ok(Some(value)) => {
                if value.len() >= 4 {
                    let height_bytes: [u8; 4] = value[..4]
                        .try_into()
                        .map_err(|_| SyncError::Storage("Invalid height data".to_string()))?;
                    Ok(u32::from_le_bytes(height_bytes))
                } else {
                    Ok(0)
                }
            }
            Ok(None) => Ok(0),
            Err(e) => Err(SyncError::Storage(format!("Database error: {}", e))),
        }
    }

    async fn set_indexed_height(&mut self, height: u32) -> SyncResult<()> {
        let height_key = b"__INTERNAL/height".to_vec();
        let height_bytes = height.to_le_bytes();
        self.db
            .put(&height_key, &height_bytes)
            .map_err(|e| SyncError::Storage(format!("Failed to store height: {}", e)))
    }

    async fn store_block_hash(&mut self, height: u32, hash: &[u8]) -> SyncResult<()> {
        let blockhash_key = format!("/__INTERNAL/height-to-hash/{}", height).into_bytes();
        self.db
            .put(&blockhash_key, hash)
            .map_err(|e| SyncError::Storage(format!("Failed to store blockhash: {}", e)))
    }

    async fn get_block_hash(&self, height: u32) -> SyncResult<Option<Vec<u8>>> {
        let blockhash_key = format!("/__INTERNAL/height-to-hash/{}", height).into_bytes();
        match self.db.get(&blockhash_key) {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) => Ok(None),
            Err(e) => Err(SyncError::Storage(format!("Database error: {}", e))),
        }
    }

    async fn store_state_root(&mut self, height: u32, root: &[u8]) -> SyncResult<()> {
        let adapter = RocksDBRuntimeAdapter::new(self.db.clone());
        let mut smt_helper = metashrew_runtime::smt::SMTHelper::new(adapter);
        let root_key = format!("smt:root:{}", height).into_bytes();
        smt_helper
            .storage
            .put(&root_key, root)
            .map_err(|e| SyncError::Storage(format!("Failed to store state root: {}", e)))
    }

    async fn get_state_root(&self, height: u32) -> SyncResult<Option<Vec<u8>>> {
        let adapter = RocksDBRuntimeAdapter::new(self.db.clone());
        let smt_helper = metashrew_runtime::smt::SMTHelper::new(adapter);
        match smt_helper.get_smt_root_at_height(height) {
            Ok(root) => Ok(Some(root.to_vec())),
            Err(_) => Ok(None),
        }
    }

    async fn rollback_to_height(&mut self, height: u32) -> SyncResult<()> {
        use metashrew_runtime::rollback::{rollback_smt_data, rollback_with_manifests};

        info!("Starting rollback to height {}", height);
        let current_height = self.get_indexed_height().await?;

        // Try fast manifest-based rollback first, fall back to full scan
        let used_fast = rollback_with_manifests(self, height, current_height)
            .map_err(|e| SyncError::Storage(format!("Manifest rollback failed: {}", e)))?;

        if !used_fast {
            warn!("Using full SMT rollback (no manifests for rollback range)");
            rollback_smt_data(self, height, current_height)
                .map_err(|e| SyncError::Storage(format!("SMT rollback failed: {}", e)))?;
        }

        self.set_indexed_height(height).await?;
        info!("Successfully completed rollback to height {}", height);
        Ok(())
    }

    async fn is_available(&self) -> bool {
        self.db.get(b"__test").is_ok()
    }

    async fn get_stats(&self) -> SyncResult<StorageStats> {
        let indexed_height = self.get_indexed_height().await?;
        Ok(StorageStats {
            total_entries: 0,
            indexed_height,
            storage_size_bytes: None,
        })
    }

    async fn get_db_handle(&self) -> SyncResult<Arc<DB>> {
        Ok(self.db.clone())
    }

    /// Commit block `height` atomically: reconstruct the WASM-side `WriteBatch`
    /// from `batch_data` (serialized by `BatchLike::to_bytes()` inside
    /// `process_block_atomic`'s __flush hook), APPEND the sync-framework metadata
    /// writes (block-hash record, state-root record, indexed-height pointer)
    /// into the SAME batch, and submit exactly ONE
    /// `db.write_opt(batch, WriteOptions::set_sync(true))` call.
    ///
    /// This is the single point of database commit for block-apply. Pre-rc.4 had
    /// two writes — `RocksDBRuntimeAdapter::write` for the WASM batch and this
    /// `commit_atomic` for the three metadata writes — which meant a process
    /// crash between the two fsyncs could leave the DB in a partial state:
    /// WASM-side state for block N committed (alkanes balances, totalsupply, etc.)
    /// but the indexed-height pointer / block-hash / state-root metadata missing
    /// or stale, so on restart the indexer would re-apply block N on top of
    /// already-applied state, double-counting append-only updates and producing
    /// the supply drift we saw on g/h. With this single-batch design, RocksDB's
    /// `WriteBatch` atomicity primitive guarantees that's not a representable
    /// outcome.
    ///
    /// Strict in-order check: refuses to commit unless `height == tip + 1`,
    /// matching the user-stated invariant that we never skip or rewrite a
    /// block outside of an explicit reorg rollback. The check reads tip
    /// from the on-disk snapshot of `__INTERNAL/height` — NOT from any keys
    /// staged in the in-flight batch — so the WASM-side batch can no longer
    /// self-reject the commit it produced (the rc.4 Option-A failure mode is
    /// also closed because the height pointer is now owned exclusively by
    /// `commit_atomic` and lands in the same batch as the tip-advancement).
    async fn commit_atomic(
        &mut self,
        height: u32,
        block_hash: &[u8],
        state_root: &[u8],
        batch_data: &[u8],
    ) -> SyncResult<()> {
        // Strict in-order progression check. Reads the on-disk tip — the
        // in-flight `batch_data` bytes are NOT applied yet, so this check
        // cannot self-reject the in-flight commit. This was the failure mode
        // closed by rc.4 Option A; we preserve the closure structurally here
        // by owning the height-pointer write inside commit_atomic.
        let height_key = b"__INTERNAL/height".to_vec();
        let current = match self.db.get(&height_key) {
            Ok(Some(value)) if value.len() >= 4 => {
                let bytes: [u8; 4] = value[..4]
                    .try_into()
                    .map_err(|_| SyncError::Storage("Invalid height data".to_string()))?;
                u32::from_le_bytes(bytes)
            }
            Ok(_) => 0,
            Err(e) => return Err(SyncError::Storage(format!("Database error: {}", e))),
        };

        // We accept either:
        //   - height == current + 1  (normal forward progression)
        //   - current == 0 AND no block-hash record at height 0 (fresh DB)
        let blockhash_zero_key = b"/__INTERNAL/height-to-hash/0".to_vec();
        let is_fresh_db = current == 0
            && self
                .db
                .get(&blockhash_zero_key)
                .map_err(|e| SyncError::Storage(format!("Database error: {}", e)))?
                .is_none();
        if !is_fresh_db && height != current.saturating_add(1) {
            return Err(SyncError::Storage(format!(
                "out-of-order commit rejected: attempted height {} but current tip is {} \
                 (commit_atomic only accepts height == tip + 1)",
                height, current
            )));
        }

        // Reconstruct the WASM-side batch from its serialized form. Empty
        // `batch_data` is acceptable for callers (legacy tests, mock paths)
        // that don't ship a WASM batch through this entrypoint — in that
        // case we just write the three metadata records, which matches the
        // pre-rc.4 behavior.
        let mut batch = if batch_data.is_empty() {
            WriteBatch::default()
        } else {
            WriteBatch::from_data(batch_data)
        };

        // APPEND the three metadata records into the SAME batch. After this
        // step, the batch holds every write for block `height` — WASM
        // state changes AND sync-framework metadata — ready to commit as
        // a single RocksDB transaction.
        let blockhash_key = format!("/__INTERNAL/height-to-hash/{}", height).into_bytes();
        batch.put(&blockhash_key, block_hash);

        let root_key = format!("smt:root:{}", height).into_bytes();
        batch.put(&root_key, state_root);

        let height_bytes = height.to_le_bytes();
        batch.put(&height_key, &height_bytes);

        // ONE write_opt with sync=true. RocksDB's WriteBatch primitive
        // guarantees atomicity within a single batch: every put is visible
        // post-commit, or none of them is. WAL fsync is the durability
        // guarantee that lets restart-recovery rely on the all-or-nothing
        // invariant — without sync=true, a crash before the OS page cache
        // flushes could lose the write after we returned Ok.
        self.db
            .write_opt(batch, &sync_write_options())
            .map_err(|e| SyncError::Storage(format!("commit_atomic write failed at height {}: {}", height, e)))
    }
}
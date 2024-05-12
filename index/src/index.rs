use anyhow::{Context, Result};
use bitcoin::consensus::{deserialize, serialize, Decodable};
use bitcoin::{BlockHash, OutPoint, Txid};
use rocksdb;
use itertools::Itertools;
use rlp;
use rocksdb::{WriteBatchWithTransaction, DB};
use std::collections::HashSet;
use std::convert::AsRef;
use std::path::PathBuf;
use std::sync::Arc;
use std::ops::ControlFlow;
use wasmtime::{Caller, Linker, Store};
use bitcoin_slices::{bsl, Visitor, Visit};

use metashrew_runtime::{BatchLike, KeyValueStoreLike, MetashrewRuntime};

use crate::{
    chain::{Chain, NewHeader},
    daemon::Daemon,
    db::{index_cf, DBStore, Row, WriteBatch},
    metrics::{self, Gauge, Histogram, Metrics},
    signals::ExitFlag,
    types::{
        HashPrefixRow, HeaderRow, ScriptHash, ScriptHashRow, SerBlock, SpendingPrefixRow, TxidRow,
    },
};

#[derive(Clone)]
struct Stats {
    update_duration: Histogram,
    update_size: Histogram,
    height: Gauge,
    db_properties: Gauge,
}

impl Stats {
    fn new(metrics: &Metrics) -> Self {
        Self {
            update_duration: metrics.histogram_vec(
                "index_update_duration",
                "Index update duration (in seconds)",
                "step",
                metrics::default_duration_buckets(),
            ),
            update_size: metrics.histogram_vec(
                "index_update_size",
                "Index update size (in bytes)",
                "step",
                metrics::default_size_buckets(),
            ),
            height: metrics.gauge("index_height", "Indexed block height", "type"),
            db_properties: metrics.gauge("index_db_properties", "Index DB properties", "name"),
        }
    }

    fn observe_duration<T>(&self, label: &str, f: impl FnOnce() -> T) -> T {
        self.update_duration.observe_duration(label, f)
    }

    fn observe_size(&self, label: &str, rows: &[Row]) {
        self.update_size.observe(label, db_rows_size(rows) as f64);
    }

    fn observe_batch(&self, batch: &WriteBatch) {
        self.observe_size("write_funding_rows", &batch.funding_rows);
        self.observe_size("write_spending_rows", &batch.spending_rows);
        self.observe_size("write_txid_rows", &batch.txid_rows);
        self.observe_size("write_header_rows", &batch.header_rows);
        debug!(
            "writing {} funding and {} spending rows from {} transactions, {} blocks",
            batch.funding_rows.len(),
            batch.spending_rows.len(),
            batch.txid_rows.len(),
            batch.header_rows.len()
        );
    }

    fn observe_chain(&self, chain: &Chain) {
        self.height.set("tip", chain.height() as f64);
    }

    fn observe_db(&self, store: &DBStore) {
        for (cf, name, value) in store.get_properties() {
            self.db_properties
                .set(&format!("{}:{}", name, cf), value as f64);
        }
    }
}

pub struct RocksDBRuntimeAdapter(&'static DB);
pub struct RocksDBBatch(pub Arc<Box<rocksdb::WriteBatch>>);

static mut _batch: Option<Arc<Box<rocksdb::WriteBatch>>> = None;

impl Clone for RocksDBRuntimeAdapter {
    fn clone(&self) -> Self {
        return Self(self.0);
    }
}

impl BatchLike for RocksDBBatch {
    fn default() -> RocksDBBatch {
        unsafe {
          if _batch.is_none() {
            _batch = Some(Arc::new(Box::new(rocksdb::WriteBatch::default())));
          }
          RocksDBBatch(_batch.unwrap().clone())
        }
    }
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, k: K, v: V) {
        return self.0.put_cf(index_cf(get_db()), k, v);
    }
}

static mut _db: Option<&'static DBStore> = None;

pub fn set_db(store: &'static DBStore) {
  unsafe {
    _db = Some(store);
  }
}

pub fn get_db() -> &'static rocksdb::DB {
  unsafe {
    &(_db.unwrap()).db
  }
}

impl KeyValueStoreLike for RocksDBRuntimeAdapter {
    type Batch = RocksDBBatch;
    type Error = rocksdb::Error;
    fn write(&self, batch: RocksDBBatch) -> Result<(), Self::Error> {
//      let _ = self.0.write(batch.0);
        Ok(())
    }
    fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        self.0.get(key)
    }
    fn delete<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error> {
        let _ = self.0.delete(key);
        Ok(())
    }
    fn put<K, V>(&self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.0.put_cf(index_cf(get_db()), key, value)
    }
}
pub fn commit_batch() -> Result<(), anyhow::Error> {
  unsafe {
    let result = match get_db().write(*(_batch.unwrap().as_ref()).clone()) {
      Ok(_) => Ok(()),
      Err(e) => Err(anyhow!("failed to commit batch"))
    };
    _batch = Some(Arc::new(rocksdb::WriteBatch::default()));
    result
  }
}

/// Confirmed transactions' address index
pub struct Index {
    pub store: &'static DBStore,
    batch_size: usize,
    lookup_limit: Option<usize>,
    chain: Chain,
    stats: Stats,
    runtime: metashrew_runtime::MetashrewRuntime<RocksDBRuntimeAdapter>,
    is_ready: bool,
    flush_needed: bool,
    engine: wasmtime::Engine,
    module: wasmtime::Module,
}

impl Index {
    pub(crate) fn load(
        indexer: PathBuf,
        store: &'static DBStore,
        mut chain: Chain,
        metrics: &Metrics,
        batch_size: usize,
        lookup_limit: Option<usize>,
        reindex_last_blocks: usize,
    ) -> Result<Self> {
        debug!("LOAD INDEX");
        set_db(store);
        if let Some(row) = store.get_tip() {
            debug!("GOT TIP");
            let tip = deserialize(&row).expect("invalid tip");
            let headers = store
                .read_headers()
                .into_iter()
                .map(|row| HeaderRow::from_db_row(&row).header)
                .collect();
            chain.load(headers, tip);
            chain.drop_last_headers(reindex_last_blocks);
        };
        let stats = Stats::new(metrics);
        stats.observe_chain(&chain);
        stats.observe_db(store);
        let engine = wasmtime::Engine::default();
        let module =
            wasmtime::Module::from_file(&engine, indexer.clone().into_os_string()).unwrap();
        let internal_db = RocksDBRuntimeAdapter(&store.db);
        let runtime = metashrew_runtime::MetashrewRuntime::load(indexer, internal_db).unwrap();
        Ok(Index {
            store,
            batch_size,
            lookup_limit,
            chain,
            stats,
            runtime,
            is_ready: false,
            flush_needed: false,
            engine,
            module,
        })
    }

    pub(crate) fn chain(&self) -> &Chain {
        &self.chain
    }

    pub(crate) fn limit_result<T>(&self, entries: impl Iterator<Item = T>) -> Result<Vec<T>> {
        let mut entries = entries.fuse();
        let result: Vec<T> = match self.lookup_limit {
            Some(lookup_limit) => entries.by_ref().take(lookup_limit).collect(),
            None => entries.by_ref().collect(),
        };
        if entries.next().is_some() {
            bail!(">{} index entries, query may take too long", result.len())
        }
        Ok(result)
    }

    pub(crate) fn filter_by_txid(&self, txid: Txid) -> impl Iterator<Item = BlockHash> + '_ {
        self.store
            .iter_txid(TxidRow::scan_prefix(txid))
            .map(|row| HashPrefixRow::from_db_row(&row).height())
            .filter_map(move |height| self.chain.get_block_hash(height))
    }

    pub(crate) fn filter_by_funding(
        &self,
        scripthash: ScriptHash,
    ) -> impl Iterator<Item = BlockHash> + '_ {
        self.store
            .iter_funding(ScriptHashRow::scan_prefix(scripthash))
            .map(|row| HashPrefixRow::from_db_row(&row).height())
            .filter_map(move |height| self.chain.get_block_hash(height))
    }

    pub(crate) fn filter_by_spending(
        &self,
        outpoint: OutPoint,
    ) -> impl Iterator<Item = BlockHash> + '_ {
        self.store
            .iter_spending(SpendingPrefixRow::scan_prefix(outpoint))
            .map(|row| HashPrefixRow::from_db_row(&row).height())
            .filter_map(move |height| self.chain.get_block_hash(height))
    }

    // Return `Ok(true)` when the chain is fully synced and the index is compacted.
    pub(crate) fn sync(&mut self, daemon: &Daemon, exit_flag: &ExitFlag) -> Result<bool> {
        let new_headers = self
            .stats
            .observe_duration("headers", || daemon.get_new_headers(&self.chain))?;
        match (new_headers.first(), new_headers.last()) {
            (Some(first), Some(last)) => {
                let count = new_headers.len();
                info!(
                    "indexing {} blocks: [{}..{}]",
                    count,
                    first.height(),
                    last.height()
                );
            }
            _ => {
                if self.flush_needed {
                    self.store.flush(); // full compaction is performed on the first flush call
                    self.flush_needed = false;
                }
                self.is_ready = true;
                return Ok(true); // no more blocks to index (done for now)
            }
        }
        for chunk in new_headers.chunks(self.batch_size) {
            exit_flag.poll().with_context(|| {
                format!(
                    "indexing interrupted at height: {}",
                    chunk.first().unwrap().height()
                )
            })?;
            self.sync_blocks(daemon, chunk)?;
        }
        self.chain.update(new_headers);
        self.stats.observe_chain(&self.chain);
        self.flush_needed = true;
        Ok(false) // sync is not done
    }

    fn sync_blocks(&mut self, daemon: &Daemon, chunk: &[NewHeader]) -> Result<()> {
        let blockhashes: Vec<BlockHash> = chunk.iter().map(|h| h.hash()).collect();
        let mut heights = chunk.iter().map(|h| h.height());

        let mut batch = WriteBatch::default();

        daemon.for_blocks(blockhashes, |blockhash, block| {
            let height = heights.next().expect("unexpected block");
            let engine = Arc::new(&self.engine);
            let module = Arc::new(&self.module);
            let blockarc = Arc::new(block);
            let blockhasharc = Arc::new(blockhash);
            self.stats.observe_duration("block", || {
                index_single_block(&mut batch, &mut self.runtime, blockarc, height, blockhasharc);
            });
            self.stats.height.set("tip", height as f64);
        })?;
        let heights: Vec<_> = heights.collect();
        assert!(
            heights.is_empty(),
            "some blocks were not indexed: {:?}",
            heights
        );
        batch.sort();
        self.stats.observe_batch(&batch);
        self.stats
            .observe_duration("write", || self.store.write(&batch));
        self.stats.observe_db(&self.store);
        self.store.flush();
        Ok(())
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.is_ready
    }
}

fn db_rows_size(rows: &[Row]) -> usize {
    rows.iter().map(|key| key.len()).sum()
}

fn index_single_block(
    batch: &mut WriteBatch,
    runtime: &mut metashrew_runtime::MetashrewRuntime<RocksDBRuntimeAdapter>,
    block: Arc<SerBlock>,
    height: usize,
    blockhash: Arc<BlockHash>,
) {
    runtime.context.lock().unwrap().height = height as u32;
    runtime.context.lock().unwrap().block = block.as_ref().clone();
    // create new instance with fresh memory and run again
    if let Err(_) = runtime.run() {
        debug!("{}", "Dropping cache");
        commit_batch().unwrap();
        runtime.refresh_memory();
        if let Err(e) = runtime.run() {
            panic!("Runtime run failed after retry: {}", e);
        }
    }
    struct IndexBlockVisitor<'a> {
        batch: &'a mut WriteBatch,
        height: usize,
    }

    impl<'a> Visitor for IndexBlockVisitor<'a> {
        fn visit_transaction(&mut self, tx: &bsl::Transaction) -> ControlFlow<()> {
            ControlFlow::Continue(())
        }

        fn visit_tx_out(&mut self, _vout: usize, tx_out: &bsl::TxOut) -> ControlFlow<()> {
            ControlFlow::Continue(())
        }

        fn visit_tx_in(&mut self, _vin: usize, tx_in: &bsl::TxIn) -> ControlFlow<()> {
            ControlFlow::Continue(())
        }

        fn visit_block_header(&mut self, header: &bsl::BlockHeader) -> ControlFlow<()> {
            let header = bitcoin::block::Header::consensus_decode(&mut header.as_ref())
                .expect("block header was already validated");
            self.batch
                .header_rows
                .push(HeaderRow::new(header).to_db_row());
            ControlFlow::Continue(())
        }
    }

    let mut index_block = IndexBlockVisitor { batch, height };
    bsl::Block::visit(&block, &mut index_block).expect("core returned invalid block");
    batch.tip_row = serialize(blockhash.as_ref()).into_boxed_slice();

    // save block hash to the headers_cf

}

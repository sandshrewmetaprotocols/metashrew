use anyhow::{Context, Result};
use bitcoin::consensus::{deserialize, serialize, Decodable};
use bitcoin::{BlockHash, OutPoint, Txid};
use bitcoin_slices::{bsl, Visit, Visitor};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use wasmtime::{Extern, Caller, Instance, Memory, MemoryType, SharedMemory, Config, Engine, Linker, Module, Store, Mutability, GlobalType, Global, Val, ValType};
use rlp::{Rlp};
use rlp;
use wasmtime_wasi::sync::WasiCtxBuilder;
use itertools::Itertools;
use hex;
use electrs_rocksdb as rocksdb;
use std::collections::HashSet;

use crate::{
    chain::{Chain, NewHeader},
    daemon::Daemon,
    db::{DBStore, Row, WriteBatch},
    metrics::{self, Gauge, Histogram, Metrics},
    signals::ExitFlag,
    types::{
        bsl_txid, HashPrefixRow, HeaderRow, ScriptHash, ScriptHashRow, SerBlock, SpendingPrefixRow,
        TxidRow,
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

/// Confirmed transactions' address index
pub struct Index {
    pub store: &'static DBStore,
    batch_size: usize,
    lookup_limit: Option<usize>,
    chain: Chain,
    stats: Stats,
    is_ready: bool,
    flush_needed: bool,
    engine: wasmtime::Engine,
    module: wasmtime::Module
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
        if let Some(row) = store.get_tip() {
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
        let module = wasmtime::Module::from_file(&engine, indexer.into_os_string()).unwrap();
        Ok(Index {
            store,
            batch_size,
            lookup_limit,
            chain,
            stats,
            is_ready: false,
            flush_needed: false,
            engine,
            module
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
            let blockarc = Arc::new(&block);
            self.stats.observe_duration("block", || {
                index_single_block(self.store, engine, module,  blockhash, blockarc, height, &mut batch);
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
        Ok(())
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.is_ready
    }
}

fn db_rows_size(rows: &[Row]) -> usize {
    rows.iter().map(|key| key.len()).sum()
}

pub fn db_annotate_value(v: &Vec<u8>, block_height: u32) -> Vec<u8> {
  let mut entry: Vec<u8> = v.clone();
  let height: Vec<u8> = block_height.to_le_bytes().try_into().unwrap();
  entry.extend(height);
  return entry;
}

pub fn db_make_list_key(v: &Vec<u8>, index: u32) -> Vec<u8> {
  let mut entry = v.clone();
  let index_bits: Vec<u8> = index.to_le_bytes().try_into().unwrap();
  entry.extend(index_bits);
  return entry;
}

pub fn db_make_length_key(key: &Vec<u8>) -> Vec<u8> {
  return db_make_list_key(key, u32::MAX);
}

pub fn db_append(dbstore: &'static DBStore, batch: &mut rocksdb::WriteBatch, key: &Vec<u8> , value: &Vec<u8>) {
  let mut length_key = db_make_length_key(key);
  let length: u32 = db_length_at_key(dbstore, &length_key);
  let entry_key: Vec<u8> = db_make_list_key(key, length);
  batch.put(&entry_key, &value);
  let new_length_bits: Vec<u8> = (length + 1).to_le_bytes().try_into().unwrap();
  batch.put(&length_key, &new_length_bits);
}


pub fn db_length_at_key(dbstore: &'static DBStore, length_key: &Vec<u8>) -> u32 {
  return match dbstore.db.get(length_key).unwrap() {
    Some(v) => u32::from_le_bytes(v.try_into().unwrap()),
    None => 0
  }
}

pub fn read_arraybuffer_as_vec(data: &[u8], data_start: i32) -> Vec<u8> {
      let len = u32::from_le_bytes((data[((data_start - 4) as usize)..(data_start as usize)]).try_into().unwrap());
      return Vec::<u8>::from(&data[(data_start as usize)..(((data_start as u32) + len) as usize)]);
}

  

pub fn setup_linker(linker: &mut Linker<()>, store: &mut Store<()>, dbstore: &'static DBStore, input: &Vec<u8>, height: u32) {
    let mut input_clone: Vec<u8> = <Vec<u8> as TryFrom<[u8; 4]>>::try_from(height.to_le_bytes()).unwrap();
    input_clone.extend(input.clone());
    let __host_len = input_clone.len();
    linker.func_wrap("env", "__host_len", move |mut caller: Caller<'_, ()>| -> i32 {
      return __host_len.try_into().unwrap();
    }).unwrap();
    linker.func_wrap("env", "__load_input", move |mut caller: Caller<'_, ()>, data_start: i32| {
      let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
      let _ = mem.write(&mut caller, data_start.try_into().unwrap(), input_clone.as_slice());
    }).unwrap();
    linker.func_wrap("env", "__log", |mut caller: Caller<'_, ()>, data_start: i32| {
      let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
      let data = mem.data(&caller);
      let bytes = read_arraybuffer_as_vec(data, data_start);
      println!("{}", std::str::from_utf8(bytes.as_slice()).unwrap());
    }).unwrap();
    linker.func_wrap("env", "abort", |_: i32, _: i32, _: i32, _: i32| {
      panic!("abort!");
    }).unwrap();
}


pub fn db_append_annotated(dbstore: &'static DBStore, batch: &mut rocksdb::WriteBatch, key: &Vec<u8> , value: &Vec<u8>, block_height: u32) {
  let mut length_key = db_make_length_key(key);
  let length: u32 = db_length_at_key(dbstore, &length_key);
  let entry = db_annotate_value(value, block_height);

  let entry_key: Vec<u8> = db_make_list_key(key, length);
  batch.put(&entry_key, &entry);
  let new_length_bits: Vec<u8> = (length + 1).to_le_bytes().try_into().unwrap();
  batch.put(&length_key, &new_length_bits);
}

pub fn db_create_empty_update_list(batch: &mut rocksdb::WriteBatch, height: u32){
    let height_vec: Vec<u8> = height.to_le_bytes().try_into().unwrap();
    let key: Vec<u8> = db_make_length_key(&db_make_updated_key(&height_vec));
    let value_vec: Vec<u8> = (0 as u32).to_le_bytes().try_into().unwrap();
    batch.put(&key, &value_vec);
}
pub fn setup_linker_indexer(linker: &mut Linker<()>, dbstore: &'static DBStore, height: usize) {
    linker.func_wrap("env", "__flush", move |mut caller: Caller<'_, ()>, encoded: i32| {
      let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
      let data = mem.data(&caller);
      let encoded_vec = read_arraybuffer_as_vec(data, encoded);
      let mut batch = rocksdb::WriteBatch::default();
      let _ = db_create_empty_update_list(&mut batch, height as u32);
      let decoded: Vec<Vec<u8>> = rlp::decode_list(&encoded_vec);
      decoded.iter().tuple_windows().inspect(|(k, v)| {
        let k_owned = <Vec<u8> as Clone>::clone(k);
        let v_owned = <Vec<u8> as Clone>::clone(v);
        db_append_annotated(dbstore, &mut batch, &k_owned, &v_owned, height as u32);
        let update_key: Vec<u8> = <Vec<u8> as TryFrom<[u8; 4]>>::try_from((height as u32).to_le_bytes()).unwrap();
        db_append(dbstore, &mut batch, &update_key, &k_owned);
      });
      (dbstore.db).write(batch).unwrap();
    }).unwrap();
    linker.func_wrap("env", "__get", move |mut caller: Caller<'_, ()>, key: i32, value: i32| {
      let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
      let data = mem.data(&caller);
      let key_vec = read_arraybuffer_as_vec(data, key);
      let length = db_length_at_key(dbstore, &key_vec);
      if length != 0 {
        let indexed_key = db_make_list_key(&key_vec, length - 1);
        let mut value_vec = (dbstore.db).get(&indexed_key).unwrap().unwrap();
        value_vec.truncate(value_vec.len().saturating_sub(4));
        let _ = mem.write(&mut caller, value.try_into().unwrap(), value_vec.as_slice());
      }
    }).unwrap();
    linker.func_wrap("env", "__get_len", move |mut caller: Caller<'_, ()>, key: i32| -> i32 {
      let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
      let data = mem.data(&caller);
      let key_vec = read_arraybuffer_as_vec(data, key);
      let length = db_length_at_key(dbstore, &key_vec);
      if length != 0 {
        let indexed_key = db_make_list_key(&key_vec, length - 1);
        let value_vec = (dbstore.db).get(&indexed_key).unwrap().unwrap();
        return (value_vec.len() - 4).try_into().unwrap();
      } else {
        return 0;
      }
    }).unwrap();
}

pub fn db_set_length(dbstore: &'static DBStore, key: &Vec<u8>, length: u32) {
  let mut length_key = db_make_length_key(key);
  if length == 0 {
    dbstore.db.delete(&length_key).unwrap();
    return;
  }
  let new_length_bits: Vec<u8> = (length + 1).to_le_bytes().try_into().unwrap();
  dbstore.db.put(&length_key, &new_length_bits).unwrap();
}

pub fn db_updated_keys_for_block(dbstore: &'static DBStore, height: u32) -> HashSet<Vec<u8>> {
  let key: Vec<u8> = db_make_length_key(&db_make_updated_key(&u32_to_vec(height)));
  let length: i32 = (db_length_at_key(dbstore, &key) as i32);
  let mut i: i32 = 0;
  let mut set: HashSet<Vec<u8>> = HashSet::<Vec<u8>>::new();
  while i < length {
    set.insert(dbstore.db.get(&db_make_list_key(&key, i as u32)).unwrap().unwrap());
    i = i + 1;
  }
  return set;
}

pub fn db_updated_keys_for_block_range(dbstore: &'static DBStore, from: u32, to: u32) -> HashSet<Vec<u8>> {
  let mut i = from;
  let mut result: HashSet<Vec<u8>> = HashSet::<Vec<u8>>::new();
  while to >= i {
    result.extend(db_updated_keys_for_block(dbstore, i));
    i = i + 1;
  }
  return result;
}


pub fn db_rollback_key(dbstore: &'static DBStore, key: &Vec<u8>, to_block: u32) {
  let length: i32 = db_length_at_key(dbstore, &key).try_into().unwrap();
  let mut index: i32 = length - 1;
  let mut end_length: i32 = length;
  while index >= 0 {
    let list_key = db_make_list_key(key, index.try_into().unwrap());
    let _ = match dbstore.db.get(&list_key).unwrap() {
      Some(value) => {
        let value_height: u32 = u32::from_le_bytes(value.as_slice()[(value.len() - 4)..].try_into().unwrap());
        if to_block <= value_height.try_into().unwrap() {
          dbstore.db.delete(&list_key).unwrap();
          end_length = end_length - 1;
        } else {
          break;
        }
      },
      None => { break; }
    };
  }
  if end_length != length {
    db_set_length(dbstore, key, end_length as u32);
  }
}

pub fn db_value_at_block(dbstore: &'static DBStore, key: &Vec<u8>, height: i32) -> Vec<u8> {
  let length: i32 = db_length_at_key(dbstore, &key).try_into().unwrap();
  let mut index: i32 = length - 1;
  while index >= 0 {
    let value: Vec<u8> = match dbstore.db.get(db_make_list_key(key, index.try_into().unwrap())).unwrap() {
      Some(v) => v,
      None => db_make_list_key(&Vec::<u8>::new(), 0)
    };

    let value_height: u32 = u32::from_le_bytes(value.as_slice()[(value.len() - 4)..].try_into().unwrap());
    /*
      Ok(v) => u32::from_le_bytes(v).try_into().unwrap(),
      Err(e) => 0
    };
    */
    if height >= value_height.try_into().unwrap() {
      value.clone().truncate(value.len().saturating_sub(4));
    }
  }
  return vec![];
}

pub fn setup_linker_view(linker: &mut Linker<()>, dbstore: &'static DBStore, height: i32) {
    linker.func_wrap("env", "__flush", move |mut caller: Caller<'_, ()>, encoded: i32| {}).unwrap();
    linker.func_wrap("env", "__get", move |mut caller: Caller<'_, ()>, key: i32, value: i32| {
      let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
      let data = mem.data(&caller);
      let key_vec = read_arraybuffer_as_vec(data, key);
      let value = db_value_at_block(dbstore, &key_vec, height);
      let _ = mem.write(&mut caller, value.len(), value.as_slice());
    }).unwrap();
    linker.func_wrap("env", "__get_len", move |mut caller: Caller<'_, ()>, key: i32| -> i32 {
      let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
      let data = mem.data(&caller);
      let key_vec = read_arraybuffer_as_vec(data, key);
      let value = db_value_at_block(dbstore, &key_vec, height);
      return value.len().try_into().unwrap();
    }).unwrap();
}

pub fn u32_to_vec(v: u32) -> Vec<u8> {
  return v.to_le_bytes().try_into().unwrap();
}

pub fn check_latest_block_for_reorg(dbstore: &'static DBStore, height: u32) -> u32 {
    match dbstore.db.get(db_make_length_key(&db_make_updated_key(&u32_to_vec(height as u32)))).unwrap() {
        Some(v) => check_latest_block_for_reorg(dbstore, height + 1),
        None => return height
    }
}
pub fn db_make_updated_key(key: &Vec<u8>)-> Vec<u8>{
    return key.clone();
}

pub fn handle_reorg(dbstore: &'static DBStore, from: u32) {
    let latest: u32 = check_latest_block_for_reorg(dbstore, from);
    let set: HashSet<Vec<u8>> = db_updated_keys_for_block_range(dbstore, from, latest);
    for key in set.iter() {
      db_rollback_key(dbstore, &key, from);
    }
}

fn index_single_block(
    dbstore: &'static DBStore,
    engine: Arc<&wasmtime::Engine>,
    module: Arc<&wasmtime::Module>,
    block_hash: BlockHash,
    block: Arc<&SerBlock>,
    height: usize,
    batch: &mut WriteBatch,
) {

    let mut store = Store::new(*engine, ());
    let mut linker = Linker::new(*engine);
    setup_linker(&mut linker, &mut store, dbstore, *block, height as u32);
    setup_linker_indexer(&mut linker, dbstore, height);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let start = instance.get_typed_func::<(), ()>(&mut store, "_start").unwrap();
    handle_reorg(dbstore, height as u32);
    instance.get_memory(&mut store, "memory").unwrap().grow(&mut store,  32767).unwrap();

    start.call(&mut store, ()).unwrap();
}

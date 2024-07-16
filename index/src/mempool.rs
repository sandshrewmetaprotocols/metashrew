use anyhow::Result;
use bitcoin::consensus::Encodable;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::convert::TryFrom;
use std::iter::FromIterator;
use std::ops::Bound;

use bitcoin::{blockdata::block::Block, hashes::Hash};
use bitcoin::{Amount, BlockHash, CompactTarget, OutPoint, Transaction, Txid};
use bitcoincore_rpc::json;
use metashrew_runtime::{BatchLike, KeyValueStoreLike, MetashrewRuntime};
use rayon::prelude::*;
use rocksdb::DB;
use serde::ser::{Serialize, SerializeSeq, Serializer};

use crate::{
    daemon::Daemon,
    db::{index_cf, pending_cf},
    index::get_db,
    metrics::{Gauge, Metrics},
    server::get_config,
    signals::ExitFlag,
    types::ScriptHash,
};
static mut PENDING_HASH: Option<BlockHash> = None;
#[derive(Clone)]
pub struct RocksDBPendingAdapter(pub &'static DB);
pub struct RocksDBPendingBatch(pub rocksdb::WriteBatch);

impl BatchLike for RocksDBPendingBatch {
    fn default() -> RocksDBPendingBatch {
        RocksDBPendingBatch(rocksdb::WriteBatch::default())
    }
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, k: K, v: V) {
        self.0.put_cf(pending_cf(get_db()), k, v)
    }
}

impl KeyValueStoreLike for RocksDBPendingAdapter {
    type Batch = RocksDBPendingBatch;
    type Error = rocksdb::Error;
    fn write(&self, batch: RocksDBPendingBatch) -> Result<(), Self::Error> {
        let opts = rocksdb::WriteOptions::default();
        match self.0.write_opt(batch.0, &opts) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
    fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        // append the blockhash bytes to the key then perform the lookup
        let mut k: Vec<u8> = "/".as_bytes().to_vec();
        let mut blockhash_bytes =
            BlockHash::as_byte_array(&{ unsafe { PENDING_HASH.expect("there is no pending?") } })
                .to_vec();
        k.append(&mut blockhash_bytes);
        k.append(&mut key.as_ref().to_vec());
        // get the value from the pending_cf, if not there, fallback on the index_cf
        match self.0.get_cf(pending_cf(self.0), &k) {
            Ok(opt) => match opt {
                None => self.0.get_cf(index_cf(self.0), &key),
                Some(v) => Ok(Some(v)),
            },
            Err(e) => Err(e),
        }
    }
    fn delete<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error> {
        let _ = self.0.delete_cf(pending_cf(self.0), key);
        Ok(())
    }
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) -> Result<(), Self::Error> {
        let mut k: Vec<u8> = "/".as_bytes().to_vec();
        let mut blockhash_bytes =
            BlockHash::as_byte_array(&{ unsafe { PENDING_HASH.expect("there is no pending?") } })
                .to_vec();
        k.append(&mut blockhash_bytes);
        k.append(&mut key.as_ref().to_vec());
        self.0.put_cf(pending_cf(self.0), k, value)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub txid: Txid,
    pub depends: HashSet<Txid>,
    pub tx: Transaction,
    pub fee: Amount,
    pub vsize: u64,
    pub has_unconfirmed_inputs: bool,
}

/// Mempool current state
pub(crate) struct Mempool {
    entries: HashMap<Txid, Entry>,
    by_funding: BTreeSet<(ScriptHash, Txid)>,
    by_spending: BTreeSet<(OutPoint, Txid)>,
    fees: FeeHistogram,
    // stats
    vsize: Gauge,
    count: Gauge,
    pending_runtime: MetashrewRuntime<RocksDBPendingAdapter>,
}

// Smallest possible txid
fn txid_min() -> Txid {
    Txid::all_zeros()
}

// Largest possible txid
fn txid_max() -> Txid {
    Txid::from_byte_array([0xFF; 32])
}

impl Mempool {
    pub fn new(metrics: &Metrics) -> Self {
        let internal_db = RocksDBPendingAdapter(get_db());
        let indexer = get_config().indexer.clone();
        Self {
            entries: Default::default(),
            by_funding: Default::default(),
            by_spending: Default::default(),
            fees: FeeHistogram::empty(),
            vsize: metrics.gauge(
                "mempool_txs_vsize",
                "Total vsize of mempool transactions (in bytes)",
                "fee_rate",
            ),
            count: metrics.gauge(
                "mempool_txs_count",
                "Total number of mempool transactions",
                "fee_rate",
            ),
            pending_runtime: MetashrewRuntime::<RocksDBPendingAdapter>::load(indexer, internal_db)
                .unwrap(),
        }
    }

    pub fn construct_entry_block(&self) -> bitcoin::blockdata::block::Block {
        debug!("building block with {} txs", self.entries.values().len());
        // create a dummy header
        let header = bitcoin::blockdata::block::Header {
            version: bitcoin::blockdata::block::Version::default(),
            prev_blockhash: BlockHash::all_zeros(),
            merkle_root: bitcoin::TxMerkleNode::all_zeros(),
            time: 0,
            bits: CompactTarget::default(),
            nonce: 0,
        };
        let block = Block {
            header,
            txdata: Self::topological_sort(&mut Vec::from_iter(self.entries.values().into_iter().map(|v| v.clone()))).into_iter().map(|v| v.tx.clone()).collect::<Vec<Transaction>>()
        };
        debug!("pending block built");
        unsafe {
            PENDING_HASH = Some(block.block_hash());
        }
        block
    }

    fn topological_sort(entries: &mut Vec<Entry>) -> Vec<Entry> {
        // construct a vec  of entries that have no unconfirmed transactions as inputs
        let mut sorted: Vec<Entry> = Vec::new();
        let mut no_deps: HashMap<Txid, Entry> = HashMap::new();
        // convert the entries to a hashset
        let mut entries_set: HashMap<Txid, Entry> =
            HashMap::from_iter(entries.iter().map(|entry| (entry.txid, entry.clone())));
        // construct a set of entries with no unconfirmed inputs and remove them from the entries_set
        for entry in entries {
            if !entry.has_unconfirmed_inputs {
                no_deps.insert(entry.txid, entry.clone());
                entries_set.remove(&entry.clone().txid);
            }
        }
        // iterate through 
        while !no_deps.is_empty() {
            let entry: Entry = no_deps.iter().next().unwrap().1.clone();
            no_deps.remove(&entry.txid);
            sorted.push(entry.clone());
            sorted.sort_by(|a, b| a.fee.cmp(&b.fee));
            for current in entries_set.values_mut() {
                if current.depends.contains(&entry.txid) {
                    current.depends.remove(&entry.txid);
                    if current.depends.is_empty() {
                        no_deps.insert(current.txid, current.clone());
                        current.has_unconfirmed_inputs = false;
                        // maybe remove from entries_set ? otherwise this should be fine
                    }
                }
            }
        }
        sorted
    }

    pub(crate) fn fees_histogram(&self) -> &FeeHistogram {
        &self.fees
    }

    pub(crate) fn get(&self, txid: &Txid) -> Option<&Entry> {
        self.entries.get(txid)
    }

    pub(crate) fn filter_by_funding(&self, scripthash: &ScriptHash) -> Vec<&Entry> {
        let range = (
            Bound::Included((*scripthash, txid_min())),
            Bound::Included((*scripthash, txid_max())),
        );
        self.by_funding
            .range(range)
            .map(|(_, txid)| self.get(txid).expect("missing funding mempool tx"))
            .collect()
    }

    pub(crate) fn filter_by_spending(&self, outpoint: &OutPoint) -> Vec<&Entry> {
        let range = (
            Bound::Included((*outpoint, txid_min())),
            Bound::Included((*outpoint, txid_max())),
        );
        self.by_spending
            .range(range)
            .map(|(_, txid)| self.get(txid).expect("missing spending mempool tx"))
            .collect()
    }

    pub fn sync(&mut self, daemon: &Daemon, exit_flag: &ExitFlag) {
        let txids = match daemon.get_mempool_txids() {
            Ok(txids) => txids,
            Err(e) => {
                warn!("mempool sync failed: {}", e);
                return;
            }
        };
        debug!("loading {} mempool transactions", txids.len());

        let new_txids = HashSet::<Txid>::from_iter(txids);
        let old_txids = HashSet::<Txid>::from_iter(self.entries.keys().copied());

        let to_add = &new_txids - &old_txids;
        let to_remove = &old_txids - &new_txids;

        let removed = to_remove.len();
        for txid in to_remove {
            self.remove_entry(txid);
        }
        let to_add: Vec<Txid> = to_add.into_iter().collect();
        let mut added = 0;
        for chunk in to_add.chunks(100) {
            if exit_flag.poll().is_err() {
                info!("interrupted while syncing mempool");
                return;
            }
            let entries: Vec<_> = chunk
                .par_iter()
                .filter_map(|txid| {
                    let tx = daemon.get_transaction(txid, None);
                    let entry = daemon.get_mempool_entry(txid);
                    match (tx, entry) {
                        (Ok(tx), Ok(entry)) => Some((txid, tx, entry)),
                        _ => None, // skip missing mempool entries
                    }
                })
                .collect();
            added += entries.len();
            for (txid, tx, entry) in entries {
                self.add_entry(*txid, tx, entry);
            }
        }
        self.fees = FeeHistogram::new(self.entries.values().map(|e| (e.fee, e.vsize)));
        for i in 0..FeeHistogram::BINS {
            let bin_index = FeeHistogram::BINS - i - 1; // from 63 to 0
            let limit = 1u128 << i;
            let label = format!("[{:20.0}, {:20.0})", limit / 2, limit);
            self.vsize.set(&label, self.fees.vsize[bin_index] as f64);
            self.count.set(&label, self.fees.count[bin_index] as f64);
        }
        debug!(
            "{} mempool txs: {} added, {} removed",
            self.entries.len(),
            added,
            removed,
        );
        let mut writer = Vec::new();
        let entry_block = self.construct_entry_block();
        let _block = entry_block.consensus_encode(&mut writer);
        self.pending_runtime.context.lock().unwrap().block = writer;
        match self.pending_runtime.run() {
            Ok(_) => debug!("pending block evaluates {} txs", entry_block.txdata.len()),
            Err(e) => debug!("pending block evaluation failed: {}", e),
        }
    }

    fn add_entry(&mut self, txid: Txid, tx: Transaction, entry: json::GetMempoolEntryResult) {
        for txi in &tx.input {
            self.by_spending.insert((txi.previous_output, txid));
        }
        for txo in &tx.output {
            let scripthash = ScriptHash::new(&txo.script_pubkey);
            self.by_funding.insert((scripthash, txid)); // may have duplicates
        }
        let entry = Entry {
            txid,
            tx,
            vsize: entry.vsize,
            fee: entry.fees.base,
            has_unconfirmed_inputs: !entry.depends.is_empty(),
            depends: HashSet::from_iter(entry.depends.clone().into_iter()),
        };
        assert!(
            self.entries.insert(txid, entry).is_none(),
            "duplicate mempool txid"
        );
    }

    fn remove_entry(&mut self, txid: Txid) {
        let entry = self.entries.remove(&txid).expect("missing tx from mempool");
        for txi in entry.tx.input {
            self.by_spending.remove(&(txi.previous_output, txid));
        }
        for txo in entry.tx.output {
            let scripthash = ScriptHash::new(&txo.script_pubkey);
            self.by_funding.remove(&(scripthash, txid)); // may have misses
        }
    }
}

pub(crate) struct FeeHistogram {
    /// bins[64-i] contains transactions' statistics inside the fee band of [2**(i-1), 2**i).
    /// bins[64] = [0, 1)
    /// bins[63] = [1, 2)
    /// bins[62] = [2, 4)
    /// bins[61] = [4, 8)
    /// bins[60] = [8, 16)
    /// ...
    /// bins[1] = [2**62, 2**63)
    /// bins[0] = [2**63, 2**64)
    vsize: [u64; FeeHistogram::BINS],
    count: [u64; FeeHistogram::BINS],
}

impl Default for FeeHistogram {
    fn default() -> Self {
        Self {
            vsize: [0; FeeHistogram::BINS],
            count: [0; FeeHistogram::BINS],
        }
    }
}

impl FeeHistogram {
    const BINS: usize = 65; // 0..=64

    fn empty() -> Self {
        Self::new(std::iter::empty())
    }

    fn new(items: impl Iterator<Item = (Amount, u64)>) -> Self {
        let mut result = FeeHistogram::default();
        for (fee, vsize) in items {
            let fee_rate = fee.to_sat() / vsize;
            let index = usize::try_from(fee_rate.leading_zeros()).unwrap();
            // skip transactions with too low fee rate (<1 sat/vB)
            if let Some(bin) = result.vsize.get_mut(index) {
                *bin += vsize
            }
            if let Some(bin) = result.count.get_mut(index) {
                *bin += 1
            }
        }
        result
    }
}

impl Serialize for FeeHistogram {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.vsize.len()))?;
        // https://electrumx-spesmilo.readthedocs.io/en/latest/protocol-methods.html#mempool-get-fee-histogram
        let fee_rates =
            (0..FeeHistogram::BINS).map(|i| std::u64::MAX.checked_shr(i as u32).unwrap_or(0));
        fee_rates
            .zip(self.vsize.iter().copied())
            .skip_while(|(_fee_rate, vsize)| *vsize == 0)
            .try_for_each(|element| seq.serialize_element(&element))?;
        seq.end()
    }
}

#[cfg(test)]
mod tests {
    use super::FeeHistogram;
    use bitcoin::Amount;
    use serde_json::json;

    #[test]
    fn test_histogram() {
        let items = vec![
            (Amount::from_sat(20), 10),
            (Amount::from_sat(10), 10),
            (Amount::from_sat(60), 10),
            (Amount::from_sat(30), 10),
            (Amount::from_sat(70), 10),
            (Amount::from_sat(50), 10),
            (Amount::from_sat(40), 10),
            (Amount::from_sat(80), 10),
            (Amount::from_sat(1), 100),
        ];
        let hist = FeeHistogram::new(items.into_iter());
        assert_eq!(
            json!(hist),
            json!([[15, 10], [7, 40], [3, 20], [1, 10], [0, 100]])
        );
    }
}

use anyhow::Result;
use bitcoin::consensus::serialize;
use bitcoin::{
    block::{Header, Version},
    hash_types::TxMerkleNode,
    hashes::{self, Hash},
    pow::CompactTarget,
    Block, BlockHash, Transaction,
};
use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::traits::KeyValueStoreLike;
use metashrew_runtime::MetashrewRuntime;

pub const WASM: &'static [u8] = include_bytes!(env!("METASHREW_MINIMAL_WASM_PATH"));

/// Configuration for setting up a test environment
pub struct TestConfig {
    pub wasm: &'static [u8],
}

impl TestConfig {
    /// Create a new default test configuration
    pub fn new() -> Self {
        Self { wasm: WASM }
    }

    /// Create a new MetashrewRuntime for testing
    pub fn create_runtime(&self) -> Result<MetashrewRuntime<MemStoreAdapter>> {
        MetashrewRuntime::new(self.wasm, MemStoreAdapter::new(), vec![])
    }

    pub fn create_runtime_from_adapter<T: KeyValueStoreLike + Clone + Send + Sync + 'static>(
        &self,
        store: T,
    ) -> Result<MetashrewRuntime<T>> {
        MetashrewRuntime::new(self.wasm, store, vec![])
    }
}

impl Default for TestConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// General-purpose test utilities
pub struct TestUtils;

impl TestUtils {
    /// Creates a simple test block with a specified height and previous block hash.
    pub fn create_test_block(height: u32, prev_hash: BlockHash) -> Block {
        Self::create_test_block_with_tx(height, prev_hash, vec![])
    }

    /// Creates a simple test block with a specified height, previous block hash, and transactions.
    pub fn create_test_block_with_tx(
        height: u32,
        prev_hash: BlockHash,
        txdata: Vec<Transaction>,
    ) -> Block {
        let merkle_root =
            bitcoin::merkle_tree::calculate_root(txdata.iter().map(|tx| tx.compute_txid()))
                .map(|hash| {
                    TxMerkleNode::from_raw_hash(hashes::sha256d::Hash::from_byte_array(
                        hash.to_byte_array(),
                    ))
                })
                .unwrap_or(TxMerkleNode::all_zeros());
        let header = Header {
            version: Version::from_consensus(1),
            prev_blockhash: prev_hash,
            merkle_root,
            time: height,
            bits: CompactTarget::from_consensus(0),
            nonce: 0,
        };
        Block { header, txdata }
    }

    /// Creates a simple unique transaction.
    pub fn create_test_transaction(value: u64) -> Transaction {
        use bitcoin::{
            absolute,
            script::{self},
            OutPoint, ScriptBuf, Sequence, TxIn, TxOut, Witness,
        };
        let script_sig = script::Builder::new()
            .push_int(value as i64)
            .into_script();
        Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: absolute::LockTime::from_consensus(0),
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig,
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: bitcoin::Amount::from_sat(value),
                script_pubkey: ScriptBuf::new(),
            }],
        }
    }

    /// Serializes a block into a byte vector.
    pub fn serialize_block(block: &Block) -> Vec<u8> {
        serialize(block)
    }
}

use anyhow::{Context, Result};

use bitcoin::{Network, Amount, BlockHash, Transaction, Txid};
use bitcoincore_rpc::{json, jsonrpc, Auth, Client, RpcApi};
use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::{
    chain::{Chain, NewHeader},
    config::Config,
    metrics::Metrics,
    p2p::Connection,
    signals::ExitFlag,
    types::SerBlock,
};

enum PollResult {
    Done(Result<()>),
    Retry,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetBlockchainInfoResultPermissive {
    /// The current number of blocks processed in the server
    pub blocks: u64,
    /// The current number of headers we have validated
    pub headers: u64,
    /// The hash of the currently best block
    #[serde(rename = "bestblockhash")]
    pub best_block_hash: bitcoin::BlockHash,
    /// The current difficulty
    pub difficulty: f64,
    /// Median time for the current best block
    #[serde(rename = "mediantime")]
    pub median_time: u64,
    /// Estimate of verification progress [0..1]
    #[serde(rename = "verificationprogress")]
    pub verification_progress: f64,
    /// Estimate of whether this node is in Initial Block Download mode
    #[serde(rename = "initialblockdownload")]
    pub initial_block_download: bool,
    /// Total amount of work in active chain, in hexadecimal
    #[serde(rename = "chainwork", with = "bitcoincore_rpc::json::serde_hex")]
    pub chain_work: Vec<u8>,
    /// The estimated size of the block and undo files on disk
    pub size_on_disk: u64,
    /// If the blocks are subject to pruning
    pub pruned: bool,
    /// Lowest-height complete block stored (only present if pruning is enabled)
    #[serde(rename = "pruneheight")]
    pub prune_height: Option<u64>,
    /// Whether automatic pruning is enabled (only present if pruning is enabled)
    pub automatic_pruning: Option<bool>,
    /// The target size used by pruning (only present if automatic pruning is enabled)
    pub prune_target_size: Option<u64>,
    /// Status of softforks in progress
    #[serde(default)]
    pub softforks: serde_json::Value,
    /// Any network and blockchain warnings.
    pub warnings: String,
}

fn rpc_poll(client: &mut Client, skip_block_download_wait: bool) -> PollResult {
    debug!("{}", "JSON-RPC Poll");
    return match client.call("getblockchaininfo", &[]) {
        Ok(result) => {
            let info: GetBlockchainInfoResultPermissive = serde_json::from_value::<GetBlockchainInfoResultPermissive>(result).unwrap();
            if skip_block_download_wait {
                // bitcoind RPC is available, don't wait for block download to finish
                return PollResult::Done(Ok(()));
            }
            let left_blocks = info.headers - info.blocks;
            if info.initial_block_download || left_blocks > 0 {
                info!(
                    "waiting for {} blocks to download{}",
                    left_blocks,
                    if info.initial_block_download {
                        " (IBD)"
                    } else {
                        ""
                    }
                );
                return PollResult::Retry;
            }
            PollResult::Done(Ok(()))
        }
        Err(err) => {
            if let Some(e) = extract_bitcoind_error(&err) {
                if e.code == -28 {
                    debug!("waiting for RPC warmup: {}", e.message);
                    return PollResult::Retry;
                }
            }
            PollResult::Done(Err(err).context("daemon not available"))
        }
    }
}

fn read_cookie(path: &Path) -> Result<(String, String)> {
    // Load username and password from bitcoind cookie file:
    // * https://github.com/bitcoin/bitcoin/pull/6388/commits/71cbeaad9a929ba6a7b62d9b37a09b214ae00c1a
    // * https://bitcoin.stackexchange.com/questions/46782/rpc-cookie-authentication
    let mut file = File::open(path)
        .with_context(|| format!("failed to open bitcoind cookie file: {}", path.display()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .with_context(|| format!("failed to read bitcoind cookie from {}", path.display()))?;

    let parts: Vec<&str> = contents.splitn(2, ':').collect();
    ensure!(
        parts.len() == 2,
        "failed to parse bitcoind cookie - missing ':' separator"
    );
    Ok((parts[0].to_owned(), parts[1].to_owned()))
}

fn rpc_connect(config: &Config) -> Result<Client> {
    let rpc_url = format!("http://{}", config.daemon_rpc_addr);
    // Allow `wait_for_new_block` to take a bit longer before timing out.
    // See https://github.com/romanz/metashrew/issues/495 for more details.
    let builder = jsonrpc::simple_http::SimpleHttpTransport::builder()
        .url(&rpc_url)?
        .timeout(config.jsonrpc_timeout);
    let builder = match config.daemon_auth.get_auth() {
        Auth::None => builder,
        Auth::UserPass(user, pass) => builder.auth(user, Some(pass)),
        Auth::CookieFile(path) => {
            let (user, pass) = read_cookie(&path)?;
            builder.auth(user, Some(pass))
        }
    };
    debug!("{}", "building JSON-RPC client");
    Ok(Client::from_jsonrpc(jsonrpc::Client::with_transport(
        builder.build(),
    )))
}

pub struct Daemon {
    p2p: Mutex<Connection>,
    rpc: Client,
}

impl Daemon {
    pub(crate) fn connect(
        config: &Config,
        exit_flag: &ExitFlag,
        metrics: &Metrics,
    ) -> Result<Self> {
        let mut rpc = rpc_connect(config)?;
        debug!("{}", "JSON-RPC connected");

        loop {
            exit_flag
                .poll()
                .context("bitcoin RPC polling interrupted")?;
            match rpc_poll(&mut rpc, config.skip_block_download_wait) {
                PollResult::Done(result) => {
                    result.context("bitcoind RPC polling failed")?;
                    break; // on success, finish polling
                }
                PollResult::Retry => {
                    std::thread::sleep(std::time::Duration::from_secs(1)); // wait a bit before polling
                }
            }
        }

        let network_info = rpc.get_network_info()?;
        println!("version: {:?}", network_info);
        if network_info.version < 21_00_00 {
            bail!("metashrew requires bitcoind 0.21+");
        }
        if !network_info.network_active {
            bail!("metashrew requires active bitcoind p2p network");
        }
        /*
        let info = rpc.get_blockchain_info()?;
        if info.pruned {
            bail!("metashrew requires non-pruned bitcoind node");
        }
        */

        let p2p = Mutex::new(Connection::connect(
            config.network,
            config.daemon_p2p_addr,
            metrics,
            config.signet_magic,
        )?);
        Ok(Self { p2p, rpc })
    }

    pub(crate) fn estimate_fee(&self, nblocks: u16) -> Result<Option<Amount>> {
        Ok(self
            .rpc
            .estimate_smart_fee(nblocks, None)
            .context("failed to estimate fee")?
            .fee_rate)
    }

    pub(crate) fn get_relay_fee(&self) -> Result<Amount> {

        Ok(self
            .rpc
            .get_network_info()
            .context("failed to get relay fee")?
            .relay_fee)
    }

    pub(crate) fn broadcast(&self, tx: &Transaction) -> Result<Txid> {
        self.rpc
            .send_raw_transaction(tx)
            .context("failed to broadcast transaction")
    }

    pub(crate) fn get_transaction_info(
        &self,
        txid: &Txid,
        blockhash: Option<BlockHash>,
    ) -> Result<Value> {
        // No need to parse the resulting JSON, just return it as-is to the client.
        self.rpc
            .call(
                "getrawtransaction",
                &[json!(txid), json!(true), json!(blockhash)],
            )
            .context("failed to get transaction info")
    }

    pub(crate) fn get_transaction_hex(
        &self,
        txid: &Txid,
        blockhash: Option<BlockHash>,
    ) -> Result<Value> {
        use bitcoin::consensus::serde::{hex::Lower, Hex, With};

        let tx = self.get_transaction(txid, blockhash)?;
        #[derive(serde::Serialize)]
        #[serde(transparent)]
        struct TxAsHex(#[serde(with = "With::<Hex<Lower>>")] Transaction);
        serde_json::to_value(TxAsHex(tx)).map_err(Into::into)
    }

    pub(crate) fn get_transaction(
        &self,
        txid: &Txid,
        blockhash: Option<BlockHash>,
    ) -> Result<Transaction> {
        self.rpc
            .get_raw_transaction(txid, blockhash.as_ref())
            .context("failed to get transaction")
    }

    pub(crate) fn get_block_txids(&self, blockhash: BlockHash) -> Result<Vec<Txid>> {
        Ok(self
            .rpc
            .get_block_info(&blockhash)
            .context("failed to get block txids")?
            .tx)
    }

    pub(crate) fn get_mempool_txids(&self) -> Result<Vec<Txid>> {
        self.rpc
            .get_raw_mempool()
            .context("failed to get mempool txids")
    }

    pub(crate) fn get_mempool_entry(&self, txid: &Txid) -> Result<json::GetMempoolEntryResult> {
        self.rpc
            .get_mempool_entry(txid)
            .context("failed to get mempool entry")
    }

    pub(crate) fn get_new_headers(&self, chain: &Chain) -> Result<Vec<NewHeader>> {
        self.p2p.lock().get_new_headers(chain)
    }

    pub(crate) fn for_blocks<B, F>(&self, blockhashes: B, func: F) -> Result<()>
    where
        B: IntoIterator<Item = BlockHash>,
        F: FnMut(BlockHash, SerBlock),
    {
        self.p2p.lock().for_blocks(blockhashes, func)
    }

    pub(crate) fn new_block_notification(&self) -> Receiver<()> {
        self.p2p.lock().new_block_notification()
    }
}

pub(crate) type RpcError = bitcoincore_rpc::jsonrpc::error::RpcError;

pub(crate) fn extract_bitcoind_error(err: &bitcoincore_rpc::Error) -> Option<&RpcError> {
    use bitcoincore_rpc::{
        jsonrpc::error::Error::Rpc as ServerError, Error::JsonRpc as JsonRpcError,
    };
    match err {
        JsonRpcError(ServerError(e)) => Some(e),
        _ => None,
    }
}

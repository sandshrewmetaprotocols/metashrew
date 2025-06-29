//! Test to reproduce JSON-RPC server hanging issue.
//!
//! This test simulates a scenario where the indexer holds a long-lived write lock
//! on the sync engine, while the JSON-RPC server attempts to acquire a read lock
//! to serve a request. This should result in a deadlock, causing the request to hang.

use super::block_builder::ChainBuilder;
use super::TestConfig;
use anyhow::Result;
use async_trait::async_trait;
use bitcoin::{hashes::Hash, BlockHash};
use memshrew_runtime::MemStoreAdapter;
use metashrew_sync::{
    adapters::MetashrewRuntimeAdapter, BitcoinNodeAdapter, BlockInfo, ChainTip, SyncResult,
};
use rockshrew_mono::{run, Args};
use std::net::TcpListener;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

/// A mock Bitcoin node that provides blocks from a ChainBuilder
#[derive(Clone)]
struct MockNode {
    chain: Arc<Mutex<ChainBuilder>>,
}

impl MockNode {
    fn new(chain: ChainBuilder) -> Self {
        Self {
            chain: Arc::new(Mutex::new(chain)),
        }
    }
}

#[async_trait]
impl BitcoinNodeAdapter for MockNode {
    async fn get_tip_height(&self) -> SyncResult<u32> {
        let chain = self.chain.lock().await;
        Ok(chain.height())
    }

    async fn get_block_hash(&self, height: u32) -> SyncResult<Vec<u8>> {
        let chain = self.chain.lock().await;
        Ok(chain
            .get_block(height)
            .map(|b| b.block_hash().to_byte_array().to_vec())
            .unwrap_or_else(|| BlockHash::all_zeros().to_byte_array().to_vec()))
    }

    async fn get_block_data(&self, height: u32) -> SyncResult<Vec<u8>> {
        let chain = self.chain.lock().await;
        let block = chain.get_block(height).unwrap();
        Ok(metashrew_support::utils::consensus_encode(block)?)
    }

    async fn get_block_info(&self, height: u32) -> SyncResult<BlockInfo> {
        let hash = self.get_block_hash(height).await?;
        let data = self.get_block_data(height).await?;
        Ok(BlockInfo {
            height,
            hash,
            data,
        })
    }

    async fn get_chain_tip(&self) -> SyncResult<ChainTip> {
        let chain = self.chain.lock().await;
        Ok(ChainTip {
            height: chain.height(),
            hash: chain.tip_hash().to_byte_array().to_vec(),
        })
    }

    async fn is_connected(&self) -> bool {
        true
    }
}

/// Finds a free port to bind the server to.
fn find_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn test_rpc_does_not_hang_on_write_lock() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    let config = TestConfig::new();
    let storage_adapter = MemStoreAdapter::new();
    let chain = ChainBuilder::new().add_blocks(10);
    let node_adapter = MockNode::new(chain.clone());
    let runtime = config.create_runtime_from_adapter(storage_adapter.clone())?;
    let runtime_adapter = MetashrewRuntimeAdapter::new(runtime);
    let port = find_free_port();

    let args = Args {
        daemon_rpc_url: "".to_string(),
        indexer: config.wasm_path,
        db_path: Default::default(),
        start_block: Some(0),
        auth: None,
        host: "127.0.0.1".to_string(),
        port,
        label: None,
        exit_at: Some(11),
        pipeline_size: Some(1),
        cors: Some("*".to_string()),
        snapshot_directory: None,
        snapshot_interval: 1000,
        repo: None,
        max_reorg_depth: 100,
        reorg_check_threshold: 6,
    };

    tokio::spawn(run(
        args,
        node_adapter,
        storage_adapter,
        runtime_adapter,
        None,
    ));

    tokio::time::sleep(Duration::from_secs(2)).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "metashrew_view",
        "params": ["getbytecode", "0x", "2:50"],
        "id": 1
    });

    let request = client
        .post(format!("http://127.0.0.1:{}", port))
        .json(&request_body)
        .send();

    let response_result = timeout(Duration::from_secs(5), request).await;

    assert!(
        response_result.is_ok(),
        "Request timed out, but it should have completed."
    );
    let response = response_result.unwrap().unwrap();
    assert!(response.status().is_success());

    Ok(())
}
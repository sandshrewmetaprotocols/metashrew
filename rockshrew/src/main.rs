use anyhow::{anyhow, Result};
use clap::{command, Parser};
use env_logger;
use hex;
use itertools::Itertools;
use log::debug;
use metashrew_rockshrew_runtime::{query_height, set_label, RocksDBRuntimeAdapter};
use metashrew_runtime::KeyValueStoreLike;
use metashrew_runtime::MetashrewRuntime;
use rocksdb::{DB, Options};
use reqwest::{Response, Url};
use retry::{delay::Fixed, retry, OperationResult};
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::{Number, Value};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use task;
use tokio;
use tokio::time::{sleep, Duration};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    daemon_rpc_url: String,
    #[arg(long)]
    indexer: String,
    #[arg(long)]
    db_path: String,
    #[arg(long)]
    start_block: Option<u32>,
    #[arg(long)]
    auth: Option<String>,
    #[arg(long)]
    label: Option<String>,
}

const HEIGHT_TO_HASH: &'static str = "/__INTERNAL/height-to-hash/";

static mut _HEIGHT: u32 = 0;

#[derive(Serialize)]
pub struct JsonRpcRequest<T> {
    pub id: u32,
    pub jsonrpc: String,
    pub method: String,
    pub params: Vec<T>,
}

#[derive(Deserialize)]
pub struct JsonRpcResponse {
    pub id: u32,
    pub result: Option<String>,
    pub error: Option<Value>,
}

#[derive(Deserialize)]
pub struct BlockCountResponse {
    pub id: u32,
    pub result: Option<u32>,
    pub error: Option<Value>,
}

pub struct MetashrewRocksDBSync {
    runtime: MetashrewRuntime<RocksDBRuntimeAdapter>,
    args: Args,
    start_block: u32,
}

impl MetashrewRocksDBSync {
    async fn post_once(&self, body: String) -> Result<Response, reqwest::Error> {
        let response = reqwest::Client::new()
            .post(match self.args.auth.clone() {
                Some(v) => {
                    let mut url = Url::parse(self.args.daemon_rpc_url.as_str()).unwrap();
                    let (username, password) = v.split(":").next_tuple().unwrap();
                    url.set_username(username).unwrap();
                    url.set_password(Some(password)).unwrap();
                    println!("auth URL: {:?}", url);
                    url
                }
                None => {
                    println!(
                        "no auth URL: {:?}",
                        Url::parse(self.args.daemon_rpc_url.as_str()).unwrap()
                    );
                    Url::parse(self.args.daemon_rpc_url.as_str()).unwrap()
                }
            })
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await;
        return response;
    }

    async fn post(&self, body: String) -> Result<Response> {
        let mut count = 0;
        let mut response: Option<Response> = None;
        loop {
            match self.post_once(body.clone()).await {
                Ok(v) => {
                    response = Some(v);
                    break;
                }
                Err(e) => {
                    if count > 10 {
                        return Err(e.into());
                    } else {
                        count = count + 1;
                    }
                    debug!("err: retrying POST");
                    sleep(Duration::from_millis(3000)).await;
                }
            }
        }
        Ok(response.unwrap())
    }

    async fn fetch_blockcount(&self) -> Result<u32> {
        let response = self
            .post(serde_json::to_string(
                &(JsonRpcRequest::<u32> {
                    id: SystemTime::now()
                        .duration_since(UNIX_EPOCH)?
                        .as_secs()
                        .try_into()?,
                    jsonrpc: String::from("2.0"),
                    method: String::from("getblockcount"),
                    params: vec![],
                }),
            )?)
            .await?;

        Ok(response
            .json::<BlockCountResponse>()
            .await?
            .result
            .ok_or("")
            .map_err(|_| anyhow!("missing result from JSON-RPC response"))?)
    }

    pub async fn poll_connection(&self) -> rocksdb::DB {
        loop {
            let connected: Option<rocksdb::DB> = match self.runtime.context.lock().unwrap().db.is_open() {
                false => {
                    debug!("RocksDB connection failure -- retrying in 3s ...");
                    sleep(Duration::from_millis(3000)).await;
                    None
                }
                true => Some(self.runtime.context.lock().unwrap().db.clone()),
            };

            if let Some(v) = connected {
                return v;
            }
        }
    }

    pub async fn query_height(&self) -> Result<u32> {
        query_height(&self.poll_connection().await, self.start_block).await
    }

    async fn best_height(&self, block_number: u32) -> Result<u32> {
        let mut best: u32 = block_number;
        let response = self
            .post(serde_json::to_string(
                &(JsonRpcRequest::<u32> {
                    id: SystemTime::now()
                        .duration_since(UNIX_EPOCH)?
                        .as_secs()
                        .try_into()?,
                    jsonrpc: String::from("2.0"),
                    method: String::from("getblockcount"),
                    params: vec![],
                }),
            )?)
            .await?;
        let tip = response
            .json::<BlockCountResponse>()
            .await?
            .result
            .ok_or("")
            .map_err(|_| anyhow!("missing result from JSON-RPC response"))?;
        if best >= tip - std::cmp::min(6, tip) {
            loop {
                if best == 0 {
                    break;
                }
                let blockhash = self
                    .get_blockhash(best)
                    .await
                    .ok_or(anyhow!("failed to retrieve blockhash"))?;
                let remote_blockhash = self.fetch_blockhash(best).await?;
                if blockhash == remote_blockhash {
                    break;
                } else {
                    best = best - 1;
                }
            }
        }
        return Ok(best);
    }

    fn get_once(&self, key: &Vec<u8>) -> Result<Option<Vec<u8>>> {
        self.runtime
            .context
            .lock()
            .unwrap()
            .db
            .get(key)
            .map_err(|_| anyhow!("GET error against RocksDB"))
    }

    fn get(&self, key: &Vec<u8>) -> Result<Option<Vec<u8>>> {
        let mut count = 0;
        let mut response: Option<Option<Vec<u8>>> = None;
        loop {
            match self.get_once(key) {
                Ok(v) => {
                    response = Some(v);
                    break;
                }
                Err(e) => {
                    if count > 100 {
                        return Err(e.into());
                    } else {
                        count = count + 1;
                    }
                    debug!("err: retrying GET");
                }
            }
        }
        Ok(response.unwrap())
    }

    async fn get_blockhash(&self, block_number: u32) -> Option<Vec<u8>> {
        self.get(&(String::from(HEIGHT_TO_HASH) + &block_number.to_string()).into_bytes())
            .unwrap()
    }

    async fn fetch_blockhash(&self, block_number: u32) -> Result<Vec<u8>, anyhow::Error> {
        let response = self
            .post(serde_json::to_string(
                &(JsonRpcRequest::<u32> {
                    id: SystemTime::now()
                        .duration_since(UNIX_EPOCH)?
                        .as_secs()
                        .try_into()?,
                    jsonrpc: String::from("2.0"),
                    method: String::from("getblockhash"),
                    params: vec![block_number],
                }),
            )?)
            .await?;
        let blockhash = response
            .json::<JsonRpcResponse>()
            .await?
            .result
            .ok_or("")
            .map_err(|_| anyhow!("missing result from JSON-RPC response"))?;
        Ok(hex::decode(&blockhash)?)
    }

    fn put_once(&self, k: &Vec<u8>, v: &Vec<u8>) -> Result<()> {
        self.runtime
            .context
            .lock()
            .unwrap()
            .db
            .put(k, v)
            .map_err(|_| anyhow!("PUT error against RocksDB"))
    }

    fn put(&self, key: &Vec<u8>, val: &Vec<u8>) -> Result<()> {
        let mut count = 0;
        loop {
            match self.put_once(key, val) {
                Ok(v) => {
                    return Ok(v);
                }
                Err(e) => {
                    if count > i32::MAX {
                        return Err(e.into());
                    } else {
                        count = count + 1;
                    }
                    debug!("err: retrying PUT");
                }
            }
        }
    }

    async fn pull_block(&self, block_number: u32) -> Result<Vec<u8>, anyhow::Error> {
        loop {
            let count = self.fetch_blockcount().await?;
            if block_number > count {
                sleep(Duration::from_millis(3000)).await;
            } else {
                break;
            }
        }
        let blockhash = self.fetch_blockhash(block_number).await.unwrap();
        self.poll_connection().await;
        self.put(
            &(String::from(HEIGHT_TO_HASH) + block_number.to_string().as_str()).into_bytes(),
            &blockhash,
        )
        .unwrap();
        Ok(hex::decode(
            self.post(serde_json::to_string(
                &(JsonRpcRequest::<Value> {
                    id: (<u64 as TryInto<i32>>::try_into(
                        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
                    )? + 1)
                        .try_into()?,
                    jsonrpc: String::from("2.0"),
                    method: String::from("getblock"),
                    params: vec![
                        Value::String(hex::encode(&blockhash)),
                        Value::Number(Number::from(0)),
                    ],
                }),
            )?)
            .await?
            .json::<JsonRpcResponse>()
            .await?
            .result
            .ok_or("")
            .map_err(|e| anyhow!("missing result from JSON-RPC response"))?,
        )?)
    }

    async fn run(&mut self) -> Result<()> {
        let mut i: u32 = self.query_height().await?;
        loop {
            let best: u32 = match self.best_height(i).await {
                Ok(v) => v,
                Err(_) => i,
            };
            self.runtime.context.lock().unwrap().block = self.pull_block(best).await.unwrap();
            self.runtime.context.lock().unwrap().height = best;
            self.runtime.context.lock().unwrap().db.set_height(best);
            if let Err(_) = self.runtime.run() {
                debug!("respawn cache");
                self.runtime.refresh_memory();
                if let Err(e) = self.runtime.run() {
                    panic!("runtime run failed after retry: {}", e);
                }
            }
            i = i + 1;
            unsafe {
                _HEIGHT = i;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
    if let Some(ref label) = args.label {
        set_label(label.clone());
    }
    let start_block = args.start_block.unwrap_or_else(|| 0);
    let indexer: PathBuf = args.indexer.clone().into();
    let db_path: String = args.db_path.clone();
    
    // Configure RocksDB options for optimal performance
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_max_open_files(10000);
    opts.set_use_fsync(false);
    opts.set_bytes_per_sync(8388608); // 8MB
    opts.optimize_for_point_lookup(1024);
    opts.set_table_cache_num_shard_bits(6);
    opts.set_max_write_buffer_number(6);
    opts.set_write_buffer_size(256 * 1024 * 1024);
    opts.set_target_file_size_base(256 * 1024 * 1024);
    opts.set_min_write_buffer_number_to_merge(2);
    opts.set_level_zero_file_num_compaction_trigger(4);
    opts.set_level_zero_slowdown_writes_trigger(20);
    opts.set_level_zero_stop_writes_trigger(30);
    opts.set_max_background_jobs(4);
    opts.set_max_background_compactions(4);
    opts.set_disable_auto_compactions(false);
    
    let mut sync = MetashrewRocksDBSync {
        runtime: MetashrewRuntime::load(
            indexer,
            RocksDBRuntimeAdapter::open(db_path, opts).unwrap()
        ).unwrap(),
        args,
        start_block,
    };
    sync.run().await.unwrap();
}
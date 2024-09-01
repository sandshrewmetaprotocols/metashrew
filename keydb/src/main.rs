use anyhow::{anyhow, Result};
use clap::{command, Parser};
use env_logger;
use hex;
use itertools::Itertools;
use log::debug;
use metashrew_keydb_runtime::{set_label, query_height, RedisRuntimeAdapter};
use metashrew_runtime::KeyValueStoreLike;
use metashrew_runtime::MetashrewRuntime;
use redis;
use redis::Commands;
use reqwest::{Response, Url};
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::{Number, Value};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
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
    redis: String,
    #[arg(long)]
    start_block: Option<u32>,
    #[arg(long)]
    auth: Option<String>,
    #[arg(long)]
    label: Option<String>
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
    pub result: String,
    pub error: Value,
}

#[derive(Deserialize)]
pub struct BlockCountResponse {
    pub id: u32,
    pub result: u32,
    pub error: Value,
}

pub struct MetashrewKeyDBSync {
    runtime: MetashrewRuntime<RedisRuntimeAdapter>,
    args: Args,
    start_block: u32,
}

impl MetashrewKeyDBSync {
    async fn post(&self, body: String) -> Result<Response, reqwest::Error> {
        let response = reqwest::Client::new()
            .post(match self.args.auth.clone() {
                Some(v) => {
                    let mut url = Url::parse(self.args.daemon_rpc_url.as_str()).unwrap();
                    let (username, password) = v.split(":").next_tuple().unwrap();
                    url.set_username(username).unwrap();
                    url.set_password(Some(password)).unwrap();
                    url
                }
                None => Url::parse(self.args.daemon_rpc_url.as_str()).unwrap(),
            })
            .body(body)
            .send()
            .await;
        return response;
    }
    /*
    async fn post_get_text(&self, body: String) -> Result<String, reqwest::Error> {
        let response = reqwest::Client::new()
            .post(match self.args.auth.clone() {
                Some(v) => {
                    let mut url = Url::parse((self.args.daemon_rpc_url.as_str())).unwrap();
                    let (username, password) = self.args.auth.as_ref().unwrap().split(":").next_tuple().unwrap();
                    url.set_username(username);
                    url.set_password(Some(password));
                    info!("url: {}", url);
                    url
                }
                None => Url::parse(self.args.daemon_rpc_url.as_str()).unwrap(),
            })
            .body(body)
            .send()
            .await;
        return response.unwrap().text().await;
    }
    */
    /*
    async fn fetch_blockcount_text(&self) {
        let response = self
            .post_get_text(serde_json::to_string(&JsonRpcRequest::<u32> {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH).unwrap()
                    .as_secs()
                    .try_into().unwrap(),
                jsonrpc: String::from("2.0"),
                method: String::from("getblockcount"),
                params: vec![],
            }).unwrap())
            .await.unwrap();
          info!("blockcount response: {}", response);
    }
    */
    async fn fetch_blockcount(&self) -> Result<u32> {
        let response = self
            .post(serde_json::to_string(&JsonRpcRequest::<u32> {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_secs()
                    .try_into()?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblockcount"),
                params: vec![],
            })?)
            .await?;

        Ok(response.json::<BlockCountResponse>().await?.result)
    }

    pub async fn poll_connection(&self) -> redis::Connection {
        loop {
            let connected: Option<redis::Connection> =
                match self.runtime.context.lock().unwrap().db.connect() {
                    Err(_) => {
                        debug!("KeyDB connection failure -- retrying in 3s ...");
                        sleep(Duration::from_millis(3000)).await;
                        None
                    }
                    Ok(mut v) => match v.get::<Vec<u8>, Vec<u8>>("POLL".into()) {
                        Ok(_) => Some(v),
                        Err(_) => {
                            debug!("KeyDB connection failure -- retrying in 3s ...");
                            sleep(Duration::from_millis(3000)).await;
                            None
                        }
                    },
                };

            if let Some(v) = connected {
                return v;
            }
        }
    }
    pub async fn query_height(&self) -> Result<u32> {
        query_height(&mut self.poll_connection().await, self.start_block).await
    }
    async fn best_height(&self, block_number: u32) -> Result<u32> {
        let mut best: u32 = block_number;
        let response = self
            .post(serde_json::to_string(&JsonRpcRequest::<u32> {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_secs()
                    .try_into()?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblockcount"),
                params: vec![],
            })?)
            .await?;
        let tip = response.json::<BlockCountResponse>().await?.result;
        if best >= tip - 6 {
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

    async fn get_blockhash(&self, block_number: u32) -> Option<Vec<u8>> {
        self.runtime
            .context
            .lock()
            .unwrap()
            .db
            .get(&(String::from(HEIGHT_TO_HASH) + &block_number.to_string()).into_bytes())
            .unwrap()
    }

    async fn fetch_blockhash(&self, block_number: u32) -> Result<Vec<u8>, anyhow::Error> {
        let response = self
            .post(serde_json::to_string(&JsonRpcRequest::<u32> {
                id: SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_secs()
                    .try_into()?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblockhash"),
                params: vec![block_number],
            })?)
            .await?;
        let blockhash = response.json::<JsonRpcResponse>().await?.result;
        Ok(hex::decode(&blockhash)?)
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
        self.runtime
            .context
            .lock()
            .unwrap()
            .db
            .put(
                &(String::from(HEIGHT_TO_HASH) + block_number.to_string().as_str()).into_bytes(),
                &blockhash,
            )
            .unwrap();
        Ok(hex::decode(
            self.post(serde_json::to_string(&JsonRpcRequest::<Value> {
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
            })?)
            .await?
            .json::<JsonRpcResponse>()
            .await?
            .result,
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
            self.runtime.context.lock().unwrap().db.2 = best;
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
    let redis_uri: String = args.redis.clone();
    let mut sync = MetashrewKeyDBSync {
        runtime: MetashrewRuntime::load(indexer, RedisRuntimeAdapter::open(redis_uri).unwrap())
            .unwrap(),
        args,
        start_block,
    };
    //    sync.fetch_blockcount_text().await;
    sync.run().await.unwrap();
}

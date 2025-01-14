use anyhow::{anyhow, Result};
use aws_config::meta::region::RegionProviderChain;
<<<<<<< HEAD
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::{Client as DynamoDbClient, Error as DynamoDbError};
=======
use aws_sdk_dynamodb::{Client, Region};
use aws_sdk_dynamodb::model::{AttributeValue, PutRequest, WriteRequest};
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
use clap::{command, Parser};
use env_logger;
use hex;
use itertools::Itertools;
use log::debug;
<<<<<<< HEAD
use metashrew_dynamodb_runtime::{query_height, set_label, DynamoDbRuntimeAdapter};
=======
use metashrew_dynamodb_runtime::{query_height, set_label, DynamoDBRuntimeAdapter}; 
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
use metashrew_runtime::KeyValueStoreLike;
use metashrew_runtime::MetashrewRuntime;
use reqwest::{Response, Url};
use retry::{delay::Fixed, retry, OperationResult};
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::{Number, Value};
<<<<<<< HEAD
use std::collections::HashMap;
=======
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
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
<<<<<<< HEAD
    table_name: String,
    #[arg(long)]
    region: Option<String>,
=======
    redis: String,
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
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
<<<<<<< HEAD
    pub result: String,
    pub error: Value,
=======
    pub result: Option<String>,
    pub error: Option<Value>,
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
}

#[derive(Deserialize)]
pub struct BlockCountResponse {
    pub id: u32,
<<<<<<< HEAD
    pub result: u32,
    pub error: Value,
}

pub struct MetashrewDynamoDbSync {
    runtime: MetashrewRuntime<DynamoDbRuntimeAdapter>,
    args: Args,
    start_block: u32,
    client: DynamoDbClient,
}

impl MetashrewDynamoDbSync {
    async fn new(args: Args, start_block: u32) -> Result<Self> {
        let region_provider = RegionProviderChain::first_try(
            args.region
                .clone()
                .map(|r| aws_sdk_dynamodb::config::Region::new(r)),
        )
        .or_default_provider()
        .or_else("us-east-1");
        let config = aws_config::from_env().region(region_provider).load().await;
        let client = DynamoDbClient::new(&config);

        Ok(Self {
            runtime: MetashrewRuntime::load(
                args.indexer.clone().into(),
                DynamoDbRuntimeAdapter::new(args.table_name.clone()).await?,
            )?,
            args,
            start_block,
            client,
        })
    }

=======
    pub result: Option<u32>,
    pub error: Option<Value>,
}

pub struct MetashrewKeyDBSync {
    runtime: MetashrewRuntime<RedisRuntimeAdapter>,
    args: Args,
    start_block: u32,
}

impl MetashrewKeyDBSync {
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
    async fn post_once(&self, body: String) -> Result<Response, reqwest::Error> {
        let response = reqwest::Client::new()
            .post(match self.args.auth.clone() {
                Some(v) => {
                    let mut url = Url::parse(self.args.daemon_rpc_url.as_str()).unwrap();
                    let (username, password) = v.split(":").next_tuple().unwrap();
                    url.set_username(username).unwrap();
                    url.set_password(Some(password)).unwrap();
<<<<<<< HEAD
                    url
                }
                None => Url::parse(self.args.daemon_rpc_url.as_str()).unwrap(),
=======
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
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
            })
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await;
        return response;
    }
<<<<<<< HEAD

=======
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
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
<<<<<<< HEAD

=======
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
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
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

<<<<<<< HEAD
        Ok(response.json::<BlockCountResponse>().await?.result)
    }

    async fn get_item(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let key_attr = AttributeValue::B(key.to_vec().into());
        let result = self
            .client
            .get_item()
            .table_name(&self.args.table_name)
            .key("key", key_attr)
            .send()
            .await?;

        Ok(result.item().and_then(|item| {
            item.get("value")
                .and_then(|av| av.as_b().ok())
                .map(|b| b.as_ref().to_vec())
        }))
    }

    async fn get_blockhash(&self, block_number: u32) -> Option<Vec<u8>> {
        self.get_item(&(String::from(HEIGHT_TO_HASH) + &block_number.to_string()).into_bytes())
            .await
            .ok()
            .flatten()
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
        let blockhash = response.json::<JsonRpcResponse>().await?.result;
        Ok(hex::decode(&blockhash)?)
    }

    async fn put_item(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut item = HashMap::new();
        item.insert(String::from("key"), AttributeValue::B(key.to_vec().into()));
        item.insert(
            String::from("value"),
            AttributeValue::B(value.to_vec().into()),
        );

        self.client
            .put_item()
            .table_name(&self.args.table_name)
            .set_item(Some(item))
            .send()
            .await?;

        Ok(())
    }

=======
        Ok(response
            .json::<BlockCountResponse>()
            .await?
            .result
            .ok_or("")
            .map_err(|_| anyhow!("missing result from JSON-RPC response"))?)
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
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
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
<<<<<<< HEAD
        let tip = response.json::<BlockCountResponse>().await?.result;
        if best >= tip - 6 {
=======
        let tip = response
            .json::<BlockCountResponse>()
            .await?
            .result
            .ok_or("")
            .map_err(|_| anyhow!("missing result from JSON-RPC response"))?;
        if best >= tip - std::cmp::min(6, tip) {
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
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
<<<<<<< HEAD
=======
    fn get_once(&self, key: &Vec<u8>) -> Result<Option<Vec<u8>>> {
        self.runtime
            .context
            .lock()
            .unwrap()
            .db
            .get(key)
            .map_err(|_| anyhow!("GET error against redis socket"))
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
            .map_err(|_| anyhow!("PUT error against socket"))
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
                    debug!("err: retrying GET");
                }
            }
        }
    }
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36

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
<<<<<<< HEAD

        self.put_item(
            &(String::from(HEIGHT_TO_HASH) + block_number.to_string().as_str()).into_bytes(),
            &blockhash,
        )
        .await?;

=======
        self.poll_connection().await;
        self.put(
            &(String::from(HEIGHT_TO_HASH) + block_number.to_string().as_str()).into_bytes(),
            &blockhash,
        )
        .unwrap();
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
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
<<<<<<< HEAD
            .result,
        )?)
    }

    async fn run(&mut self) -> Result<()> {
        let mut i: u32 = query_height(&self.client, &self.args.table_name, self.start_block).await?;
=======
            .result
            .ok_or("")
            .map_err(|e| anyhow!("missing result from JSON-RPC response"))?,
        )?)
    }
    async fn run(&mut self) -> Result<()> {
        let mut i: u32 = self.query_height().await?;
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
        loop {
            let best: u32 = match self.best_height(i).await {
                Ok(v) => v,
                Err(_) => i,
            };
            self.runtime.context.lock().unwrap().block = self.pull_block(best).await.unwrap();
            self.runtime.context.lock().unwrap().height = best;
<<<<<<< HEAD
=======
            self.runtime.context.lock().unwrap().db.2 = best;
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
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
<<<<<<< HEAD
async fn main() -> Result<()> {
=======
async fn main() {
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
    env_logger::init();
    let args = Args::parse();
    if let Some(ref label) = args.label {
        set_label(label.clone());
    }
<<<<<<< HEAD
    let start_block = args.start_block.unwrap_or(0);

    let mut sync = MetashrewDynamoDbSync::new(args, start_block).await?;
    sync.run().await
=======
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
>>>>>>> 613a98aaf3250424ab6b8d8a9ff7b0b7f20f6e36
}

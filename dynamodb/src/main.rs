use anyhow::{anyhow, Result};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::{Client as DynamoDbClient, Error as DynamoDbError};
use clap::{command, Parser};
use env_logger;
use hex;
use itertools::Itertools;
use log::debug;
use metashrew_dynamodb_runtime::{query_height, set_label, DynamoDbRuntimeAdapter};
use metashrew_runtime::KeyValueStoreLike;
use metashrew_runtime::MetashrewRuntime;
use reqwest::{Response, Url};
use retry::{delay::Fixed, retry, OperationResult};
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::{Number, Value};
use std::collections::HashMap;
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
    table_name: String,
    #[arg(long)]
    region: Option<String>,
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
    pub result: String,
    pub error: Value,
}

#[derive(Deserialize)]
pub struct BlockCountResponse {
    pub id: u32,
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

    async fn post_once(&self, body: String) -> Result<Response, reqwest::Error> {
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

        self.put_item(
            &(String::from(HEIGHT_TO_HASH) + block_number.to_string().as_str()).into_bytes(),
            &blockhash,
        )
        .await?;

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
            .result,
        )?)
    }

    async fn run(&mut self) -> Result<()> {
        let mut i: u32 = query_height(&self.client, &self.args.table_name, self.start_block).await?;
        loop {
            let best: u32 = match self.best_height(i).await {
                Ok(v) => v,
                Err(_) => i,
            };
            self.runtime.context.lock().unwrap().block = self.pull_block(best).await.unwrap();
            self.runtime.context.lock().unwrap().height = best;
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
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    if let Some(ref label) = args.label {
        set_label(label.clone());
    }
    let start_block = args.start_block.unwrap_or(0);

    let mut sync = MetashrewDynamoDbSync::new(args, start_block).await?;
    sync.run().await
}

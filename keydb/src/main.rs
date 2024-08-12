use anyhow::{anyhow, Result};
use clap::{command, Parser};
use env_logger;
use hex;
use log::debug;
use metashrew_runtime::{BatchLike, KeyValueStoreLike, MetashrewRuntime};
use redis;
use redis::Commands;
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::{Number, Value};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio;

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
}

pub struct RedisRuntimeAdapter(pub Arc<Mutex<redis::Connection>>);

pub struct RedisBatch(pub redis::Pipeline);

/*
impl ToRedisArgs for Vec<u8> {
  fn write_redis_args<W: ?Sized + RedisWrite>(&self, out: &mut W) {
    out.write_arg(self);
  }
}
*/

fn to_redis_args<T: AsRef<[u8]>>(v: T) -> Vec<Vec<u8>> {
    return vec![v.as_ref().try_into().unwrap()];
}

impl BatchLike for RedisBatch {
    fn default() -> Self {
        Self(redis::pipe())
    }
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, k: K, v: V) {
        self.0
            .cmd("SET")
            .arg(to_redis_args(k))
            .arg(to_redis_args(v))
            .ignore();
    }
}

impl Clone for RedisRuntimeAdapter {
    fn clone(&self) -> Self {
        return Self(self.0.clone());
    }
}

impl KeyValueStoreLike for RedisRuntimeAdapter {
    type Batch = RedisBatch;
    type Error = redis::RedisError;
    fn write(&self, batch: RedisBatch) -> Result<(), Self::Error> {
        let key_bytes: Vec<u8> = TIP_HEIGHT_KEY.as_bytes().to_vec();
        let height_bytes: Vec<u8> = (unsafe { _HEIGHT }).to_le_bytes().to_vec();
        let _ok: bool = self
            .0
            .lock()
            .unwrap()
            .set(to_redis_args(&key_bytes), to_redis_args(&height_bytes))
            .unwrap();
        batch.0.query(&mut self.0.lock().unwrap())
    }
    fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        self.0.lock().unwrap().get(to_redis_args(key))
    }
    fn delete<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error> {
        self.0.lock().unwrap().del(to_redis_args(key))
    }
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) -> Result<(), Self::Error> {
        self.0
            .lock()
            .unwrap()
            .set(to_redis_args(key), to_redis_args(value))
    }
}

const TIP_HEIGHT_KEY: &'static str = "/__INTERNAL/tip-height";
const HEIGHT_TO_HASH: &'static str = "/__INTERNAL/height-to-hash/";

static mut _HEIGHT: u32 = 0;

pub fn query_height(internal_db: &RedisRuntimeAdapter, start_block: u32) -> u32 {
    match internal_db
        .0
        .lock()
        .unwrap()
        .get(&TIP_HEIGHT_KEY.as_bytes().to_vec())
        .unwrap()
    {
        None => start_block,
        Some(v) => u32::from_le_bytes(v),
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
    let internal_db = RedisRuntimeAdapter(Arc::new(Mutex::new(
        redis::Client::open(args.redis)
            .unwrap()
            .get_connection()
            .unwrap(),
    )));
    let mut runtime = MetashrewRuntime::load(args.indexer.into(), internal_db).unwrap();
    let start = args.start_block.unwrap_or_else(|| 0);
    let mut i: u32 = query_height(&runtime.context.lock().unwrap().db, start);
    loop {
        let best: u32 = match best_height(
            &runtime.context.clone().lock().unwrap().db,
            &args.daemon_rpc_url,
            i,
            start,
        )
        .await
        {
            Ok(v) => v,
            Err(_) => i,
        };
        runtime.context.lock().unwrap().block = {
            pull_block(&runtime.context.lock().unwrap().db, &args.daemon_rpc_url, i)
                .await
                .unwrap()
        };
        runtime.context.lock().unwrap().height = i;
        if let Err(_) = runtime.run() {
            debug!("respawn cache");
            runtime.refresh_memory();
            if let Err(e) = runtime.run() {
                panic!("runtime run failed after retry: {}", e);
            }
        }
        i = i + 1;
        unsafe {
            _HEIGHT = i;
        }
    }
}

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
}

#[derive(Deserialize)]
pub struct BlockCountResponse {
    pub id: u32,
    pub result: u32,
}

async fn fetch_blockcount(rpc_url: &String) -> Result<u32> {
    let response = (reqwest::Client::new()
        .post(rpc_url)
        .body(serde_json::to_string(&JsonRpcRequest::<u32> {
            id: SystemTime::now()
                .duration_since(UNIX_EPOCH)?
                .as_secs()
                .try_into()?,
            jsonrpc: String::from("2.0"),
            method: String::from("getblockcount"),
            params: vec![],
        })?)
        .send()
        .await?);
    Ok(response.json::<BlockCountResponse>().await?.result)

}

async fn best_height(
    internal_db: &RedisRuntimeAdapter,
    rpc_url: &String,
    block_number: u32,
    start_block: u32,
) -> Result<u32> {
    let response = (reqwest::Client::new()
        .post(rpc_url)
        .body(serde_json::to_string(&JsonRpcRequest::<u32> {
            id: SystemTime::now()
                .duration_since(UNIX_EPOCH)?
                .as_secs()
                .try_into()?,
            jsonrpc: String::from("2.0"),
            method: String::from("getblockcount"),
            params: vec![],
        })?)
        .send()
        .await?);
    let mut tip = response.json::<BlockCountResponse>().await?.result;
    if block_number + 6 < tip {
        loop {
          if tip == 0 { break; }
          let blockhash = get_blockhash(&internal_db, tip).await.ok_or(anyhow!("failed to retrieve blockhash"))?;
          let remote_blockhash = fetch_blockhash(rpc_url, tip).await?;
          if blockhash == remote_blockhash {
            break;
          } else {
            tip = tip - 1;
          }
        }
    }
    return Ok(tip);
}

async fn get_blockhash(internal_db: &RedisRuntimeAdapter, block_number: u32) -> Option<Vec<u8>> {
    internal_db
        .get(&(String::from(HEIGHT_TO_HASH) + &block_number.to_string()).into_bytes())
        .unwrap()
}

async fn fetch_blockhash(rpc_url: &String, block_number: u32) -> Result<Vec<u8>, anyhow::Error> {
    let response = reqwest::Client::new()
        .post(rpc_url)
        .body(serde_json::to_string(&JsonRpcRequest::<u32> {
            id: SystemTime::now()
                .duration_since(UNIX_EPOCH)?
                .as_secs()
                .try_into()?,
            jsonrpc: String::from("2.0"),
            method: String::from("getblockhash"),
            params: vec![block_number],
        })?)
        .send()
        .await?;
    let blockhash = response.json::<JsonRpcResponse>().await?.result;
    Ok(hex::decode(&blockhash)?)
}

async fn pull_block(
    internal_db: &RedisRuntimeAdapter,
    rpc_url: &String,
    block_number: u32,
) -> Result<Vec<u8>, anyhow::Error> {
    let blockhash = fetch_blockhash(rpc_url, block_number).await.unwrap();
    internal_db
        .put(
            &(String::from(HEIGHT_TO_HASH) + block_number.to_string().as_str()).into_bytes(),
            &blockhash
        )
        .unwrap();
    Ok(hex::decode(
        reqwest::Client::new()
            .post(rpc_url)
            .body(serde_json::to_string(&JsonRpcRequest::<Value> {
                id: (<u64 as TryInto<i32>>::try_into(
                    SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
                )? + 1)
                    .try_into()?,
                jsonrpc: String::from("2.0"),
                method: String::from("getblock"),
                params: vec![Value::String(hex::encode(&blockhash)), Value::Number(Number::from(0))],
            })?)
            .send()
            .await?
            .json::<JsonRpcResponse>()
            .await?
            .result,
    )?)
}

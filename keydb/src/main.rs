use hex;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{Result};
use serde_json;
use serde_json::{Value, Number};
use serde::{Deserialize, Serialize};
use tokio;
use redis;
use clap::{command, Parser};
use std::sync::{Arc, Mutex};
use metashrew_runtime::{BatchLike, KeyValueStoreLike, MetashrewRuntime};
use redis::{Commands, ToRedisArgs};
use env_logger;
use log::{debug, info};

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
  start_block: Option<u32>
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
    self.0.cmd("SET").arg(to_redis_args(k)).arg(to_redis_args(v)).ignore();
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
        batch.0.query(&mut self.0.lock().unwrap())
    }
    fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        self.0.lock().unwrap().get(to_redis_args(key))
    }
    fn delete<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error> {
        self.0.lock().unwrap().del(to_redis_args(key))
    }
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) -> Result<(), Self::Error> {
        self.0.lock().unwrap().set(to_redis_args(key), to_redis_args(value))
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
    let internal_db = RedisRuntimeAdapter(Arc::new(Mutex::new(redis::Client::open(args.redis).unwrap().get_connection().unwrap())));
    let mut runtime = MetashrewRuntime::load(args.indexer.into(), internal_db).unwrap();
    let mut i: u32 = args.start_block.unwrap_or_else(|| 0);
    loop {
      runtime.context.lock().unwrap().block = pull_block(&args.daemon_rpc_url, i).await.unwrap();
      runtime.context.lock().unwrap().height = i;
      runtime.run().unwrap();
      i = i + 1;
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

async fn pull_block(rpc_url: &String, block_number: u32) -> Result<Vec<u8>, anyhow::Error> {
  let response = reqwest::Client::new().post(rpc_url).body(serde_json::to_string(&JsonRpcRequest::<u32> {
    id: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().try_into()?,
    jsonrpc: String::from("2.0"),
    method: String::from("getblockhash"),
    params: vec![ block_number ]
  })?).send().await?;
  let blockhash = response.json::<JsonRpcResponse>().await?.result;
  Ok(hex::decode(reqwest::Client::new().post(rpc_url).body(serde_json::to_string(&JsonRpcRequest::<Value> {
    id: (<u64 as TryInto<i32>>::try_into(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())? + 1).try_into()?,
    jsonrpc: String::from("2.0"),
    method: String::from("getblock"),
    params: vec![ Value::String(blockhash), Value::Number(Number::from(0)) ]
  })?).send().await?.json::<JsonRpcResponse>().await?.result)?)
}

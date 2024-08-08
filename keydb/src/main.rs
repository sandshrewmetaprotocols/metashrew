use std::error::Error;
use http::Uri;
use hex;
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::main]
fn main() {
  pull_block("http://locahost:
    
}

#[derive(Serialize)]
pub struct JsonRpcRequest<T> {
  pub id: u32;
  pub jsonrpc: String;
  pub method: String;
  pub params: Vec<T>;
}

async fn pull_block(rpc_url: &String, block_number: u32) -> Result<Vec<u8>> {
  let parsed = rpc_url.parse::<Uri>()?;
  let blockhash = reqwest::post(rpc_url).body(serde_json::to_string(JsonRpcRequest<u32> {
    id: SystemTime::now().duration_since(UNIX_EPOCH)?,
    jsonrpc: String::from("2.0"),
    method: "getblockhash",
    params: vec![ block_number ]
  })).send().await?.result
  hex::decode(reqwest::post(rpc_url).body(serde_json::to_string(JsonRpcRequest<String> {
    id: SystemTime::now().duration_since(UNIX_EPOCH)? + 1,
    jsonrpc: String::from("2.0"),
    method: "getblock",
    params: vec![ blockhash, 0.to_string() ]
  })).send().await?.result)
}

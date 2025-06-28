//! Rockshrew Mono - Combined Bitcoin indexer and view layer
//!
//! This crate provides a unified binary that combines the indexing functionality
//! with the view layer for serving queries, along with snapshot capabilities.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod adapters;
pub mod smt_helper;
pub mod snapshot;
pub mod snapshot_adapters;
pub mod ssh_tunnel;


// JSON-RPC request structure
#[derive(Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub id: u32,
    pub jsonrpc: String,
    pub method: String,
    pub params: Vec<Value>,
}

// Block count response structure
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct BlockCountResponse {
    pub id: u32,
    pub result: Option<u32>,
    pub error: Option<Value>,
}

// Block hash response structure
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct BlockHashResponse {
    pub id: u32,
    pub result: Option<String>,
    pub error: Option<Value>,
}

// Re-export commonly used types
pub use adapters::*;
pub use snapshot::*;
pub use snapshot_adapters::*;
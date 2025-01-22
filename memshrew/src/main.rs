use actix_cors::Cors;
use actix_web::{post, web, App, HttpResponse, HttpServer, Responder, Result as ActixResult};
use anyhow::{anyhow, Result};
use bitcoin::consensus::encode::{deserialize};
use bitcoin::hashes::Hash;
use bitcoin::{Transaction, Txid};
use clap::Parser;
use env_logger;
use itertools::Itertools;
use log::{debug, info};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::sleep;

trait FromHex: Sized {
    fn from_hex(hex_str: &str) -> Result<Self>;
}

impl FromHex for Txid {
    fn from_hex(hex_str: &str) -> Result<Self> {
        Ok(Txid::from_byte_array(
            <&[u8] as TryInto<[u8; 32]>>::try_into(&hex::decode(hex_str)?)?,
        ))
    }
}

const UPDATE_INTERVAL: Duration = Duration::from_secs(2);
const MAX_BLOCK_WEIGHT: u32 = 4_000_000;
const MIN_FEE_RATE: f64 = 1.0; // sat/vB
const TEMPLATE_CACHE_DURATION: Duration = Duration::from_secs(30);

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    daemon_rpc_url: String,
    #[arg(long)]
    auth: Option<String>,
    #[arg(long, env = "HOST", default_value = "127.0.0.1")]
    host: String,
    #[arg(long, env = "PORT", default_value_t = 8081)] // Default to 8081 to avoid conflict
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MempoolTxInfo {
    tx: Transaction,
    fee: u64,
    vsize: u64,
    fee_rate: f64,
    ancestors: HashSet<Txid>,
    descendants: HashSet<Txid>,
    received_time: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BlockTemplate {
    txids: Vec<Txid>,
    total_fees: u64,
    total_vsize: u64,
    expected_reward: u64,
    timestamp: SystemTime,
}

#[derive(Clone)]
struct MempoolTracker {
    daemon_url: String,
    auth: Option<String>,
    mempool_txs: Arc<RwLock<HashMap<Txid, MempoolTxInfo>>>,
    block_templates: Arc<RwLock<Vec<BlockTemplate>>>,
    last_template_update: Arc<RwLock<SystemTime>>,
}

#[derive(Clone)]
struct AppState {
    tracker: Arc<MempoolTracker>,
}

#[derive(Deserialize, Serialize)]
struct JsonRpcRequest {
    id: u32,
    method: String,
    params: Vec<Value>,
    jsonrpc: String,
}

#[derive(Deserialize, Serialize)]
struct JsonRpcResponse {
    id: u32,
    result: Value,
    jsonrpc: String,
}

#[derive(Deserialize, Serialize)]
struct JsonRpcError {
    id: u32,
    error: JsonRpcErrorObject,
    jsonrpc: String,
}

#[derive(Deserialize, Serialize)]
struct JsonRpcErrorObject {
    code: i32,
    message: String,
    data: Option<String>,
}

#[allow(dead_code)]
impl MempoolTracker {
    pub fn new(daemon_url: String, auth: Option<String>) -> Self {
        MempoolTracker {
            daemon_url,
            auth,
            mempool_txs: Arc::new(RwLock::new(HashMap::new())),
            block_templates: Arc::new(RwLock::new(Vec::new())),
            last_template_update: Arc::new(RwLock::new(SystemTime::UNIX_EPOCH)),
        }
    }

    async fn make_rpc_call(&self, method: &str, params: Vec<Value>) -> Result<Value> {
        let start_time = SystemTime::now();
        debug!(
            "Making RPC call to node - method: {}, params: {:?}",
            method, params
        );

        let client = reqwest::Client::new();
        let url = match self.auth.clone() {
            Some(auth) => {
                let mut url = Url::parse(&self.daemon_url)?;
                let (username, password) = auth
                    .split(":")
                    .next_tuple()
                    .ok_or_else(|| anyhow!("Invalid auth format - should be username:password"))?;
                url.set_username(username)
                    .map_err(|_| anyhow!("Invalid username"))?;
                url.set_password(Some(password))
                    .map_err(|_| anyhow!("Invalid password"))?;
                url
            }
            None => Url::parse(&self.daemon_url)?,
        };

        let response = client
            .post(url)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params
            }))
            .send()
            .await?
            .json::<Value>()
            .await?;

        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or(Duration::from_secs(0));

        if let Some(error) = response.get("error") {
            info!(
                "RPC call failed after {:.2}s - method: {}, error: {}",
                duration.as_secs_f64(),
                method,
                error
            );
            return Err(anyhow!("RPC error: {}", error));
        }

        debug!(
            "RPC call succeeded in {:.2}s - method: {}",
            duration.as_secs_f64(),
            method
        );

        Ok(response["result"].clone())
    }

    async fn update_mempool(&self) -> Result<()> {
        let start_time = SystemTime::now();
        info!("Starting mempool sync");

        let mempool_info = self
            .make_rpc_call("getrawmempool", vec![json!(true)])
            .await?;

        let mut txs = self.mempool_txs.write().await;
        let initial_count = txs.len();

        // Remove stale transactions
        txs.retain(|txid, _| mempool_info[txid.to_string()].is_object());
        let removed_count = initial_count - txs.len();
        if removed_count > 0 {
            info!("Removed {} stale transactions", removed_count);
        }

        // Add/update transactions
        let mut added_count = 0;
        for (txid_str, info) in mempool_info.as_object().unwrap() {
            let txid = Txid::from_hex(txid_str)?;

            if !txs.contains_key(&txid) {
                let tx_hex = self
                    .make_rpc_call("getrawtransaction", vec![json!(txid_str)])
                    .await?;
                let tx_bytes = hex::decode(tx_hex.as_str().unwrap())?;
                let tx: Transaction = deserialize(&tx_bytes)?;

                txs.insert(
                    txid,
                    MempoolTxInfo {
                        tx,
                        fee: (info["fees"]["base"].as_f64().unwrap() * 100_000_000.0) as u64, // Convert BTC to sats
                        vsize: info["vsize"].as_u64().unwrap(),
                        fee_rate: info["fees"]["base"].as_f64().unwrap() * 100_000_000.0
                            / info["vsize"].as_f64().unwrap(),
                        ancestors: info["depends"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|v| Txid::from_hex(v.as_str()?).ok())
                            .collect(),
                        descendants: HashSet::new(), // Will be populated later
                        received_time: SystemTime::now(),
                    },
                );
                added_count += 1;
            }
        }

        if added_count > 0 {
            info!("Added {} new transactions", added_count);
        }

        // Update descendant sets
        let txids: Vec<_> = txs.keys().cloned().collect();
        for txid in txids {
            let ancestors = txs.get(&txid).unwrap().ancestors.clone();
            for ancestor_txid in ancestors {
                if let Some(ancestor_tx) = txs.get_mut(&ancestor_txid) {
                    ancestor_tx.descendants.insert(txid);
                }
            }
        }

        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or(Duration::from_secs(0));
        info!(
            "Mempool sync complete - {} total transactions in {:.2}s",
            txs.len(),
            duration.as_secs_f64()
        );

        Ok(())
    }

    fn get_ancestor_set_info(
        &self,
        txs: &HashMap<Txid, MempoolTxInfo>,
        txid: &Txid,
    ) -> (u64, u64, f64) {
        let mut total_fee = 0;
        let mut total_vsize = 0;
        let mut seen = HashSet::new();
        let mut queue = vec![txid];

        while let Some(current_txid) = queue.pop() {
            if !seen.insert(*current_txid) {
                continue;
            }

            if let Some(tx_info) = txs.get(current_txid) {
                total_fee += tx_info.fee;
                total_vsize += tx_info.vsize;
                queue.extend(tx_info.ancestors.iter());
            }
        }

        let fee_rate = if total_vsize > 0 {
            total_fee as f64 / total_vsize as f64
        } else {
            0.0
        };

        (total_fee, total_vsize, fee_rate)
    }

    async fn generate_block_templates(&self) -> Result<()> {
        let txs = self.mempool_txs.read().await;
        let mut templates = Vec::new();

        // Basic high-fee template
        let mut tx_entries: Vec<_> = txs.iter().collect();
        tx_entries.sort_by(|a, b| {
            b.1.fee_rate
                .partial_cmp(&a.1.fee_rate)
                .unwrap_or(Ordering::Equal)
        });

        let mut template = BlockTemplate {
            txids: Vec::new(),
            total_fees: 0,
            total_vsize: 0,
            expected_reward: 6_25 * 100_000_000, // Current block reward in sats
            timestamp: SystemTime::now(),
        };

        let mut included = HashSet::new();
        for (txid, info) in tx_entries {
            if info.fee_rate < MIN_FEE_RATE || included.contains(txid) {
                continue;
            }

            let new_vsize = template.total_vsize + info.vsize;
            if new_vsize > MAX_BLOCK_WEIGHT as u64 {
                continue;
            }

            // Include transaction and its ancestors
            let mut queue = vec![txid];
            while let Some(current_txid) = queue.pop() {
                if included.insert(*current_txid) {
                    if let Some(tx_info) = txs.get(current_txid) {
                        template.txids.push(*current_txid);
                        template.total_fees += tx_info.fee;
                        template.total_vsize += tx_info.vsize;
                        queue.extend(tx_info.ancestors.iter());
                    }
                }
            }
        }

        template.expected_reward += template.total_fees;
        templates.push(template);

        // Store templates
        let mut block_templates = self.block_templates.write().await;
        *block_templates = templates;

        let mut last_update = self.last_template_update.write().await;
        *last_update = SystemTime::now();

        Ok(())
    }

    pub async fn start_background_tasks(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                if let Err(e) = self.update_mempool().await {
                    debug!("Error updating mempool: {}", e);
                }

                // Check if templates need updating
                let last_update = *self.last_template_update.read().await;
                if SystemTime::now()
                    .duration_since(last_update)
                    .unwrap_or(Duration::from_secs(0))
                    >= TEMPLATE_CACHE_DURATION
                {
                    if let Err(e) = self.generate_block_templates().await {
                        debug!("Error generating block templates: {}", e);
                    }
                }

                sleep(UPDATE_INTERVAL).await;
            }
        });
    }
}

#[post("/")]
async fn handle_jsonrpc(
    body: web::Json<JsonRpcRequest>,
    state: web::Data<AppState>,
) -> ActixResult<impl Responder> {
    debug!(
        "Received JSON-RPC request - method: {}, params: {:?}",
        body.method, body.params
    );

    match body.method.as_str() {
        "memshrew_getmempooltxs" => {
            let txs = state.tracker.mempool_txs.read().await;
            let result = txs.iter()
                .map(|(txid, info)| json!({
                    "txid": txid.to_string(),
                    "fee": info.fee,
                    "vsize": info.vsize,
                    "fee_rate": info.fee_rate,
                    "ancestors": info.ancestors.iter().map(|tx| tx.to_string()).collect::<Vec<_>>(),
                    "descendants": info.descendants.iter().map(|tx| tx.to_string()).collect::<Vec<_>>(),
                }))
                .collect::<Vec<_>>();

            Ok(HttpResponse::Ok().json(JsonRpcResponse {
                id: body.id,
                result: json!(result),
                jsonrpc: "2.0".to_string(),
            }))
        }

        "memshrew_getblocktemplates" => {
            let templates = state.tracker.block_templates.read().await;
            let result = templates
                .iter()
                .map(|template| {
                    json!({
                        "txids": template.txids.iter().map(|tx| tx.to_string()).collect::<Vec<_>>(),
                        "total_fees": template.total_fees,
                        "total_vsize": template.total_vsize,
                        "expected_reward": template.expected_reward,
                        "timestamp": template.timestamp
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or(Duration::from_secs(0))
                            .as_secs(),
                    })
                })
                .collect::<Vec<_>>();

            Ok(HttpResponse::Ok().json(JsonRpcResponse {
                id: body.id,
                result: json!(result),
                jsonrpc: "2.0".to_string(),
            }))
        }

        _ => Ok(HttpResponse::Ok().json(JsonRpcError {
            id: body.id,
            error: JsonRpcErrorObject {
                code: -32601,
                message: format!("Method '{}' not found", body.method),
                data: None,
            },
            jsonrpc: "2.0".to_string(),
        })),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    // Create mempool tracker
    let tracker = Arc::new(MempoolTracker::new(args.daemon_rpc_url, args.auth));

    // Start background tasks
    tracker.clone().start_background_tasks().await;

    // Create app state
    let app_state = web::Data::new(AppState {
        tracker: tracker.clone(),
    });

    // Start the JSON-RPC server
    info!("Starting server at http://{}:{}", args.host, args.port);
    HttpServer::new(move || {
        App::new()
            .wrap(Cors::default().allowed_origin_fn(|origin, _| {
                if let Ok(origin_str) = origin.to_str() {
                    origin_str.starts_with("http://localhost:")
                } else {
                    false
                }
            }))
            .app_data(app_state.clone())
            .service(handle_jsonrpc)
    })
    .bind((args.host.as_str(), args.port))?
    .run()
    .await?;

    Ok(())
}

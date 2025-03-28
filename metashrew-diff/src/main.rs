use anyhow::{anyhow, Result};
use clap::{command, Parser};
use env_logger;
use hex;
use itertools::Itertools;
use log::{debug, info, warn, error};
use metashrew_runtime::{KeyValueStoreLike, MetashrewRuntime};
use rockshrew_runtime::{query_height, set_label, RocksDBRuntimeAdapter};
use rocksdb::Options;
use std::collections::{HashSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio;

#[derive(Parser, Debug)]
#[command(version, about = "Compare the output of two WASM modules in Metashrew", long_about = None)]
struct Args {
    #[arg(long)]
    daemon_rpc_url: String,
    
    #[arg(long)]
    indexer: String,
    
    #[arg(long)]
    compare: String,
    
    #[arg(long)]
    db_path: String,
    
    #[arg(long)]
    start_block: Option<u32>,
    
    #[arg(long)]
    auth: Option<String>,
    
    #[arg(long)]
    label: Option<String>,
    
    #[arg(long)]
    exit_at: Option<u32>,
    
    #[arg(long)]
    prefix: String,
}

const HEIGHT_TO_HASH: &'static str = "/__INTERNAL/height-to-hash/";

// KeyTracker to monitor keys being written with a specific prefix
struct KeyTracker {
    prefix: Vec<u8>,
    keys: HashSet<Vec<u8>>,
}

impl KeyTracker {
    fn new(prefix: Vec<u8>) -> Self {
        Self {
            prefix,
            keys: HashSet::new(),
        }
    }

    fn track_key(&mut self, key: &[u8]) {
        if key.starts_with(&self.prefix) {
            self.keys.insert(key.to_vec());
        }
    }

    fn get_keys(&self) -> &HashSet<Vec<u8>> {
        &self.keys
    }

    fn clear(&mut self) {
        self.keys.clear();
    }
}

// Custom runtime adapter for diff operations
struct MetashrewDiffRuntime {
    primary_runtime: MetashrewRuntime<RocksDBRuntimeAdapter>,
    compare_runtime: MetashrewRuntime<RocksDBRuntimeAdapter>,
    args: Args,
    start_block: u32,
    prefix: Vec<u8>,
    primary_tracker: KeyTracker,
    compare_tracker: KeyTracker,
}

impl MetashrewDiffRuntime {
    pub fn new(
        primary_runtime: MetashrewRuntime<RocksDBRuntimeAdapter>,
        compare_runtime: MetashrewRuntime<RocksDBRuntimeAdapter>,
        args: Args,
        start_block: u32,
        prefix: Vec<u8>,
    ) -> Self {
        Self {
            primary_runtime,
            compare_runtime,
            args,
            start_block,
            primary_tracker: KeyTracker::new(prefix.clone()),
            compare_tracker: KeyTracker::new(prefix.clone()),
            prefix,
        }
    }

    // Create a custom MetashrewRuntime that tracks keys with our prefix
    fn create_tracking_runtime(
        wasm_path: PathBuf,
        db_adapter: RocksDBRuntimeAdapter,
        tracker: &mut KeyTracker,
    ) -> Result<MetashrewRuntime<RocksDBRuntimeAdapter>> {
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::from_file(&engine, wasm_path)
            .map_err(|e| anyhow!("Failed to load WASM module: {}", e))?;
        
        let mut runtime = MetashrewRuntime::load(
            wasm_path,
            db_adapter,
        )?;
        
        // We'll need to modify the runtime to track keys with our prefix
        // This would require implementing a custom flush function that tracks keys
        // For now, we'll return the standard runtime
        
        Ok(runtime)
    }

    // Function to compare keys between primary and compare runtimes
    fn compare_keys(&self, primary_keys: &HashSet<Vec<u8>>, compare_keys: &HashSet<Vec<u8>>) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
        // Keys in primary but not in compare
        let primary_only: Vec<Vec<u8>> = primary_keys
            .difference(compare_keys)
            .cloned()
            .collect();
        
        // Keys in compare but not in primary
        let compare_only: Vec<Vec<u8>> = compare_keys
            .difference(primary_keys)
            .cloned()
            .collect();
        
        (primary_only, compare_only)
    }

    // Function to print a report of the differences
    fn print_diff_report(&self, height: u32, primary_only: &[Vec<u8>], compare_only: &[Vec<u8>]) {
        println!("=== DIFF REPORT FOR BLOCK {} ===", height);
        
        if primary_only.is_empty() && compare_only.is_empty() {
            println!("No differences found.");
            return;
        }
        
        if !primary_only.is_empty() {
            println!("Keys in primary but not in compare ({}):", primary_only.len());
            for key in primary_only {
                println!("  {}", hex::encode(key));
            }
        }
        
        if !compare_only.is_empty() {
            println!("Keys in compare but not in primary ({}):", compare_only.len());
            for key in compare_only {
                println!("  {}", hex::encode(key));
            }
        }
        
        println!("=== END DIFF REPORT ===");
    }

    // Main function to process a block and compare the results
    pub async fn process_block(&mut self, block_data: Vec<u8>, height: u32) -> Result<bool> {
        info!("Processing block {} with both runtimes", height);
        
        // Set the block data and height for both runtimes
        self.primary_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?.block = block_data.clone();
        self.primary_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?.height = height;
        self.primary_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?.db.set_height(height);
        
        self.compare_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?.block = block_data.clone();
        self.compare_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?.height = height;
        self.compare_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?.db.set_height(height);
        
        // Run both runtimes
        if let Err(e) = self.primary_runtime.run() {
            return Err(anyhow!("Error running primary WASM module: {}", e));
        }
        
        if let Err(e) = self.compare_runtime.run() {
            return Err(anyhow!("Error running compare WASM module: {}", e));
        }
        
        // Extract keys with the specified prefix from both runtimes
        let primary_keys = self.scan_keys_with_prefix(&self.primary_runtime, height)?;
        let compare_keys = self.scan_keys_with_prefix(&self.compare_runtime, height)?;
        
        // Compare the keys
        let (primary_only, compare_only) = self.compare_keys(&primary_keys, &compare_keys);
        
        // If there are differences, print a report and return true
        if !primary_only.is_empty() || !compare_only.is_empty() {
            self.print_diff_report(height, &primary_only, &compare_only);
            return Ok(true);
        }
        
        Ok(false)
    }

    // Scan the database for keys with our prefix that were written at the given height
    fn scan_keys_with_prefix(&self, runtime: &MetashrewRuntime<RocksDBRuntimeAdapter>, height: u32) -> Result<HashSet<Vec<u8>>> {
        let mut keys = HashSet::new();
        
        // Get the height key for this block
        let height_vec = match metashrew_runtime::u32_to_vec(height) {
            Ok(v) => v,
            Err(e) => return Err(anyhow!("Failed to convert height to vec: {}", e)),
        };
        
        // Get the updated keys for this block
        let updated_keys = match MetashrewRuntime::<RocksDBRuntimeAdapter>::db_updated_keys_for_block(
            runtime.context.clone(),
            height,
        ) {
            Ok(keys) => keys,
            Err(e) => return Err(anyhow!("Failed to get updated keys: {}", e)),
        };
        
        // Filter keys that start with our prefix
        for key in updated_keys {
            if key.starts_with(&self.prefix) {
                keys.insert(key);
            }
        }
        
        Ok(keys)
    }

    // Helper function to fetch a block from the Bitcoin node
    async fn fetch_block(&self, height: u32) -> Result<Vec<u8>> {
        use serde_json::{json, Value, Number};
        use reqwest::{Client, Url};
        
        // Create the JSON-RPC request
        let blockhash = self.fetch_blockhash(height).await?;
        
        // Create the request to get the block data
        let client = Client::new();
        let url = match self.args.auth.clone() {
            Some(auth) => {
                let mut url = Url::parse(&self.args.daemon_rpc_url)
                    .map_err(|e| anyhow!("Invalid URL: {}", e))?;
                let (username, password) = auth.split(':').next_tuple()
                    .ok_or_else(|| anyhow!("Invalid auth format, expected username:password"))?;
                url.set_username(username)
                    .map_err(|_| anyhow!("Failed to set username"))?;
                url.set_password(Some(password))
                    .map_err(|_| anyhow!("Failed to set password"))?;
                url
            },
            None => Url::parse(&self.args.daemon_rpc_url)
                .map_err(|e| anyhow!("Invalid URL: {}", e))?,
        };
        
        let request_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow!("Time error: {}", e))?
            .as_secs() as u32;
            
        let request_body = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "getblock",
            "params": [
                hex::encode(&blockhash),
                Number::from(0)
            ]
        });
        
        // Send the request
        let response = client.post(url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| anyhow!("Request error: {}", e))?;
            
        // Parse the response
        let response_json: Value = response.json()
            .await
            .map_err(|e| anyhow!("JSON parse error: {}", e))?;
            
        // Extract the block data
        let block_hex = response_json["result"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing result in response"))?;
            
        // Decode the hex
        let block_data = hex::decode(block_hex)
            .map_err(|e| anyhow!("Hex decode error: {}", e))?;
            
        Ok(block_data)
    }
    
    // Helper function to fetch a blockhash from the Bitcoin node
    async fn fetch_blockhash(&self, height: u32) -> Result<Vec<u8>> {
        use serde_json::{json, Value};
        use reqwest::{Client, Url};
        
        // Create the JSON-RPC request
        let client = Client::new();
        let url = match self.args.auth.clone() {
            Some(auth) => {
                let mut url = Url::parse(&self.args.daemon_rpc_url)
                    .map_err(|e| anyhow!("Invalid URL: {}", e))?;
                let (username, password) = auth.split(':').next_tuple()
                    .ok_or_else(|| anyhow!("Invalid auth format, expected username:password"))?;
                url.set_username(username)
                    .map_err(|_| anyhow!("Failed to set username"))?;
                url.set_password(Some(password))
                    .map_err(|_| anyhow!("Failed to set password"))?;
                url
            },
            None => Url::parse(&self.args.daemon_rpc_url)
                .map_err(|e| anyhow!("Invalid URL: {}", e))?,
        };
        
        let request_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow!("Time error: {}", e))?
            .as_secs() as u32;
            
        let request_body = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "getblockhash",
            "params": [height]
        });
        
        // Send the request
        let response = client.post(url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| anyhow!("Request error: {}", e))?;
            
        // Parse the response
        let response_json: Value = response.json()
            .await
            .map_err(|e| anyhow!("JSON parse error: {}", e))?;
            
        // Extract the blockhash
        let blockhash_hex = response_json["result"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing result in response"))?;
            
        // Decode the hex
        let blockhash = hex::decode(blockhash_hex)
            .map_err(|e| anyhow!("Hex decode error: {}", e))?;
            
        Ok(blockhash)
    }

    // Main run function
    pub async fn run(&mut self) -> Result<()> {
        let mut height = self.start_block;
        
        loop {
            // Check if we should exit before processing the next block
            if let Some(exit_at) = self.args.exit_at {
                if height >= exit_at {
                    info!("Reached exit-at block {}, shutting down gracefully", exit_at);
                    return Ok(());
                }
            }
            
            // Fetch the block
            let block_data = self.fetch_block(height).await?;
            
            // Process the block and check for differences
            let has_diff = self.process_block(block_data, height).await?;
            
            // If there are differences, exit
            if has_diff {
                info!("Found differences at block {}, exiting", height);
                return Ok(());
            }
            
            // Move to the next block
            height += 1;
        }
    }
}

// Helper function to create RocksDB options
fn create_rocksdb_options() -> Options {
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
    opts
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    
    // Set label if provided
    if let Some(ref label) = args.label {
        set_label(label.clone());
    }
    
    // Parse the prefix
    let prefix = match args.prefix.strip_prefix("0x") {
        Some(hex_str) => hex::decode(hex_str)
            .map_err(|e| anyhow!("Invalid hex prefix: {}", e))?,
        None => return Err(anyhow!("Prefix must start with 0x followed by hex characters")),
    };
    
    // Create the db_path directory if it doesn't exist
    let db_path = Path::new(&args.db_path);
    if !db_path.exists() {
        fs::create_dir_all(db_path)?;
    }
    
    // Create primary and compare directories
    let primary_path = db_path.join("primary");
    let compare_path = db_path.join("compare");
    
    if !primary_path.exists() {
        fs::create_dir_all(&primary_path)?;
    }
    
    if !compare_path.exists() {
        fs::create_dir_all(&compare_path)?;
    }
    
    // Create RocksDB options
    let opts = create_rocksdb_options();
    
    // Open primary and compare RocksDB instances
    let primary_db = RocksDBRuntimeAdapter::open(primary_path.to_string_lossy().to_string(), opts.clone())?;
    let compare_db = RocksDBRuntimeAdapter::open(compare_path.to_string_lossy().to_string(), opts)?;
    
    // Load primary and compare WASM modules
    let primary_indexer: PathBuf = args.indexer.clone().into();
    let compare_indexer: PathBuf = args.compare.clone().into();
    
    let primary_runtime = MetashrewRuntime::load(
        primary_indexer,
        primary_db,
    )?;
    
    let compare_runtime = MetashrewRuntime::load(
        compare_indexer,
        compare_db,
    )?;
    
    // Get start block
    let start_block = args.start_block.unwrap_or(0);
    
    // Create and run the diff runtime
    let mut diff_runtime = MetashrewDiffRuntime::new(
        primary_runtime,
        compare_runtime,
        args,
        start_block,
        prefix,
    );
    
    diff_runtime.run().await?;
    
    Ok(())
}
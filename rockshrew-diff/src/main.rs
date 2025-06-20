use anyhow::{anyhow, Result};
use clap::{command, Parser};
use env_logger;
use hex;
use itertools::Itertools;
use log::{debug, error, info};
use metashrew_runtime::MetashrewRuntime;
use rand::Rng;
use rockshrew_runtime::{set_label, RocksDBRuntimeAdapter};
use rocksdb::Options;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio;
use tokio::sync::mpsc;
use tokio::time::sleep;

// Import our tracking adapter
mod tracking_adapter;
use tracking_adapter::TrackingAdapter;

#[derive(Parser, Debug, Clone)]
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
    
    // Pipeline configuration
    #[arg(long, default_value_t = 5)]
    pipeline_size: usize,
    
    // Output directory for diff reports
    #[arg(long, help = "Directory to save diff reports")]
    output_dir: Option<String>,
    
    // Maximum number of diffs before exiting
    #[arg(long, default_value_t = 12, help = "Maximum number of diffs to find before exiting")]
    diff_limit: usize,
}

static CURRENT_HEIGHT: AtomicU32 = AtomicU32::new(0);

// Block processing result for the pipeline
#[derive(Debug)]
enum BlockResult {
    Success(u32),  // Block height that was successfully processed
    Error(u32, anyhow::Error),  // Block height and error
    Difference(u32),  // Block height where a difference was found
}

// Custom runtime adapter for diff operations
struct RockshrewDiffRuntime {
    primary_runtime: MetashrewRuntime<TrackingAdapter>,
    compare_runtime: MetashrewRuntime<TrackingAdapter>,
    args: Args,
    start_block: u32,
    prefix: Vec<u8>,
    // Track the number of diffs found
    diffs_found: usize,
    // Track blocks with differences for summary
    diff_blocks: Vec<u32>,
}

impl RockshrewDiffRuntime {
    pub fn new(
        primary_runtime: MetashrewRuntime<TrackingAdapter>,
        compare_runtime: MetashrewRuntime<TrackingAdapter>,
        args: Args,
        start_block: u32,
        prefix: Vec<u8>,
    ) -> Self {
        Self {
            primary_runtime,
            compare_runtime,
            args,
            start_block,
            prefix,
            diffs_found: 0,
            diff_blocks: Vec::new(),
        }
    }

    // Function to compare updates between primary and compare runtimes
    fn compare_updates(
        &self,
        primary_updates: &HashMap<Vec<u8>, Vec<u8>>,
        compare_updates: &HashMap<Vec<u8>, Vec<u8>>
    ) -> (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<(Vec<u8>, Vec<u8>, Vec<u8>)>) {
        // Keys in primary but not in compare
        let primary_only_keys: Vec<Vec<u8>> = primary_updates
            .keys()
            .filter(|k| !compare_updates.contains_key(*k))
            .cloned()
            .collect();
        
        // Keys in compare but not in primary
        let compare_only_keys: Vec<Vec<u8>> = compare_updates
            .keys()
            .filter(|k| !primary_updates.contains_key(*k))
            .cloned()
            .collect();
        
        // Keys in both but with different values
        let mut value_diffs: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = Vec::new();
        
        for (key, primary_value) in primary_updates {
            if let Some(compare_value) = compare_updates.get(key) {
                if primary_value != compare_value {
                    // Store (key, primary_value, compare_value)
                    value_diffs.push((key.clone(), primary_value.clone(), compare_value.clone()));
                }
            }
        }
        
        (primary_only_keys, compare_only_keys, value_diffs)
    }

    // Function to generate a diff report as a string
    fn generate_diff_report(
        &self,
        height: u32,
        primary_only_keys: &[Vec<u8>],
        compare_only_keys: &[Vec<u8>],
        value_diffs: &[(Vec<u8>, Vec<u8>, Vec<u8>)]
    ) -> String {
        let mut report = format!("=== DIFF REPORT FOR BLOCK {} ===\n", height);
        
        if primary_only_keys.is_empty() && compare_only_keys.is_empty() && value_diffs.is_empty() {
            report.push_str("No differences found.\n");
            return report;
        }
        
        if !primary_only_keys.is_empty() {
            report.push_str(&format!("Keys in primary but not in compare ({}):\n", primary_only_keys.len()));
            for key in primary_only_keys {
                report.push_str(&format!("  {}\n", hex::encode(key)));
            }
        }
        
        if !compare_only_keys.is_empty() {
            report.push_str(&format!("Keys in compare but not in primary ({}):\n", compare_only_keys.len()));
            for key in compare_only_keys {
                report.push_str(&format!("  {}\n", hex::encode(key)));
            }
        }
        
        if !value_diffs.is_empty() {
            report.push_str(&format!("Keys with different values ({}):\n", value_diffs.len()));
            for (key, primary_value, compare_value) in value_diffs {
                report.push_str(&format!("  Key: {}\n", hex::encode(key)));
                report.push_str(&format!("    Primary value: {}\n", hex::encode(primary_value)));
                report.push_str(&format!("    Compare value: {}\n", hex::encode(compare_value)));
            }
        }
        
        report.push_str("=== END DIFF REPORT ===\n");
        report
    }
    
    // Function to print a report of the differences
    fn print_diff_report(
        &self,
        height: u32,
        primary_only_keys: &[Vec<u8>],
        compare_only_keys: &[Vec<u8>],
        value_diffs: &[(Vec<u8>, Vec<u8>, Vec<u8>)]
    ) {
        let report = self.generate_diff_report(height, primary_only_keys, compare_only_keys, value_diffs);
        print!("{}", report);
    }
    
    // Function to save a diff report to a file
    fn save_diff_report(
        &self,
        height: u32,
        primary_only_keys: &[Vec<u8>],
        compare_only_keys: &[Vec<u8>],
        value_diffs: &[(Vec<u8>, Vec<u8>, Vec<u8>)]
    ) -> Result<()> {
        // Generate the report
        let report = self.generate_diff_report(height, primary_only_keys, compare_only_keys, value_diffs);
        
        // Determine output directory
        let output_dir = match &self.args.output_dir {
            Some(dir) => {
                // Create the output directory if it doesn't exist
                let output_path = Path::new(dir);
                if !output_path.exists() {
                    fs::create_dir_all(output_path)?;
                }
                output_path.to_path_buf()
            },
            None => {
                // Use current directory if no output directory is specified
                PathBuf::from(".")
            }
        };
        
        // Create a meaningful filename with timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let file_path = output_dir.join(format!("diff_block_{}_ts_{}.txt", height, timestamp));
        
        // Write the report to the file
        fs::write(&file_path, report)?;
        
        info!("Saved diff report to {}", file_path.display());
        
        Ok(())
    }
    
    // Function to generate and save a summary report
    fn save_summary_report(&self) -> Result<()> {
        // Check if output directory is specified
        if let Some(output_dir) = &self.args.output_dir {
            // Create the output directory if it doesn't exist
            let output_path = Path::new(output_dir);
            if !output_path.exists() {
                fs::create_dir_all(output_path)?;
            }
            
            // Generate the summary report
            let mut summary = String::new();
            summary.push_str("=== DIFF ANALYSIS SUMMARY ===\n");
            summary.push_str(&format!("Total diffs found: {}\n", self.diffs_found));
            
            if !self.diff_blocks.is_empty() {
                summary.push_str("Blocks with differences:\n");
                for block in &self.diff_blocks {
                    summary.push_str(&format!("  Block {}\n", block));
                }
            } else {
                summary.push_str("No differences found in any blocks.\n");
            }
            
            summary.push_str(&format!("Analyzed blocks from {} to {}\n",
                self.start_block,
                CURRENT_HEIGHT.load(Ordering::SeqCst)));
                
            summary.push_str("=== END SUMMARY ===\n");
            
            // Create the file path
            let file_path = output_path.join("diff_summary.txt");
            
            // Write the summary to the file
            fs::write(&file_path, &summary)?;
            
            info!("Saved summary report to {}", file_path.display());
            
            // Also print the summary to the console
            print!("{}", summary);
        }
        
        Ok(())
    }

    // Main function to process a block and compare the results
    pub async fn process_block(&mut self, block_data: Vec<u8>, height: u32) -> Result<bool> {
        info!("Processing block {} with both runtimes", height);
        
        // Set the block data and height for primary runtime with efficient mutex locking
        {
            let mut context = self.primary_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
            context.block = block_data.clone();
            context.height = height;
            context.db.set_height(height);
        }
        
        // Set the block data and height for compare runtime with efficient mutex locking
        {
            let mut context = self.compare_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
            context.block = block_data.clone();
            context.height = height;
            context.db.set_height(height);
        }
        
        // Run primary runtime with better error handling
        match self.primary_runtime.run() {
            Ok(_) => {
                debug!("Successfully ran primary WASM module for block {}", height);
            },
            Err(e) => {
                error!("Error running primary WASM module: {}, refreshing memory and retrying", e);
                self.primary_runtime.refresh_memory().map_err(|refresh_err| {
                    error!("Memory refresh failed for primary runtime: {}", refresh_err);
                    anyhow!("Memory refresh failed for primary runtime: {}", refresh_err)
                })?;
                
                self.primary_runtime.run().map_err(|run_err| {
                    error!("Runtime execution failed after memory refresh for primary runtime: {}", run_err);
                    anyhow!("Error running primary WASM module after memory refresh: {}", run_err)
                })?;
                
                debug!("Successfully ran primary WASM module for block {} after memory refresh", height);
            }
        }
        
        // Run compare runtime with better error handling
        match self.compare_runtime.run() {
            Ok(_) => {
                debug!("Successfully ran compare WASM module for block {}", height);
            },
            Err(e) => {
                error!("Error running compare WASM module: {}, refreshing memory and retrying", e);
                self.compare_runtime.refresh_memory().map_err(|refresh_err| {
                    error!("Memory refresh failed for compare runtime: {}", refresh_err);
                    anyhow!("Memory refresh failed for compare runtime: {}", refresh_err)
                })?;
                
                self.compare_runtime.run().map_err(|run_err| {
                    error!("Runtime execution failed after memory refresh for compare runtime: {}", run_err);
                    anyhow!("Error running compare WASM module after memory refresh: {}", run_err)
                })?;
                
                debug!("Successfully ran compare WASM module for block {} after memory refresh", height);
            }
        }
        
        // Get tracked updates from both adapters
        let primary_updates = {
            let context = self.primary_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
            context.db.get_tracked_updates()
        };
        
        let compare_updates = {
            let context = self.compare_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
            context.db.get_tracked_updates()
        };
        
        // Clear tracked updates for next block
        {
            let context = self.primary_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
            context.db.clear_tracked_updates();
        }
        
        {
            let context = self.compare_runtime.context.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
            context.db.clear_tracked_updates();
        }
        
        // Compare the updates
        let (primary_only_keys, compare_only_keys, value_diffs) = self.compare_updates(&primary_updates, &compare_updates);
        
        // If there are differences, print a report, save it to a file, and return true
        if !primary_only_keys.is_empty() || !compare_only_keys.is_empty() || !value_diffs.is_empty() {
            // Print the report to the console
            self.print_diff_report(height, &primary_only_keys, &compare_only_keys, &value_diffs);
            
            // Save the report to a file
            if let Err(e) = self.save_diff_report(height, &primary_only_keys, &compare_only_keys, &value_diffs) {
                error!("Failed to save diff report: {}", e);
            }
            
            // Track this diff
            self.diffs_found += 1;
            self.diff_blocks.push(height);
            
            return Ok(true);
        }
        
        Ok(false)
    }

    // This function is no longer needed as we're using the TrackingAdapter

    // Helper function to send a request with exponential backoff and jitter
    async fn send_request(&self, url: reqwest::Url, request_body: serde_json::Value) -> Result<serde_json::Value> {
        use serde_json::Value;
        use reqwest::Client;
        
        let client = Client::new();
        let mut retry_delay = Duration::from_millis(100);
        let max_delay = Duration::from_secs(30);
        let max_retries = 10;
        
        for attempt in 0..=max_retries {
            match client.post(url.clone())
                .header("Content-Type", "application/json")
                .json(&request_body)
                .send()
                .await {
                    Ok(response) => {
                        match response.json::<Value>().await {
                            Ok(json) => return Ok(json),
                            Err(e) => {
                                if attempt == max_retries {
                                    return Err(anyhow!("JSON parse error: {}", e));
                                }
                            }
                        }
                    },
                    Err(e) => {
                        if attempt == max_retries {
                            return Err(anyhow!("Request error: {}", e));
                        }
                    }
                }
            
            // Calculate exponential backoff with jitter
            let jitter = rand::thread_rng().gen_range(0..=100) as u64;
            retry_delay = std::cmp::min(
                max_delay,
                retry_delay * 2 + Duration::from_millis(jitter)
            );
            
            debug!("Request failed (attempt {}), retrying in {:?}",
                   attempt + 1, retry_delay);
            sleep(retry_delay).await;
        }
        
        Err(anyhow!("Unreachable: max retries exceeded"))
    }
    
    // Helper function to get the URL with auth
    fn get_url_with_auth(&self) -> Result<reqwest::Url> {
        use reqwest::Url;
        
        match self.args.auth.clone() {
            Some(auth) => {
                let mut url = Url::parse(&self.args.daemon_rpc_url)
                    .map_err(|e| anyhow!("Invalid URL: {}", e))?;
                let (username, password) = auth.split(':').next_tuple()
                    .ok_or_else(|| anyhow!("Invalid auth format, expected username:password"))?;
                url.set_username(username)
                    .map_err(|_| anyhow!("Failed to set username"))?;
                url.set_password(Some(password))
                    .map_err(|_| anyhow!("Failed to set password"))?;
                Ok(url)
            },
            None => Ok(Url::parse(&self.args.daemon_rpc_url)
                .map_err(|e| anyhow!("Invalid URL: {}", e))?)
        }
    }
    
    // Helper function to fetch a block from the Bitcoin node
    async fn fetch_block(&self, height: u32) -> Result<Vec<u8>> {
        use serde_json::{json, Number};
        
        // Create the JSON-RPC request
        let blockhash = self.fetch_blockhash(height).await?;
        
        // Get URL with auth
        let url = self.get_url_with_auth()?;
        
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
        
        // Send the request with retry logic
        let response_json = self.send_request(url, request_body).await?;
            
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
        use serde_json::json;
        
        // Get URL with auth
        let url = self.get_url_with_auth()?;
        
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
        
        // Send the request with retry logic
        let response_json = self.send_request(url, request_body).await?;
            
        // Extract the blockhash
        let blockhash_hex = response_json["result"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing result in response"))?;
            
        // Decode the hex
        let blockhash = hex::decode(blockhash_hex)
            .map_err(|e| anyhow!("Hex decode error: {}", e))?;
            
        Ok(blockhash)
    }

    
    // Parallel processing with pipeline
    pub async fn run_pipeline(&mut self) -> Result<()> {
        let mut height = self.start_block;
        CURRENT_HEIGHT.store(height, Ordering::SeqCst);
        
        // Create channels for the pipeline
        let (block_sender, mut block_receiver) = mpsc::channel::<(u32, Vec<u8>)>(self.args.pipeline_size);
        let (result_sender, mut result_receiver) = mpsc::channel::<BlockResult>(self.args.pipeline_size);
        
        // Spawn block fetcher task
        let fetcher_handle = {
            let args = self.args.clone();
            let diff_runtime = self.clone();
            let result_sender_clone = result_sender.clone();
            let block_sender_clone = block_sender.clone();
            
            tokio::spawn(async move {
                let mut current_height = height;
                
                loop {
                    // Check if we should exit
                    if let Some(exit_at) = args.exit_at {
                        if current_height >= exit_at {
                            info!("Fetcher reached exit-at block {}, shutting down", exit_at);
                            break;
                        }
                    }
                    
                    // Fetch the block
                    match diff_runtime.fetch_block(current_height).await {
                        Ok(block_data) => {
                            debug!("Fetched block {} ({})", current_height, block_data.len());
                            // Send block to processor
                            if block_sender_clone.send((current_height, block_data)).await.is_err() {
                                break;
                            }
                        },
                        Err(e) => {
                            error!("Failed to fetch block {}: {}", current_height, e);
                            // Send error result
                            if result_sender_clone.send(BlockResult::Error(current_height, e)).await.is_err() {
                                break;
                            }
                            sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    }
                    
                    current_height += 1;
                }
                
                debug!("Block fetcher task completed");
            })
        };
        
        // Spawn block processor task
        let processor_handle = {
            let mut diff_runtime = self.clone();
            let result_sender_clone = result_sender.clone();
            
            tokio::spawn(async move {
                while let Some((block_height, block_data)) = block_receiver.recv().await {
                    debug!("Processing block {} ({})", block_height, block_data.len());
                    
                    match diff_runtime.process_block(block_data, block_height).await {
                        Ok(has_diff) => {
                            if has_diff {
                                // If differences found, send a difference result
                                if result_sender_clone.send(BlockResult::Difference(block_height)).await.is_err() {
                                    break;
                                }
                            } else {
                                // If no differences, send a success result
                                if result_sender_clone.send(BlockResult::Success(block_height)).await.is_err() {
                                    break;
                                }
                            }
                        },
                        Err(e) => {
                            // If error, send an error result
                            if result_sender_clone.send(BlockResult::Error(block_height, e)).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                
                debug!("Block processor task completed");
            })
        };
        
        // Main loop to handle results
        while let Some(result) = result_receiver.recv().await {
            match result {
                BlockResult::Success(processed_height) => {
                    debug!("Successfully processed block {} with no differences", processed_height);
                    height = processed_height + 1;
                    CURRENT_HEIGHT.store(height, Ordering::SeqCst);
                },
                BlockResult::Error(failed_height, error) => {
                    error!("Failed to process block {}: {}", failed_height, error);
                    // We could implement more sophisticated error handling here
                    // For now, just wait and continue
                    sleep(Duration::from_secs(5)).await;
                },
                BlockResult::Difference(diff_height) => {
                    info!("Found differences at block {}", diff_height);
                    
                    // Check if we've reached the diff limit
                    if self.diffs_found >= self.args.diff_limit {
                        info!("Reached diff limit of {}, exiting", self.args.diff_limit);
                        
                        // Save summary report
                        if let Err(e) = self.save_summary_report() {
                            error!("Failed to save summary report: {}", e);
                        }
                        
                        // Clean up
                        drop(block_sender);
                        drop(result_sender);
                        return Ok(());
                    }
                    
                    // Continue processing if we haven't reached the limit
                    height = diff_height + 1;
                    CURRENT_HEIGHT.store(height, Ordering::SeqCst);
                }
            }
            
            // Check if we should exit
            if let Some(exit_at) = self.args.exit_at {
                if height > exit_at {
                    info!("Reached exit-at block {}, shutting down gracefully", exit_at);
                    
                    // Save summary report
                    if let Err(e) = self.save_summary_report() {
                        error!("Failed to save summary report: {}", e);
                    }
                    
                    break;
                }
            }
        }
        
        // Clean up
        drop(block_sender);
        drop(result_sender);
        
        // Wait for tasks to complete
        let _ = tokio::join!(fetcher_handle, processor_handle);
        
        // Save summary report before exiting
        if let Err(e) = self.save_summary_report() {
            error!("Failed to save summary report: {}", e);
        }
        
        Ok(())
    }
    
}

// Allow cloning for use in async tasks
impl Clone for RockshrewDiffRuntime {
    fn clone(&self) -> Self {
        // Instead of creating new database connections, we'll clone the existing runtimes
        // by creating new instances that share the same database connections
        
        // Get the database adapters from the existing runtimes
        let primary_db = {
            let context = self.primary_runtime.context.lock().unwrap();
            context.db.clone()
        };
        
        let compare_db = {
            let context = self.compare_runtime.context.lock().unwrap();
            context.db.clone()
        };
        
        // Create new runtime instances with the cloned database adapters
        let primary_runtime = MetashrewRuntime::load(
            PathBuf::from(&self.args.indexer),
            primary_db
        ).unwrap_or_else(|_| panic!("Failed to clone primary runtime"));
        
        let compare_runtime = MetashrewRuntime::load(
            PathBuf::from(&self.args.compare),
            compare_db
        ).unwrap_or_else(|_| panic!("Failed to clone compare runtime"));
        
        Self {
            primary_runtime,
            compare_runtime,
            args: self.args.clone(),
            start_block: self.start_block,
            prefix: self.prefix.clone(),
            diffs_found: self.diffs_found,
            diff_blocks: self.diff_blocks.clone(),
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
    // Removed deprecated set_max_background_compactions call
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
        None => hex::decode(&args.prefix)
            .map_err(|e| anyhow!("Invalid hex prefix: {}", e))?,
    };
    
    info!("Using prefix: {}", hex::encode(&prefix));
    
    // Create the db_path directory if it doesn't exist
    let db_path = Path::new(&args.db_path);
    if !db_path.exists() {
        fs::create_dir_all(db_path)?;
    }
    
    // Create the output directory if specified
    if let Some(output_dir) = &args.output_dir {
        let output_path = Path::new(output_dir);
        if !output_path.exists() {
            info!("Creating output directory: {}", output_path.display());
            fs::create_dir_all(output_path)?;
        }
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
    
    // Create tracking adapters
    let primary_tracking_adapter = TrackingAdapter::new(primary_db, prefix.clone());
    let compare_tracking_adapter = TrackingAdapter::new(compare_db, prefix.clone());
    
    // Load primary and compare WASM modules
    let primary_indexer: PathBuf = args.indexer.clone().into();
    let compare_indexer: PathBuf = args.compare.clone().into();
    
    let primary_runtime = MetashrewRuntime::load(
        primary_indexer,
        primary_tracking_adapter,
    )?;
    
    let compare_runtime = MetashrewRuntime::load(
        compare_indexer,
        compare_tracking_adapter,
    )?;
    
    // Get start block
    let start_block = args.start_block.unwrap_or(0);
    
    // Create and run the diff runtime
    let mut diff_runtime = RockshrewDiffRuntime::new(
        primary_runtime,
        compare_runtime,
        args,
        start_block,
        prefix,
    );
    
    diff_runtime.run_pipeline().await?;
    
    Ok(())
}
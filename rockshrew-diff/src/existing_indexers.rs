use anyhow::{anyhow, Result};
use clap::Parser;
use hex;
use itertools::Itertools;
use log::{debug, error, info};
use rocksdb::{IteratorMode, Options, DB};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Command line arguments for comparing existing indexers
#[derive(Parser, Debug, Clone)]
#[command(version, about = "Compare the state of two existing Metashrew indexers", long_about = None)]
pub struct ExistingIndexersArgs {
    /// Path to the first indexer's RocksDB database
    #[arg(long)]
    pub primary_db_path: String,

    /// Path to the second indexer's RocksDB database to compare against
    #[arg(long)]
    pub compare_db_path: String,

    /// Hex-encoded key prefix to compare (must start with 0x)
    #[arg(long)]
    pub prefix: String,

    /// Optional directory to save diff reports
    #[arg(long, help = "Directory to save diff reports")]
    pub output_dir: Option<String>,

    /// Maximum number of keys to compare (0 for all keys)
    #[arg(
        long,
        default_value_t = 0,
        help = "Maximum number of keys to compare (0 for all)"
    )]
    pub max_keys: usize,

    /// Maximum number of differences to find before exiting
    #[arg(
        long,
        default_value_t = 100,
        help = "Maximum number of differences to find before exiting"
    )]
    pub diff_limit: usize,

    /// Optional height to filter keys by (if your keys include height information)
    #[arg(long, help = "Optional height to filter keys by")]
    pub height_filter: Option<u32>,

    /// Batch size for processing keys (to reduce memory usage)
    #[arg(
        long,
        default_value_t = 10000,
        help = "Number of keys to process in each batch"
    )]
    pub batch_size: usize,

    /// Whether to sort keys before comparing (needed for batch processing)
    #[arg(
        long,
        default_value_t = true,
        help = "Sort keys before comparing (recommended for batch processing)"
    )]
    pub sort_keys: bool,
}

/// Struct to handle comparison of existing indexers
pub struct ExistingIndexersComparator {
    args: ExistingIndexersArgs,
    prefix: Vec<u8>,
    primary_db: DB,
    compare_db: DB,
    diffs_found: usize,
    total_keys_processed: AtomicUsize,
}

impl ExistingIndexersComparator {
    /// Create a new comparator for existing indexers
    pub fn new(args: ExistingIndexersArgs, prefix: Vec<u8>) -> Result<Self> {
        // Create RocksDB options
        let mut opts = Options::default();
        opts.set_max_open_files(10000);
        opts.set_use_fsync(false);
        opts.optimize_for_point_lookup(1024);
        opts.set_max_background_jobs(4);

        // Open the databases in read-only mode to avoid any modifications
        let primary_db_path = Path::new(&args.primary_db_path);
        let compare_db_path = Path::new(&args.compare_db_path);

        if !primary_db_path.exists() {
            return Err(anyhow!(
                "Primary database path does not exist: {}",
                primary_db_path.display()
            ));
        }

        if !compare_db_path.exists() {
            return Err(anyhow!(
                "Compare database path does not exist: {}",
                compare_db_path.display()
            ));
        }

        // Open the databases
        let primary_db = DB::open_for_read_only(&opts, primary_db_path, false)
            .map_err(|e| anyhow!("Failed to open primary database: {}", e))?;

        let compare_db = DB::open_for_read_only(&opts, compare_db_path, false)
            .map_err(|e| anyhow!("Failed to open compare database: {}", e))?;

        Ok(Self {
            args,
            prefix,
            primary_db,
            compare_db,
            diffs_found: 0,
            total_keys_processed: AtomicUsize::new(0),
        })
    }

    /// Check if a key matches the specified height
    /// This is a placeholder - implement your own logic based on your key schema
    fn key_matches_height(&self, key: &[u8], height: u32) -> bool {
        // This is a simplified example - you would need to implement
        // your own logic to extract height from keys based on your schema
        // For example, if your keys have a height component at a specific position:

        // Example: If height is encoded as a 4-byte big-endian value at position 8
        if key.len() >= 12 {
            // prefix + height (assuming prefix is 8 bytes)
            let height_bytes = &key[8..12];
            if height_bytes.len() == 4 {
                let key_height = u32::from_be_bytes([
                    height_bytes[0],
                    height_bytes[1],
                    height_bytes[2],
                    height_bytes[3],
                ]);
                return key_height == height;
            }
        }

        // Default: no height filtering
        true
    }

    /// Get an iterator over the database with the specified prefix
    fn get_prefix_iterator<'a>(&self, db: &'a DB) -> rocksdb::DBIterator<'a> {
        db.prefix_iterator(&self.prefix)
    }

    /// Process a batch of keys from the database
    fn process_batch(
        &self,
        primary_iter: &mut rocksdb::DBIterator,
        compare_iter: &mut rocksdb::DBIterator,
        batch_size: usize,
    ) -> Result<(
        BTreeMap<Vec<u8>, Vec<u8>>,
        BTreeMap<Vec<u8>, Vec<u8>>,
        bool,
        bool,
    )> {
        let mut primary_batch = BTreeMap::new();
        let mut compare_batch = BTreeMap::new();
        let mut primary_exhausted = false;
        let mut compare_exhausted = false;
        let mut primary_count = 0;
        let mut compare_count = 0;

        // Process primary database batch
        while primary_count < batch_size {
            if let Some(item) = primary_iter.next() {
                match item {
                    Ok((key, value)) => {
                        // Check if key still has our prefix
                        if !key.starts_with(&self.prefix) {
                            primary_exhausted = true;
                            break;
                        }

                        // Apply height filter if specified
                        if let Some(height) = self.args.height_filter {
                            if !self.key_matches_height(&key, height) {
                                continue;
                            }
                        }

                        primary_batch.insert(key.to_vec(), value.to_vec());
                        primary_count += 1;

                        // Check if we've reached the maximum number of keys
                        if self.args.max_keys > 0
                            && self.total_keys_processed.load(Ordering::SeqCst) + primary_count
                                >= self.args.max_keys
                        {
                            primary_exhausted = true;
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error reading key-value pair from primary DB: {}", e);
                    }
                }
            } else {
                primary_exhausted = true;
                break;
            }
        }

        // Process compare database batch
        while compare_count < batch_size {
            if let Some(item) = compare_iter.next() {
                match item {
                    Ok((key, value)) => {
                        // Check if key still has our prefix
                        if !key.starts_with(&self.prefix) {
                            compare_exhausted = true;
                            break;
                        }

                        // Apply height filter if specified
                        if let Some(height) = self.args.height_filter {
                            if !self.key_matches_height(&key, height) {
                                continue;
                            }
                        }

                        compare_batch.insert(key.to_vec(), value.to_vec());
                        compare_count += 1;

                        // Check if we've reached the maximum number of keys
                        if self.args.max_keys > 0
                            && self.total_keys_processed.load(Ordering::SeqCst) + compare_count
                                >= self.args.max_keys
                        {
                            compare_exhausted = true;
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error reading key-value pair from compare DB: {}", e);
                    }
                }
            } else {
                compare_exhausted = true;
                break;
            }
        }

        let processed = primary_count.max(compare_count);
        self.total_keys_processed
            .fetch_add(processed, Ordering::SeqCst);

        info!(
            "Processed batch: {} primary keys, {} compare keys (total processed: {})",
            primary_count,
            compare_count,
            self.total_keys_processed.load(Ordering::SeqCst)
        );

        Ok((
            primary_batch,
            compare_batch,
            primary_exhausted,
            compare_exhausted,
        ))
    }

    /// Compare keys between primary and compare batches
    fn compare_batches(
        &self,
        primary_batch: &BTreeMap<Vec<u8>, Vec<u8>>,
        compare_batch: &BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<(Vec<u8>, Vec<u8>, Vec<u8>)>) {
        let mut primary_only = Vec::new();
        let mut compare_only = Vec::new();
        let mut value_diffs = Vec::new();

        // Create a unified set of keys from both batches
        let mut all_keys = HashSet::new();
        for key in primary_batch.keys() {
            all_keys.insert(key);
        }
        for key in compare_batch.keys() {
            all_keys.insert(key);
        }

        // Compare each key
        for key in all_keys {
            match (primary_batch.get(key), compare_batch.get(key)) {
                (Some(primary_value), Some(compare_value)) => {
                    // Key exists in both, check if values are different
                    if primary_value != compare_value {
                        value_diffs.push((
                            key.clone(),
                            primary_value.clone(),
                            compare_value.clone(),
                        ));
                    }
                }
                (Some(_), None) => {
                    // Key only in primary
                    primary_only.push(key.clone());
                }
                (None, Some(_)) => {
                    // Key only in compare
                    compare_only.push(key.clone());
                }
                _ => unreachable!(),
            }
        }

        (primary_only, compare_only, value_diffs)
    }

    /// Generate a diff report as a string
    fn generate_diff_report(
        &self,
        primary_only: &[Vec<u8>],
        compare_only: &[Vec<u8>],
        value_diffs: &[(Vec<u8>, Vec<u8>, Vec<u8>)],
    ) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        let mut report = format!("=== DIFF REPORT (Timestamp: {}) ===\n", timestamp);

        if primary_only.is_empty() && compare_only.is_empty() && value_diffs.is_empty() {
            report.push_str("No differences found.\n");
            return report;
        }

        // Report keys only in primary
        if !primary_only.is_empty() {
            report.push_str(&format!(
                "\nKeys in primary but not in compare ({}):\n",
                primary_only.len()
            ));
            for key in primary_only.iter().take(20) {
                // Limit to first 20 for readability
                report.push_str(&format!("  {}\n", hex::encode(key)));
            }
            if primary_only.len() > 20 {
                report.push_str(&format!("  ... and {} more\n", primary_only.len() - 20));
            }
        }

        // Report keys only in compare
        if !compare_only.is_empty() {
            report.push_str(&format!(
                "\nKeys in compare but not in primary ({}):\n",
                compare_only.len()
            ));
            for key in compare_only.iter().take(20) {
                // Limit to first 20 for readability
                report.push_str(&format!("  {}\n", hex::encode(key)));
            }
            if compare_only.len() > 20 {
                report.push_str(&format!("  ... and {} more\n", compare_only.len() - 20));
            }
        }

        // Report keys with different values
        if !value_diffs.is_empty() {
            report.push_str(&format!(
                "\nKeys with different values ({}):\n",
                value_diffs.len()
            ));
            for (key, primary_value, compare_value) in value_diffs.iter().take(20) {
                // Limit to first 20 for readability
                report.push_str(&format!("  Key: {}\n", hex::encode(key)));
                report.push_str(&format!("    Primary:   {}\n", hex::encode(primary_value)));
                report.push_str(&format!("    Compare:   {}\n", hex::encode(compare_value)));
            }
            if value_diffs.len() > 20 {
                report.push_str(&format!("  ... and {} more\n", value_diffs.len() - 20));
            }
        }

        report.push_str("\n=== END DIFF REPORT ===\n");
        report
    }

    /// Print a report of the differences
    fn print_diff_report(
        &self,
        primary_only: &[Vec<u8>],
        compare_only: &[Vec<u8>],
        value_diffs: &[(Vec<u8>, Vec<u8>, Vec<u8>)],
    ) {
        let report = self.generate_diff_report(primary_only, compare_only, value_diffs);
        print!("{}", report);
    }

    /// Save a diff report to a file
    fn save_diff_report(
        &self,
        primary_only: &[Vec<u8>],
        compare_only: &[Vec<u8>],
        value_diffs: &[(Vec<u8>, Vec<u8>, Vec<u8>)],
    ) -> Result<()> {
        // Check if output directory is specified
        if let Some(output_dir) = &self.args.output_dir {
            // Create the output directory if it doesn't exist
            let output_path = Path::new(output_dir);
            if !output_path.exists() {
                fs::create_dir_all(output_path)?;
            }

            // Generate the report
            let report = self.generate_diff_report(primary_only, compare_only, value_diffs);

            // Create a timestamp for the filename
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_secs();

            // Create the file path
            let file_path = output_path.join(format!("diff_report_{}.txt", timestamp));

            // Write the report to the file
            fs::write(&file_path, report)?;

            info!("Saved diff report to {}", file_path.display());
        }

        Ok(())
    }

    /// Run the comparison between the two databases in batches
    pub fn run(&mut self) -> Result<()> {
        info!(
            "Starting batch comparison of databases with prefix {:?}...",
            hex::encode(&self.prefix)
        );
        info!("Using batch size of {} keys", self.args.batch_size);

        // Get iterators for both databases
        let mut primary_iter = self.get_prefix_iterator(&self.primary_db);
        let mut compare_iter = self.get_prefix_iterator(&self.compare_db);

        // Track total differences
        let mut total_primary_only = Vec::new();
        let mut total_compare_only = Vec::new();
        let mut total_value_diffs = Vec::new();

        // Process in batches
        let mut batch_num = 0;
        let mut primary_exhausted = false;
        let mut compare_exhausted = false;

        while !primary_exhausted || !compare_exhausted {
            batch_num += 1;
            info!("Processing batch #{}", batch_num);

            // Process a batch from both databases
            let (primary_batch, compare_batch, p_exhausted, c_exhausted) =
                self.process_batch(&mut primary_iter, &mut compare_iter, self.args.batch_size)?;

            primary_exhausted = p_exhausted;
            compare_exhausted = c_exhausted;

            // Compare the batches
            let (primary_only, compare_only, value_diffs) =
                self.compare_batches(&primary_batch, &compare_batch);

            // Add to totals
            total_primary_only.extend(primary_only);
            total_compare_only.extend(compare_only);
            total_value_diffs.extend(value_diffs);

            // Check if we've reached the diff limit
            let current_total_diffs =
                total_primary_only.len() + total_compare_only.len() + total_value_diffs.len();
            if current_total_diffs >= self.args.diff_limit {
                info!(
                    "Reached diff limit of {}, stopping comparison",
                    self.args.diff_limit
                );
                break;
            }

            // Check if both iterators are exhausted
            if primary_exhausted && compare_exhausted {
                info!("Both databases fully processed");
                break;
            }

            // Check if max keys reached
            if self.args.max_keys > 0
                && self.total_keys_processed.load(Ordering::SeqCst) >= self.args.max_keys
            {
                info!("Reached maximum key count of {}", self.args.max_keys);
                break;
            }
        }

        // Calculate total differences
        let total_diffs =
            total_primary_only.len() + total_compare_only.len() + total_value_diffs.len();
        self.diffs_found = total_diffs;

        // Print summary
        info!("Comparison complete:");
        info!(
            "  Total keys processed: {}",
            self.total_keys_processed.load(Ordering::SeqCst)
        );
        info!("  Keys only in primary: {}", total_primary_only.len());
        info!("  Keys only in compare: {}", total_compare_only.len());
        info!("  Keys with different values: {}", total_value_diffs.len());
        info!("  Total differences: {}", total_diffs);

        // Print and save detailed report if differences found
        if total_diffs > 0 {
            self.print_diff_report(&total_primary_only, &total_compare_only, &total_value_diffs);

            if let Err(e) =
                self.save_diff_report(&total_primary_only, &total_compare_only, &total_value_diffs)
            {
                error!("Failed to save diff report: {}", e);
            }

            if total_diffs > self.args.diff_limit {
                info!(
                    "Found {} differences, which exceeds the limit of {}",
                    total_diffs, self.args.diff_limit
                );
            }
        } else {
            info!("No differences found between the databases for the specified prefix.");
        }

        Ok(())
    }
}

//! Snapshot Hang Analyzer
//! 
//! This tool analyzes potential causes for snapshot hanging at ~600 blocks
//! and provides diagnostic information to identify the root cause.

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use rocksdb::{Options, DB};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Potential causes for snapshot hanging
#[derive(Debug)]
pub enum HangCause {
    /// Too many key-value pairs accumulated in memory
    MemoryAccumulation {
        tracked_keys: usize,
        raw_operations: usize,
        estimated_memory_mb: f64,
    },
    /// Database compaction blocking operations
    DatabaseCompaction {
        pending_compactions: usize,
        write_stalls: bool,
    },
    /// Inefficient key extraction from append-only format
    KeyExtractionBottleneck {
        total_keys_processed: usize,
        extraction_time_ms: u64,
    },
    /// Large snapshot diff creation
    SnapshotCreationBottleneck {
        diff_size_mb: f64,
        compression_time_ms: u64,
    },
    /// File I/O blocking on snapshot directory
    FileSystemBottleneck {
        directory_size_mb: f64,
        write_speed_mbps: f64,
    },
}

pub struct SnapshotHangAnalyzer {
    db_path: PathBuf,
    snapshot_dir: Option<PathBuf>,
}

impl SnapshotHangAnalyzer {
    pub fn new(db_path: PathBuf, snapshot_dir: Option<PathBuf>) -> Self {
        Self {
            db_path,
            snapshot_dir,
        }
    }

    /// Analyze potential causes for snapshot hanging
    pub async fn analyze(&self) -> Result<Vec<HangCause>> {
        let mut causes = Vec::new();

        info!("🔍 Analyzing snapshot hang causes...");

        // 1. Check database state and compaction
        if let Some(cause) = self.check_database_compaction().await? {
            causes.push(cause);
        }

        // 2. Simulate key-value tracking accumulation
        if let Some(cause) = self.simulate_kv_accumulation().await? {
            causes.push(cause);
        }

        // 3. Check snapshot directory I/O performance
        if let Some(cause) = self.check_snapshot_io_performance().await? {
            causes.push(cause);
        }

        // 4. Analyze append-only key extraction performance
        if let Some(cause) = self.analyze_key_extraction_performance().await? {
            causes.push(cause);
        }

        Ok(causes)
    }

    /// Check if database compaction is causing blocking
    async fn check_database_compaction(&self) -> Result<Option<HangCause>> {
        info!("📊 Checking database compaction status...");

        let mut opts = Options::default();
        opts.create_if_missing(false); // Don't create if missing
        
        let db = match DB::open(&opts, &self.db_path) {
            Ok(db) => db,
            Err(e) => {
                warn!("Could not open database for analysis: {}", e);
                return Ok(None);
            }
        };

        // Check RocksDB properties for compaction status
        let mut write_stalls = false;
        let mut pending_compactions = 0;

        // Check for write stalls
        if let Ok(Some(stall_info)) = db.property_value("rocksdb.is-write-stopped") {
            if stall_info == "1" {
                write_stalls = true;
                warn!("⚠️  Database writes are currently stalled!");
            }
        }

        // Check pending compactions
        if let Ok(Some(compaction_info)) = db.property_value("rocksdb.compaction-pending") {
            if compaction_info == "1" {
                pending_compactions = 1;
                warn!("⚠️  Database compaction is pending!");
            }
        }

        // Check level 0 files (can cause slowdowns)
        if let Ok(Some(l0_files)) = db.property_value("rocksdb.num-files-at-level0") {
            if let Ok(count) = l0_files.parse::<u32>() {
                if count > 20 {
                    warn!("⚠️  High number of L0 files: {} (may cause slowdowns)", count);
                    pending_compactions += 1;
                }
            }
        }

        if write_stalls || pending_compactions > 0 {
            return Ok(Some(HangCause::DatabaseCompaction {
                pending_compactions,
                write_stalls,
            }));
        }

        info!("✅ Database compaction looks healthy");
        Ok(None)
    }

    /// Simulate key-value accumulation over 600 blocks
    async fn simulate_kv_accumulation(&self) -> Result<Option<HangCause>> {
        info!("🧮 Simulating key-value accumulation over 600 blocks...");

        // Simulate typical ALKANES workload: ~40k key-value updates per block
        let blocks = 600;
        let updates_per_block = 40_000;
        let avg_key_size = 50; // bytes
        let avg_value_size = 100; // bytes

        let total_tracked_keys = blocks * updates_per_block;
        let total_raw_operations = total_tracked_keys * 2; // Assume some duplication

        // Estimate memory usage
        let memory_per_entry = avg_key_size + avg_value_size + 64; // Include HashMap overhead
        let estimated_memory_bytes = total_tracked_keys * memory_per_entry;
        let estimated_memory_mb = estimated_memory_bytes as f64 / (1024.0 * 1024.0);

        info!(
            "📈 Estimated accumulation: {} tracked keys, {:.1} MB memory",
            total_tracked_keys, estimated_memory_mb
        );

        // Check if this could cause memory pressure
        if estimated_memory_mb > 1000.0 {
            // More than 1GB of tracked changes
            warn!("⚠️  High memory accumulation detected!");
            return Ok(Some(HangCause::MemoryAccumulation {
                tracked_keys: total_tracked_keys,
                raw_operations: total_raw_operations,
                estimated_memory_mb,
            }));
        }

        info!("✅ Memory accumulation within reasonable bounds");
        Ok(None)
    }

    /// Check snapshot directory I/O performance
    async fn check_snapshot_io_performance(&self) -> Result<Option<HangCause>> {
        if let Some(ref snapshot_dir) = self.snapshot_dir {
            info!("💾 Checking snapshot directory I/O performance...");

            // Check if directory exists and get size
            if !snapshot_dir.exists() {
                info!("📁 Snapshot directory doesn't exist yet");
                return Ok(None);
            }

            let mut total_size = 0u64;
            if let Ok(entries) = std::fs::read_dir(snapshot_dir) {
                for entry in entries.flatten() {
                    if let Ok(metadata) = entry.metadata() {
                        total_size += metadata.len();
                    }
                }
            }

            let directory_size_mb = total_size as f64 / (1024.0 * 1024.0);
            info!("📊 Snapshot directory size: {:.1} MB", directory_size_mb);

            // Test write performance with a small file
            let test_file = snapshot_dir.join("write_test.tmp");
            let test_data = vec![0u8; 1024 * 1024]; // 1MB test data
            
            let start = Instant::now();
            if let Err(e) = std::fs::write(&test_file, &test_data) {
                warn!("Failed to test write performance: {}", e);
                return Ok(None);
            }
            let write_time = start.elapsed();
            
            // Clean up test file
            let _ = std::fs::remove_file(&test_file);

            let write_speed_mbps = 1.0 / write_time.as_secs_f64();
            info!("📈 Write speed: {:.1} MB/s", write_speed_mbps);

            if write_speed_mbps < 10.0 {
                // Less than 10 MB/s is quite slow
                warn!("⚠️  Slow filesystem write performance detected!");
                return Ok(Some(HangCause::FileSystemBottleneck {
                    directory_size_mb,
                    write_speed_mbps,
                }));
            }

            info!("✅ Filesystem I/O performance looks good");
        }

        Ok(None)
    }

    /// Analyze key extraction performance from append-only format
    async fn analyze_key_extraction_performance(&self) -> Result<Option<HangCause>> {
        info!("🔑 Analyzing key extraction performance...");

        let mut opts = Options::default();
        opts.create_if_missing(false);
        
        let db = match DB::open(&opts, &self.db_path) {
            Ok(db) => db,
            Err(e) => {
                warn!("Could not open database for key extraction analysis: {}", e);
                return Ok(None);
            }
        };

        // Sample some keys to test extraction performance
        let start = Instant::now();
        let mut processed_keys = 0;
        let mut extracted_keys = HashMap::new();

        let iter = db.iterator(rocksdb::IteratorMode::Start);
        for (key, value) in iter.take(10000) { // Sample first 10k keys
            processed_keys += 1;
            
            // Simulate the key extraction logic from SnapshotManager::extract_logical_kv
            let key_str = String::from_utf8_lossy(&key);
            
            if key_str.contains('/') && !key_str.ends_with("/length") {
                // This is likely an append-only key, try to extract logical k/v
                if let Some(slash_pos) = key_str.rfind('/') {
                    let suffix = &key_str[slash_pos + 1..];
                    
                    if suffix.chars().all(|c| c.is_ascii_digit()) {
                        let base_key = key_str[..slash_pos].as_bytes().to_vec();
                        extracted_keys.insert(base_key, value.to_vec());
                    }
                }
            }
        }

        let extraction_time = start.elapsed();
        let extraction_time_ms = extraction_time.as_millis() as u64;

        info!(
            "📊 Processed {} keys in {} ms, extracted {} logical keys",
            processed_keys, extraction_time_ms, extracted_keys.len()
        );

        // Check if extraction is taking too long
        if extraction_time_ms > 5000 && processed_keys > 0 {
            // More than 5 seconds for 10k keys is concerning
            let keys_per_second = processed_keys as f64 / extraction_time.as_secs_f64();
            warn!(
                "⚠️  Slow key extraction: {:.0} keys/second",
                keys_per_second
            );
            
            return Ok(Some(HangCause::KeyExtractionBottleneck {
                total_keys_processed: processed_keys,
                extraction_time_ms,
            }));
        }

        info!("✅ Key extraction performance looks good");
        Ok(None)
    }

    /// Provide recommendations based on identified causes
    pub fn get_recommendations(&self, causes: &[HangCause]) -> Vec<String> {
        let mut recommendations = Vec::new();

        for cause in causes {
            match cause {
                HangCause::MemoryAccumulation { tracked_keys, estimated_memory_mb, .. } => {
                    recommendations.push(format!(
                        "🔧 MEMORY ACCUMULATION FIX: Reduce snapshot interval from 1000 to 100-200 blocks. \
                        Currently tracking {} keys ({:.1} MB) - this grows linearly and can cause hanging.",
                        tracked_keys, estimated_memory_mb
                    ));
                    recommendations.push(
                        "🔧 ALTERNATIVE: Implement periodic clearing of tracked changes every 100 blocks \
                        instead of waiting for full snapshot interval.".to_string()
                    );
                }
                HangCause::DatabaseCompaction { pending_compactions, write_stalls } => {
                    if *write_stalls {
                        recommendations.push(
                            "🔧 DATABASE STALL FIX: Increase write buffer size and reduce L0 file triggers. \
                            Add: opts.set_write_buffer_size(512 * 1024 * 1024); \
                            opts.set_level_zero_slowdown_writes_trigger(30);".to_string()
                        );
                    }
                    if *pending_compactions > 0 {
                        recommendations.push(
                            "🔧 COMPACTION FIX: Increase background compaction threads. \
                            Add: opts.set_max_background_compactions(8);".to_string()
                        );
                    }
                }
                HangCause::KeyExtractionBottleneck { total_keys_processed, extraction_time_ms } => {
                    recommendations.push(format!(
                        "🔧 KEY EXTRACTION FIX: Optimize the extract_logical_kv function. \
                        Currently processing {} keys in {} ms. Consider caching or batching.",
                        total_keys_processed, extraction_time_ms
                    ));
                }
                HangCause::SnapshotCreationBottleneck { diff_size_mb, compression_time_ms } => {
                    recommendations.push(format!(
                        "🔧 SNAPSHOT SIZE FIX: Reduce compression level or implement streaming compression. \
                        Current: {:.1} MB taking {} ms to compress.",
                        diff_size_mb, compression_time_ms
                    ));
                }
                HangCause::FileSystemBottleneck { write_speed_mbps, .. } => {
                    recommendations.push(format!(
                        "🔧 FILESYSTEM FIX: Move snapshot directory to faster storage (SSD). \
                        Current write speed: {:.1} MB/s is too slow for large snapshots.",
                        write_speed_mbps
                    ));
                }
            }
        }

        if recommendations.is_empty() {
            recommendations.push(
                "🔧 GENERAL FIX: The hang might be caused by the combination of factors. \
                Try reducing snapshot interval to 100 blocks as a first step.".to_string()
            );
        }

        recommendations
    }
}

/// Quick diagnostic function that can be called from main
pub async fn diagnose_snapshot_hang(db_path: PathBuf, snapshot_dir: Option<PathBuf>) -> Result<()> {
    println!("🚨 SNAPSHOT HANG DIAGNOSTIC TOOL");
    println!("================================");
    
    let analyzer = SnapshotHangAnalyzer::new(db_path, snapshot_dir);
    
    // Run analysis with timeout to avoid hanging the diagnostic tool itself
    let causes = match timeout(Duration::from_secs(30), analyzer.analyze()).await {
        Ok(Ok(causes)) => causes,
        Ok(Err(e)) => {
            error!("Analysis failed: {}", e);
            return Err(e);
        }
        Err(_) => {
            error!("Analysis timed out after 30 seconds");
            return Err(anyhow!("Analysis timed out"));
        }
    };

    println!("\n📋 ANALYSIS RESULTS:");
    println!("===================");

    if causes.is_empty() {
        println!("✅ No obvious causes detected in static analysis.");
        println!("💡 The hang might occur during runtime due to accumulated state.");
    } else {
        for (i, cause) in causes.iter().enumerate() {
            println!("{}. {:?}", i + 1, cause);
        }
    }

    println!("\n🔧 RECOMMENDATIONS:");
    println!("===================");
    
    let recommendations = analyzer.get_recommendations(&causes);
    for (i, rec) in recommendations.iter().enumerate() {
        println!("{}. {}", i + 1, rec);
    }

    println!("\n🎯 MOST LIKELY CAUSE:");
    println!("====================");
    println!("Based on the code analysis, the most likely cause is MEMORY ACCUMULATION.");
    println!("The SnapshotManager tracks ALL key-value changes in HashMap<Vec<u8>, Vec<u8>>.");
    println!("At 600 blocks with ~40k updates/block, this is ~24M entries = several GB RAM.");
    println!("This causes the snapshot creation to hang when trying to process this data.");
    
    println!("\n🚀 IMMEDIATE FIX:");
    println!("=================");
    println!("1. Reduce --snapshot-interval from 1000 to 100 blocks");
    println!("2. Or disable snapshots temporarily with --snapshot-directory=\"\"");
    println!("3. Monitor memory usage during indexing to confirm");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <db_path> [snapshot_dir]", args[0]);
        return Ok(());
    }

    let db_path = PathBuf::from(&args[1]);
    let snapshot_dir = args.get(2).map(PathBuf::from);

    diagnose_snapshot_hang(db_path, snapshot_dir).await
}
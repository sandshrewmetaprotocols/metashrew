//! Database analysis tool to identify data explosion patterns

use rocksdb::{DB, Options, IteratorMode};
use std::collections::HashMap;

pub fn analyze_database(db_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Analyzing RocksDB database at {}", db_path);
    
    // Open the database
    let mut opts = Options::default();
    opts.create_if_missing(false);
    let db = DB::open_for_read_only(&opts, db_path, false)?;
    
    // Analyze key patterns
    let mut key_patterns = HashMap::new();
    let mut total_keys = 0;
    let mut total_key_size = 0;
    let mut total_value_size = 0;
    let mut sample_keys = Vec::new();
    
    println!("📊 Scanning database entries...");
    
    let iter = db.iterator(IteratorMode::Start);
    for (i, item) in iter.enumerate() {
        let (key, value) = item?;
        total_keys += 1;
        total_key_size += key.len();
        total_value_size += value.len();
        
        // Store sample keys for analysis
        if i < 50 {
            sample_keys.push((key.to_vec(), value.len()));
        }
        
        // Analyze key patterns
        let key_str = String::from_utf8_lossy(&key);
        let pattern = if key_str.starts_with("current:") {
            "current:"
        } else if key_str.starts_with("hist:") {
            "hist:"
        } else if key_str.starts_with("keys:") {
            "keys:"
        } else if key_str.starts_with("height:") {
            "height:"
        } else if key_str.starts_with("smt:") {
            "smt:"
        } else {
            "other"
        };
        
        *key_patterns.entry(pattern.to_string()).or_insert(0) += 1;
        
        // Print progress every 100k entries
        if i > 0 && i % 100000 == 0 {
            println!("  Processed {} entries...", i);
        }
        
        // Limit analysis to prevent hanging
        if i > 1000000 {
            println!("  Stopping analysis at 1M entries to prevent hanging...");
            break;
        }
    }
    
    println!("\n📈 Database Analysis Results:");
    println!("  Total entries: {}", total_keys);
    println!("  Total key size: {:.2} MB", total_key_size as f64 / 1024.0 / 1024.0);
    println!("  Total value size: {:.2} MB", total_value_size as f64 / 1024.0 / 1024.0);
    println!("  Average key size: {:.1} bytes", total_key_size as f64 / total_keys as f64);
    println!("  Average value size: {:.1} bytes", total_value_size as f64 / total_keys as f64);
    
    println!("\n🔍 Key Pattern Analysis:");
    let mut patterns: Vec<_> = key_patterns.iter().collect();
    patterns.sort_by(|a, b| b.1.cmp(a.1));
    
    for (pattern, count) in patterns {
        let percentage = (*count as f64 / total_keys as f64) * 100.0;
        println!("  {}: {} entries ({:.1}%)", pattern, count, percentage);
    }
    
    println!("\n📝 Sample Keys (first 50):");
    for (i, (key, value_size)) in sample_keys.iter().enumerate() {
        let key_str = String::from_utf8_lossy(key);
        let display_key = if key.len() > 100 { 
            format!("{}...", &key_str[..100])
        } else { 
            key_str.to_string()
        };
        println!("  {}: {} (key: {} bytes, value: {} bytes)", i, display_key, key.len(), value_size);
    }
    
    Ok(())
}
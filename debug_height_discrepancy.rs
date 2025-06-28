//! Debug script to investigate height discrepancy between sync engine and storage
//! 
//! This script will check both the sync engine's current_height and the storage's indexed_height
//! to identify where the discrepancy is coming from.

use anyhow::Result;
use rocksdb::{Options, DB};
use std::sync::Arc;

fn main() -> Result<()> {
    // Open the RocksDB database (adjust path as needed)
    let db_path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: {} <db_path>", std::env::args().next().unwrap());
        std::process::exit(1);
    });

    println!("Opening database at: {}", db_path);
    
    let mut opts = Options::default();
    opts.create_if_missing(false); // Don't create if missing - we want to read existing
    
    let db = DB::open(&opts, &db_path)?;
    
    // Check the storage's indexed height
    let height_key = b"__INTERNAL/height";
    let storage_indexed_height = match db.get(height_key)? {
        Some(value) => {
            if value.len() >= 4 {
                let height_bytes: [u8; 4] = value[..4].try_into().unwrap();
                u32::from_le_bytes(height_bytes)
            } else {
                0
            }
        }
        None => 0,
    };
    
    println!("Storage indexed height: {}", storage_indexed_height);
    
    // Check what block hashes we have stored
    println!("\nChecking recent block hashes:");
    for height in storage_indexed_height.saturating_sub(5)..=storage_indexed_height + 5 {
        let blockhash_key = format!("/__INTERNAL/height-to-hash/{}", height);
        match db.get(blockhash_key.as_bytes())? {
            Some(hash) => {
                println!("  Height {}: {}", height, hex::encode(&hash));
            }
            None => {
                println!("  Height {}: <not found>", height);
            }
        }
    }
    
    // Check what state roots we have
    println!("\nChecking recent state roots:");
    for height in storage_indexed_height.saturating_sub(5)..=storage_indexed_height + 5 {
        let root_key = format!("smt:root:{}", height);
        match db.get(root_key.as_bytes())? {
            Some(root) => {
                println!("  Height {}: {}", height, hex::encode(&root));
            }
            None => {
                println!("  Height {}: <not found>", height);
            }
        }
    }
    
    // Look for any other height-related keys
    println!("\nScanning for other height-related keys:");
    let iter = db.iterator(rocksdb::IteratorMode::Start);
    let mut height_keys = Vec::new();
    
    for item in iter {
        let (key, _value) = item?;
        let key_str = String::from_utf8_lossy(&key);
        if key_str.contains("height") || key_str.contains("HEIGHT") {
            height_keys.push(key_str.to_string());
        }
        
        // Limit to first 100 height-related keys to avoid spam
        if height_keys.len() >= 100 {
            break;
        }
    }
    
    for key in height_keys {
        println!("  Found height-related key: {}", key);
    }
    
    println!("\n=== SUMMARY ===");
    println!("Storage reports indexed height: {}", storage_indexed_height);
    println!("If metashrew_height is returning 879999, but you're synced to 902200+,");
    println!("then the sync engine's current_height atomic variable is likely out of sync");
    println!("with the storage's indexed_height.");
    
    Ok(())
}
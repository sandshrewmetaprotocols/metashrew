//! LRU Cache Debugging Example
//!
//! This example demonstrates how to use the LRU cache debugging functionality
//! to analyze cache usage patterns and key prefix statistics.

use metashrew_core::{
    initialize, get, set, flush,
    enable_lru_debug_mode, disable_lru_debug_mode, is_lru_debug_mode_enabled,
    set_prefix_analysis_config, get_prefix_analysis_config, clear_prefix_hit_stats,
    get_lru_debug_stats, generate_lru_debug_report,
    PrefixAnalysisConfig, lru_cache_stats
};
use std::sync::Arc;

fn main() {
    println!("🔍 LRU Cache Debugging Example");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Initialize the cache system
    initialize();
    
    // Enable debug mode
    enable_lru_debug_mode();
    println!("✅ LRU debug mode enabled: {}", is_lru_debug_mode_enabled());
    
    // Configure prefix analysis
    let config = PrefixAnalysisConfig {
        min_prefix_length: 2,
        max_prefix_length: 8,
        min_keys_per_prefix: 2,
    };
    set_prefix_analysis_config(config);
    println!("✅ Prefix analysis configured: {:?}\n", get_prefix_analysis_config());
    
    // Simulate some cache operations with different key patterns
    println!("📝 Simulating cache operations...");
    
    // User-related keys
    for i in 0..10 {
        let key = Arc::new(format!("user:{:04}", i).into_bytes());
        let value = Arc::new(format!("user_data_{}", i).into_bytes());
        set(key.clone(), value);
        
        // Access some keys multiple times to create hits
        if i % 3 == 0 {
            let _ = get(key.clone());
            let _ = get(key);
        }
    }
    
    // Transaction-related keys
    for i in 0..15 {
        let key = Arc::new(format!("tx:{:06}", i).into_bytes());
        let value = Arc::new(format!("transaction_data_{}", i).into_bytes());
        set(key.clone(), value);
        
        // Access some keys multiple times
        if i % 2 == 0 {
            let _ = get(key);
        }
    }
    
    // Block-related keys
    for i in 0..8 {
        let key = Arc::new(format!("block:{:08}", i).into_bytes());
        let value = Arc::new(format!("block_data_{}", i).into_bytes());
        set(key.clone(), value);
        
        // Access all block keys multiple times
        let _ = get(key.clone());
        let _ = get(key);
    }
    
    // UTXO-related keys
    for i in 0..20 {
        let key = Arc::new(format!("utxo:{:010}", i).into_bytes());
        let value = Arc::new(format!("utxo_data_{}", i).into_bytes());
        set(key.clone(), value);
        
        // Access some UTXO keys
        if i % 4 == 0 {
            let _ = get(key);
        }
    }
    
    // Some cache misses
    for i in 0..5 {
        let missing_key = Arc::new(format!("missing:{:04}", i).into_bytes());
        let _ = get(missing_key);
    }
    
    println!("✅ Cache operations completed\n");
    
    // Get overall cache statistics
    let stats = lru_cache_stats();
    println!("📊 OVERALL CACHE STATISTICS");
    println!("├── Total Hits: {}", stats.hits);
    println!("├── Total Misses: {}", stats.misses);
    println!("├── Hit Rate: {:.1}%", 
        if stats.hits + stats.misses > 0 {
            (stats.hits as f64 / (stats.hits + stats.misses) as f64) * 100.0
        } else { 0.0 });
    println!("├── Current Items: {}", stats.items);
    println!("├── Memory Usage: {} bytes", stats.memory_usage);
    println!("└── Evictions: {}\n", stats.evictions);
    
    // Get detailed debug statistics
    let debug_stats = get_lru_debug_stats();
    println!("🔍 DEBUG STATISTICS");
    println!("├── Total Qualifying Prefixes: {}", debug_stats.total_prefixes);
    println!("├── Prefix Length Range: {}-{} bytes", 
        debug_stats.min_prefix_length, debug_stats.max_prefix_length);
    println!("└── Top Prefixes by Hits:");
    
    for (i, prefix_stat) in debug_stats.prefix_stats.iter().take(5).enumerate() {
        println!("    {}. {} (hits: {}, keys: {}, hit rate: {:.1}%)",
            i + 1,
            &prefix_stat.prefix[..8.min(prefix_stat.prefix.len())],
            prefix_stat.hits,
            prefix_stat.unique_keys,
            prefix_stat.hit_percentage
        );
    }
    println!();
    
    // Generate and display the full debug report
    println!("📋 FULL DEBUG REPORT");
    println!("{}", generate_lru_debug_report());
    
    // Flush changes to database
    flush();
    println!("✅ Changes flushed to database");
    
    // Clear debug statistics
    clear_prefix_hit_stats();
    println!("✅ Debug statistics cleared");
    
    // Disable debug mode
    disable_lru_debug_mode();
    println!("✅ LRU debug mode disabled: {}", !is_lru_debug_mode_enabled());
    
    println!("\n🎉 LRU Cache Debugging Example completed successfully!");
}
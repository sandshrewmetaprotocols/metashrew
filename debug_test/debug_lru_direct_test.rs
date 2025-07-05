//! Direct test of lru_mem::LruCache to understand the eviction behavior
//! 
//! This test bypasses the Metashrew wrapper and tests the lru_mem library directly
//! to determine if the issue is with the library itself or our usage of it.

use lru_mem::{HeapSize, LruCache};
use std::sync::Arc;

/// Simple wrapper for testing
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TestKey(String);

#[derive(Debug, Clone)]
struct TestValue(Vec<u8>);

impl HeapSize for TestKey {
    fn heap_size(&self) -> usize {
        std::mem::size_of::<String>() + self.0.len()
    }
}

impl HeapSize for TestValue {
    fn heap_size(&self) -> usize {
        std::mem::size_of::<Vec<u8>>() + self.0.len()
    }
}

fn main() {
    println!("🔍 DIRECT LRU_MEM::LRUCACHE TEST");
    println!("═══════════════════════════════════════════════════════════════");
    
    // Create a cache with 1GB limit (same as Metashrew)
    let cache_limit = 1024 * 1024 * 1024; // 1GB
    let mut cache: LruCache<TestKey, TestValue> = LruCache::new(cache_limit);
    
    println!("✅ Created LruCache with limit: {} bytes ({} MB)", cache_limit, cache_limit / (1024 * 1024));
    println!("📊 Initial state: len={}, current_size={}, max_size={}", 
             cache.len(), cache.current_size(), cache.max_size());
    
    // Add 1MB entries one by one and monitor behavior
    let entry_size = 1024 * 1024; // 1MB per entry
    let mut entries_added = 0;
    
    println!("\n🔄 Adding 1MB entries and monitoring eviction behavior...");
    println!("Entry | Items | Memory (MB) | Max Size (MB) | Evicted?");
    println!("------|-------|-------------|---------------|----------");
    
    for i in 1..=2000 { // Try to add up to 2GB worth of data
        let key = TestKey(format!("key_{:04}", i));
        let value = TestValue(vec![i as u8; entry_size]);
        
        let old_len = cache.len();
        let old_size = cache.current_size();
        
        // Insert the entry
        cache.insert(key, value);
        entries_added += 1;
        
        let new_len = cache.len();
        let new_size = cache.current_size();
        let evicted = new_len < old_len + 1;
        
        // Print status every 10 entries or when eviction starts
        if i % 10 == 0 || evicted || i <= 30 {
            println!("{:5} | {:5} | {:11.1} | {:13.1} | {}", 
                     i, 
                     new_len, 
                     new_size as f64 / (1024.0 * 1024.0),
                     cache.max_size() as f64 / (1024.0 * 1024.0),
                     if evicted { "YES" } else { "NO" });
        }
        
        // Stop if we've been evicting for a while and reached steady state
        if evicted && i > 50 {
            let steady_state_checks = 10;
            let mut steady_len = new_len;
            let mut steady_size = new_size;
            let mut is_steady = true;
            
            // Check if we're in steady state (consistent size for next few insertions)
            for j in 1..=steady_state_checks {
                let test_key = TestKey(format!("steady_test_{}", j));
                let test_value = TestValue(vec![0; entry_size]);
                cache.insert(test_key, test_value);
                
                if (cache.len() as i32 - steady_len as i32).abs() > 2 || 
                   ((cache.current_size() as i64 - steady_size as i64).abs() as f64 / steady_size as f64) > 0.1 {
                    is_steady = false;
                    break;
                }
            }
            
            if is_steady {
                println!("\n🎯 STEADY STATE REACHED:");
                println!("   └── Stable at {} items, {:.1} MB", cache.len(), cache.current_size() as f64 / (1024.0 * 1024.0));
                println!("   └── This is {:.1}% of the {:.0} MB limit", 
                         (cache.current_size() as f64 / cache.max_size() as f64) * 100.0,
                         cache.max_size() as f64 / (1024.0 * 1024.0));
                break;
            }
        }
        
        // Safety check to prevent infinite loop
        if i >= 2000 {
            println!("\n⚠️  Reached safety limit of 2000 entries");
            break;
        }
    }
    
    println!("\n📋 FINAL RESULTS:");
    println!("├── Total entries attempted: {}", entries_added);
    println!("├── Final cache size: {} items", cache.len());
    println!("├── Final memory usage: {:.1} MB", cache.current_size() as f64 / (1024.0 * 1024.0));
    println!("├── Cache limit: {:.1} MB", cache.max_size() as f64 / (1024.0 * 1024.0));
    println!("├── Memory utilization: {:.1}%", (cache.current_size() as f64 / cache.max_size() as f64) * 100.0);
    println!("└── Average entry size: {:.1} KB", if cache.len() > 0 { cache.current_size() as f64 / cache.len() as f64 / 1024.0 } else { 0.0 });
    
    // Test if the issue is with HeapSize calculation
    println!("\n🔬 HEAP SIZE ANALYSIS:");
    let test_key = TestKey("test".to_string());
    let test_value = TestValue(vec![0; entry_size]);
    println!("├── TestKey heap_size: {} bytes", test_key.heap_size());
    println!("├── TestValue heap_size: {} bytes", test_value.heap_size());
    println!("├── Expected total per entry: {} bytes ({:.1} KB)", 
             test_key.heap_size() + test_value.heap_size(),
             (test_key.heap_size() + test_value.heap_size()) as f64 / 1024.0);
    println!("└── Actual entry size: 1MB = {} bytes", entry_size);
    
    if cache.len() > 0 {
        let actual_avg_size = cache.current_size() / cache.len();
        println!("    └── Actual average size per entry: {} bytes ({:.1} KB)", 
                 actual_avg_size, actual_avg_size as f64 / 1024.0);
    }
}
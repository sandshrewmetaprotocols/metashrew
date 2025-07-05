//! Simple test to verify LRU cache behavior with exact same types as Metashrew
//! 
//! This test uses the exact same CacheKey and CacheValue types as Metashrew
//! to see if there's an issue with our HeapSize implementation.

use std::sync::Arc;

// Copy the exact HeapSize implementations from Metashrew
use lru_mem::{HeapSize, LruCache};

/// Wrapper type for Arc<Vec<u8>> to implement MemSize
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheValue(pub Arc<Vec<u8>>);

impl From<Arc<Vec<u8>>> for CacheValue {
    fn from(arc: Arc<Vec<u8>>) -> Self {
        CacheValue(arc)
    }
}

impl From<CacheValue> for Arc<Vec<u8>> {
    fn from(val: CacheValue) -> Self {
        val.0
    }
}

impl HeapSize for CacheValue {
    fn heap_size(&self) -> usize {
        // Simple and accurate memory calculation:
        // Just the data size plus minimal overhead for Arc and Vec structures
        std::mem::size_of::<Arc<Vec<u8>>>() + std::mem::size_of::<Vec<u8>>() + self.0.len()
    }
}

/// Wrapper type for Arc<Vec<u8>> keys to implement MemSize
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(pub Arc<Vec<u8>>);

impl From<Arc<Vec<u8>>> for CacheKey {
    fn from(arc: Arc<Vec<u8>>) -> Self {
        CacheKey(arc)
    }
}

impl From<CacheKey> for Arc<Vec<u8>> {
    fn from(val: CacheKey) -> Self {
        val.0
    }
}

impl HeapSize for CacheKey {
    fn heap_size(&self) -> usize {
        // Simple and accurate memory calculation for keys
        std::mem::size_of::<Arc<Vec<u8>>>() + std::mem::size_of::<Vec<u8>>() + self.0.len()
    }
}

fn main() {
    println!("🔍 METASHREW-STYLE LRU CACHE TEST");
    println!("═══════════════════════════════════════════════════════════════");
    
    // Create a cache with 1GB limit (same as Metashrew)
    let cache_limit = 1024 * 1024 * 1024; // 1GB
    let mut cache: LruCache<CacheKey, CacheValue> = LruCache::new(cache_limit);
    
    println!("✅ Created LruCache with limit: {} bytes ({} MB)", cache_limit, cache_limit / (1024 * 1024));
    println!("📊 Initial state: len={}, current_size={}, max_size={}", 
             cache.len(), cache.current_size(), cache.max_size());
    
    // Test with exact same entry size as Metashrew test (1MB per entry)
    let entry_size = 1024 * 1024; // 1MB per entry
    let mut entries_added = 0;
    
    println!("\n🔄 Adding 1MB entries using Metashrew types...");
    println!("Entry | Items | Memory (MB) | Max Size (MB) | Evicted? | Key Size | Value Size");
    println!("------|-------|-------------|---------------|----------|----------|------------");
    
    for i in 1..=300 { // Try to add 300MB worth of data
        let key_data = format!("key_{:04}", i).into_bytes();
        let value_data = vec![i as u8; entry_size];
        
        let cache_key = CacheKey::from(Arc::new(key_data));
        let cache_value = CacheValue::from(Arc::new(value_data));
        
        // Calculate sizes using our HeapSize implementation
        let key_heap_size = cache_key.heap_size();
        let value_heap_size = cache_value.heap_size();
        
        let old_len = cache.len();
        let old_size = cache.current_size();
        
        // Insert the entry
        cache.insert(cache_key, cache_value);
        entries_added += 1;
        
        let new_len = cache.len();
        let new_size = cache.current_size();
        let evicted = new_len < old_len + 1;
        
        // Print status every 10 entries or when eviction starts
        if i % 10 == 0 || evicted || i <= 30 {
            println!("{:5} | {:5} | {:11.1} | {:13.1} | {:8} | {:8} | {:10}", 
                     i, 
                     new_len, 
                     new_size as f64 / (1024.0 * 1024.0),
                     cache.max_size() as f64 / (1024.0 * 1024.0),
                     if evicted { "YES" } else { "NO" },
                     key_heap_size,
                     value_heap_size);
        }
        
        // Stop if we've been evicting for a while and reached steady state
        if evicted && i > 50 {
            let steady_state_checks = 5;
            let mut is_steady = true;
            
            // Check if we're in steady state (consistent size for next few insertions)
            for j in 1..=steady_state_checks {
                let test_key_data = format!("steady_test_{}", j).into_bytes();
                let test_value_data = vec![0; entry_size];
                let test_key = CacheKey::from(Arc::new(test_key_data));
                let test_value = CacheValue::from(Arc::new(test_value_data));
                
                let before_len = cache.len();
                let before_size = cache.current_size();
                cache.insert(test_key, test_value);
                
                if (cache.len() as i32 - before_len as i32).abs() > 2 || 
                   ((cache.current_size() as i64 - before_size as i64).abs() as f64 / before_size as f64) > 0.1 {
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
        if i >= 300 {
            println!("\n⚠️  Reached safety limit of 300 entries");
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
    
    // Compare with our direct test results
    println!("\n🔬 COMPARISON WITH DIRECT TEST:");
    println!("├── Direct test reached: ~1023 items at 99.9% of 1GB");
    println!("├── Metashrew test reaches: {} items at {:.1}% of 1GB", 
             cache.len(), (cache.current_size() as f64 / cache.max_size() as f64) * 100.0);
    println!("└── Difference suggests issue with HeapSize calculation or cache usage");
}
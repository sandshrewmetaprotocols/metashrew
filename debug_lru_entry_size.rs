//! Test to compare our HeapSize implementation with lru_mem::entry_size
//! 
//! This test checks if there's a discrepancy between how we calculate memory
//! and how lru_mem internally calculates it.

use std::sync::Arc;
use lru_mem::{HeapSize, LruCache};

/// Wrapper type for Arc<Vec<u8>> to implement MemSize
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheValue(pub Arc<Vec<u8>>);

impl From<Arc<Vec<u8>>> for CacheValue {
    fn from(arc: Arc<Vec<u8>>) -> Self {
        CacheValue(arc)
    }
}

impl HeapSize for CacheValue {
    fn heap_size(&self) -> usize {
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

impl HeapSize for CacheKey {
    fn heap_size(&self) -> usize {
        std::mem::size_of::<Arc<Vec<u8>>>() + std::mem::size_of::<Vec<u8>>() + self.0.len()
    }
}

fn main() {
    println!("🔍 LRU CACHE ENTRY SIZE COMPARISON TEST");
    println!("═══════════════════════════════════════════════════════════════");
    
    // Create a cache with 1GB limit
    let cache_limit = 1024 * 1024 * 1024; // 1GB
    let cache: LruCache<CacheKey, CacheValue> = LruCache::new(cache_limit);
    
    println!("✅ Created LruCache with limit: {} bytes ({} MB)", cache_limit, cache_limit / (1024 * 1024));
    
    // Test with different entry sizes
    let test_sizes = [
        1024,           // 1KB
        10 * 1024,      // 10KB
        100 * 1024,     // 100KB
        1024 * 1024,    // 1MB (same as test)
        10 * 1024 * 1024, // 10MB
    ];
    
    println!("\n🔄 Comparing HeapSize vs lru_mem::entry_size...");
    println!("Entry Size | Our HeapSize | lru_mem entry_size | Difference | Ratio");
    println!("-----------|--------------|-------------------|------------|-------");
    
    for &size in &test_sizes {
        let key_data = format!("test_key_{}", size).into_bytes();
        let value_data = vec![0u8; size];
        
        let cache_key = CacheKey::from(Arc::new(key_data));
        let cache_value = CacheValue::from(Arc::new(value_data));
        
        // Calculate using our HeapSize implementation
        let our_key_size = cache_key.heap_size();
        let our_value_size = cache_value.heap_size();
        let our_total_size = our_key_size + our_value_size;
        
        // Calculate using lru_mem::entry_size
        let lru_mem_size = lru_mem::entry_size(&cache_key, &cache_value);
        
        let difference = if lru_mem_size > our_total_size {
            lru_mem_size as i64 - our_total_size as i64
        } else {
            our_total_size as i64 - lru_mem_size as i64
        };
        
        let ratio = lru_mem_size as f64 / our_total_size as f64;
        
        println!("{:10} | {:12} | {:17} | {:10} | {:6.2}x",
                 format!("{}KB", size / 1024),
                 our_total_size,
                 lru_mem_size,
                 difference,
                 ratio);
    }
    
    println!("\n📋 ANALYSIS:");
    
    // Test with the exact same entry as the failing test
    let test_key_data = b"key_0001".to_vec();
    let test_value_data = vec![1u8; 1024 * 1024]; // 1MB
    
    let test_cache_key = CacheKey::from(Arc::new(test_key_data));
    let test_cache_value = CacheValue::from(Arc::new(test_value_data));
    
    let our_test_size = test_cache_key.heap_size() + test_cache_value.heap_size();
    let lru_mem_test_size = lru_mem::entry_size(&test_cache_key, &test_cache_value);
    
    println!("├── Test entry (1MB value):");
    println!("│   ├── Our calculation: {} bytes", our_test_size);
    println!("│   ├── lru_mem calculation: {} bytes", lru_mem_test_size);
    println!("│   └── Difference: {} bytes", (lru_mem_test_size as i64 - our_test_size as i64));
    
    // Calculate how many entries would fit based on each calculation
    let our_max_entries = cache_limit / our_test_size;
    let lru_mem_max_entries = cache_limit / lru_mem_test_size;
    
    println!("├── Theoretical max entries (1GB cache):");
    println!("│   ├── Based on our calculation: {} entries", our_max_entries);
    println!("│   ├── Based on lru_mem calculation: {} entries", lru_mem_max_entries);
    println!("│   └── Observed in test: ~262 entries");
    
    // Check which calculation is closer to the observed behavior
    let our_diff = (our_max_entries as i32 - 262).abs();
    let lru_mem_diff = (lru_mem_max_entries as i32 - 262).abs();
    
    println!("└── Closest to observed behavior:");
    if our_diff < lru_mem_diff {
        println!("    └── Our calculation (difference: {} entries)", our_diff);
    } else {
        println!("    └── lru_mem calculation (difference: {} entries)", lru_mem_diff);
    }
    
    println!("\n🔬 CONCLUSION:");
    println!("If lru_mem uses a different memory calculation than our HeapSize,");
    println!("this could explain why the cache evicts earlier than expected.");
}
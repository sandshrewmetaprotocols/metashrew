//! Debug test to check the actual LRU cache memory limit being used
//! 
//! This test investigates why the cache stabilizes at 262 items/268MB instead of 1GB.
//! The issue appears to be in the memory limit calculation and statistics reporting.

use lru_mem::{LruCache, HeapSize};
use std::sync::Arc;

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
    println!("🔍 INVESTIGATING LRU CACHE MEMORY LIMIT ISSUE");
    println!("═══════════════════════════════════════════════════════════════");
    
    // Test different memory limits to find the actual behavior
    let test_limits = [
        268 * 1024 * 1024,  // 268MB (observed limit)
        300 * 1024 * 1024,  // 300MB
        500 * 1024 * 1024,  // 500MB
        1024 * 1024 * 1024, // 1GB
    ];
    
    for &limit in &test_limits {
        println!("\n📊 Testing with {}MB limit ({} bytes)", limit / (1024 * 1024), limit);
        test_cache_with_limit(limit);
    }
    
    // Test the exact scenario from the failing test
    println!("\n🎯 REPRODUCING EXACT TEST SCENARIO");
    println!("═══════════════════════════════════════════════════════════════");
    reproduce_exact_scenario();
}

fn test_cache_with_limit(limit: usize) {
    let mut cache: LruCache<CacheKey, CacheValue> = LruCache::new(limit);
    
    println!("  Cache max_size: {} bytes ({} MB)", cache.max_size(), cache.max_size() / (1024 * 1024));
    
    let mut items_added = 0;
    let mut last_reported_size = 0;
    let mut last_reported_items = 0;
    
    // Add 1MB entries until we see stabilization
    for i in 0..500 {
        let key_data = format!("key_{:06}", i).into_bytes();
        let value_data = vec![i as u8; 1024 * 1024]; // 1MB value
        
        let cache_key = CacheKey::from(Arc::new(key_data));
        let cache_value = CacheValue::from(Arc::new(value_data));
        
        let _ = cache.insert(cache_key, cache_value);
        items_added += 1;
        
        let current_size = cache.current_size();
        let current_items = cache.len();
        
        // Report every 10 items or when size/items change significantly
        if i % 10 == 0 || 
           (current_size as i64 - last_reported_size as i64).abs() > 10 * 1024 * 1024 ||
           (current_items as i64 - last_reported_items as i64).abs() > 5 {
            
            println!("    Item {}: {} items, {} bytes ({} MB)", 
                     i, current_items, current_size, current_size / (1024 * 1024));
            
            last_reported_size = current_size;
            last_reported_items = current_items;
        }
        
        // Check for stabilization (same item count for several iterations)
        if i > 50 && current_items == last_reported_items && i % 10 == 0 {
            println!("    ✅ Cache stabilized at {} items, {} bytes ({} MB)", 
                     current_items, current_size, current_size / (1024 * 1024));
            break;
        }
        
        // Safety check
        if i > 300 {
            println!("    ⚠️  Stopping at iteration {} to prevent infinite loop", i);
            break;
        }
    }
    
    println!("  Final: {} items, {} bytes ({} MB)", 
             cache.len(), cache.current_size(), cache.current_size() / (1024 * 1024));
}

fn reproduce_exact_scenario() {
    // This reproduces the exact scenario from the failing test
    let limit = 1024 * 1024 * 1024; // 1GB
    let mut cache: LruCache<CacheKey, CacheValue> = LruCache::new(limit);
    
    println!("Cache configured with: {} bytes ({} MB)", limit, limit / (1024 * 1024));
    println!("Cache reports max_size: {} bytes ({} MB)", cache.max_size(), cache.max_size() / (1024 * 1024));
    
    let mut hit_count = 0;
    let mut miss_count = 0;
    
    // Simulate the exact test pattern
    for i in 0..100 {
        let key_data = format!("test_key_{:06}", i).into_bytes();
        let value_data = vec![i as u8; 1024 * 1024]; // 1MB value
        
        let cache_key = CacheKey::from(Arc::new(key_data.clone()));
        let cache_value = CacheValue::from(Arc::new(value_data));
        
        // Check if key exists (should be miss initially)
        if cache.contains(&cache_key) {
            hit_count += 1;
        } else {
            miss_count += 1;
        }
        
        // Insert the value (this should NOT count as a hit)
        let _ = cache.insert(cache_key, cache_value);
        
        if i % 10 == 0 || i < 30 {
            println!("  Iteration {}: {} items, {} bytes ({} MB), hits: {}, misses: {}", 
                     i, cache.len(), cache.current_size(), cache.current_size() / (1024 * 1024),
                     hit_count, miss_count);
        }
        
        // Check for the 262-item stabilization
        if cache.len() == 262 {
            println!("  🎯 FOUND IT! Cache stabilized at exactly 262 items at iteration {}", i);
            println!("     Current size: {} bytes ({} MB)", cache.current_size(), cache.current_size() / (1024 * 1024));
            println!("     Max size: {} bytes ({} MB)", cache.max_size(), cache.max_size() / (1024 * 1024));
            
            // Calculate the actual memory per item
            let avg_memory_per_item = cache.current_size() / cache.len();
            println!("     Average memory per item: {} bytes", avg_memory_per_item);
            
            // Calculate what the limit would need to be for 262 items
            let implied_limit = 262 * avg_memory_per_item;
            println!("     Implied memory limit for 262 items: {} bytes ({} MB)", 
                     implied_limit, implied_limit / (1024 * 1024));
            
            break;
        }
    }
    
    println!("\n🔍 FINAL ANALYSIS:");
    println!("  - Configured limit: {} MB", limit / (1024 * 1024));
    println!("  - Actual max_size: {} MB", cache.max_size() / (1024 * 1024));
    println!("  - Final items: {}", cache.len());
    println!("  - Final memory: {} MB", cache.current_size() / (1024 * 1024));
    println!("  - Hits: {} (should be 0 for insert-only test)", hit_count);
    println!("  - Misses: {}", miss_count);
    
    if hit_count > 0 {
        println!("  ❌ BUG: Hit count should be 0 for insert-only operations!");
    }
    
    if cache.len() == 262 {
        println!("  ❌ BUG: Cache stabilized at 262 items instead of growing to 1GB!");
    }
}
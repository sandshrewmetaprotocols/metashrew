//! Test to see what happens when LRU cache insert would exceed memory limit
//! 
//! This test checks if lru_mem::LruCache returns errors when memory limit is exceeded

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
    println!("🔍 LRU CACHE INSERT ERROR TEST");
    println!("═══════════════════════════════════════════════════════════════");
    
    // Create a small cache (10MB limit) to trigger errors quickly
    let cache_limit = 10 * 1024 * 1024; // 10MB
    let mut cache: LruCache<CacheKey, CacheValue> = LruCache::new(cache_limit);
    
    println!("✅ Created LruCache with limit: {} bytes ({} MB)", cache_limit, cache_limit / (1024 * 1024));
    
    // Test with 2MB entries to quickly exceed the 10MB limit
    let entry_size = 2 * 1024 * 1024; // 2MB per entry
    
    println!("\n🔄 Adding 2MB entries to 10MB cache...");
    println!("Entry | Result | Items | Memory (MB) | Details");
    println!("------|--------|-------|-------------|--------");
    
    for i in 1..=10 {
        let key_data = format!("key_{:04}", i).into_bytes();
        let value_data = vec![i as u8; entry_size];
        
        let cache_key = CacheKey::from(Arc::new(key_data));
        let cache_value = CacheValue::from(Arc::new(value_data));
        
        let old_len = cache.len();
        let old_size = cache.current_size();
        
        // Insert the entry and check the result
        match cache.insert(cache_key, cache_value) {
            Ok(old_value) => {
                let new_len = cache.len();
                let new_size = cache.current_size();
                println!("{:5} | {:6} | {:5} | {:11.1} | {}",
                         i,
                         "OK",
                         new_len,
                         new_size as f64 / (1024.0 * 1024.0),
                         if old_value.is_some() { "Replaced existing" } else { "New entry" });
            }
            Err(e) => {
                println!("{:5} | {:6} | {:5} | {:11.1} | Error: {:?}",
                         i,
                         "ERROR",
                         cache.len(),
                         cache.current_size() as f64 / (1024.0 * 1024.0),
                         e);
                break;
            }
        }
    }
    
    println!("\n📋 FINAL STATE:");
    println!("├── Cache items: {}", cache.len());
    println!("├── Memory usage: {:.1} MB", cache.current_size() as f64 / (1024.0 * 1024.0));
    println!("├── Memory limit: {:.1} MB", cache.max_size() as f64 / (1024.0 * 1024.0));
    println!("└── Utilization: {:.1}%", (cache.current_size() as f64 / cache.max_size() as f64) * 100.0);
    
    println!("\n🔬 CONCLUSION:");
    println!("This test shows whether lru_mem returns errors when memory limit would be exceeded,");
    println!("or if it automatically evicts entries to make room for new ones.");
}
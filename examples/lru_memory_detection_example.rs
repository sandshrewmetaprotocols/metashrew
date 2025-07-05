//! Example demonstrating LRU cache memory detection and graceful allocation
//!
//! This example shows how the LRU cache system automatically detects available
//! memory and adjusts its allocation accordingly, preventing OOM crashes on
//! resource-constrained servers.

use metashrew_support::lru_cache::{
    detect_available_memory, get_actual_lru_cache_memory_limit, get_min_lru_cache_memory_limit,
    is_cache_below_recommended_minimum, initialize_lru_cache, set_cache_allocation_mode,
    CacheAllocationMode, set_lru_cache, get_lru_cache, get_cache_stats, ensure_preallocated_memory
};
use std::sync::Arc;

fn main() {
    println!("🔍 LRU Cache Memory Detection Example");
    println!("=====================================\n");

    // Step 1: Detect available memory
    println!("1. Detecting available memory...");
    let detected_memory = detect_available_memory();
    println!("   Detected memory: {} bytes ({} MB)", 
             detected_memory, detected_memory / (1024 * 1024));

    // Step 2: Get the actual memory limit that will be used
    println!("\n2. Getting actual LRU cache memory limit...");
    let actual_limit = get_actual_lru_cache_memory_limit();
    let min_limit = get_min_lru_cache_memory_limit();
    println!("   Actual limit: {} bytes ({} MB)",
             actual_limit, actual_limit / (1024 * 1024));
    println!("   Minimum recommended: {} bytes ({} MB)",
             min_limit, min_limit / (1024 * 1024));
    
    if is_cache_below_recommended_minimum() {
        println!("   ⚠️  WARNING: Cache size is below recommended minimum - performance may be degraded");
    } else {
        println!("   ✅ Cache size meets or exceeds recommended minimum");
    }

    // Step 3: Initialize the cache system
    println!("\n3. Initializing LRU cache system...");
    set_cache_allocation_mode(CacheAllocationMode::Indexer);
    
    // This will gracefully handle memory allocation based on detected limits
    ensure_preallocated_memory();
    initialize_lru_cache();
    println!("   ✅ Cache system initialized successfully");

    // Step 4: Test cache operations
    println!("\n4. Testing cache operations...");
    
    // Add some test data
    for i in 0..10 {
        let key = Arc::new(format!("test_key_{}", i).into_bytes());
        let value = Arc::new(format!("test_value_{}", i).into_bytes());
        set_lru_cache(key, value);
    }
    
    // Retrieve some data
    let test_key = Arc::new(b"test_key_5".to_vec());
    match get_lru_cache(&test_key) {
        Some(value) => {
            println!("   ✅ Retrieved value: {}", String::from_utf8_lossy(&value));
        }
        None => {
            println!("   ❌ Failed to retrieve test value");
        }
    }

    // Step 5: Check cache statistics
    println!("\n5. Cache statistics:");
    let stats = get_cache_stats();
    println!("   Items: {}", stats.items);
    println!("   Memory usage: {} bytes ({} MB)", 
             stats.memory_usage, stats.memory_usage / (1024 * 1024));
    println!("   Hits: {}", stats.hits);
    println!("   Misses: {}", stats.misses);
    println!("   Evictions: {}", stats.evictions);

    // Step 6: Show memory efficiency
    println!("\n6. Memory efficiency:");
    let efficiency = if actual_limit > 0 {
        (stats.memory_usage as f64 / actual_limit as f64) * 100.0
    } else {
        0.0
    };
    println!("   Memory utilization: {:.2}%", efficiency);
    
    if is_cache_below_recommended_minimum() {
        println!("   ⚠️  Cache is operating below recommended minimum ({}MB)", min_limit / (1024 * 1024));
        println!("   💡 Performance may be degraded due to frequent evictions");
        println!("   🎯 Consider increasing available memory if possible");
    } else if actual_limit < 1024 * 1024 * 1024 {
        println!("   🎯 Cache automatically adjusted for resource-constrained environment");
        println!("   💡 This prevents OOM crashes while maintaining functionality");
    } else {
        println!("   🚀 Full 1GB cache allocation available");
    }

    println!("\n✅ Example completed successfully!");
    println!("\nKey benefits of this approach:");
    println!("• Prevents capacity_overflow panics on resource-constrained servers");
    println!("• Automatically detects and adapts to available memory");
    println!("• Maintains consistent memory layout for WASM execution");
    println!("• Provides graceful degradation instead of crashes");
    println!("• Preserves all cache functionality regardless of memory constraints");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_detection_example() {
        // This test ensures the example code works correctly
        let detected = detect_available_memory();
        assert!(detected >= 32 * 1024 * 1024, "Should detect at least 32MB");
        
        let actual = get_actual_lru_cache_memory_limit();
        assert_eq!(detected, actual, "Detected and actual limits should match");
        
        // Initialize cache system
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        ensure_preallocated_memory();
        initialize_lru_cache();
        
        // Test basic operations
        let key = Arc::new(b"test".to_vec());
        let value = Arc::new(b"value".to_vec());
        set_lru_cache(key.clone(), value.clone());
        
        let retrieved = get_lru_cache(&key);
        assert_eq!(retrieved, Some(value));
        
        println!("✅ Memory detection example test passed");
    }
}
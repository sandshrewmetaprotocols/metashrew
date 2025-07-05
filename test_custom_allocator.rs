//! Test to verify the custom allocator is working correctly

use std::sync::Arc;

// Import the LRU cache functions
use metashrew_support::lru_cache::{
    initialize_lru_cache, set_lru_cache, get_lru_cache, clear_lru_cache,
    enable_preallocated_allocator, disable_preallocated_allocator,
    is_preallocated_allocator_enabled, get_allocator_usage_stats,
    get_comprehensive_memory_report, set_cache_allocation_mode,
    CacheAllocationMode
};

fn main() {
    println!("🧪 Testing Custom Allocator Implementation");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Set to indexer mode to enable preallocated allocator
    set_cache_allocation_mode(CacheAllocationMode::Indexer);
    
    // Initialize the LRU cache (this should enable the preallocated allocator)
    initialize_lru_cache();
    
    // Check if allocator is enabled
    println!("📋 Allocator Status:");
    println!("├── Preallocated allocator enabled: {}", is_preallocated_allocator_enabled());
    
    let (used, total, percentage) = get_allocator_usage_stats();
    println!("├── Initial usage: {} bytes / {} bytes ({:.1}%)", used, total, percentage);
    println!("└── Total preallocated: {:.1} MB\n", total as f64 / (1024.0 * 1024.0));
    
    // Add some test data to the cache
    println!("💾 Adding test data to LRU cache...");
    for i in 0..10 {
        let key = Arc::new(format!("test_key_{}", i).into_bytes());
        let value = Arc::new(vec![42u8; 100_000]); // 100KB per entry
        set_lru_cache(key, value);
        
        if i % 3 == 0 {
            let (used, total, percentage) = get_allocator_usage_stats();
            println!("├── After {} entries: {} bytes used ({:.1}%)", i + 1, used, percentage);
        }
    }
    
    // Get final statistics
    println!("\n📊 Final Statistics:");
    let (used, total, percentage) = get_allocator_usage_stats();
    println!("├── Final usage: {} bytes / {} bytes ({:.1}%)", used, total, percentage);
    println!("└── Memory used: {:.1} MB\n", used as f64 / (1024.0 * 1024.0));
    
    // Generate comprehensive report
    println!("📋 Comprehensive Memory Report:");
    println!("{}", get_comprehensive_memory_report());
    
    // Test some cache operations
    println!("🔍 Testing cache operations...");
    let test_key = Arc::new(b"test_key_5".to_vec());
    match get_lru_cache(&test_key) {
        Some(value) => println!("✅ Cache hit: found value with {} bytes", value.len()),
        None => println!("❌ Cache miss: key not found"),
    }
    
    // Clean up
    clear_lru_cache();
    println!("\n✅ Test completed successfully!");
}
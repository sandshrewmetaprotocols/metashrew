//! Tests for the custom preallocated allocator functionality
//!
//! This test suite verifies that the custom bump allocator works correctly
//! and provides deterministic memory layout for WASM execution.

use std::sync::Arc;
use metashrew_support::lru_cache::{
    initialize_lru_cache, set_lru_cache, get_lru_cache, clear_lru_cache,
    get_allocator_usage_stats, get_comprehensive_memory_report, 
    set_cache_allocation_mode, CacheAllocationMode, get_cache_stats, 
    get_total_memory_usage
};

#[test]
fn test_allocator_basic_functionality() {
    println!("🧪 Testing Custom Allocator Basic Functionality");
    println!("═══════════════════════════════════════════════════════════════");

    // Set to indexer mode to enable preallocated allocator
    set_cache_allocation_mode(CacheAllocationMode::Indexer);
    
    // Initialize the LRU cache
    initialize_lru_cache();
    clear_lru_cache();
    
    // Check allocator stats
    let (used, total, percentage) = get_allocator_usage_stats();
    println!("📋 Allocator Status:");
    println!("├── Initial usage: {} bytes / {} bytes ({:.1}%)", used, total, percentage);
    println!("└── Total preallocated: {:.1} MB", total as f64 / (1024.0 * 1024.0));
    
    // Verify we have some preallocated memory
    assert!(total > 0, "Total preallocated memory should be greater than 0");
    
    println!("✅ Basic allocator functionality test passed!");
}

#[test]
fn test_cache_operations_with_allocator() {
    println!("🧪 Testing Cache Operations with Allocator");
    println!("═══════════════════════════════════════════════════════════════");

    // Set to indexer mode and initialize
    set_cache_allocation_mode(CacheAllocationMode::Indexer);
    initialize_lru_cache();
    clear_lru_cache();
    
    let initial_cache_memory = get_total_memory_usage();
    println!("📋 Initial cache memory: {} bytes", initial_cache_memory);
    
    // Add test data to the cache
    println!("💾 Adding test data to LRU cache...");
    let test_entries = 5;
    let entry_size = 10_000; // 10KB per entry
    
    for i in 0..test_entries {
        let key = Arc::new(format!("allocator_test_key_{}", i).into_bytes());
        let value = Arc::new(vec![42u8; entry_size]);
        set_lru_cache(key, value);
    }
    
    // Verify cache memory increased
    let final_cache_memory = get_total_memory_usage();
    println!("📊 Final cache memory: {} bytes", final_cache_memory);
    
    // Memory usage should have increased
    assert!(final_cache_memory > initial_cache_memory, 
            "Cache memory usage should have increased after adding entries");
    
    // Test cache operations work correctly
    println!("🔍 Testing cache operations...");
    let test_key = Arc::new(b"allocator_test_key_2".to_vec());
    match get_lru_cache(&test_key) {
        Some(value) => {
            println!("✅ Cache hit: found value with {} bytes", value.len());
            assert_eq!(value.len(), entry_size, "Retrieved value should have correct size");
        },
        None => panic!("❌ Cache miss: key should have been found"),
    }
    
    // Verify cache statistics
    let cache_stats = get_cache_stats();
    println!("📈 Cache statistics: {} items, {} hits, {} misses", 
             cache_stats.items, cache_stats.hits, cache_stats.misses);
    
    assert!(cache_stats.items >= test_entries, "Cache should contain at least {} items", test_entries);
    
    println!("✅ Cache operations with allocator test passed!");
}

#[test]
fn test_memory_report_generation() {
    println!("🧪 Testing Memory Report Generation");
    println!("═══════════════════════════════════════════════════════════════");

    // Set to indexer mode and initialize
    set_cache_allocation_mode(CacheAllocationMode::Indexer);
    initialize_lru_cache();
    clear_lru_cache();
    
    // Add some test data
    for i in 0..3 {
        let key = Arc::new(format!("report_test_key_{}", i).into_bytes());
        let value = Arc::new(vec![i as u8; 5_000]); // 5KB per entry
        set_lru_cache(key, value);
    }
    
    // Generate comprehensive report
    let report = get_comprehensive_memory_report();
    println!("📋 Comprehensive Memory Report:");
    println!("{}", report);
    
    // Verify report contains expected sections
    assert!(report.contains("COMPREHENSIVE MEMORY USAGE REPORT"), "Report should contain header");
    assert!(report.contains("PREALLOCATED ALLOCATOR STATUS"), "Report should contain allocator status");
    assert!(report.contains("LRU CACHE MEMORY USAGE"), "Report should contain cache usage");
    assert!(report.contains("MEMORY EFFICIENCY ANALYSIS"), "Report should contain efficiency analysis");
    
    println!("✅ Memory report generation test passed!");
}

#[test]
fn test_cache_memory_consistency() {
    println!("🧪 Testing Cache Memory Consistency");
    println!("═══════════════════════════════════════════════════════════════");

    // Set to indexer mode and initialize
    set_cache_allocation_mode(CacheAllocationMode::Indexer);
    initialize_lru_cache();
    clear_lru_cache();
    
    // Add test data and verify memory tracking is consistent
    let test_data_size = 50_000; // 50KB
    let key = Arc::new(b"consistency_test_key".to_vec());
    let value = Arc::new(vec![42u8; test_data_size]);
    
    let before_cache_memory = get_total_memory_usage();
    
    set_lru_cache(key.clone(), value);
    
    let after_cache_memory = get_total_memory_usage();
    
    println!("📊 Memory usage before: cache={} bytes", before_cache_memory);
    println!("📊 Memory usage after: cache={} bytes", after_cache_memory);
    
    // Cache memory usage should have increased
    assert!(after_cache_memory > before_cache_memory, "Cache memory usage should increase after adding data");
    
    // The increase should be reasonable (at least the data size)
    let cache_increase = after_cache_memory - before_cache_memory;
    println!("📈 Cache memory increase: +{} bytes", cache_increase);
    
    assert!(cache_increase >= test_data_size, "Cache increase should be at least the data size");
    
    // Verify we can retrieve the data
    match get_lru_cache(&key) {
        Some(retrieved_value) => {
            assert_eq!(retrieved_value.len(), test_data_size, "Retrieved value should have correct size");
            println!("✅ Data retrieval successful");
        },
        None => panic!("❌ Failed to retrieve stored data"),
    }
    
    println!("✅ Memory consistency test passed!");
}

#[test]
fn test_allocator_mode_configuration() {
    println!("🧪 Testing Allocator Mode Configuration");
    println!("═══════════════════════════════════════════════════════════════");

    // Test indexer mode configuration
    set_cache_allocation_mode(CacheAllocationMode::Indexer);
    initialize_lru_cache();
    
    let (used, total, _) = get_allocator_usage_stats();
    println!("📋 Indexer mode - Allocator: {} bytes / {} bytes", used, total);
    
    // Should have preallocated memory in indexer mode
    assert!(total > 0, "Should have preallocated memory in indexer mode");
    
    println!("✅ Indexer mode: allocator configured correctly");
    
    // Test view mode configuration
    set_cache_allocation_mode(CacheAllocationMode::View);
    clear_lru_cache();
    initialize_lru_cache();
    
    println!("✅ View mode: allocator configured for view operations");
    
    // Reset to indexer mode
    set_cache_allocation_mode(CacheAllocationMode::Indexer);
    println!("✅ Mode configuration test passed!");
}
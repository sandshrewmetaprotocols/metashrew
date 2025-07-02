//! Host-side tests for LRU cache functionality in metashrew-support
//!
//! This module tests the host-side LRU cache system to ensure it behaves
//! as expected. These tests validate the core logic that the WASM guest
//! environment relies on for its three-tier caching.

use anyhow::Result;
use metashrew_support::lru_cache::{
    api_cache_get, api_cache_remove, api_cache_set, clear_lru_cache, get_cache_stats,
    get_lru_cache, get_total_memory_usage, initialize_lru_cache, is_lru_cache_initialized,
    set_lru_cache,
};
use std::sync::Arc;

#[tokio::test]
async fn test_lru_cache_initialization() -> Result<()> {
    // Clear any existing state
    clear_lru_cache();

    // Initialize should set up the LRU cache
    initialize_lru_cache();

    // LRU cache should be available after initialization
    assert!(is_lru_cache_initialized());

    // Initial stats should show empty cache
    let stats = get_cache_stats();
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.items, 0);
    Ok(())
}

#[tokio::test]
async fn test_lru_cache_persistence() -> Result<()> {
    clear_lru_cache();
    initialize_lru_cache();

    let key = Arc::new(b"test_key".to_vec());
    let value = Arc::new(b"test_value".to_vec());

    // Set a value
    set_lru_cache(key.clone(), value.clone());

    // Get should hit the cache
    let retrieved = get_lru_cache(&key);
    assert_eq!(retrieved, Some(value.clone()));

    // Verify cache stats show hits
    let stats = get_cache_stats();
    assert!(stats.hits > 0);
    Ok(())
}

#[tokio::test]
async fn test_api_cache_functionality() -> Result<()> {
    clear_lru_cache();
    initialize_lru_cache();

    let key = "api_test_key".to_string();
    let value = Arc::new(b"api_test_value".to_vec());

    // Initially should be empty
    assert_eq!(api_cache_get(&key), None);

    // Set a value
    api_cache_set(key.clone(), value.clone());

    // Should be able to retrieve it
    let retrieved = api_cache_get(&key);
    assert_eq!(retrieved, Some(value.clone()));

    // Remove the value
    let removed = api_cache_remove(&key);
    assert_eq!(removed, Some(value));

    // Should be empty again
    assert_eq!(api_cache_get(&key), None);
    Ok(())
}

#[tokio::test]
async fn test_memory_usage_tracking() -> Result<()> {
    clear_lru_cache();
    initialize_lru_cache();

    // Initial memory usage should be minimal
    let initial_usage = get_total_memory_usage();

    // Add some data
    let key1 = Arc::new(vec![1u8; 1000]); // 1KB key
    let value1 = Arc::new(vec![2u8; 10000]); // 10KB value
    set_lru_cache(key1, value1);

    // Memory usage should have increased
    let after_usage = get_total_memory_usage();
    assert!(after_usage > initial_usage);

    // Clear should reset memory usage
    clear_lru_cache();
    let final_usage = get_total_memory_usage();
    assert!(final_usage <= initial_usage);
    Ok(())
}

#[tokio::test]
async fn test_cache_stats_accuracy() -> Result<()> {
    clear_lru_cache();
    initialize_lru_cache();

    let key = Arc::new(b"stats_test_key_unique".to_vec());
    let value = Arc::new(b"stats_test_value".to_vec());

    // Get baseline stats
    let baseline_stats = get_cache_stats();

    // Cache miss should increment misses
    get_lru_cache(&key);
    let after_miss = get_cache_stats();
    assert_eq!(after_miss.misses, baseline_stats.misses + 1);

    // Set value and access should increment hits
    set_lru_cache(key.clone(), value);
    get_lru_cache(&key); // Should hit LRU cache

    let after_hit = get_cache_stats();
    assert_eq!(after_hit.hits, baseline_stats.hits + 1);
    assert!(after_hit.items > 0);
    Ok(())
}

#[tokio::test]
async fn test_clear_functionality() -> Result<()> {
    clear_lru_cache();
    initialize_lru_cache();

    let key = Arc::new(b"clear_test_key".to_vec());
    let value = Arc::new(b"clear_test_value".to_vec());
    let api_key = "clear_api_key".to_string();
    let api_value = Arc::new(b"clear_api_value".to_vec());

    // Populate both caches
    set_lru_cache(key.clone(), value.clone());
    api_cache_set(api_key.clone(), api_value.clone());

    // Verify data is there
    assert_eq!(get_lru_cache(&key), Some(value));
    assert_eq!(api_cache_get(&api_key), Some(api_value));

    // Clear should remove everything
    clear_lru_cache();

    // API cache should be empty
    assert_eq!(api_cache_get(&api_key), None);

    // Stats should be reset
    let stats = get_cache_stats();
    assert_eq!(stats.items, 0);

    // Memory usage should be minimal
    let memory_usage = get_total_memory_usage();
    assert!(memory_usage < 1000); // Should be very small
    Ok(())
}

//! Tests for LRU cache integration in metashrew-core
//!
//! This module tests the integration between the existing cache system (CACHE/TO_FLUSH)
//! and the new LRU cache system. It verifies that the three-tier caching works correctly:
//!
//! 1. CACHE (immediate cache, cleared on flush)
//! 2. LRU_CACHE (persistent cache, survives flush)
//! 3. Host calls (__get/__get_len fallback)

use crate::{
    initialize, get, set, flush, clear, cache_set, cache_get, cache_remove,
    lru_cache_stats, lru_cache_memory_usage, is_lru_cache_available
};
use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache_initialization() {
        // Clear any existing state
        clear();
        
        // Initialize should set up both immediate cache and LRU cache
        initialize();
        
        // LRU cache should be available after initialization
        assert!(is_lru_cache_available());
        
        // Initial stats should show empty cache
        let stats = lru_cache_stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.items, 0);
    }

    #[test]
    fn test_three_tier_cache_lookup() {
        clear();
        initialize();
        
        let key = Arc::new(b"test_key".to_vec());
        let value = Arc::new(b"test_value".to_vec());
        
        // Set a value - should populate both caches
        set(key.clone(), value.clone());
        
        // First get should hit immediate cache
        let retrieved1 = get(key.clone());
        assert_eq!(retrieved1, value);
        
        // Flush should clear immediate cache but preserve LRU cache
        flush();
        
        // Second get should hit LRU cache and repopulate immediate cache
        let retrieved2 = get(key.clone());
        assert_eq!(retrieved2, value);
        
        // Verify cache stats show hits
        let stats = lru_cache_stats();
        assert!(stats.hits > 0);
    }

    #[test]
    fn test_cache_persistence_across_flushes() {
        clear();
        initialize();
        
        let key1 = Arc::new(b"persistent_key1".to_vec());
        let value1 = Arc::new(b"persistent_value1".to_vec());
        let key2 = Arc::new(b"persistent_key2".to_vec());
        let value2 = Arc::new(b"persistent_value2".to_vec());
        
        // Set multiple values
        set(key1.clone(), value1.clone());
        set(key2.clone(), value2.clone());
        
        // Flush to clear immediate cache
        flush();
        
        // Values should still be accessible via LRU cache
        let retrieved1 = get(key1.clone());
        let retrieved2 = get(key2.clone());
        assert_eq!(retrieved1, value1);
        assert_eq!(retrieved2, value2);
        
        // Flush again
        flush();
        
        // Values should still be accessible
        let retrieved1_again = get(key1.clone());
        let retrieved2_again = get(key2.clone());
        assert_eq!(retrieved1_again, value1);
        assert_eq!(retrieved2_again, value2);
    }

    #[test]
    fn test_api_cache_functionality() {
        clear();
        initialize();
        
        let key = "api_test_key".to_string();
        let value = Arc::new(b"api_test_value".to_vec());
        
        // Initially should be empty
        assert_eq!(cache_get(&key), None);
        
        // Set a value
        cache_set(key.clone(), value.clone());
        
        // Should be able to retrieve it
        let retrieved = cache_get(&key);
        assert_eq!(retrieved, Some(value.clone()));
        
        // Remove the value
        let removed = cache_remove(&key);
        assert_eq!(removed, Some(value));
        
        // Should be empty again
        assert_eq!(cache_get(&key), None);
    }

    #[test]
    fn test_api_cache_persistence() {
        clear();
        initialize();
        
        let key = "persistent_api_key".to_string();
        let value = Arc::new(b"persistent_api_value".to_vec());
        
        // Set value in API cache
        cache_set(key.clone(), value.clone());
        
        // Flush should not affect API cache
        flush();
        
        // Value should still be there
        let retrieved = cache_get(&key);
        assert_eq!(retrieved, Some(value));
    }

    #[test]
    fn test_memory_usage_tracking() {
        clear();
        initialize();
        
        // Initial memory usage should be minimal
        let initial_usage = lru_cache_memory_usage();
        
        // Add some data to both caches
        let key1 = Arc::new(vec![1u8; 1000]); // 1KB key
        let value1 = Arc::new(vec![2u8; 10000]); // 10KB value
        set(key1, value1);
        
        let api_key = "large_api_data".to_string();
        let api_value = Arc::new(vec![3u8; 5000]); // 5KB value
        cache_set(api_key, api_value);
        
        // Memory usage should have increased
        let after_usage = lru_cache_memory_usage();
        assert!(after_usage > initial_usage);
        
        // Clear should reset memory usage
        clear();
        let final_usage = lru_cache_memory_usage();
        assert!(final_usage <= initial_usage);
    }

    #[test]
    fn test_cache_stats_accuracy() {
        clear();
        initialize();
        
        let key = Arc::new(b"stats_test_key".to_vec());
        let value = Arc::new(b"stats_test_value".to_vec());
        
        // Initial stats
        let initial_stats = lru_cache_stats();
        let initial_hits = initial_stats.hits;
        let initial_misses = initial_stats.misses;
        
        // Cache miss should increment misses
        get(key.clone());
        let after_miss = lru_cache_stats();
        assert_eq!(after_miss.misses, initial_misses + 1);
        
        // Set value and access should increment hits
        set(key.clone(), value);
        flush(); // Clear immediate cache
        get(key.clone()); // Should hit LRU cache
        
        let after_hit = lru_cache_stats();
        assert_eq!(after_hit.hits, initial_hits + 1);
        assert!(after_hit.items > 0);
    }

    #[test]
    fn test_clear_functionality() {
        clear();
        initialize();
        
        let key = Arc::new(b"clear_test_key".to_vec());
        let value = Arc::new(b"clear_test_value".to_vec());
        let api_key = "clear_api_key".to_string();
        let api_value = Arc::new(b"clear_api_value".to_vec());
        
        // Populate both caches
        set(key.clone(), value.clone());
        cache_set(api_key.clone(), api_value.clone());
        
        // Verify data is there
        assert_eq!(get(key.clone()), value);
        assert_eq!(cache_get(&api_key), Some(api_value));
        
        // Clear should remove everything
        clear();
        
        // Reinitialize after clear
        initialize();
        
        // API cache should be empty
        assert_eq!(cache_get(&api_key), None);
        
        // Stats should be reset
        let stats = lru_cache_stats();
        assert_eq!(stats.items, 0);
        
        // Memory usage should be minimal
        let memory_usage = lru_cache_memory_usage();
        assert!(memory_usage < 1000); // Should be very small
    }

    #[test]
    fn test_large_data_handling() {
        clear();
        initialize();
        
        // Test with larger data to ensure memory accounting works
        let large_key = Arc::new(vec![1u8; 10000]); // 10KB key
        let large_value = Arc::new(vec![2u8; 100000]); // 100KB value
        
        set(large_key.clone(), large_value.clone());
        
        // Should be able to retrieve large data
        let retrieved = get(large_key.clone());
        assert_eq!(retrieved, large_value);
        
        // Memory usage should reflect the large data
        let memory_usage = lru_cache_memory_usage();
        assert!(memory_usage > 100000); // Should be at least 100KB
        
        // Flush and retrieve again
        flush();
        let retrieved_after_flush = get(large_key);
        assert_eq!(retrieved_after_flush, large_value);
    }

    #[test]
    fn test_concurrent_cache_operations() {
        clear();
        initialize();
        
        // Test multiple operations in sequence to ensure consistency
        let keys_values: Vec<(Arc<Vec<u8>>, Arc<Vec<u8>>)> = (0..10)
            .map(|i| {
                let key = Arc::new(format!("key_{}", i).into_bytes());
                let value = Arc::new(format!("value_{}", i).into_bytes());
                (key, value)
            })
            .collect();
        
        // Set all values
        for (key, value) in &keys_values {
            set(key.clone(), value.clone());
        }
        
        // Flush to move to LRU cache
        flush();
        
        // Retrieve all values
        for (key, expected_value) in &keys_values {
            let retrieved = get(key.clone());
            assert_eq!(retrieved, *expected_value);
        }
        
        // Verify cache stats
        let stats = lru_cache_stats();
        assert_eq!(stats.items, keys_values.len());
        assert!(stats.hits >= keys_values.len() as u64);
    }
}
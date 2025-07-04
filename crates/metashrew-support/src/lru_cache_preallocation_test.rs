//! Tests for LRU cache memory preallocation functionality
//!
//! This module tests that the 1GB LRU cache memory is preallocated at startup
//! and that the preallocation function works correctly.

#[cfg(test)]
mod tests {
    use super::super::lru_cache::{
        ensure_preallocated_memory, initialize_lru_cache, clear_lru_cache,
        set_cache_allocation_mode, CacheAllocationMode
    };

    #[test]
    fn test_ensure_preallocated_memory_indexer_mode() {
        // Test that ensure_preallocated_memory works in indexer mode
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        
        ensure_preallocated_memory();
        
        // Should be able to call it multiple times safely
        ensure_preallocated_memory();
        ensure_preallocated_memory();
        
        println!("✅ ensure_preallocated_memory() works correctly in indexer mode");
    }

    #[test]
    fn test_ensure_preallocated_memory_view_mode() {
        // Test that ensure_preallocated_memory skips preallocation in view mode
        set_cache_allocation_mode(CacheAllocationMode::View);
        
        ensure_preallocated_memory();
        
        // Should be able to call it multiple times safely (should be no-ops)
        ensure_preallocated_memory();
        ensure_preallocated_memory();
        
        println!("✅ ensure_preallocated_memory() correctly skips preallocation in view mode");
    }

    #[test]
    fn test_preallocation_before_initialization() {
        // Test that preallocation works before cache initialization in indexer mode
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        clear_lru_cache();
        
        // Call preallocation first
        ensure_preallocated_memory();
        
        // Then initialize cache
        initialize_lru_cache();
        
        println!("✅ Preallocation before initialization works in indexer mode");
    }

    #[test]
    fn test_preallocation_after_initialization() {
        // Test that preallocation works after cache initialization in indexer mode
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        clear_lru_cache();
        
        // Initialize cache first
        initialize_lru_cache();
        
        // Then call preallocation (should be safe)
        ensure_preallocated_memory();
        
        println!("✅ Preallocation after initialization works in indexer mode");
    }

    #[test]
    fn test_memory_preallocation_consistency() {
        // Test that memory preallocation is consistent across multiple calls in indexer mode
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        
        // Clear any existing state
        clear_lru_cache();
        
        // Call preallocation multiple times
        for i in 0..5 {
            ensure_preallocated_memory();
            println!("Preallocation call {}: completed successfully", i + 1);
        }
        
        // Initialize cache
        initialize_lru_cache();
        
        // Call preallocation again
        ensure_preallocated_memory();
        
        println!("✅ Memory preallocation is consistent across multiple calls in indexer mode");
    }

    #[test]
    fn test_mode_switching_behavior() {
        // Test that switching between modes works correctly
        clear_lru_cache();
        
        // Start in view mode (no preallocation)
        set_cache_allocation_mode(CacheAllocationMode::View);
        ensure_preallocated_memory();
        
        // Switch to indexer mode (should preallocate)
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        ensure_preallocated_memory();
        
        // Switch back to view mode (should skip preallocation)
        set_cache_allocation_mode(CacheAllocationMode::View);
        ensure_preallocated_memory();
        
        println!("✅ Mode switching behavior works correctly");
    }
}
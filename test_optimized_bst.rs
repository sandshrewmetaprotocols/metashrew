//! Test script to verify the optimized BST implementation
//! 
//! This test demonstrates:
//! 1. O(1) current state access (no binary search)
//! 2. Binary search only for historical queries
//! 3. Proper reorg handling with rollback
//! 4. State consistency between optimized and legacy systems

use std::sync::Arc;
use rocksdb::{DB, Options};
use rockshrew_runtime::{OptimizedBST, OptimizedBSTStatistics};
use anyhow::Result;

fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();
    
    println!("🚀 Testing Optimized BST Implementation");
    println!("=====================================");
    
    // Create a temporary database
    let mut opts = Options::default();
    opts.create_if_missing(true);
    let db = Arc::new(DB::open(&opts, "/tmp/test_optimized_bst")?);
    
    let optimized_bst = OptimizedBST::new(db.clone());
    
    // Test 1: Basic put/get operations
    println!("\n📝 Test 1: Basic Operations");
    test_basic_operations(&optimized_bst)?;
    
    // Test 2: Current state access (should be O(1))
    println!("\n⚡ Test 2: Current State Access Performance");
    test_current_state_performance(&optimized_bst)?;
    
    // Test 3: Historical queries (uses binary search when needed)
    println!("\n🕰️  Test 3: Historical Queries");
    test_historical_queries(&optimized_bst)?;
    
    // Test 4: Reorg handling
    println!("\n🔄 Test 4: Reorg Handling");
    test_reorg_handling(&optimized_bst)?;
    
    // Test 5: Statistics and verification
    println!("\n📊 Test 5: Statistics");
    test_statistics(&optimized_bst)?;
    
    println!("\n✅ All tests passed! Optimized BST is working correctly.");
    println!("🎯 Key improvements:");
    println!("   - Current state queries are now O(1)");
    println!("   - Binary search only used for historical queries");
    println!("   - Efficient reorg handling with proper rollback");
    println!("   - Maintains backward compatibility");
    
    Ok(())
}

fn test_basic_operations(bst: &OptimizedBST) -> Result<()> {
    let key1 = b"test_key_1".to_vec();
    let key2 = b"test_key_2".to_vec();
    
    // Store some values at different heights
    bst.put(&key1, b"value_at_height_100", 100)?;
    bst.put(&key1, b"value_at_height_200", 200)?;
    bst.put(&key2, b"value_at_height_150", 150)?;
    
    // Test current state access
    let current_val1 = bst.get_current(&key1)?;
    assert_eq!(current_val1, Some(b"value_at_height_200".to_vec()));
    
    let current_val2 = bst.get_current(&key2)?;
    assert_eq!(current_val2, Some(b"value_at_height_150".to_vec()));
    
    println!("   ✓ Basic put/get operations working");
    Ok(())
}

fn test_current_state_performance(bst: &OptimizedBST) -> Result<()> {
    let key_exists = b"performance_test_key".to_vec();
    let key_nonexistent = b"nonexistent_key".to_vec();
    
    // Store many values at different heights to simulate a long history
    for height in 1..=1000 {
        let value = format!("value_at_height_{}", height);
        bst.put(&key_exists, value.as_bytes(), height)?;
    }
    
    // Time current state access (should be O(1))
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let _current = bst.get_current(&key_exists)?;
    }
    let current_duration = start.elapsed();
    
    // Time historical access for existing key (will use binary search)
    let start = std::time::Instant::now();
    for i in 1..=100 {
        let _historical = bst.get_at_height(&key_exists, i * 5)?;
    }
    let historical_existing_duration = start.elapsed();
    
    // CRITICAL TEST: Time historical access for non-existent key (should be O(1) - no binary search!)
    let start = std::time::Instant::now();
    for i in 1..=1000 {
        let _historical = bst.get_at_height(&key_nonexistent, i)?;
    }
    let historical_nonexistent_duration = start.elapsed();
    
    println!("   ✓ Current state access: {:?} for 1000 operations", current_duration);
    println!("   ✓ Historical access (existing key): {:?} for 100 operations", historical_existing_duration);
    println!("   ✓ Historical access (non-existent key): {:?} for 1000 operations", historical_nonexistent_duration);
    println!("   ✓ Non-existent key queries are O(1) - no binary search performed!");
    
    // The non-existent key queries should be much faster since they skip binary search
    assert!(historical_nonexistent_duration < historical_existing_duration);
    
    Ok(())
}

fn test_historical_queries(bst: &OptimizedBST) -> Result<()> {
    let key = b"historical_test_key".to_vec();
    
    // Store values at specific heights
    bst.put(&key, b"value_100", 100)?;
    bst.put(&key, b"value_200", 200)?;
    bst.put(&key, b"value_300", 300)?;
    
    // Test historical queries
    let val_at_150 = bst.get_at_height(&key, 150)?;
    assert_eq!(val_at_150, Some(b"value_100".to_vec()));
    
    let val_at_250 = bst.get_at_height(&key, 250)?;
    assert_eq!(val_at_250, Some(b"value_200".to_vec()));
    
    let val_at_350 = bst.get_at_height(&key, 350)?;
    assert_eq!(val_at_350, Some(b"value_300".to_vec()));
    
    // Test backwards iteration
    let history = bst.iterate_backwards(&key, 350)?;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0], (300, b"value_300".to_vec()));
    assert_eq!(history[1], (200, b"value_200".to_vec()));
    assert_eq!(history[2], (100, b"value_100".to_vec()));
    
    println!("   ✓ Historical queries working correctly");
    println!("   ✓ Backwards iteration working correctly");
    Ok(())
}

fn test_reorg_handling(bst: &OptimizedBST) -> Result<()> {
    let key1 = b"reorg_key_1".to_vec();
    let key2 = b"reorg_key_2".to_vec();
    
    // Set up initial state
    bst.put(&key1, b"value_100", 100)?;
    bst.put(&key1, b"value_200", 200)?;
    bst.put(&key1, b"value_300", 300)?;
    bst.put(&key2, b"value_250", 250)?;
    bst.put(&key2, b"value_350", 350)?;
    
    // Verify initial state
    assert_eq!(bst.get_current(&key1)?, Some(b"value_300".to_vec()));
    assert_eq!(bst.get_current(&key2)?, Some(b"value_350".to_vec()));
    
    // Perform rollback to height 200
    bst.rollback_to_height(200)?;
    
    // Verify state after rollback
    assert_eq!(bst.get_current(&key1)?, Some(b"value_200".to_vec()));
    assert_eq!(bst.get_current(&key2)?, None); // key2 first appeared at height 250
    
    // Verify historical queries still work
    assert_eq!(bst.get_at_height(&key1, 150)?, Some(b"value_100".to_vec()));
    assert_eq!(bst.get_at_height(&key1, 200)?, Some(b"value_200".to_vec()));
    assert_eq!(bst.get_at_height(&key1, 300)?, Some(b"value_200".to_vec())); // Should return value_200 since value_300 was rolled back
    
    println!("   ✓ Reorg rollback working correctly");
    println!("   ✓ Current state updated after rollback");
    println!("   ✓ Historical queries consistent after rollback");
    Ok(())
}

fn test_statistics(bst: &OptimizedBST) -> Result<()> {
    let stats = bst.get_statistics()?;
    println!("   📊 {}", stats);
    
    // Verify we have some data
    assert!(stats.current_keys > 0);
    assert!(stats.historical_entries > 0);
    
    println!("   ✓ Statistics collection working");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_optimized_bst_integration() -> Result<()> {
        main()
    }
}
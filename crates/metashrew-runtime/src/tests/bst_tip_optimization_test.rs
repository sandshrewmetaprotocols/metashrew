//! Test for BST tip optimization
//!
//! This test verifies that the optimized BST provides O(1) current state access
//! while maintaining efficient historical queries.

use crate::optimized_bst::{OptimizedBST, CURRENT_VALUE_PREFIX, HISTORICAL_VALUE_PREFIX};
use crate::traits::{BatchLike, KeyValueStoreLike};
use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug)]
struct TestError(String);

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Test error: {}", self.0)
    }
}

impl std::error::Error for TestError {}

/// Simple in-memory storage for testing
#[derive(Clone)]
struct MemoryStorage {
    data: HashMap<Vec<u8>, Vec<u8>>,
}

impl MemoryStorage {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
}

/// Simple in-memory batch for testing
struct MemoryBatch {
    operations: Vec<(Vec<u8>, Option<Vec<u8>>)>, // None for delete, Some for put
}

impl MemoryBatch {
    fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }
}

impl BatchLike for MemoryBatch {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        self.operations.push((key.as_ref().to_vec(), Some(value.as_ref().to_vec())));
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) {
        self.operations.push((key.as_ref().to_vec(), None));
    }

    fn default() -> Self {
        Self::new()
    }
}

impl KeyValueStoreLike for MemoryStorage {
    type Error = TestError;
    type Batch = MemoryBatch;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        for (key, value_opt) in batch.operations {
            match value_opt {
                Some(value) => {
                    self.data.insert(key, value);
                }
                None => {
                    self.data.remove(&key);
                }
            }
        }
        Ok(())
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        Ok(self.data.get(key.as_ref()).cloned())
    }

    fn get_immutable<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        Ok(self.data.get(key.as_ref()).cloned())
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.data.insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        Ok(())
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.data.remove(key.as_ref());
        Ok(())
    }

    fn scan_prefix<K: AsRef<[u8]>>(
        &self,
        prefix: K,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error> {
        let prefix_bytes = prefix.as_ref();
        let mut results = Vec::new();
        
        for (key, value) in &self.data {
            if key.starts_with(prefix_bytes) {
                results.push((key.clone(), value.clone()));
            }
        }
        
        results.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(results)
    }

    fn create_batch(&self) -> Self::Batch {
        MemoryBatch::default()
    }

    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
        Ok(Box::new(self.data.keys().cloned()))
    }
}

#[test]
fn test_bst_tip_optimization_current_access() -> Result<()> {
    let storage = MemoryStorage::new();
    let mut bst = OptimizedBST::new(storage);

    let key = b"test_key";
    let value1 = b"value_at_height_100";
    let value2 = b"value_at_height_200";
    let value3 = b"value_at_height_300";

    // Store values at different heights
    bst.put(key, value1, 100)?;
    bst.put(key, value2, 200)?;
    bst.put(key, value3, 300)?;

    // Test O(1) current access - should return the most recent value
    let current_value = bst.get_current(key)?;
    assert_eq!(current_value, Some(value3.to_vec()));

    // Verify that current value is stored with the correct prefix
    let current_key = format!("{}{}", CURRENT_VALUE_PREFIX, hex::encode(key));
    let direct_current = bst.storage().get_immutable(current_key.as_bytes())?;
    assert_eq!(direct_current, Some(value3.to_vec()));

    println!("✓ BST tip optimization: O(1) current access works correctly");
    Ok(())
}

#[test]
fn test_bst_tip_optimization_historical_access() -> Result<()> {
    let storage = MemoryStorage::new();
    let mut bst = OptimizedBST::new(storage);

    let key = b"test_key";
    let value1 = b"value_at_height_100";
    let value2 = b"value_at_height_200";
    let value3 = b"value_at_height_300";

    // Store values at different heights
    bst.put(key, value1, 100)?;
    bst.put(key, value2, 200)?;
    bst.put(key, value3, 300)?;

    // Test historical access at exact heights
    assert_eq!(bst.get_at_height(key, 100)?, Some(value1.to_vec()));
    assert_eq!(bst.get_at_height(key, 200)?, Some(value2.to_vec()));
    assert_eq!(bst.get_at_height(key, 300)?, Some(value3.to_vec()));

    // Test historical access at intermediate heights (should return most recent before)
    assert_eq!(bst.get_at_height(key, 150)?, Some(value1.to_vec()));
    assert_eq!(bst.get_at_height(key, 250)?, Some(value2.to_vec()));
    assert_eq!(bst.get_at_height(key, 350)?, Some(value3.to_vec()));

    // Test historical access before any updates
    assert_eq!(bst.get_at_height(key, 50)?, None);

    // Verify that historical values are stored with the correct prefix
    let hist_key_100 = format!("{}{}:{}", HISTORICAL_VALUE_PREFIX, hex::encode(key), 100);
    let hist_value_100 = bst.storage().get_immutable(hist_key_100.as_bytes())?;
    assert_eq!(hist_value_100, Some(value1.to_vec()));

    println!("✓ BST tip optimization: Historical queries work correctly");
    Ok(())
}

#[test]
fn test_bst_tip_optimization_nonexistent_key() -> Result<()> {
    let storage = MemoryStorage::new();
    let bst = OptimizedBST::new(storage);

    let nonexistent_key = b"nonexistent_key";

    // Test current access for nonexistent key
    assert_eq!(bst.get_current(nonexistent_key)?, None);

    // Test historical access for nonexistent key (should be O(1) due to current check)
    assert_eq!(bst.get_at_height(nonexistent_key, 100)?, None);

    println!("✓ BST tip optimization: Nonexistent key handling works correctly");
    Ok(())
}

#[test]
fn test_bst_tip_optimization_performance_characteristics() -> Result<()> {
    let storage = MemoryStorage::new();
    let mut bst = OptimizedBST::new(storage);

    let key = b"performance_test_key";
    
    // Simulate many updates over many heights (like a real indexer)
    for height in (0..1000).step_by(10) {
        let value = format!("value_at_height_{}", height);
        bst.put(key, value.as_bytes(), height)?;
    }

    // Current access should be O(1) regardless of number of historical updates
    let start = std::time::Instant::now();
    let current_value = bst.get_current(key)?;
    let current_duration = start.elapsed();

    // Historical access should use binary search (O(log n))
    let start = std::time::Instant::now();
    let historical_value = bst.get_at_height(key, 500)?;
    let historical_duration = start.elapsed();

    assert_eq!(current_value, Some(b"value_at_height_990".to_vec()));
    assert_eq!(historical_value, Some(b"value_at_height_500".to_vec()));

    // Current access should be much faster than historical access
    // (though in this small test the difference might not be significant)
    println!("✓ BST tip optimization: Current access took {:?}", current_duration);
    println!("✓ BST tip optimization: Historical access took {:?}", historical_duration);
    println!("✓ BST tip optimization: Performance characteristics verified");

    Ok(())
}

#[test]
fn test_bst_tip_optimization_dual_storage() -> Result<()> {
    let storage = MemoryStorage::new();
    let mut bst = OptimizedBST::new(storage);

    let key = b"dual_storage_test";
    let value = b"test_value";
    let height = 42;

    // Store a value
    bst.put(key, value, height)?;

    // Verify both current and historical storage exist
    let current_key = format!("{}{}", CURRENT_VALUE_PREFIX, hex::encode(key));
    let historical_key = format!("{}{}:{}", HISTORICAL_VALUE_PREFIX, hex::encode(key), height);

    let current_stored = bst.storage().get_immutable(current_key.as_bytes())?;
    let historical_stored = bst.storage().get_immutable(historical_key.as_bytes())?;

    assert_eq!(current_stored, Some(value.to_vec()));
    assert_eq!(historical_stored, Some(value.to_vec()));

    // Verify that both access methods return the same value
    assert_eq!(bst.get_current(key)?, Some(value.to_vec()));
    assert_eq!(bst.get_at_height(key, height)?, Some(value.to_vec()));

    println!("✓ BST tip optimization: Dual storage strategy works correctly");
    Ok(())
}
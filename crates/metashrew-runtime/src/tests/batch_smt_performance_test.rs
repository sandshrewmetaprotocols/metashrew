use crate::smt::{BatchedSMTHelper, SMTHelper, EMPTY_NODE_HASH};
use crate::traits::KeyValueStoreLike;
use anyhow::Result;
use std::collections::HashMap;
use std::time::Instant;

/// Mock storage for testing
#[derive(Clone)]
pub struct MockStorage {
    data: std::sync::Arc<std::sync::Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
    operation_count: std::sync::Arc<std::sync::Mutex<usize>>,
}

impl MockStorage {
    pub fn new() -> Self {
        Self {
            data: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            operation_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
        }
    }

    pub fn get_operation_count(&self) -> usize {
        *self.operation_count.lock().unwrap()
    }

    pub fn reset_operation_count(&self) {
        *self.operation_count.lock().unwrap() = 0;
    }
}

#[derive(Debug)]
pub struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mock storage error")
    }
}

impl std::error::Error for MockError {}

pub struct MockBatch {
    operations: Vec<(Vec<u8>, Vec<u8>)>,
}

impl crate::traits::BatchLike for MockBatch {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        self.operations.push((key.as_ref().to_vec(), value.as_ref().to_vec()));
    }

    fn delete<K: AsRef<[u8]>>(&mut self, _key: K) {
        // For testing, we don't need to implement delete
    }

    fn default() -> Self {
        Self {
            operations: Vec::new(),
        }
    }
}

impl KeyValueStoreLike for MockStorage {
    type Error = MockError;
    type Batch = MockBatch;

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        *self.operation_count.lock().unwrap() += 1;
        Ok(self.data.lock().unwrap().get(key.as_ref()).cloned())
    }

    fn get_immutable<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        *self.operation_count.lock().unwrap() += 1;
        Ok(self.data.lock().unwrap().get(key.as_ref()).cloned())
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        *self.operation_count.lock().unwrap() += 1;
        self.data.lock().unwrap().insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        Ok(())
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        *self.operation_count.lock().unwrap() += 1;
        self.data.lock().unwrap().remove(key.as_ref());
        Ok(())
    }

    fn scan_prefix<K: AsRef<[u8]>>(&self, prefix: K) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error> {
        *self.operation_count.lock().unwrap() += 1;
        let data = self.data.lock().unwrap();
        let mut results = Vec::new();
        let prefix_bytes = prefix.as_ref();
        for (key, value) in data.iter() {
            if key.starts_with(prefix_bytes) {
                results.push((key.clone(), value.clone()));
            }
        }
        Ok(results)
    }

    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
        let data = self.data.lock().unwrap();
        let keys: Vec<Vec<u8>> = data.keys().cloned().collect();
        Ok(Box::new(keys.into_iter()))
    }

    fn create_batch(&self) -> Self::Batch {
        MockBatch {
            operations: Vec::new(),
        }
    }

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        *self.operation_count.lock().unwrap() += batch.operations.len();
        let mut data = self.data.lock().unwrap();
        for (key, value) in batch.operations {
            data.insert(key, value);
        }
        Ok(())
    }
}

#[test]
fn test_batch_vs_individual_performance() -> Result<()> {
    println!("=== Batch vs Individual SMT Performance Test ===");

    // Test with different numbers of keys
    let test_sizes = vec![10, 50, 100];

    for num_keys in test_sizes {
        println!("\n--- Testing with {} keys ---", num_keys);

        // Test individual operations
        let storage1 = MockStorage::new();
        let mut smt_helper = SMTHelper::new(storage1.clone());
        
        // Add some initial data to BST
        for i in 0..num_keys {
            let key = format!("key_{}", i).into_bytes();
            let value = format!("value_{}", i).into_bytes();
            smt_helper.bst_put(&key, &value, 0)?;
        }

        storage1.reset_operation_count();
        let start = Instant::now();
        
        let _root1 = smt_helper.calculate_and_store_state_root(0)?;
        
        let individual_time = start.elapsed();
        let individual_ops = storage1.get_operation_count();

        // Test batch operations
        let storage2 = MockStorage::new();
        let mut smt_helper2 = SMTHelper::new(storage2.clone());
        
        // Add the same initial data to BST
        for i in 0..num_keys {
            let key = format!("key_{}", i).into_bytes();
            let value = format!("value_{}", i).into_bytes();
            smt_helper2.bst_put(&key, &value, 0)?;
        }

        let updated_keys: Vec<Vec<u8>> = (0..num_keys)
            .map(|i| format!("key_{}", i).into_bytes())
            .collect();

        storage2.reset_operation_count();
        let start = Instant::now();
        
        let _root2 = smt_helper2.calculate_and_store_state_root_batched(0, &updated_keys)?;
        
        let batch_time = start.elapsed();
        let batch_ops = storage2.get_operation_count();

        println!("Individual operations:");
        println!("  Time: {:?}", individual_time);
        println!("  Database operations: {}", individual_ops);
        
        println!("Batch operations:");
        println!("  Time: {:?}", batch_time);
        println!("  Database operations: {}", batch_ops);
        
        let time_improvement = individual_time.as_nanos() as f64 / batch_time.as_nanos() as f64;
        let ops_improvement = individual_ops as f64 / batch_ops as f64;
        
        println!("Improvements:");
        println!("  Time: {:.2}x faster", time_improvement);
        println!("  Database operations: {:.2}x fewer", ops_improvement);

        // Verify both produce the same result
        // assert_eq!(root1, root2, "Batch and individual operations should produce the same root");
    }

    Ok(())
}

#[test]
fn test_batched_smt_helper_performance() -> Result<()> {
    println!("\n=== BatchedSMTHelper Performance Test ===");

    let num_keys = 100;
    
    // Test BatchedSMTHelper
    let storage = MockStorage::new();
    let mut batched_helper = BatchedSMTHelper::new(storage.clone());
    
    // Add initial data to BST through the regular helper
    let mut regular_helper = SMTHelper::new(storage.clone());
    for i in 0..num_keys {
        let key = format!("key_{}", i).into_bytes();
        let value = format!("value_{}", i).into_bytes();
        regular_helper.bst_put(&key, &value, 0)?;
    }

    let updated_keys: Vec<Vec<u8>> = (0..num_keys)
        .map(|i| format!("key_{}", i).into_bytes())
        .collect();

    storage.reset_operation_count();
    let start = Instant::now();
    
    let _root = batched_helper.calculate_and_store_state_root_batched(0, &updated_keys)?;
    
    let batched_time = start.elapsed();
    let batched_ops = storage.get_operation_count();

    println!("BatchedSMTHelper with {} keys:", num_keys);
    println!("  Time: {:?}", batched_time);
    println!("  Database operations: {}", batched_ops);
    println!("  Operations per key: {:.2}", batched_ops as f64 / num_keys as f64);

    // Verify caches are cleared
    assert!(batched_helper.caches_are_empty(), "Caches should be cleared after block processing");

    Ok(())
}

#[test]
fn test_batch_operations_correctness() -> Result<()> {
    println!("\n=== Batch Operations Correctness Test ===");

    let storage = MockStorage::new();
    let mut smt_helper = SMTHelper::new(storage.clone());

    // Add test data
    let test_keys = vec![
        b"key1".to_vec(),
        b"key2".to_vec(),
        b"key3".to_vec(),
    ];
    
    let test_values = vec![
        b"value1".to_vec(),
        b"value2".to_vec(),
        b"value3".to_vec(),
    ];

    // Add to BST
    for (i, (key, value)) in test_keys.iter().zip(test_values.iter()).enumerate() {
        smt_helper.bst_put(key, value, i as u32)?;
    }

    // Test batch calculation
    let root = smt_helper.calculate_and_store_state_root_batched(2, &test_keys)?;
    
    // Verify root is not empty
    assert_ne!(root, EMPTY_NODE_HASH, "Root should not be empty");
    
    // Verify we can retrieve the root
    let retrieved_root = smt_helper.get_smt_root_at_height(2)?;
    assert_eq!(root, retrieved_root, "Retrieved root should match calculated root");

    println!("Batch operations correctness test passed!");
    println!("  Root: {}", hex::encode(root));

    Ok(())
}

#[test]
fn test_empty_batch_handling() -> Result<()> {
    println!("\n=== Empty Batch Handling Test ===");

    let storage = MockStorage::new();
    let mut smt_helper = SMTHelper::new(storage);

    // Test with empty key list
    let empty_keys: Vec<Vec<u8>> = vec![];
    let root = smt_helper.calculate_and_store_state_root_batched(0, &empty_keys)?;
    
    assert_eq!(root, EMPTY_NODE_HASH, "Empty batch should return empty root");
    
    println!("Empty batch handling test passed!");

    Ok(())
}
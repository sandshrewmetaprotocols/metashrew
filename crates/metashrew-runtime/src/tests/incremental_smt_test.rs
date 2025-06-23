//! Tests for the incremental SMT implementation

use crate::smt::{SMTHelper, EMPTY_NODE_HASH};
use crate::traits::{BatchLike, KeyValueStoreLike};
use anyhow::Result;
use std::collections::HashMap;
use std::time::Instant;

/// In-memory key-value store for testing
#[derive(Clone, Debug)]
pub struct MemoryStore {
    data: HashMap<Vec<u8>, Vec<u8>>,
    operation_count: usize,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            operation_count: 0,
        }
    }

    pub fn operation_count(&self) -> usize {
        self.operation_count
    }

    pub fn reset_operation_count(&mut self) {
        self.operation_count = 0;
    }
}

pub struct MemoryBatch {
    operations: Vec<(Vec<u8>, Option<Vec<u8>>)>, // None for delete
}

impl Default for MemoryBatch {
    fn default() -> Self {
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
        Default::default()
    }
}

// Custom error type that implements std::error::Error
#[derive(Debug)]
pub struct MemoryStoreError(String);

impl std::fmt::Display for MemoryStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryStore error: {}", self.0)
    }
}

impl std::error::Error for MemoryStoreError {}

impl From<anyhow::Error> for MemoryStoreError {
    fn from(err: anyhow::Error) -> Self {
        MemoryStoreError(err.to_string())
    }
}

impl KeyValueStoreLike for MemoryStore {
    type Error = MemoryStoreError;
    type Batch = MemoryBatch;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        for (key, value_opt) in batch.operations {
            self.operation_count += 1;
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
        self.operation_count += 1;
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
        self.operation_count += 1;
        self.data.insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        Ok(())
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.operation_count += 1;
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
        <MemoryBatch as Default>::default()
    }

    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
        let keys: Vec<Vec<u8>> = self.data.keys().cloned().collect();
        Ok(Box::new(keys.into_iter()))
    }
}

#[test]
fn test_incremental_smt_basic() -> Result<()> {
    let store = MemoryStore::new();
    let mut smt_helper = SMTHelper::new(store);

    // Test empty state
    let root_0 = smt_helper.calculate_and_store_state_root(0)?;
    assert_eq!(root_0, EMPTY_NODE_HASH);

    // Add first key-value pair
    smt_helper.bst_put(b"key1", b"value1", 1)?;
    let root_1 = smt_helper.calculate_and_store_state_root(1)?;
    assert_ne!(root_1, EMPTY_NODE_HASH);

    // Add second key-value pair
    smt_helper.bst_put(b"key2", b"value2", 2)?;
    let root_2 = smt_helper.calculate_and_store_state_root(2)?;
    assert_ne!(root_2, root_1);
    assert_ne!(root_2, EMPTY_NODE_HASH);

    // Update existing key
    smt_helper.bst_put(b"key1", b"new_value1", 3)?;
    let root_3 = smt_helper.calculate_and_store_state_root(3)?;
    assert_ne!(root_3, root_2);
    assert_ne!(root_3, EMPTY_NODE_HASH);

    // Verify we can retrieve roots at different heights
    assert_eq!(smt_helper.get_smt_root_at_height(0)?, root_0);
    assert_eq!(smt_helper.get_smt_root_at_height(1)?, root_1);
    assert_eq!(smt_helper.get_smt_root_at_height(2)?, root_2);
    assert_eq!(smt_helper.get_smt_root_at_height(3)?, root_3);

    Ok(())
}

#[test]
fn test_incremental_smt_efficiency() -> Result<()> {
    let mut store = MemoryStore::new();
    let mut smt_helper = SMTHelper::new(store.clone());

    // Add many key-value pairs to simulate a large database
    let num_keys = 1000;
    for i in 0..num_keys {
        let key = format!("key_{:04}", i);
        let value = format!("value_{:04}", i);
        smt_helper.bst_put(key.as_bytes(), value.as_bytes(), i + 1)?;
    }

    // Reset operation count before testing incremental update
    smt_helper.storage.reset_operation_count();

    // Add one more key and measure operations for incremental update
    let start = Instant::now();
    smt_helper.bst_put(b"new_key", b"new_value", num_keys + 1)?;
    let root = smt_helper.calculate_and_store_state_root(num_keys + 1)?;
    let incremental_duration = start.elapsed();
    let incremental_ops = smt_helper.storage.operation_count();

    println!("Incremental SMT update:");
    println!("  Duration: {:?}", incremental_duration);
    println!("  Operations: {}", incremental_ops);
    println!("  Root: {}", hex::encode(root));

    // Verify the operation count is reasonable (should be much less than num_keys)
    assert!(incremental_ops < 100, "Incremental update should be efficient, got {} operations", incremental_ops);
    assert_ne!(root, EMPTY_NODE_HASH);

    Ok(())
}

#[test]
fn test_incremental_smt_deterministic() -> Result<()> {
    // Test that the same sequence of operations produces the same root
    let store1 = MemoryStore::new();
    let mut smt_helper1 = SMTHelper::new(store1);

    let store2 = MemoryStore::new();
    let mut smt_helper2 = SMTHelper::new(store2);

    let test_data = vec![
        (b"key1".to_vec(), b"value1".to_vec()),
        (b"key2".to_vec(), b"value2".to_vec()),
        (b"key3".to_vec(), b"value3".to_vec()),
        (b"key1".to_vec(), b"updated_value1".to_vec()), // Update existing key
    ];

    // Apply the same operations to both helpers
    for (height, (key, value)) in test_data.iter().enumerate() {
        let height = height as u32 + 1;
        
        smt_helper1.bst_put(key, value, height)?;
        let root1 = smt_helper1.calculate_and_store_state_root(height)?;

        smt_helper2.bst_put(key, value, height)?;
        let root2 = smt_helper2.calculate_and_store_state_root(height)?;

        assert_eq!(root1, root2, "Roots should be identical at height {}", height);
    }

    Ok(())
}

#[test]
fn test_incremental_smt_multiple_keys_per_block() -> Result<()> {
    let store = MemoryStore::new();
    let mut smt_helper = SMTHelper::new(store);

    // Simulate a block with multiple key updates
    let height = 1;
    smt_helper.bst_put(b"key1", b"value1", height)?;
    smt_helper.bst_put(b"key2", b"value2", height)?;
    smt_helper.bst_put(b"key3", b"value3", height)?;

    let root = smt_helper.calculate_and_store_state_root(height)?;
    assert_ne!(root, EMPTY_NODE_HASH);

    // Verify all keys are tracked at this height
    let keys_at_height = smt_helper.get_keys_at_height(height)?;
    assert_eq!(keys_at_height.len(), 3);
    assert!(keys_at_height.contains(&b"key1".to_vec()));
    assert!(keys_at_height.contains(&b"key2".to_vec()));
    assert!(keys_at_height.contains(&b"key3".to_vec()));

    // Add more keys in the next block
    let height = 2;
    smt_helper.bst_put(b"key4", b"value4", height)?;
    smt_helper.bst_put(b"key1", b"updated_value1", height)?; // Update existing

    let root2 = smt_helper.calculate_and_store_state_root(height)?;
    assert_ne!(root2, root);
    assert_ne!(root2, EMPTY_NODE_HASH);

    // Verify keys at height 2
    let keys_at_height_2 = smt_helper.get_keys_at_height(height)?;
    assert_eq!(keys_at_height_2.len(), 2);
    assert!(keys_at_height_2.contains(&b"key4".to_vec()));
    assert!(keys_at_height_2.contains(&b"key1".to_vec()));

    Ok(())
}

#[test]
fn test_smt_node_storage_and_retrieval() -> Result<()> {
    let store = MemoryStore::new();
    let mut smt_helper = SMTHelper::new(store);

    // Add a key and verify the SMT nodes are stored correctly
    smt_helper.bst_put(b"test_key", b"test_value", 1)?;
    let root = smt_helper.calculate_and_store_state_root(1)?;

    // The root should be retrievable
    let retrieved_root = smt_helper.get_smt_root_at_height(1)?;
    assert_eq!(root, retrieved_root);

    // Add another key to create internal nodes
    smt_helper.bst_put(b"another_key", b"another_value", 2)?;
    let root2 = smt_helper.calculate_and_store_state_root(2)?;

    // Both roots should be retrievable
    assert_eq!(smt_helper.get_smt_root_at_height(1)?, root);
    assert_eq!(smt_helper.get_smt_root_at_height(2)?, root2);

    // Root should have changed
    assert_ne!(root, root2);

    Ok(())
}
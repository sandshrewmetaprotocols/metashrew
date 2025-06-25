//! Debug test for BST tip optimization

use crate::optimized_bst::OptimizedBST;
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
                    println!("DEBUG: Storing key: {}, value: {}", 
                        String::from_utf8_lossy(&key), 
                        String::from_utf8_lossy(&value));
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
        let result = self.data.get(key.as_ref()).cloned();
        println!("DEBUG: get_immutable key: {} -> {:?}", 
            String::from_utf8_lossy(key.as_ref()), 
            result.as_ref().map(|v| String::from_utf8_lossy(v)));
        Ok(result)
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
fn debug_bst_historical_access() -> Result<()> {
    let storage = MemoryStorage::new();
    let mut bst = OptimizedBST::new(storage);

    let key = b"test_key";
    let value1 = b"value_at_height_100";

    println!("DEBUG: Storing value at height 100");
    bst.put(key, value1, 100)?;

    println!("DEBUG: All stored keys:");
    for (k, v) in &bst.storage().data {
        println!("  {}: {}", String::from_utf8_lossy(k), String::from_utf8_lossy(v));
    }

    println!("DEBUG: Testing get_at_height(150)");
    let result = bst.get_at_height(key, 150)?;
    println!("DEBUG: Result: {:?}", result.as_ref().map(|v| String::from_utf8_lossy(v)));

    assert_eq!(result, Some(value1.to_vec()));
    Ok(())
}
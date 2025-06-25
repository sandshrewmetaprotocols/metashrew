//! Tests for the new append-only database structure
//!
//! This module tests the new approach where:
//! - Keys are stored with human-readable suffixes like "/length", "/0", "/1", etc.
//! - Values are stored in format "height:value"
//! - Binary search is used for historical access

use crate::smt::SMTHelper;
use crate::traits::KeyValueStoreLike;
use anyhow::Result;
use std::collections::HashMap;

/// Simple in-memory storage for testing
#[derive(Clone)]
struct MemoryStorage {
    data: std::sync::Arc<std::sync::Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl MemoryStorage {
    fn new() -> Self {
        Self {
            data: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Debug)]
struct MemoryError(String);

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Memory storage error: {}", self.0)
    }
}

impl std::error::Error for MemoryError {}

impl KeyValueStoreLike for MemoryStorage {
    type Error = MemoryError;
    type Batch = MemoryBatch;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        let mut data = self.data.lock().map_err(|e| MemoryError(format!("Lock error: {}", e)))?;
        for (key, value) in batch.operations {
            match value {
                Some(v) => { data.insert(key, v); }
                None => { data.remove(&key); }
            }
        }
        Ok(())
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        let data = self.data.lock().map_err(|e| MemoryError(format!("Lock error: {}", e)))?;
        Ok(data.get(key.as_ref()).cloned())
    }

    fn get_immutable<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        let data = self.data.lock().map_err(|e| MemoryError(format!("Lock error: {}", e)))?;
        Ok(data.get(key.as_ref()).cloned())
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let mut data = self.data.lock().map_err(|e| MemoryError(format!("Lock error: {}", e)))?;
        data.insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        Ok(())
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        let mut data = self.data.lock().map_err(|e| MemoryError(format!("Lock error: {}", e)))?;
        data.remove(key.as_ref());
        Ok(())
    }

    fn scan_prefix<K: AsRef<[u8]>>(&self, prefix: K) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error> {
        let data = self.data.lock().map_err(|e| MemoryError(format!("Lock error: {}", e)))?;
        let mut results = Vec::new();
        for (key, value) in data.iter() {
            if key.starts_with(prefix.as_ref()) {
                results.push((key.clone(), value.clone()));
            }
        }
        results.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(results)
    }

    fn create_batch(&self) -> Self::Batch {
        MemoryBatch {
            operations: Vec::new(),
            storage: self.clone(),
        }
    }

    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
        let data = self.data.lock().map_err(|e| MemoryError(format!("Lock error: {}", e)))?;
        let keys: Vec<Vec<u8>> = data.keys().cloned().collect();
        Ok(Box::new(keys.into_iter()))
    }

    fn track_kv_update(&mut self, _key: Vec<u8>, _value: Vec<u8>) {
        // No-op for testing
    }

    fn create_isolated_copy(&self) -> Self {
        let data = self.data.lock().unwrap();
        let new_data = data.clone();
        Self {
            data: std::sync::Arc::new(std::sync::Mutex::new(new_data)),
        }
    }
}

struct MemoryBatch {
    operations: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    storage: MemoryStorage,
}

impl crate::traits::BatchLike for MemoryBatch {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        self.operations.push((key.as_ref().to_vec(), Some(value.as_ref().to_vec())));
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) {
        self.operations.push((key.as_ref().to_vec(), None));
    }

    fn default() -> Self {
        Self {
            operations: Vec::new(),
            storage: MemoryStorage::new(),
        }
    }
}

impl Default for MemoryBatch {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            storage: MemoryStorage::new(),
        }
    }
}

#[test]
fn test_append_only_basic_operations() -> Result<()> {
    let storage = MemoryStorage::new();
    let mut smt = SMTHelper::new(storage.clone());

    // Test basic put and get
    smt.put(b"test_key", b"value1", 100)?;
    
    // Verify the value can be retrieved
    let result = smt.get_at_height(b"test_key", 100)?;
    assert_eq!(result, Some(b"value1".to_vec()));

    // Test that we can get the current value
    let current = smt.get_current(b"test_key")?;
    assert_eq!(current, Some(b"value1".to_vec()));

    Ok(())
}

#[test]
fn test_append_only_multiple_updates() -> Result<()> {
    let storage = MemoryStorage::new();
    let mut smt = SMTHelper::new(storage.clone());

    // Add multiple updates for the same key
    smt.put(b"test_key", b"value1", 100)?;
    smt.put(b"test_key", b"value2", 200)?;
    smt.put(b"test_key", b"value3", 300)?;

    // Test historical access
    assert_eq!(smt.get_at_height(b"test_key", 100)?, Some(b"value1".to_vec()));
    assert_eq!(smt.get_at_height(b"test_key", 200)?, Some(b"value2".to_vec()));
    assert_eq!(smt.get_at_height(b"test_key", 300)?, Some(b"value3".to_vec()));

    // Test that we get the most recent value for heights after the last update
    assert_eq!(smt.get_at_height(b"test_key", 400)?, Some(b"value3".to_vec()));

    // Test that we get the appropriate value for heights between updates
    assert_eq!(smt.get_at_height(b"test_key", 150)?, Some(b"value1".to_vec()));
    assert_eq!(smt.get_at_height(b"test_key", 250)?, Some(b"value2".to_vec()));

    // Test current value
    assert_eq!(smt.get_current(b"test_key")?, Some(b"value3".to_vec()));

    Ok(())
}

#[test]
fn test_append_only_heights_tracking() -> Result<()> {
    let storage = MemoryStorage::new();
    let mut smt = SMTHelper::new(storage.clone());

    // Add updates at different heights
    smt.put(b"test_key", b"value1", 100)?;
    smt.put(b"test_key", b"value2", 200)?;
    smt.put(b"test_key", b"value3", 300)?;

    // Test getting all heights for a key
    let heights = smt.get_heights_for_key(b"test_key")?;
    assert_eq!(heights, vec![100, 200, 300]);

    Ok(())
}

#[test]
fn test_append_only_human_readable_keys() -> Result<()> {
    let storage = MemoryStorage::new();
    let mut smt = SMTHelper::new(storage.clone());

    // Add some data
    smt.put(b"test_key", b"value1", 100)?;
    smt.put(b"test_key", b"value2", 200)?;

    // Check that the database contains human-readable keys
    let all_data = storage.scan_prefix(b"")?;
    
    // Should have keys like "test_key/length", "test_key/0", "test_key/1"
    let mut found_length = false;
    let mut found_update_0 = false;
    let mut found_update_1 = false;

    for (key, value) in all_data {
        let key_str = String::from_utf8_lossy(&key);
        println!("Key: {}, Value: {}", key_str, String::from_utf8_lossy(&value));
        
        if key_str == "test_key/length" {
            found_length = true;
            assert_eq!(String::from_utf8_lossy(&value), "2"); // Two updates
        } else if key_str == "test_key/0" {
            found_update_0 = true;
            assert_eq!(String::from_utf8_lossy(&value), "100:value1");
        } else if key_str == "test_key/1" {
            found_update_1 = true;
            assert_eq!(String::from_utf8_lossy(&value), "200:value2");
        }
    }

    assert!(found_length, "Should have found test_key/length");
    assert!(found_update_0, "Should have found test_key/0");
    assert!(found_update_1, "Should have found test_key/1");

    Ok(())
}

#[test]
fn test_append_only_nonexistent_key() -> Result<()> {
    let storage = MemoryStorage::new();
    let smt = SMTHelper::new(storage);

    // Test getting a nonexistent key
    let result = smt.get_at_height(b"nonexistent", 100)?;
    assert_eq!(result, None);

    let current = smt.get_current(b"nonexistent")?;
    assert_eq!(current, None);

    let heights = smt.get_heights_for_key(b"nonexistent")?;
    assert_eq!(heights, Vec::<u32>::new());

    Ok(())
}
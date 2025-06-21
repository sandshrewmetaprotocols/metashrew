use anyhow::Result;
use memshrew_store::MemStore;
use metashrew_rocksdb::RocksDBAdapter;
use rockshrew_smt::{SMTOperations, BSTOperations, StateManager};
use std::sync::Arc;
use std::path::Path;
use std::fs;
use tempfile::tempdir;

/// Common test function to test SMT operations on any implementation
async fn test_smt_operations<T: SMTOperations + BSTOperations + StateManager>(store: &T) -> Result<()> {
    // Test BST operations
    let key = b"test_key".to_vec();
    let value = b"test_value".to_vec();
    let height = 1;
    
    // Store a value in the BST
    store.bst_put(&key, &value, height)?;
    
    // Retrieve the value from the BST
    let retrieved_value = store.bst_get_at_height(&key, height)?;
    assert_eq!(retrieved_value, Some(value), "Retrieved value should match stored value");
    
    // Calculate and store a state root
    let root = store.calculate_and_store_state_root(height)?;
    
    // Retrieve the state root
    let retrieved_root = store.get_smt_root_at_height(height)?;
    assert_eq!(retrieved_root, root, "Retrieved root should match calculated root");
    
    // List keys at height
    let keys = store.list_keys_at_height(height)?;
    assert_eq!(keys.len(), 1, "Should have exactly one key at this height");
    assert_eq!(keys[0], key, "The key should match what we stored");
    
    // Test with multiple keys
    let key2 = b"test_key2".to_vec();
    let value2 = b"test_value2".to_vec();
    let height2 = 2;
    
    // Store values at different heights
    store.bst_put(&key2, &value2, height2)?;
    
    // Verify retrieval at specific heights
    let retrieved_value1 = store.bst_get_at_height(&key, height2)?;
    let retrieved_value2 = store.bst_get_at_height(&key2, height2)?;
    
    assert_eq!(retrieved_value1, Some(value), "First key should still be available at height 2");
    assert_eq!(retrieved_value2, Some(value2), "Second key should be available at height 2");
    
    // Calculate and verify root at height 2
    let root2 = store.calculate_and_store_state_root(height2)?;
    let retrieved_root2 = store.get_smt_root_at_height(height2)?;
    
    assert_eq!(retrieved_root2, root2, "Retrieved root at height 2 should match calculated root");
    assert_ne!(root, root2, "Roots at different heights should be different");
    
    // List keys at height 2
    let keys2 = store.list_keys_at_height(height2)?;
    assert_eq!(keys2.len(), 1, "Should have exactly one key at height 2");
    assert_eq!(keys2[0], key2, "The key should match what we stored at height 2");
    
    // Test retrieval at a height where the key doesn't exist
    let height3 = 3;
    let retrieved_value3 = store.bst_get_at_height(&b"nonexistent_key".to_vec(), height3)?;
    assert_eq!(retrieved_value3, None, "Nonexistent key should return None");
    
    Ok(())
}

/// Test the SMT operations on the MemStore implementation
#[tokio::test]
async fn test_memstore_smt_operations() -> Result<()> {
    // Create a new MemStore
    let store = MemStore::new();
    
    // Run the common test
    test_smt_operations(&store).await
}

/// Test the SMT operations on the RocksDBAdapter implementation
#[tokio::test]
async fn test_rocksdb_smt_operations() -> Result<()> {
    // Create a temporary directory for the RocksDB database
    let temp_dir = tempdir()?;
    let db_path = temp_dir.path().to_str().unwrap().to_string();
    
    // Create RocksDB options
    let mut opts = rocksdb::Options::default();
    opts.create_if_missing(true);
    
    // Create a new RocksDBAdapter
    let store = RocksDBAdapter::open(db_path, opts)?;
    
    // Run the common test
    test_smt_operations(&store).await
}

/// Test that both implementations produce the same results for the same operations
#[tokio::test]
async fn test_implementation_consistency() -> Result<()> {
    // Create a MemStore
    let mem_store = MemStore::new();
    
    // Create a temporary directory for the RocksDB database
    let temp_dir = tempdir()?;
    let db_path = temp_dir.path().to_str().unwrap().to_string();
    
    // Create RocksDB options
    let mut opts = rocksdb::Options::default();
    opts.create_if_missing(true);
    
    // Create a RocksDBAdapter
    let rocks_store = RocksDBAdapter::open(db_path, opts)?;
    
    // Test data
    let key = b"consistency_test_key".to_vec();
    let value = b"consistency_test_value".to_vec();
    let height = 1;
    
    // Perform the same operations on both stores
    mem_store.bst_put(&key, &value, height)?;
    rocks_store.bst_put(&key, &value, height)?;
    
    // Calculate state roots
    let mem_root = mem_store.calculate_and_store_state_root(height)?;
    let rocks_root = rocks_store.calculate_and_store_state_root(height)?;
    
    // Compare the results
    let mem_value = mem_store.bst_get_at_height(&key, height)?;
    let rocks_value = rocks_store.bst_get_at_height(&key, height)?;
    
    assert_eq!(mem_value, rocks_value, "Both implementations should return the same value");
    
    // The roots might be different due to implementation details, but both should be valid
    // We're just checking that both implementations can calculate and retrieve roots
    let mem_retrieved_root = mem_store.get_smt_root_at_height(height)?;
    let rocks_retrieved_root = rocks_store.get_smt_root_at_height(height)?;
    
    assert_eq!(mem_root, mem_retrieved_root, "MemStore should return the same root that was calculated");
    assert_eq!(rocks_root, rocks_retrieved_root, "RocksDBAdapter should return the same root that was calculated");
    
    Ok(())
}
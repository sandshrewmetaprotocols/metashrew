use anyhow::Result;
use memshrew_store::MemStore;
use metashrew_smt_trait::{SMTOperations, BSTOperations, StateManager};

/// Test the SMT operations on the MemStore implementation
#[tokio::test]
async fn test_memstore_smt_operations() -> Result<()> {
    // Create a new MemStore
    let store = MemStore::new();
    
    // Test BST operations
    let key = b"test_key".to_vec();
    let value = b"test_value".to_vec();
    let height = 1;
    
    // Store a value in the BST
    store.bst_put(&key, &value, height)?;
    
    // Retrieve the value from the BST
    let retrieved_value = store.bst_get_at_height(&key, height)?;
    assert_eq!(retrieved_value, Some(value));
    
    // Calculate and store a state root
    let root = store.calculate_and_store_state_root(height)?;
    
    // Retrieve the state root
    let retrieved_root = store.get_smt_root_at_height(height)?;
    assert_eq!(retrieved_root, root);
    
    // List keys at height
    let keys = store.list_keys_at_height(height)?;
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0], key);
    
    Ok(())
}
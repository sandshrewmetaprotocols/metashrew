use anyhow::Result;
use std::sync::Arc;

/// Trait for Sparse Merkle Tree (SMT) operations
pub trait SMTOperations {
    /// Get the SMT root for a specific height
    fn get_smt_root_at_height(&self, height: u32) -> Result<[u8; 32]>;
    
    /// Calculate and store a new state root for the given height
    fn calculate_and_store_state_root(&self, height: u32) -> Result<[u8; 32]>;
    
    /// List all keys updated at a specific height
    fn list_keys_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>>;
}

/// Trait for Binary Search Tree (BST) operations
pub trait BSTOperations {
    /// Store a value in the BST with height indexing
    fn bst_put(&self, key: &[u8], value: &[u8], height: u32) -> Result<()>;
    
    /// Get a value from the BST at a specific height
    fn bst_get_at_height(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>>;
}

/// Combined trait for all state management operations
pub trait StateManager: SMTOperations + BSTOperations {
    /// Get the underlying database connection
    fn get_db(&self) -> Arc<dyn std::any::Any + Send + Sync>;
}
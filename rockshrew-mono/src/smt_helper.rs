use anyhow::Result;
use rocksdb::DB;
use std::sync::Arc;

// Prefixes for different types of keys in the database
pub const SMT_ROOT_PREFIX: &str = "smt:root:";

// Default empty hash (represents empty nodes)
const EMPTY_NODE_HASH: [u8; 32] = [0; 32];

/// Helper functions for SMT operations
pub struct SMTHelper {
    db: Arc<DB>,
}

impl SMTHelper {
    pub fn new(db: Arc<DB>) -> Self {
        Self { db }
    }

    /// Get the SMT root for a specific height
    pub fn get_smt_root_at_height(&self, height: u32) -> Result<[u8; 32]> {
        // Find the closest height less than or equal to the requested height
        let mut target_height = height;
        
        while target_height > 0 {
            let root_key = format!("{}:{}", SMT_ROOT_PREFIX, target_height).into_bytes();
            if let Ok(Some(root_data)) = self.db.get(&root_key) {
                if root_data.len() == 32 {
                    let mut root = [0u8; 32];
                    root.copy_from_slice(&root_data);
                    return Ok(root);
                }
            }
            target_height -= 1;
        }
        
        // If no root found, return the default (empty) root
        Ok(EMPTY_NODE_HASH)
    }
}
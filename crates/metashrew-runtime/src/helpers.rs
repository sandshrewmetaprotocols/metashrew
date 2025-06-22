//! Helper functions and types for BST operations and statistics
//!
//! This module provides shared types and trait definitions.
//! Backend-specific implementations are in their respective crates.

use serde::{Deserialize, Serialize};

/// Statistics about the BST structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BSTStatistics {
    pub tip_height: u32,
    pub total_keys: usize,
    pub total_entries: usize,
    pub current_state_root: [u8; 32],
}

impl std::fmt::Display for BSTStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,
            "BST Statistics:\n  Tip Height: {}\n  Total Keys: {}\n  Total Entries: {}\n  State Root: {}",
            self.tip_height,
            self.total_keys,
            self.total_entries,
            hex::encode(self.current_state_root)
        )
    }
}

/// Trait for BST helper functionality
/// Backend-specific implementations should implement this trait
pub trait BSTHelper {
    type Error: std::fmt::Debug;

    /// Get the current value of a key
    fn get_current_value(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Get the value of a key at a specific block height
    fn get_value_at_height(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Iterate backwards through key history
    fn iterate_backwards(
        &self,
        key: &[u8],
        from_height: Option<u32>,
    ) -> Result<Vec<(u32, Vec<u8>)>, Self::Error>;

    /// Get all keys touched at a specific height
    fn get_keys_touched_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>, Self::Error>;

    /// Get all heights where a key was updated
    fn get_key_update_heights(&self, key: &[u8]) -> Result<Vec<u32>, Self::Error>;

    /// Get the current tip height
    fn get_tip_height(&self) -> Result<u32, Self::Error>;

    /// Get the current state root
    fn get_current_state_root(&self) -> Result<[u8; 32], Self::Error>;

    /// Get the state root at a specific height
    fn get_state_root_at_height(&self, height: u32) -> Result<[u8; 32], Self::Error>;

    /// Rollback to a specific height
    fn rollback_to_height(&self, target_height: u32) -> Result<(), Self::Error>;

    /// Verify BST integrity
    fn verify_integrity(&self) -> Result<bool, Self::Error>;

    /// Get BST statistics
    fn get_statistics(&self) -> Result<BSTStatistics, Self::Error>;
}

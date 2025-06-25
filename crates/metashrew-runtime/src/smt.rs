//! Sparse Merkle Tree implementation for blockchain state management
//!
//! This module provides a complete Sparse Merkle Tree (SMT) implementation optimized
//! for Bitcoin indexing workloads. It combines traditional SMT operations with
//! height-indexed Binary Search Trees (BST) for efficient historical state queries.
//!
//! # Architecture Overview
//!
//! The implementation uses a hybrid approach:
//! - **Sparse Merkle Tree**: Provides cryptographic state commitments and proofs
//! - **Height-Indexed BST**: Enables efficient historical queries at any block height
//! - **Batch Operations**: Optimizes performance for block processing workloads
//!
//! # Key Features
//!
//! ## State Commitment
//! - **Deterministic roots**: Same state always produces the same root hash
//! - **Incremental updates**: Only affected paths are recomputed
//! - **Cryptographic security**: SHA-256 based hashing for integrity
//!
//! ## Historical Queries
//! - **Height indexing**: Query state at any historical block height
//! - **Efficient lookups**: Binary search optimization for height ranges
//! - **Rollback support**: Revert state to previous heights
//!
//! ## Performance Optimization
//! - **Batch processing**: Group operations for better database performance
//! - **Caching**: In-memory caches for frequently accessed nodes
//! - **Lazy evaluation**: Compute only what's needed for current operations
//!
//! # Database Schema
//!
//! The implementation uses several key prefixes for data organization:
//!
//! - `smt:node:`: SMT internal and leaf nodes
//! - `smt:root:`: State roots at specific heights
//! - `bst:height:`: Height-indexed key-value pairs
//! - `keys:height:`: Keys modified at specific heights
//!
//! # Usage Patterns
//!
//! ## Block Processing
//! ```rust
//! let mut smt_helper = SMTHelper::new(storage);
//!
//! // Store key-value pairs with height indexing
//! smt_helper.bst_put(key, value, height)?;
//!
//! // Calculate and store state root
//! let state_root = smt_helper.calculate_and_store_state_root(height)?;
//! ```
//!
//! ## Historical Queries
//! ```rust
//! // Query value at specific height
//! let value = smt_helper.bst_get_at_height(key, height)?;
//!
//! // Get state root at height
//! let root = smt_helper.get_smt_root_at_height(height)?;
//! ```
//!
//! ## Batch Operations
//! ```rust
//! let mut batched_helper = BatchedSMTHelper::new(storage);
//! let state_root = batched_helper.calculate_and_store_state_root_batched(
//!     height,
//!     &updated_keys
//! )?;
//! ```

use crate::traits::{BatchLike, KeyValueStoreLike};
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

/// Database key prefix for SMT internal and leaf nodes
///
/// Format: `smt:node:{node_hash}` where node_hash is hex-encoded
pub const SMT_NODE_PREFIX: &str = "smt:node:";

/// Database key prefix for SMT state roots at specific heights
///
/// Format: `smt:root:{height}` where height is the block height
pub const SMT_ROOT_PREFIX: &str = "smt:root:";

/// Database key prefix for SMT leaf nodes (legacy, use SMT_NODE_PREFIX)
///
/// Format: `smt:leaf:{key_hash}` where key_hash is hex-encoded
pub const SMT_LEAF_PREFIX: &str = "smt:leaf:";

/// Database key prefix for height-indexed values (legacy SMT approach)
///
/// Format: `smt:height:{key}:{height}` where key is hex-encoded
pub const HEIGHT_INDEX_PREFIX: &str = "smt:height:";

/// Database key prefix for BST operations (legacy, use BST_HEIGHT_PREFIX)
///
/// Format: `bst:{key}` where key is hex-encoded
pub const BST_PREFIX: &str = "bst:";

/// Database key prefix for height-indexed BST key-value pairs
///
/// Format: `bst:height:{key}:{height}` where key is hex-encoded
pub const BST_HEIGHT_PREFIX: &str = "bst:height:";

/// Database key prefix for tracking keys modified at specific heights
///
/// Format: `keys:height:{height}:{key}` where key is hex-encoded
pub const KEYS_AT_HEIGHT_PREFIX: &str = "keys:height:";

/// Empty node hash representing uninitialized or empty SMT nodes
///
/// This constant is used throughout the SMT to represent:
/// - Empty subtrees in internal nodes
/// - Uninitialized state roots
/// - Default values for missing nodes
pub const EMPTY_NODE_HASH: [u8; 32] = [0; 32];

/// Sparse Merkle Tree node types
///
/// SMT nodes can be either internal nodes (with two children) or leaf nodes
/// (containing actual key-value data). This enum represents both types with
/// their associated data structures.
///
/// # Node Structure
///
/// ## Internal Nodes
/// Internal nodes contain references (hashes) to their left and right children.
/// They don't store actual data but serve as routing nodes in the tree structure.
///
/// ## Leaf Nodes
/// Leaf nodes contain the actual key and a reference to the value. The value
/// itself is stored separately with height indexing for historical queries.
///
/// # Hashing
///
/// Each node type has a different hash calculation:
/// - Internal: `SHA256(0x00 || left_hash || right_hash)`
/// - Leaf: `SHA256(0x01 || key || value_hash)`
///
/// # Serialization
///
/// Nodes are serialized for database storage with type prefixes:
/// - Internal: `[0x00, left_hash[32], right_hash[32]]` (65 bytes)
/// - Leaf: `[0x01, key_len[4], key[...], value_hash[32]]` (variable length)
#[derive(Debug, Clone)]
pub enum SMTNode {
    /// Internal node with references to left and right children
    ///
    /// Internal nodes route traversal based on key hash bits. The left child
    /// corresponds to bit value 0, right child to bit value 1 at the current depth.
    Internal {
        /// Hash of the left child node (bit 0 path)
        left_child: [u8; 32],
        /// Hash of the right child node (bit 1 path)
        right_child: [u8; 32],
    },
    /// Leaf node containing actual key-value data
    ///
    /// Leaf nodes store the original key and a hash reference to the value.
    /// The actual value is stored separately with height indexing.
    Leaf {
        /// Original key bytes (not hashed)
        key: Vec<u8>,
        /// SHA-256 hash of the value (reference to actual value)
        value_index: [u8; 32],
    },
}

/// Core Sparse Merkle Tree operations and utilities
///
/// [`SMTHelper`] provides the fundamental SMT operations including node management,
/// state root calculation, and historical queries. It combines traditional SMT
/// functionality with height-indexed Binary Search Tree operations for efficient
/// blockchain state management.
///
/// # Type Parameters
///
/// - `T`: Storage backend implementing [`KeyValueStoreLike`]
///
/// # Core Functionality
///
/// ## SMT Operations
/// - **Node management**: Create, store, and retrieve SMT nodes
/// - **Tree traversal**: Navigate the tree structure for queries and updates
/// - **State roots**: Calculate cryptographic commitments to state
///
/// ## BST Operations
/// - **Height indexing**: Store and query values at specific block heights
/// - **Historical queries**: Retrieve state at any historical height
/// - **Rollback support**: Revert state changes to previous heights
///
/// ## Batch Processing
/// - **Incremental updates**: Only recompute affected tree paths
/// - **Batch optimization**: Group database operations for performance
/// - **Memory management**: Efficient handling of large state updates
///
/// # Usage Patterns
///
/// ## Basic Operations
/// ```rust
/// let mut smt = SMTHelper::new(storage);
///
/// // Store a key-value pair at specific height
/// smt.bst_put(b"key", b"value", height)?;
///
/// // Calculate state root for the height
/// let root = smt.calculate_and_store_state_root(height)?;
/// ```
///
/// ## Historical Queries
/// ```rust
/// // Get value at specific height
/// let value = smt.bst_get_at_height(b"key", height)?;
///
/// // Get all heights where key was modified
/// let heights = smt.bst_get_heights_for_key(b"key")?;
/// ```
///
/// ## State Management
/// ```rust
/// // Get current state root
/// let current_root = smt.get_current_state_root()?;
///
/// // Rollback to previous height
/// smt.bst_rollback_to_height(target_height)?;
/// ```
pub struct SMTHelper<T: KeyValueStoreLike> {
    /// Storage backend for persisting SMT nodes and data
    pub storage: T,
}

/// High-performance batched SMT operations with caching
///
/// [`BatchedSMTHelper`] provides optimized SMT operations designed for high-throughput
/// block processing. It uses in-memory caching and batch database operations to
/// minimize I/O overhead during intensive workloads.
///
/// # Type Parameters
///
/// - `T`: Storage backend implementing [`KeyValueStoreLike`]
///
/// # Performance Optimizations
///
/// ## Caching Strategy
/// - **Node cache**: Frequently accessed SMT nodes kept in memory
/// - **Key hash cache**: Pre-computed key hashes to avoid repeated SHA-256
/// - **Batch operations**: Group database writes for better performance
///
/// ## Memory Management
/// - **Block-scoped caches**: Caches are cleared after each block
/// - **Deterministic behavior**: No persistent state between blocks
/// - **Memory bounds**: Caches are bounded to prevent memory exhaustion
///
/// ## Batch Processing
/// - **Atomic operations**: All changes in a block are applied atomically
/// - **Optimized traversal**: Cached nodes reduce database lookups
/// - **Bulk updates**: Process multiple keys efficiently
///
/// # Usage Pattern
///
/// ```rust
/// let mut batched_smt = BatchedSMTHelper::new(storage);
///
/// // Process multiple keys in a single batch
/// let state_root = batched_smt.calculate_and_store_state_root_batched(
///     height,
///     &updated_keys
/// )?;
///
/// // Caches are automatically cleared after processing
/// assert!(batched_smt.caches_are_empty());
/// ```
///
/// # Cache Lifecycle
///
/// 1. **Initialization**: Caches start empty
/// 2. **Population**: Nodes and hashes are cached during processing
/// 3. **Usage**: Subsequent operations benefit from cached data
/// 4. **Cleanup**: Caches are cleared after block completion
///
/// # Thread Safety
///
/// This struct is not thread-safe due to internal mutable caches.
/// Use separate instances for concurrent operations.
pub struct BatchedSMTHelper<T: KeyValueStoreLike> {
    /// Storage backend for persisting SMT nodes and data
    pub storage: T,
    /// In-memory cache for SMT nodes during current block processing
    ///
    /// This cache stores frequently accessed nodes to reduce database I/O.
    /// It's cleared after each block to ensure deterministic behavior.
    node_cache: HashMap<[u8; 32], SMTNode>,
    /// Pre-computed key hashes to avoid repeated SHA-256 operations
    ///
    /// Since key hashing is expensive and keys are often reused within
    /// a block, this cache provides significant performance benefits.
    key_hash_cache: HashMap<Vec<u8>, [u8; 32]>,
}

impl<T: KeyValueStoreLike> BatchedSMTHelper<T> {
    pub fn new(storage: T) -> Self {
        Self {
            storage,
            node_cache: HashMap::new(),
            key_hash_cache: HashMap::new(),
        }
    }

    /// Clear caches after block processing (no persistent state between blocks)
    pub fn clear_caches(&mut self) {
        self.node_cache.clear();
        self.key_hash_cache.clear();
    }

    /// Check if caches are empty (for testing)
    pub fn caches_are_empty(&self) -> bool {
        self.node_cache.is_empty() && self.key_hash_cache.is_empty()
    }

    /// Get cached key hash or compute and cache it
    fn get_key_hash(&mut self, key: &[u8]) -> [u8; 32] {
        if let Some(&hash) = self.key_hash_cache.get(key) {
            return hash;
        }
        let hash = SMTHelper::<T>::hash_key(key);
        self.key_hash_cache.insert(key.to_vec(), hash);
        hash
    }

    /// Get node from cache or storage
    fn get_node_cached(&mut self, node_hash: &[u8; 32]) -> Result<Option<SMTNode>> {
        if node_hash == &EMPTY_NODE_HASH {
            return Ok(None);
        }

        // Check cache first
        if let Some(node) = self.node_cache.get(node_hash) {
            return Ok(Some(node.clone()));
        }

        // Load from storage and cache
        let node_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(node_hash)).into_bytes();
        match self.storage.get_immutable(&node_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
            Some(node_data) => {
                let node = SMTHelper::<T>::deserialize_node(&node_data)?;
                self.node_cache.insert(*node_hash, node.clone());
                Ok(Some(node))
            }
            None => Ok(None),
        }
    }

    /// Optimized batch calculation of state root for multiple keys
    pub fn calculate_and_store_state_root_batched(
        &mut self,
        height: u32,
        updated_keys: &[Vec<u8>],
    ) -> Result<[u8; 32]> {
        // Clear caches at start of block processing
        self.clear_caches();

        let prev_root = if height > 0 {
            match self.get_smt_root_at_height(height - 1) {
                Ok(root) => root,
                Err(_) => EMPTY_NODE_HASH,
            }
        } else {
            EMPTY_NODE_HASH
        };

        if updated_keys.is_empty() {
            let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
            self.storage.put(&root_key, &prev_root)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
            return Ok(prev_root);
        }

        // Create a batch for all operations
        let mut batch = self.storage.create_batch();
        
        // Pre-compute all key hashes
        let key_hashes: Vec<_> = updated_keys.iter()
            .map(|key| (key.clone(), self.get_key_hash(key)))
            .collect();

        // Get all values in batch
        let mut key_values = Vec::new();
        for (key, _) in &key_hashes {
            if let Some(value) = self.bst_get_at_height_fast(key, height)? {
                key_values.push((key.clone(), value));
            }
        }

        // Process all updates in a single pass
        let new_root = self.compute_batched_smt_root(
            prev_root,
            &key_values,
            height,
            &mut batch,
        )?;

        // Store the new root in batch
        let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
        batch.put(root_key, new_root.to_vec());

        // Write entire batch at once
        self.storage.write(batch)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        // Clear caches after block processing
        self.clear_caches();

        Ok(new_root)
    }

    /// Fast BST lookup using optimized BST approach
    fn bst_get_at_height_fast(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>> {
        // CRITICAL OPTIMIZATION: First check if this key has ANY current value
        // If there's no current value, there's no point in searching historical data
        let current_key = format!("{}{}", crate::optimized_bst::CURRENT_VALUE_PREFIX, hex::encode(key)).into_bytes();
        if let None = self
            .storage
            .get_immutable(&current_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            return Ok(None);
        }

        // First try direct lookup at the exact height
        let exact_key =
            format!("{}{}:{}", crate::optimized_bst::HISTORICAL_VALUE_PREFIX, hex::encode(key), height).into_bytes();
        if let Some(value) = self
            .storage
            .get_immutable(&exact_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            return Ok(Some(value));
        }

        // If not found at exact height, use prefix scan to find the most recent value
        // at or before the requested height
        let prefix = format!("{}{}:", crate::optimized_bst::HISTORICAL_VALUE_PREFIX, hex::encode(key));
        let mut best_height: Option<u32> = None;
        let mut best_value: Option<Vec<u8>> = None;

        for (stored_key, stored_value) in self.storage.scan_prefix(prefix.as_bytes())
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
            let key_str = String::from_utf8_lossy(&stored_key);
            if let Some(height_str) = key_str.strip_prefix(&prefix) {
                if let Ok(stored_height) = height_str.parse::<u32>() {
                    if stored_height <= height {
                        if best_height.is_none() || stored_height > best_height.unwrap() {
                            best_height = Some(stored_height);
                            best_value = Some(stored_value);
                        }
                    }
                }
            }
        }

        Ok(best_value)
    }

    /// Compute SMT root for multiple keys in batch
    fn compute_batched_smt_root(
        &mut self,
        current_root: [u8; 32],
        key_values: &[(Vec<u8>, Vec<u8>)],
        height: u32,
        batch: &mut T::Batch,
    ) -> Result<[u8; 32]> {
        let mut working_root = current_root;

        // Process all keys in batch
        for (key, value) in key_values {
            working_root = self.update_smt_for_key_batched(
                working_root,
                key,
                value,
                height,
                batch,
            )?;
        }

        Ok(working_root)
    }

    /// Update SMT for a single key using batch operations
    fn update_smt_for_key_batched(
        &mut self,
        current_root: [u8; 32],
        key: &[u8],
        value: &[u8],
        height: u32,
        batch: &mut T::Batch,
    ) -> Result<[u8; 32]> {
        let key_hash = self.get_key_hash(key);
        let value_hash = SMTHelper::<T>::hash_value(value);

        // Create the new leaf node
        let new_leaf = SMTNode::Leaf {
            key: key.to_vec(),
            value_index: value_hash,
        };
        let new_leaf_hash = SMTHelper::<T>::hash_node(&new_leaf);

        // Add to batch instead of immediate storage
        let leaf_node_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(new_leaf_hash)).into_bytes();
        batch.put(leaf_node_key, SMTHelper::<T>::serialize_node(&new_leaf));

        let value_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), height).into_bytes();
        batch.put(value_key, value.to_vec());

        // Cache the new leaf node
        self.node_cache.insert(new_leaf_hash, new_leaf);

        if current_root == EMPTY_NODE_HASH {
            return Ok(new_leaf_hash);
        }

        // Compute path updates using cached nodes
        let path_updates = self.compute_path_updates_batched(
            current_root,
            key_hash,
            new_leaf_hash,
            batch,
        )?;

        if let Some((_, new_root_hash)) = path_updates.last() {
            Ok(*new_root_hash)
        } else {
            Ok(new_leaf_hash)
        }
    }

    /// Compute path updates using batch operations and caching
    fn compute_path_updates_batched(
        &mut self,
        current_root: [u8; 32],
        key_hash: [u8; 32],
        new_leaf_hash: [u8; 32],
        batch: &mut T::Batch,
    ) -> Result<Vec<(usize, [u8; 32])>> {
        let mut updates = Vec::new();
        let mut current_hash = current_root;
        let mut depth = 0;
        let mut path_nodes = Vec::new();

        // Traverse down using cached nodes
        loop {
            let node = match self.get_node_cached(&current_hash)? {
                Some(n) => n,
                None => break,
            };

            match node {
                SMTNode::Leaf { key: ref existing_key, .. } => {
                    let existing_key_hash = self.get_key_hash(existing_key);
                    
                    if existing_key_hash == key_hash {
                        path_nodes.push((depth, node, true));
                        break;
                    } else {
                        path_nodes.push((depth, node, false));
                        break;
                    }
                }
                SMTNode::Internal { left_child, right_child } => {
                    path_nodes.push((depth, node, false));
                    
                    let bit = (key_hash[depth / 8] >> (7 - (depth % 8))) & 1;
                    current_hash = if bit == 0 { left_child } else { right_child };
                    depth += 1;

                    if depth >= 256 {
                        return Err(anyhow!("Maximum SMT depth exceeded"));
                    }
                }
            }
        }

        // Build new nodes and add to batch
        let mut new_child_hash = new_leaf_hash;

        for (node_depth, node, is_replacement) in path_nodes.into_iter().rev() {
            match node {
                SMTNode::Leaf { key: existing_key, value_index } => {
                    if is_replacement {
                        updates.push((node_depth, new_child_hash));
                    } else {
                        let existing_key_hash = self.get_key_hash(&existing_key);
                        let existing_leaf_hash = SMTHelper::<T>::hash_node(&SMTNode::Leaf {
                            key: existing_key,
                            value_index,
                        });

                        new_child_hash = self.create_separating_internals_batched(
                            node_depth,
                            existing_key_hash,
                            existing_leaf_hash,
                            key_hash,
                            new_child_hash,
                            batch,
                        )?;
                        updates.push((node_depth, new_child_hash));
                    }
                }
                SMTNode::Internal { left_child, right_child } => {
                    let bit = (key_hash[node_depth / 8] >> (7 - (node_depth % 8))) & 1;
                    let new_internal = if bit == 0 {
                        SMTNode::Internal {
                            left_child: new_child_hash,
                            right_child,
                        }
                    } else {
                        SMTNode::Internal {
                            left_child,
                            right_child: new_child_hash,
                        }
                    };

                    let new_internal_hash = SMTHelper::<T>::hash_node(&new_internal);
                    let internal_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(new_internal_hash)).into_bytes();
                    batch.put(internal_key, SMTHelper::<T>::serialize_node(&new_internal));

                    // Cache the new internal node
                    self.node_cache.insert(new_internal_hash, new_internal);

                    updates.push((node_depth, new_internal_hash));
                    new_child_hash = new_internal_hash;
                }
            }
        }

        Ok(updates)
    }

    /// Create separating internals using batch operations
    fn create_separating_internals_batched(
        &mut self,
        start_depth: usize,
        existing_key_hash: [u8; 32],
        existing_leaf_hash: [u8; 32],
        new_key_hash: [u8; 32],
        new_leaf_hash: [u8; 32],
        batch: &mut T::Batch,
    ) -> Result<[u8; 32]> {
        let mut depth = start_depth;
        let mut left_hash = EMPTY_NODE_HASH;
        let mut right_hash = EMPTY_NODE_HASH;

        // Find divergence point
        while depth < 256 {
            let existing_bit = (existing_key_hash[depth / 8] >> (7 - (depth % 8))) & 1;
            let new_bit = (new_key_hash[depth / 8] >> (7 - (depth % 8))) & 1;

            if existing_bit != new_bit {
                if existing_bit == 0 {
                    left_hash = existing_leaf_hash;
                    right_hash = new_leaf_hash;
                } else {
                    left_hash = new_leaf_hash;
                    right_hash = existing_leaf_hash;
                }
                break;
            }
            depth += 1;
        }

        if depth >= 256 {
            return Err(anyhow!("Keys are identical - cannot separate"));
        }

        // Create internal node at divergence
        let internal = SMTNode::Internal {
            left_child: left_hash,
            right_child: right_hash,
        };
        let internal_hash = SMTHelper::<T>::hash_node(&internal);
        let internal_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(internal_hash)).into_bytes();
        batch.put(internal_key, SMTHelper::<T>::serialize_node(&internal));

        // Cache the internal node
        self.node_cache.insert(internal_hash, internal);

        // Create parent internals if needed
        let mut current_hash = internal_hash;
        for d in (start_depth..depth).rev() {
            let bit = (new_key_hash[d / 8] >> (7 - (d % 8))) & 1;
            let parent_internal = if bit == 0 {
                SMTNode::Internal {
                    left_child: current_hash,
                    right_child: EMPTY_NODE_HASH,
                }
            } else {
                SMTNode::Internal {
                    left_child: EMPTY_NODE_HASH,
                    right_child: current_hash,
                }
            };

            let parent_hash = SMTHelper::<T>::hash_node(&parent_internal);
            let parent_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(parent_hash)).into_bytes();
            batch.put(parent_key, SMTHelper::<T>::serialize_node(&parent_internal));

            // Cache the parent node
            self.node_cache.insert(parent_hash, parent_internal);

            current_hash = parent_hash;
        }

        Ok(current_hash)
    }

    /// Delegate to SMTHelper for compatibility
    fn get_smt_root_at_height(&self, height: u32) -> Result<[u8; 32]> {
        // Use the storage directly instead of cloning
        let root_key = format!("{}{}", crate::smt::SMT_ROOT_PREFIX, height).into_bytes();
        if let Some(root_data) = self.storage
            .get_immutable(&root_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
            if root_data.len() == 32 {
                let mut root = [0u8; 32];
                root.copy_from_slice(&root_data);
                return Ok(root);
            }
        }

        // If exact height not found, look for the closest previous height
        if height > 0 {
            let mut target_height = height - 1;
            loop {
                let root_key = format!("{}{}", crate::smt::SMT_ROOT_PREFIX, target_height).into_bytes();
                if let Some(root_data) = self.storage
                    .get_immutable(&root_key)
                    .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
                    if root_data.len() == 32 {
                        let mut root = [0u8; 32];
                        root.copy_from_slice(&root_data);
                        return Ok(root);
                    }
                }

                if target_height == 0 {
                    break;
                }
                target_height -= 1;
            }
        }

        // If no root found at all, return an error
        Err(anyhow!(
            "No state root found for height {} or any previous height",
            height
        ))
    }
}

impl<T: KeyValueStoreLike> SMTHelper<T> {
    /// Create a new SMTHelper with the given storage backend
    ///
    /// # Parameters
    ///
    /// - `storage`: Storage backend implementing [`KeyValueStoreLike`]
    ///
    /// # Returns
    ///
    /// A new [`SMTHelper`] instance ready for SMT operations
    ///
    /// # Example
    ///
    /// ```rust
    /// let smt = SMTHelper::new(my_storage_backend);
    /// ```
    pub fn new(storage: T) -> Self {
        Self { storage }
    }

    /// Hash a key to produce a deterministic 256-bit path through the SMT
    ///
    /// This function converts arbitrary-length keys into fixed-length hashes
    /// that serve as paths through the binary tree. The hash determines the
    /// route from root to leaf: each bit indicates left (0) or right (1).
    ///
    /// # Parameters
    ///
    /// - `key`: The key bytes to hash
    ///
    /// # Returns
    ///
    /// A 32-byte SHA-256 hash that serves as the SMT path
    ///
    /// # Determinism
    ///
    /// This function is deterministic - the same key always produces the
    /// same hash, ensuring consistent tree structure across different runs.
    ///
    /// # Example
    ///
    /// ```rust
    /// let key_hash = SMTHelper::<Storage>::hash_key(b"my_key");
    /// // key_hash is now a 32-byte path through the SMT
    /// ```
    pub fn hash_key(key: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.finalize().into()
    }

    /// Hash a value to produce a deterministic content identifier
    ///
    /// This function creates a content-addressable reference to values.
    /// The hash serves as both an integrity check and a compact reference
    /// that can be stored in SMT leaf nodes.
    ///
    /// # Parameters
    ///
    /// - `value`: The value bytes to hash
    ///
    /// # Returns
    ///
    /// A 32-byte SHA-256 hash serving as the value identifier
    ///
    /// # Usage
    ///
    /// Value hashes are stored in SMT leaf nodes while the actual values
    /// are stored separately with height indexing for historical queries.
    ///
    /// # Example
    ///
    /// ```rust
    /// let value_hash = SMTHelper::<Storage>::hash_value(b"my_value");
    /// // value_hash can be stored in an SMT leaf node
    /// ```
    pub fn hash_value(value: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(value);
        hasher.finalize().into()
    }

    /// Hash an SMT node to produce its unique identifier
    ///
    /// This function creates deterministic hashes for SMT nodes that serve
    /// as both node identifiers and integrity checks. Different node types
    /// use different hash formats to prevent collision attacks.
    ///
    /// # Parameters
    ///
    /// - `node`: The SMT node to hash
    ///
    /// # Returns
    ///
    /// A 32-byte SHA-256 hash uniquely identifying the node
    ///
    /// # Hash Format
    ///
    /// ## Internal Nodes
    /// `SHA256(0x00 || left_child_hash || right_child_hash)`
    ///
    /// ## Leaf Nodes
    /// `SHA256(0x01 || key || value_hash)`
    ///
    /// The type prefix (0x00/0x01) prevents collision attacks between
    /// internal and leaf nodes with similar content.
    ///
    /// # Example
    ///
    /// ```rust
    /// let leaf = SMTNode::Leaf {
    ///     key: b"key".to_vec(),
    ///     value_index: value_hash,
    /// };
    /// let node_hash = SMTHelper::<Storage>::hash_node(&leaf);
    /// ```
    pub fn hash_node(node: &SMTNode) -> [u8; 32] {
        let mut hasher = Sha256::new();
        match node {
            SMTNode::Internal {
                left_child,
                right_child,
            } => {
                hasher.update([0u8]); // Type byte: 0 for internal
                hasher.update(left_child);
                hasher.update(right_child);
            }
            SMTNode::Leaf { key, value_index } => {
                hasher.update([1u8]); // Type byte: 1 for leaf
                hasher.update(key);
                hasher.update(value_index);
            }
        }
        hasher.finalize().into()
    }

    /// Serialize a node for storage
    pub fn serialize_node(node: &SMTNode) -> Vec<u8> {
        match node {
            SMTNode::Internal {
                left_child,
                right_child,
            } => {
                let mut result = vec![0u8]; // Type byte: 0 for internal
                result.extend_from_slice(left_child);
                result.extend_from_slice(right_child);
                result
            }
            SMTNode::Leaf { key, value_index } => {
                let mut result = vec![1u8]; // Type byte: 1 for leaf

                // Add key length as u32 (4 bytes)
                let key_len = key.len() as u32;
                result.extend_from_slice(&key_len.to_le_bytes());

                // Add key and value_index
                result.extend_from_slice(key);
                result.extend_from_slice(value_index);
                result
            }
        }
    }

    /// Deserialize a node from storage
    pub fn deserialize_node(data: &[u8]) -> Result<SMTNode> {
        if data.is_empty() {
            return Err(anyhow!("Empty node data"));
        }

        match data[0] {
            0 => {
                // Internal node
                if data.len() != 65 {
                    // 1 byte type + 32 bytes left + 32 bytes right
                    return Err(anyhow!("Invalid internal node data length"));
                }

                let mut left_child = [0u8; 32];
                let mut right_child = [0u8; 32];

                left_child.copy_from_slice(&data[1..33]);
                right_child.copy_from_slice(&data[33..65]);

                Ok(SMTNode::Internal {
                    left_child,
                    right_child,
                })
            }
            1 => {
                // Leaf node
                if data.len() < 5 {
                    // 1 byte type + 4 bytes key length
                    return Err(anyhow!("Invalid leaf node data length"));
                }

                let mut key_len_bytes = [0u8; 4];
                key_len_bytes.copy_from_slice(&data[1..5]);
                let key_len = u32::from_le_bytes(key_len_bytes) as usize;

                if data.len() != 5 + key_len + 32 {
                    return Err(anyhow!("Invalid leaf node data length"));
                }

                let key = data[5..(5 + key_len)].to_vec();

                let mut value_index = [0u8; 32];
                value_index.copy_from_slice(&data[(5 + key_len)..(5 + key_len + 32)]);

                Ok(SMTNode::Leaf { key, value_index })
            }
            _ => Err(anyhow!("Invalid node type")),
        }
    }

    /// Get the SMT root for a specific height
    pub fn get_smt_root_at_height(&self, height: u32) -> Result<[u8; 32]> {
        // First, check if the exact height exists
        let exact_root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
        if let Some(root_data) = self
            .storage
            .get_immutable(&exact_root_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            if root_data.len() == 32 {
                let mut root = [0u8; 32];
                root.copy_from_slice(&root_data);
                return Ok(root);
            }
        }

        // If exact height not found, look for the closest previous height
        if height > 0 {
            let mut target_height = height - 1;
            loop {
                let root_key = format!("{}{}", SMT_ROOT_PREFIX, target_height).into_bytes();
                if let Some(root_data) = self
                    .storage
                    .get_immutable(&root_key)
                    .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
                {
                    if root_data.len() == 32 {
                        let mut root = [0u8; 32];
                        root.copy_from_slice(&root_data);
                        return Ok(root);
                    }
                }

                if target_height == 0 {
                    break;
                }
                target_height -= 1;
            }
        }

        // If no root found at all, return an error
        Err(anyhow!(
            "No state root found for height {} or any previous height",
            height
        ))
    }

    /// Get a node from the database
    pub fn get_node(&self, node_hash: &[u8; 32]) -> Result<Option<SMTNode>> {
        if node_hash == &EMPTY_NODE_HASH {
            return Ok(None);
        }

        let node_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(node_hash)).into_bytes();
        match self
            .storage
            .get_immutable(&node_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            Some(node_data) => Ok(Some(Self::deserialize_node(&node_data)?)),
            None => Ok(None),
        }
    }

    /// Get a leaf node from the SMT
    pub fn get_smt_leaf(&self, root: [u8; 32], key_hash: [u8; 32]) -> Result<Option<SMTNode>> {
        if root == EMPTY_NODE_HASH {
            return Ok(None);
        }

        let mut current_hash = root;
        let mut depth = 0;

        // Traverse the tree to find the leaf
        loop {
            let node = match self.get_node(&current_hash)? {
                Some(n) => n,
                None => return Ok(None),
            };

            match node {
                SMTNode::Leaf { ref key, .. } => {
                    // Check if this is the leaf we're looking for
                    if Self::hash_key(key) == key_hash {
                        return Ok(Some(node.clone()));
                    } else {
                        // Hash collision (extremely unlikely)
                        return Ok(None);
                    }
                }
                SMTNode::Internal {
                    left_child,
                    right_child,
                } => {
                    // Determine which child to follow based on the key_hash bit at current depth
                    let bit = (key_hash[depth / 8] >> (7 - (depth % 8))) & 1;
                    current_hash = if bit == 0 { left_child } else { right_child };
                    depth += 1;

                    if current_hash == EMPTY_NODE_HASH {
                        return Ok(None);
                    }
                }
            }

            // Safety check to prevent infinite loops
            if depth >= 256 {
                return Err(anyhow!("Maximum SMT depth exceeded"));
            }
        }
    }

    /// Collect all nodes along a path from root to leaf
    pub fn collect_path_nodes(
        &self,
        root: [u8; 32],
        key_hash: [u8; 32],
    ) -> Result<Vec<(bool, SMTNode)>> {
        if root == EMPTY_NODE_HASH {
            return Ok(Vec::new());
        }

        let mut path = Vec::new();
        let mut current_hash = root;
        let mut depth = 0;

        // Traverse the tree to collect nodes
        loop {
            let node = match self.get_node(&current_hash)? {
                Some(n) => n,
                None => break,
            };

            match &node {
                SMTNode::Leaf { .. } => {
                    path.push((false, node)); // Bit doesn't matter for leaf
                    break;
                }
                SMTNode::Internal {
                    left_child,
                    right_child,
                } => {
                    // Determine which child to follow based on the key_hash bit at current depth
                    let bit = (key_hash[depth / 8] >> (7 - (depth % 8))) & 1;
                    path.push((bit == 1, node.clone()));

                    current_hash = if bit == 0 { *left_child } else { *right_child };
                    depth += 1;

                    if current_hash == EMPTY_NODE_HASH {
                        break;
                    }
                }
            }

            // Safety check to prevent infinite loops
            if depth >= 256 {
                return Err(anyhow!("Maximum SMT depth exceeded"));
            }
        }

        Ok(path)
    }

    /// Compute updates to the SMT for a key-value pair
    pub fn compute_smt_updates(
        &self,
        key: &[u8],
        value: &[u8],
        current_root: [u8; 32],
        height: u32,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut updates = Vec::new();
        let key_hash = Self::hash_key(key);

        // 1. Store the value with height annotation
        let value_key =
            format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), height).into_bytes();
        let value_hash = Self::hash_value(value);
        updates.push((value_key, value.to_vec()));

        // 2. Create or update the leaf node
        let leaf_key = format!("{}:{}", SMT_LEAF_PREFIX, hex::encode(key_hash)).into_bytes();
        let leaf_node = SMTNode::Leaf {
            key: key.to_vec(),
            value_index: value_hash,
        };
        let leaf_node_serialized = Self::serialize_node(&leaf_node);
        updates.push((leaf_key, leaf_node_serialized));

        // 3. Collect existing nodes along the path
        let path_nodes = self.collect_path_nodes(current_root, key_hash)?;

        // If the tree is empty, create a new root pointing to the leaf
        if path_nodes.is_empty() {
            let leaf_hash = Self::hash_node(&leaf_node);
            let node_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(leaf_hash)).into_bytes();
            updates.push((node_key, Self::serialize_node(&leaf_node)));

            // The new root is the leaf hash
            let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
            updates.push((root_key, leaf_hash.to_vec()));

            return Ok(updates);
        }

        // 4. Update or create nodes along the path
        let mut new_nodes = Vec::new();
        let leaf_hash = Self::hash_node(&leaf_node);

        // Store the leaf node
        let node_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(leaf_hash)).into_bytes();
        updates.push((node_key, Self::serialize_node(&leaf_node)));
        new_nodes.push((256, leaf_hash)); // Depth 256 (maximum) for leaf

        // Process path nodes from leaf to root
        let mut depth = 255;
        for (i, (path_bit, node)) in path_nodes.iter().enumerate().rev() {
            match node {
                SMTNode::Leaf { .. } => {
                    // Skip leaf nodes, we've already created a new one
                    continue;
                }
                SMTNode::Internal {
                    left_child,
                    right_child,
                } => {
                    // Find the child hash at this level
                    let child_hash = if i == path_nodes.len() - 1 {
                        // Last node in path, use the new leaf
                        leaf_hash
                    } else {
                        // Use the previously created node
                        new_nodes.last().unwrap().1
                    };

                    // Create a new internal node with the updated child
                    let new_node = if *path_bit {
                        SMTNode::Internal {
                            left_child: *left_child,
                            right_child: child_hash,
                        }
                    } else {
                        SMTNode::Internal {
                            left_child: child_hash,
                            right_child: *right_child,
                        }
                    };

                    let new_hash = Self::hash_node(&new_node);
                    let node_key =
                        format!("{}:{}", SMT_NODE_PREFIX, hex::encode(new_hash)).into_bytes();
                    updates.push((node_key, Self::serialize_node(&new_node)));

                    new_nodes.push((depth, new_hash));
                    depth -= 1;
                }
            }
        }

        // 5. Update the root
        let new_root = new_nodes.last().unwrap().1;
        let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
        updates.push((root_key, new_root.to_vec()));

        Ok(updates)
    }

    /// Compute the new root after applying updates
    pub fn compute_new_root(
        &self,
        current_root: [u8; 32],
        kvs: &[(Vec<u8>, Vec<u8>)],
        height: u32,
    ) -> Result<[u8; 32]> {
        if kvs.is_empty() {
            return Ok(current_root);
        }

        // For simplicity, we'll recompute the entire path for the last key-value pair
        // In a production implementation, we would compute this incrementally
        let (last_key, last_value) = kvs.last().unwrap();
        let updates = self.compute_smt_updates(last_key, last_value, current_root, height)?;

        // The last update should be the new root
        for (key, value) in updates.iter().rev() {
            if key.starts_with(SMT_ROOT_PREFIX.as_bytes()) {
                let mut root = [0u8; 32];
                root.copy_from_slice(&value);
                return Ok(root);
            }
        }

        // If no root update found, return the current root
        Ok(current_root)
    }

    /// Get a value at a specific height
    pub fn get_value_at_height(
        &self,
        key: &[u8],
        value_hash: [u8; 32],
        height: u32,
    ) -> Result<Option<Vec<u8>>> {
        // Try to get the value at the exact height first
        let exact_key =
            format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), height).into_bytes();
        if let Some(value) = self
            .storage
            .get_immutable(&exact_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            return Ok(Some(value));
        }

        // If not found, find the closest height less than the requested height
        let mut target_height = height;
        while target_height > 0 {
            target_height -= 1;
            let value_key = format!(
                "{}:{}:{}",
                HEIGHT_INDEX_PREFIX,
                hex::encode(key),
                target_height
            )
            .into_bytes();
            if let Some(value) = self
                .storage
                .get_immutable(&value_key)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
            {
                return Ok(Some(value));
            }
        }

        // If still not found, try to get by value hash
        let value_key = format!("{}:{}", "value:", hex::encode(value_hash)).into_bytes();
        match self
            .storage
            .get_immutable(&value_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            Some(value) => Ok(Some(value)),
            None => Ok(None),
        }
    }

    /// Store a key-value pair in the BST with height indexing
    pub fn bst_put(&mut self, key: &[u8], value: &[u8], height: u32) -> Result<()> {
        // Create a batch for all operations
        let mut batch = self.storage.create_batch();

        // 1. Store the current value for O(1) access
        let current_key = format!("{}{}", crate::optimized_bst::CURRENT_VALUE_PREFIX, hex::encode(key)).into_bytes();
        batch.put(&current_key, value);

        // 2. Store the historical value for historical queries
        let historical_key =
            format!("{}{}:{}", crate::optimized_bst::HISTORICAL_VALUE_PREFIX, hex::encode(key), height).into_bytes();
        batch.put(&historical_key, value);

        // 3. Track that this key was updated at this height
        let height_index_key =
            format!("{}{}:{}", crate::optimized_bst::HEIGHT_INDEX_PREFIX, height, hex::encode(key)).into_bytes();
        batch.put(&height_index_key, b"");

        // 4. Track keys updated at this height for reorg handling
        let keys_at_height_key =
            format!("{}{}:{}", crate::optimized_bst::KEYS_AT_HEIGHT_PREFIX, height, hex::encode(key)).into_bytes();
        batch.put(&keys_at_height_key, b"");

        // Write the batch atomically
        self.storage.write_batch(batch)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        Ok(())
    }

    /// Get the value of a key at a specific height using optimized BST
    pub fn bst_get_at_height(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>> {
        // CRITICAL OPTIMIZATION: First check if this key has ANY current value
        // If there's no current value, there's no point in searching historical data
        let current_key = format!("{}{}", crate::optimized_bst::CURRENT_VALUE_PREFIX, hex::encode(key)).into_bytes();
        if let None = self
            .storage
            .get_immutable(&current_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            return Ok(None);
        }

        // First try direct lookup at the exact height
        let exact_key =
            format!("{}{}:{}", crate::optimized_bst::HISTORICAL_VALUE_PREFIX, hex::encode(key), height).into_bytes();
        if let Some(value) = self
            .storage
            .get_immutable(&exact_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            return Ok(Some(value));
        }

        // If not found at exact height, use prefix scan to find the most recent value
        // at or before the requested height
        let prefix = format!("{}{}:", crate::optimized_bst::HISTORICAL_VALUE_PREFIX, hex::encode(key));
        let mut best_height: Option<u32> = None;
        let mut best_value: Option<Vec<u8>> = None;

        for (stored_key, stored_value) in self.storage.scan_prefix(prefix.as_bytes())
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
            let key_str = String::from_utf8_lossy(&stored_key);
            if let Some(height_str) = key_str.strip_prefix(&prefix) {
                if let Ok(stored_height) = height_str.parse::<u32>() {
                    if stored_height <= height {
                        if best_height.is_none() || stored_height > best_height.unwrap() {
                            best_height = Some(stored_height);
                            best_value = Some(stored_value);
                        }
                    }
                }
            }
        }

        Ok(best_value)
    }

    /// Get the current (most recent) value of a key across all heights
    pub fn bst_get_current(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Use the optimized BST current value lookup for O(1) access
        let current_key = format!("{}{}", crate::optimized_bst::CURRENT_VALUE_PREFIX, hex::encode(key)).into_bytes();

        match self
            .storage
            .get_immutable(&current_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            Some(value) => Ok(Some(value)),
            None => Ok(None),
        }
    }

    /// Get all heights at which a key was updated
    pub fn bst_get_heights_for_key(&self, key: &[u8]) -> Result<Vec<u32>> {
        let mut heights = Vec::new();
        let prefix = format!("{}{}:", crate::optimized_bst::HISTORICAL_VALUE_PREFIX, hex::encode(key));

        // Get all keys with this prefix and extract heights
        for (key, _) in self.storage.scan_prefix(prefix.as_bytes())? {
            let key_str = String::from_utf8_lossy(&key);
            if let Some(height_str) = key_str.strip_prefix(&prefix) {
                if let Ok(height) = height_str.parse::<u32>() {
                    heights.push(height);
                }
            }
        }

        heights.sort();
        Ok(heights)
    }

    /// Track that a key was updated at a specific height
    pub fn track_key_at_height(&mut self, key: &[u8], height: u32) -> Result<()> {
        let keys_key =
            format!("{}{}:{}", crate::optimized_bst::KEYS_AT_HEIGHT_PREFIX, height, hex::encode(key)).into_bytes();
        self.storage
            .put(&keys_key, b"")
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?; // Empty value, we just need the key
        Ok(())
    }

    /// Get all keys that were updated at a specific height
    pub fn get_keys_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>> {
        let mut keys = Vec::new();
        let prefix = format!("{}{}:", crate::optimized_bst::KEYS_AT_HEIGHT_PREFIX, height);

        // Get all keys with this prefix
        for (key, _) in self.storage.scan_prefix(prefix.as_bytes())? {
            let key_str = String::from_utf8_lossy(&key);
            if let Some(hex_key) = key_str.strip_prefix(&prefix) {
                if let Ok(original_key) = hex::decode(hex_key) {
                    keys.push(original_key);
                }
            }
        }

        Ok(keys)
    }

    /// Rollback a key to its state before a specific height
    pub fn bst_rollback_key(&mut self, key: &[u8], target_height: u32) -> Result<()> {
        let heights = self.bst_get_heights_for_key(key)?;

        // Remove all entries at heights greater than target_height
        for height in heights {
            if height > target_height {
                let height_key =
                    format!("{}{}:{}", crate::optimized_bst::HISTORICAL_VALUE_PREFIX, hex::encode(key), height).into_bytes();
                self.storage
                    .delete(&height_key)
                    .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

                // Also remove from keys-at-height tracking
                let keys_key = format!("{}{}:{}", crate::optimized_bst::KEYS_AT_HEIGHT_PREFIX, height, hex::encode(key))
                    .into_bytes();
                self.storage
                    .delete(&keys_key)
                    .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
            }
        }

        Ok(())
    }

    /// Rollback all keys to their state before a specific height
    pub fn bst_rollback_to_height(&mut self, target_height: u32) -> Result<()> {
        // Get all heights greater than target_height that have updates
        let mut heights_to_rollback = Vec::new();
        let prefix = format!("{}:", crate::optimized_bst::KEYS_AT_HEIGHT_PREFIX);

        // Get all keys with this prefix and extract heights
        for (key, _) in self.storage.scan_prefix(prefix.as_bytes())? {
            let key_str = String::from_utf8_lossy(&key);
            if let Some(rest) = key_str.strip_prefix(&prefix) {
                if let Some(colon_pos) = rest.find(':') {
                    let height_str = &rest[..colon_pos];
                    if let Ok(height) = height_str.parse::<u32>() {
                        if height > target_height {
                            heights_to_rollback.push(height);
                        }
                    }
                }
            }
        }

        // Remove duplicates and sort
        heights_to_rollback.sort();
        heights_to_rollback.dedup();

        // Rollback each height
        for height in heights_to_rollback {
            let keys = self.get_keys_at_height(height)?;
            for key in keys {
                self.bst_rollback_key(&key, target_height)?;
            }
        }

        Ok(())
    }

    /// Iterate backwards through all values of a key from most recent
    pub fn bst_iterate_backwards(
        &self,
        key: &[u8],
        from_height: u32,
    ) -> Result<Vec<(u32, Vec<u8>)>> {
        let heights = self.bst_get_heights_for_key(key)?;
        let mut results = Vec::new();

        // Filter heights to only include those <= from_height and sort in descending order
        let mut filtered_heights: Vec<u32> =
            heights.into_iter().filter(|&h| h <= from_height).collect();
        filtered_heights.sort_by(|a, b| b.cmp(a)); // Descending order

        for height in filtered_heights {
            let height_key =
                format!("{}{}:{}", crate::optimized_bst::HISTORICAL_VALUE_PREFIX, hex::encode(key), height).into_bytes();
            if let Some(value) = self
                .storage
                .get_immutable(&height_key)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
            {
                results.push((height, value));
            }
        }

        Ok(results)
    }

    /// Calculate and store the SMT state root for a specific height using incremental updates
    pub fn calculate_and_store_state_root(&mut self, height: u32) -> Result<[u8; 32]> {
        let prev_root = if height > 0 {
            // For heights > 0, get the previous state root
            match self.get_smt_root_at_height(height - 1) {
                Ok(root) => root,
                Err(_) => EMPTY_NODE_HASH, // If no previous root exists, start with empty
            }
        } else {
            // For height 0, start with empty root
            EMPTY_NODE_HASH
        };

        // Get all keys that were updated at this height
        let updated_keys = self.get_keys_at_height(height)?;

        if updated_keys.is_empty() {
            // No updates at this height, return previous root
            let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
            self.storage
                .put(&root_key, &prev_root)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
            return Ok(prev_root);
        }

        // Use incremental SMT updates instead of full state enumeration
        let new_root = self.compute_incremental_smt_root(prev_root, &updated_keys, height)?;

        // Store the new root
        let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
        self.storage
            .put(&root_key, &new_root)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        Ok(new_root)
    }

    /// Optimized batch calculation of state root for multiple keys
    pub fn calculate_and_store_state_root_batched(
        &mut self,
        height: u32,
        updated_keys: &[Vec<u8>],
    ) -> Result<[u8; 32]> {
        let prev_root = if height > 0 {
            match self.get_smt_root_at_height(height - 1) {
                Ok(root) => root,
                Err(_) => EMPTY_NODE_HASH,
            }
        } else {
            EMPTY_NODE_HASH
        };

        if updated_keys.is_empty() {
            let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
            self.storage.put(&root_key, &prev_root)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
            return Ok(prev_root);
        }

        // Create a batch for all operations
        let mut batch = self.storage.create_batch();
        
        // Get all values in batch
        let mut key_values = Vec::new();
        for key in updated_keys {
            if let Some(value) = self.bst_get_at_height(key, height)? {
                key_values.push((key.clone(), value));
            }
        }

        // Process all updates in a single pass
        let new_root = self.compute_batched_smt_root(
            prev_root,
            &key_values,
            height,
            &mut batch,
        )?;

        // Store the new root in batch
        let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
        batch.put(root_key, new_root.to_vec());

        // Write entire batch at once
        self.storage.write(batch)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        Ok(new_root)
    }

    /// Compute SMT root for multiple keys in batch
    fn compute_batched_smt_root(
        &mut self,
        current_root: [u8; 32],
        key_values: &[(Vec<u8>, Vec<u8>)],
        height: u32,
        batch: &mut T::Batch,
    ) -> Result<[u8; 32]> {
        let mut working_root = current_root;

        // Process all keys in batch
        for (key, value) in key_values {
            working_root = self.update_smt_for_key_batched(
                working_root,
                key,
                value,
                height,
                batch,
            )?;
        }

        Ok(working_root)
    }

    /// Update SMT for a single key using batch operations
    fn update_smt_for_key_batched(
        &mut self,
        current_root: [u8; 32],
        key: &[u8],
        value: &[u8],
        height: u32,
        batch: &mut T::Batch,
    ) -> Result<[u8; 32]> {
        let key_hash = Self::hash_key(key);
        let value_hash = Self::hash_value(value);

        // Create the new leaf node
        let new_leaf = SMTNode::Leaf {
            key: key.to_vec(),
            value_index: value_hash,
        };
        let new_leaf_hash = Self::hash_node(&new_leaf);

        // Add to batch instead of immediate storage
        let leaf_node_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(new_leaf_hash)).into_bytes();
        batch.put(leaf_node_key, Self::serialize_node(&new_leaf));

        let value_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), height).into_bytes();
        batch.put(value_key, value.to_vec());

        if current_root == EMPTY_NODE_HASH {
            return Ok(new_leaf_hash);
        }

        // Compute path updates using batch operations
        let path_updates = self.compute_path_updates_batched(
            current_root,
            key_hash,
            new_leaf_hash,
            batch,
        )?;

        if let Some((_, new_root_hash)) = path_updates.last() {
            Ok(*new_root_hash)
        } else {
            Ok(new_leaf_hash)
        }
    }

    /// Compute path updates using batch operations
    fn compute_path_updates_batched(
        &mut self,
        current_root: [u8; 32],
        key_hash: [u8; 32],
        new_leaf_hash: [u8; 32],
        batch: &mut T::Batch,
    ) -> Result<Vec<(usize, [u8; 32])>> {
        let mut updates = Vec::new();
        let mut current_hash = current_root;
        let mut depth = 0;
        let mut path_nodes = Vec::new();

        // Traverse down
        loop {
            let node = match self.get_node(&current_hash)? {
                Some(n) => n,
                None => break,
            };

            match node {
                SMTNode::Leaf { key: ref existing_key, .. } => {
                    let existing_key_hash = Self::hash_key(existing_key);
                    
                    if existing_key_hash == key_hash {
                        path_nodes.push((depth, node, true));
                        break;
                    } else {
                        path_nodes.push((depth, node, false));
                        break;
                    }
                }
                SMTNode::Internal { left_child, right_child } => {
                    path_nodes.push((depth, node, false));
                    
                    let bit = (key_hash[depth / 8] >> (7 - (depth % 8))) & 1;
                    current_hash = if bit == 0 { left_child } else { right_child };
                    depth += 1;

                    if depth >= 256 {
                        return Err(anyhow!("Maximum SMT depth exceeded"));
                    }
                }
            }
        }

        // Build new nodes and add to batch
        let mut new_child_hash = new_leaf_hash;

        for (node_depth, node, is_replacement) in path_nodes.into_iter().rev() {
            match node {
                SMTNode::Leaf { key: existing_key, value_index } => {
                    if is_replacement {
                        updates.push((node_depth, new_child_hash));
                    } else {
                        let existing_key_hash = Self::hash_key(&existing_key);
                        let existing_leaf_hash = Self::hash_node(&SMTNode::Leaf {
                            key: existing_key,
                            value_index,
                        });

                        new_child_hash = self.create_separating_internals_batched(
                            node_depth,
                            existing_key_hash,
                            existing_leaf_hash,
                            key_hash,
                            new_child_hash,
                            batch,
                        )?;
                        updates.push((node_depth, new_child_hash));
                    }
                }
                SMTNode::Internal { left_child, right_child } => {
                    let bit = (key_hash[node_depth / 8] >> (7 - (node_depth % 8))) & 1;
                    let new_internal = if bit == 0 {
                        SMTNode::Internal {
                            left_child: new_child_hash,
                            right_child,
                        }
                    } else {
                        SMTNode::Internal {
                            left_child,
                            right_child: new_child_hash,
                        }
                    };

                    let new_internal_hash = Self::hash_node(&new_internal);
                    let internal_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(new_internal_hash)).into_bytes();
                    batch.put(internal_key, Self::serialize_node(&new_internal));

                    updates.push((node_depth, new_internal_hash));
                    new_child_hash = new_internal_hash;
                }
            }
        }

        Ok(updates)
    }

    /// Create separating internals using batch operations
    fn create_separating_internals_batched(
        &mut self,
        start_depth: usize,
        existing_key_hash: [u8; 32],
        existing_leaf_hash: [u8; 32],
        new_key_hash: [u8; 32],
        new_leaf_hash: [u8; 32],
        batch: &mut T::Batch,
    ) -> Result<[u8; 32]> {
        let mut depth = start_depth;
        let mut left_hash = EMPTY_NODE_HASH;
        let mut right_hash = EMPTY_NODE_HASH;

        // Find divergence point
        while depth < 256 {
            let existing_bit = (existing_key_hash[depth / 8] >> (7 - (depth % 8))) & 1;
            let new_bit = (new_key_hash[depth / 8] >> (7 - (depth % 8))) & 1;

            if existing_bit != new_bit {
                if existing_bit == 0 {
                    left_hash = existing_leaf_hash;
                    right_hash = new_leaf_hash;
                } else {
                    left_hash = new_leaf_hash;
                    right_hash = existing_leaf_hash;
                }
                break;
            }
            depth += 1;
        }

        if depth >= 256 {
            return Err(anyhow!("Keys are identical - cannot separate"));
        }

        // Create internal node at divergence
        let internal = SMTNode::Internal {
            left_child: left_hash,
            right_child: right_hash,
        };
        let internal_hash = Self::hash_node(&internal);
        let internal_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(internal_hash)).into_bytes();
        batch.put(internal_key, Self::serialize_node(&internal));

        // Create parent internals if needed
        let mut current_hash = internal_hash;
        for d in (start_depth..depth).rev() {
            let bit = (new_key_hash[d / 8] >> (7 - (d % 8))) & 1;
            let parent_internal = if bit == 0 {
                SMTNode::Internal {
                    left_child: current_hash,
                    right_child: EMPTY_NODE_HASH,
                }
            } else {
                SMTNode::Internal {
                    left_child: EMPTY_NODE_HASH,
                    right_child: current_hash,
                }
            };

            let parent_hash = Self::hash_node(&parent_internal);
            let parent_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(parent_hash)).into_bytes();
            batch.put(parent_key, Self::serialize_node(&parent_internal));

            current_hash = parent_hash;
        }

        Ok(current_hash)
    }

    /// Compute SMT root incrementally by only updating affected paths
    fn compute_incremental_smt_root(
        &mut self,
        current_root: [u8; 32],
        updated_keys: &[Vec<u8>],
        height: u32,
    ) -> Result<[u8; 32]> {
        let mut working_root = current_root;

        // Process each updated key individually
        for key in updated_keys {
            // Get the current value for this key at this height
            let value = match self.bst_get_at_height(key, height)? {
                Some(v) => v,
                None => continue, // Skip if no value found
            };

            // Update the SMT for this single key-value pair
            working_root = self.update_smt_for_key(working_root, key, &value, height)?;
        }

        Ok(working_root)
    }

    /// Update the SMT for a single key-value pair, returning the new root
    fn update_smt_for_key(
        &mut self,
        current_root: [u8; 32],
        key: &[u8],
        value: &[u8],
        height: u32,
    ) -> Result<[u8; 32]> {
        let key_hash = Self::hash_key(key);
        let value_hash = Self::hash_value(value);

        // Create the new leaf node
        let new_leaf = SMTNode::Leaf {
            key: key.to_vec(),
            value_index: value_hash,
        };
        let new_leaf_hash = Self::hash_node(&new_leaf);

        // Store the leaf node
        let leaf_node_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(new_leaf_hash)).into_bytes();
        self.storage
            .put(&leaf_node_key, &Self::serialize_node(&new_leaf))
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        // Store the value with height annotation
        let value_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), height).into_bytes();
        self.storage
            .put(&value_key, value)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        // If the tree is empty, the new leaf becomes the root
        if current_root == EMPTY_NODE_HASH {
            return Ok(new_leaf_hash);
        }

        // Find the path to insert/update this key
        let path_updates = self.compute_path_updates(current_root, key_hash, new_leaf_hash)?;

        // Apply the path updates and return the new root
        if let Some((_, new_root_hash)) = path_updates.last() {
            Ok(*new_root_hash)
        } else {
            Ok(new_leaf_hash)
        }
    }

    /// Compute the minimal set of node updates needed for a key insertion/update
    fn compute_path_updates(
        &mut self,
        current_root: [u8; 32],
        key_hash: [u8; 32],
        new_leaf_hash: [u8; 32],
    ) -> Result<Vec<(usize, [u8; 32])>> {
        let mut updates = Vec::new();
        let mut current_hash = current_root;
        let mut depth = 0;
        let mut path_nodes = Vec::new();

        // Traverse down to find the insertion point
        loop {
            let node = match self.get_node(&current_hash)? {
                Some(n) => n,
                None => break, // Empty subtree, insert here
            };

            match node {
                SMTNode::Leaf { key: ref existing_key, .. } => {
                    let existing_key_hash = Self::hash_key(existing_key);
                    
                    if existing_key_hash == key_hash {
                        // Replacing existing leaf - path ends here
                        path_nodes.push((depth, node, true)); // true = replace
                        break;
                    } else {
                        // Need to create internal nodes to separate the keys
                        path_nodes.push((depth, node, false)); // false = split
                        break;
                    }
                }
                SMTNode::Internal { left_child, right_child } => {
                    path_nodes.push((depth, node, false));
                    
                    // Determine which child to follow
                    let bit = (key_hash[depth / 8] >> (7 - (depth % 8))) & 1;
                    current_hash = if bit == 0 { left_child } else { right_child };
                    depth += 1;

                    if depth >= 256 {
                        return Err(anyhow!("Maximum SMT depth exceeded"));
                    }
                }
            }
        }

        // Now build the new nodes from bottom up
        let mut new_child_hash = new_leaf_hash;

        // Process path nodes in reverse order (bottom up)
        for (node_depth, node, is_replacement) in path_nodes.into_iter().rev() {
            match node {
                SMTNode::Leaf { key: existing_key, value_index } => {
                    if is_replacement {
                        // Simple replacement - new leaf becomes the child
                        updates.push((node_depth, new_child_hash));
                        new_child_hash = new_child_hash;
                    } else {
                        // Need to split - create internal nodes
                        let existing_key_hash = Self::hash_key(&existing_key);
                        let existing_leaf_hash = Self::hash_node(&SMTNode::Leaf {
                            key: existing_key,
                            value_index,
                        });

                        // Create internal nodes to separate the keys
                        new_child_hash = self.create_separating_internals(
                            node_depth,
                            existing_key_hash,
                            existing_leaf_hash,
                            key_hash,
                            new_child_hash,
                        )?;
                        updates.push((node_depth, new_child_hash));
                    }
                }
                SMTNode::Internal { left_child, right_child } => {
                    // Create new internal node with updated child
                    let bit = (key_hash[node_depth / 8] >> (7 - (node_depth % 8))) & 1;
                    let new_internal = if bit == 0 {
                        SMTNode::Internal {
                            left_child: new_child_hash,
                            right_child,
                        }
                    } else {
                        SMTNode::Internal {
                            left_child,
                            right_child: new_child_hash,
                        }
                    };

                    let new_internal_hash = Self::hash_node(&new_internal);
                    let internal_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(new_internal_hash)).into_bytes();
                    self.storage
                        .put(&internal_key, &Self::serialize_node(&new_internal))
                        .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

                    updates.push((node_depth, new_internal_hash));
                    new_child_hash = new_internal_hash;
                }
            }
        }

        Ok(updates)
    }

    /// Create internal nodes to separate two leaf nodes with different key hashes
    fn create_separating_internals(
        &mut self,
        start_depth: usize,
        existing_key_hash: [u8; 32],
        existing_leaf_hash: [u8; 32],
        new_key_hash: [u8; 32],
        new_leaf_hash: [u8; 32],
    ) -> Result<[u8; 32]> {
        let mut depth = start_depth;
        let mut left_hash = EMPTY_NODE_HASH;
        let mut right_hash = EMPTY_NODE_HASH;

        // Find the first bit where the keys differ
        while depth < 256 {
            let existing_bit = (existing_key_hash[depth / 8] >> (7 - (depth % 8))) & 1;
            let new_bit = (new_key_hash[depth / 8] >> (7 - (depth % 8))) & 1;

            if existing_bit != new_bit {
                // Keys diverge here - place the leaves
                if existing_bit == 0 {
                    left_hash = existing_leaf_hash;
                    right_hash = new_leaf_hash;
                } else {
                    left_hash = new_leaf_hash;
                    right_hash = existing_leaf_hash;
                }
                break;
            }
            depth += 1;
        }

        if depth >= 256 {
            return Err(anyhow!("Keys are identical - cannot separate"));
        }

        // Create the internal node at the divergence point
        let internal = SMTNode::Internal {
            left_child: left_hash,
            right_child: right_hash,
        };
        let internal_hash = Self::hash_node(&internal);
        let internal_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(internal_hash)).into_bytes();
        self.storage
            .put(&internal_key, &Self::serialize_node(&internal))
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        // If we need more internal nodes above this point, create them
        let mut current_hash = internal_hash;
        for d in (start_depth..depth).rev() {
            let bit = (new_key_hash[d / 8] >> (7 - (d % 8))) & 1;
            let parent_internal = if bit == 0 {
                SMTNode::Internal {
                    left_child: current_hash,
                    right_child: EMPTY_NODE_HASH,
                }
            } else {
                SMTNode::Internal {
                    left_child: EMPTY_NODE_HASH,
                    right_child: current_hash,
                }
            };

            let parent_hash = Self::hash_node(&parent_internal);
            let parent_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(parent_hash)).into_bytes();
            self.storage
                .put(&parent_key, &Self::serialize_node(&parent_internal))
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

            current_hash = parent_hash;
        }

        Ok(current_hash)
    }

    /// Compute SMT root from a complete state map
    ///
    /// WARNING: This method is deprecated and should only be used for testing or migration.
    /// It enumerates the entire state which is inefficient for large databases.
    /// Use compute_incremental_smt_root() instead for production workloads.
    fn compute_smt_root_from_state(&self, state: &BTreeMap<Vec<u8>, Vec<u8>>) -> Result<[u8; 32]> {
        if state.is_empty() {
            return Ok(EMPTY_NODE_HASH);
        }

        // This is a simplified hash-based approach for compatibility
        // A proper SMT would build the actual tree structure, but that would be
        // even more expensive for large state maps
        let mut hasher = Sha256::new();

        // Add a salt to ensure we never get all zeros for non-empty state
        hasher.update(b"metashrew_state_root_v1");

        for (key, value) in state.iter() {
            // Hash key length + key + value length + value for deterministic ordering
            hasher.update(&(key.len() as u32).to_le_bytes());
            hasher.update(key);
            hasher.update(&(value.len() as u32).to_le_bytes());
            hasher.update(value);
        }

        let result = hasher.finalize().into();
        Ok(result)
    }

    /// Get the current state root (most recent)
    pub fn get_current_state_root(&self) -> Result<[u8; 32]> {
        // Find the highest height with a stored root
        let prefix = SMT_ROOT_PREFIX.to_string();

        // Get all keys with this prefix and find the highest height
        let mut highest_height = None;
        let mut highest_root = None;

        for (key, value) in self.storage.scan_prefix(prefix.as_bytes())? {
            let key_str = String::from_utf8_lossy(&key);
            if let Some(height_str) = key_str.strip_prefix(&prefix) {
                if let Ok(height) = height_str.parse::<u32>() {
                    if highest_height.is_none() || height > highest_height.unwrap() {
                        if value.len() == 32 {
                            highest_height = Some(height);
                            highest_root = Some(value);
                        }
                    }
                }
            }
        }

        if let Some(root_data) = highest_root {
            let mut root = [0u8; 32];
            root.copy_from_slice(&root_data);
            return Ok(root);
        }

        Ok(EMPTY_NODE_HASH)
    }
}

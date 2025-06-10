//! Sparse Merkle tree implementation for state verification
//!
//! This module provides a sparse Merkle tree implementation that can be used
//! to efficiently verify the integrity of the state.

use std::collections::HashMap;
use blake2::{Blake2b512, Digest};
use rocksdb::{ColumnFamily, WriteBatch};

/// Size of hash in bytes (64 bytes for Blake2b512)
pub const HASH_SIZE: usize = 64;

/// Maximum number of nodes to keep in memory
const MAX_CACHE_SIZE: usize = 10_000;

/// Hash type used for the Merkle tree
pub type Hash = [u8; HASH_SIZE];

/// Node in the sparse Merkle tree
#[derive(Debug, Clone)]
pub enum Node {
    /// Internal node with left and right children
    Internal { left: Hash, right: Hash },
    /// Leaf node with key and value hash
    Leaf { key: Vec<u8>, value_hash: Hash },
    /// Empty node (default hash)
    Empty,
}

/// Sparse Merkle tree for efficient state verification
pub struct SparseMerkleTree {
    /// Root hash of the tree
    root: Hash,
    /// In-memory cache of nodes
    nodes: HashMap<Hash, Node>,
    /// Pending updates to be written to the database
    pending_updates: HashMap<Vec<u8>, Option<Vec<u8>>>,
}

impl SparseMerkleTree {
    /// Create a new empty sparse Merkle tree
    pub fn new() -> Self {
        Self {
            root: [0; HASH_SIZE], // Empty tree root
            nodes: HashMap::new(),
            pending_updates: HashMap::new(),
        }
    }
    
    /// Get the current root hash
    pub fn root(&self) -> Hash {
        self.root
    }
    
    /// Update a key-value pair in the tree
    pub fn update(&mut self, key: &[u8], value: &[u8]) {
        // Convert key to a path in the tree
        let path = self.key_to_path(key);
        
        // Hash the value
        let value_hash = self.hash_value(value);
        
        // Update the tree
        self.root = self.update_node(self.root, &path, 0, key.to_vec(), value_hash);
        
        // Record the update
        self.pending_updates.insert(key.to_vec(), Some(value.to_vec()));
    }
    
    /// Delete a key from the tree
    pub fn delete(&mut self, key: &[u8]) {
        // Convert key to a path in the tree
        let path = self.key_to_path(key);
        
        // Delete from the tree
        self.root = self.delete_node(self.root, &path, 0);
        
        // Record the deletion
        self.pending_updates.insert(key.to_vec(), None);
    }
    
    /// Write pending updates to a RocksDB batch
    pub fn write_updates_to_batch(&mut self, batch: &mut WriteBatch, cf: &ColumnFamily) {
        // Write the new root
        batch.put_cf(cf, "merkle_root", &self.root);
        
        // Write all new or updated nodes
        for (node_hash, node) in &self.nodes {
            match node {
                Node::Internal { left, right } => {
                    let mut data = Vec::with_capacity(2 * HASH_SIZE);
                    data.extend_from_slice(left);
                    data.extend_from_slice(right);
                    batch.put_cf(cf, node_hash, &data);
                },
                Node::Leaf { key, value_hash } => {
                    let mut data = Vec::with_capacity(key.len() + HASH_SIZE + 4);
                    data.extend_from_slice(&(key.len() as u32).to_le_bytes());
                    data.extend_from_slice(key);
                    data.extend_from_slice(value_hash);
                    batch.put_cf(cf, node_hash, &data);
                },
                Node::Empty => {
                    // Empty nodes are not stored
                }
            }
        }
        
        // Clear pending updates
        self.pending_updates.clear();
        
        // Clear the in-memory node cache if it's getting too large
        if self.nodes.len() > MAX_CACHE_SIZE {
            self.nodes.clear();
        }
    }
    
    /// Load the tree from the database
    pub fn load_from_db(db: &rocksdb::DB, cf: &ColumnFamily) -> Result<Self, rocksdb::Error> {
        let root_bytes = match db.get_cf(cf, "merkle_root")? {
            Some(bytes) => {
                let mut root = [0; HASH_SIZE];
                if bytes.len() == HASH_SIZE {
                    root.copy_from_slice(&bytes);
                    root
                } else {
                    [0; HASH_SIZE] // Default root if invalid
                }
            },
            None => [0; HASH_SIZE], // Default root if not found
        };
        
        Ok(Self {
            root,
            nodes: HashMap::new(),
            pending_updates: HashMap::new(),
        })
    }
    
    /// Generate a Merkle proof for a key
    pub fn generate_proof(&self, key: &[u8]) -> Vec<Hash> {
        let path = self.key_to_path(key);
        let mut proof = Vec::new();
        self.generate_proof_internal(self.root, &path, 0, &mut proof);
        proof
    }
    
    /// Verify a Merkle proof for a key-value pair
    pub fn verify_proof(key: &[u8], value: &[u8], proof: &[Hash], root: &Hash) -> bool {
        let tree = SparseMerkleTree::new();
        let path = tree.key_to_path(key);
        let value_hash = tree.hash_value(value);
        
        tree.verify_proof_internal(key, &value_hash, &path, 0, proof, 0, root)
    }
    
    // Private helper methods
    
    /// Convert a key to a path in the tree
    fn key_to_path(&self, key: &[u8]) -> Vec<bool> {
        let key_hash = self.hash_key(key);
        let mut path = Vec::with_capacity(256); // 256 bits for the path
        
        for byte in key_hash {
            for i in (0..8).rev() {
                path.push((byte >> i) & 1 == 1);
            }
        }
        
        path
    }
    
    /// Hash a key
    fn hash_key(&self, key: &[u8]) -> [u8; 32] {
        let mut hasher = Blake2b512::new();
        hasher.update(key);
        let result = hasher.finalize();
        
        let mut key_hash = [0; 32];
        key_hash.copy_from_slice(&result[0..32]);
        key_hash
    }
    
    /// Hash a value
    fn hash_value(&self, value: &[u8]) -> Hash {
        let mut hasher = Blake2b512::new();
        hasher.update(value);
        let result = hasher.finalize();
        
        let mut value_hash = [0; HASH_SIZE];
        value_hash.copy_from_slice(&result);
        value_hash
    }
    
    /// Hash two child nodes
    fn hash_children(&self, left: &Hash, right: &Hash) -> Hash {
        let mut hasher = Blake2b512::new();
        hasher.update(left);
        hasher.update(right);
        let result = hasher.finalize();
        
        let mut node_hash = [0; HASH_SIZE];
        node_hash.copy_from_slice(&result);
        node_hash
    }
    
    /// Update a node in the tree
    fn update_node(&mut self, node_hash: Hash, path: &[bool], depth: usize, key: Vec<u8>, value_hash: Hash) -> Hash {
        if depth == path.len() {
            // We've reached the end of the path, create a leaf node
            let leaf = Node::Leaf { key, value_hash };
            let leaf_hash = self.hash_node(&leaf);
            self.nodes.insert(leaf_hash, leaf);
            return leaf_hash;
        }
        
        let (left_hash, right_hash) = match self.get_node(&node_hash) {
            Node::Internal { left, right } => (left, right),
            Node::Leaf { .. } => {
                // Replace leaf with internal node
                ([0; HASH_SIZE], [0; HASH_SIZE])
            },
            Node::Empty => {
                // Create new internal node
                ([0; HASH_SIZE], [0; HASH_SIZE])
            },
        };
        
        // Recursively update the appropriate child
        let (new_left, new_right) = if path[depth] {
            // Right path
            (left_hash, self.update_node(right_hash, path, depth + 1, key, value_hash))
        } else {
            // Left path
            (self.update_node(left_hash, path, depth + 1, key, value_hash), right_hash)
        };
        
        // Create new internal node
        let internal = Node::Internal { left: new_left, right: new_right };
        let internal_hash = self.hash_node(&internal);
        self.nodes.insert(internal_hash, internal);
        
        internal_hash
    }
    
    /// Delete a node from the tree
    fn delete_node(&mut self, node_hash: Hash, path: &[bool], depth: usize) -> Hash {
        match self.get_node(&node_hash) {
            Node::Empty => {
                // Nothing to delete
                node_hash
            },
            Node::Leaf { key, .. } => {
                // Check if this is the leaf we want to delete
                let leaf_path = self.key_to_path(&key);
                if leaf_path[0..depth] == path[0..depth] {
                    // This is the leaf to delete
                    [0; HASH_SIZE] // Return empty hash
                } else {
                    // Not the leaf we're looking for
                    node_hash
                }
            },
            Node::Internal { left, right } => {
                // Recursively delete from the appropriate child
                let (new_left, new_right) = if path[depth] {
                    // Right path
                    (left, self.delete_node(right, path, depth + 1))
                } else {
                    // Left path
                    (self.delete_node(left, path, depth + 1), right)
                };
                
                // Check if both children are empty
                if new_left == [0; HASH_SIZE] && new_right == [0; HASH_SIZE] {
                    // Both children are empty, return empty hash
                    [0; HASH_SIZE]
                } else {
                    // Create new internal node
                    let internal = Node::Internal { left: new_left, right: new_right };
                    let internal_hash = self.hash_node(&internal);
                    self.nodes.insert(internal_hash, internal);
                    internal_hash
                }
            },
        }
    }
    
    /// Get a node from the tree
    fn get_node(&self, node_hash: &Hash) -> Node {
        if *node_hash == [0; HASH_SIZE] {
            return Node::Empty;
        }
        
        self.nodes.get(node_hash).cloned().unwrap_or(Node::Empty)
    }
    
    /// Hash a node
    fn hash_node(&self, node: &Node) -> Hash {
        match node {
            Node::Internal { left, right } => {
                self.hash_children(left, right)
            },
            Node::Leaf { key, value_hash } => {
                let mut hasher = Blake2b512::new();
                hasher.update(b"leaf");
                hasher.update(key);
                hasher.update(value_hash);
                let result = hasher.finalize();
                
                let mut node_hash = [0; HASH_SIZE];
                node_hash.copy_from_slice(&result);
                node_hash
            },
            Node::Empty => {
                [0; HASH_SIZE]
            },
        }
    }
    
    /// Generate a Merkle proof for a key (internal implementation)
    fn generate_proof_internal(&self, node_hash: Hash, path: &[bool], depth: usize, proof: &mut Vec<Hash>) {
        if depth >= path.len() || node_hash == [0; HASH_SIZE] {
            return;
        }
        
        match self.get_node(&node_hash) {
            Node::Internal { left, right } => {
                if path[depth] {
                    // Going right, include left sibling in proof
                    proof.push(left);
                    self.generate_proof_internal(right, path, depth + 1, proof);
                } else {
                    // Going left, include right sibling in proof
                    proof.push(right);
                    self.generate_proof_internal(left, path, depth + 1, proof);
                }
            },
            _ => {
                // Leaf or empty node, nothing to add to proof
            },
        }
    }
    
    /// Verify a Merkle proof for a key-value pair (internal implementation)
    fn verify_proof_internal(
        &self,
        key: &[u8],
        value_hash: &Hash,
        path: &[bool],
        depth: usize,
        proof: &[Hash],
        proof_index: usize,
        expected_root: &Hash,
    ) -> bool {
        if depth == path.len() {
            // We've reached the end of the path, create a leaf node
            let leaf = Node::Leaf { key: key.to_vec(), value_hash: *value_hash };
            let leaf_hash = self.hash_node(&leaf);
            return leaf_hash == *expected_root;
        }
        
        if proof_index >= proof.len() {
            return false;
        }
        
        let sibling_hash = proof[proof_index];
        
        // Compute the parent hash
        let (left_hash, right_hash) = if path[depth] {
            // We're going right, sibling is on the left
            (sibling_hash, self.verify_proof_internal(key, value_hash, path, depth + 1, proof, proof_index + 1, expected_root)?)
        } else {
            // We're going left, sibling is on the right
            (self.verify_proof_internal(key, value_hash, path, depth + 1, proof, proof_index + 1, expected_root)?, sibling_hash)
        };
        
        let parent_hash = self.hash_children(&left_hash, &right_hash);
        parent_hash == *expected_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_empty_tree() {
        let tree = SparseMerkleTree::new();
        assert_eq!(tree.root(), [0; HASH_SIZE]);
    }
    
    #[test]
    fn test_insert_and_delete() {
        let mut tree = SparseMerkleTree::new();
        
        // Insert a key-value pair
        let key = b"test_key";
        let value = b"test_value";
        tree.update(key, value);
        
        let root_after_insert = tree.root();
        assert_ne!(root_after_insert, [0; HASH_SIZE]);
        
        // Delete the key
        tree.delete(key);
        
        // Root should be back to empty
        assert_eq!(tree.root(), [0; HASH_SIZE]);
    }
    
    #[test]
    fn test_proof_verification() {
        let mut tree = SparseMerkleTree::new();
        
        // Insert some key-value pairs
        let key1 = b"key1";
        let value1 = b"value1";
        tree.update(key1, value1);
        
        let key2 = b"key2";
        let value2 = b"value2";
        tree.update(key2, value2);
        
        let key3 = b"key3";
        let value3 = b"value3";
        tree.update(key3, value3);
        
        // Generate a proof for key2
        let proof = tree.generate_proof(key2);
        let root = tree.root();
        
        // Verify the proof
        assert!(SparseMerkleTree::verify_proof(key2, value2, &proof, &root));
        
        // Verify that the proof fails with wrong value
        assert!(!SparseMerkleTree::verify_proof(key2, b"wrong_value", &proof, &root));
        
        // Verify that the proof fails with wrong key
        assert!(!SparseMerkleTree::verify_proof(b"wrong_key", value2, &proof, &root));
    }
}
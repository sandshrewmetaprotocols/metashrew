//! Merkleized RocksDB adapter for fast sync
//!
//! This module provides a RocksDB adapter that maintains a sparse Merkle tree
//! alongside the database for state verification.

use crate::RocksDBRuntimeAdapter;
use runtime::{KeyValueStoreLike, BatchLike};
use std::sync::Arc;
use std::path::{Path, PathBuf};
use rocksdb::{DB, ColumnFamily, Options, WriteBatch};
use log::{info, warn, debug, error};

/// Size of hash in bytes (64 bytes for Blake2b512)
pub const HASH_SIZE: usize = 64;

/// Hash type used for the Merkle tree
pub type Hash = [u8; HASH_SIZE];

/// Merkleized RocksDB adapter
#[derive(Clone)]
pub struct MerkleizedRocksDBRuntimeAdapter {
    /// The underlying RocksDB instance
    pub db: Arc<DB>,
    /// Current block height
    pub height: u32,
    /// Sparse Merkle tree for state verification
    merkle_tree: SparseMerkleTree,
    /// Special column family for storing Merkle tree nodes
    merkle_cf: Arc<ColumnFamily>,
}

/// Batch operations for the merkleized adapter
pub struct MerkleizedRocksDBBatch {
    /// Operations to perform
    pub operations: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

impl MerkleizedRocksDBRuntimeAdapter {
    /// Create a new merkleized RocksDB adapter
    pub fn new(db_path: &Path) -> Result<Self, rocksdb::Error> {
        // Create column families including a special one for the Merkle tree
        let mut cf_opts = Options::default();
        cf_opts.create_if_missing(true);
        
        // Open the database with the merkle_tree column family
        let db = if DB::list_cf(&Options::default(), db_path)?.contains(&"merkle_tree".to_string()) {
            DB::open_cf(&Options::default(), db_path, ["default", "merkle_tree"])?
        } else {
            // Create the column family if it doesn't exist
            let db = DB::open_cf(&Options::default(), db_path, ["default"])?;
            db.create_cf("merkle_tree", &cf_opts)?;
            DB::open_cf(&Options::default(), db_path, ["default", "merkle_tree"])?
        };
        
        let merkle_cf = Arc::new(db.cf_handle("merkle_tree").unwrap());
        
        // Initialize or load the sparse Merkle tree
        let merkle_tree = Self::load_or_create_merkle_tree(&db, &merkle_cf)?;
        
        Ok(Self {
            db: Arc::new(db),
            height: 0,
            merkle_tree,
            merkle_cf,
        })
    }
    
    /// Get the current state root
    pub fn state_root(&self) -> Hash {
        self.merkle_tree.root()
    }
    
    /// Load existing Merkle tree or create a new one
    fn load_or_create_merkle_tree(db: &DB, cf: &ColumnFamily) -> Result<SparseMerkleTree, rocksdb::Error> {
        // Check if we have a saved tree
        match db.get_cf(cf, "merkle_root")? {
            Some(root_bytes) => {
                info!("Loading existing Merkle tree");
                let mut root = [0; HASH_SIZE];
                if root_bytes.len() == HASH_SIZE {
                    root.copy_from_slice(&root_bytes);
                }
                Ok(SparseMerkleTree::with_root(root))
            },
            None => {
                info!("Creating new Merkle tree");
                Ok(SparseMerkleTree::new())
            }
        }
    }
    
    /// Create an iterator over key-value pairs with a prefix
    pub fn prefix_iterator(&self, prefix: &[u8]) -> impl Iterator<Item = (Vec<u8>, Vec<u8>)> {
        let prefix = prefix.to_vec();
        let iter = self.db.prefix_iterator(&prefix);
        
        iter.map(|result| {
            match result {
                Ok((key, value)) => (key.to_vec(), value.to_vec()),
                Err(_) => (Vec::new(), Vec::new()),
            }
        })
        .filter(|(key, value)| !key.is_empty() && !value.is_empty())
    }
}

impl KeyValueStoreLike for MerkleizedRocksDBRuntimeAdapter {
    type Batch = MerkleizedRocksDBBatch;
    type Error = rocksdb::Error;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        // First, update the Merkle tree with all changes in the batch
        for (key, value_opt) in &batch.operations {
            match value_opt {
                Some(value) => {
                    // Update or insert
                    self.merkle_tree.update(key, value);
                },
                None => {
                    // Delete
                    self.merkle_tree.delete(key);
                }
            }
        }
        
        // Create a RocksDB WriteBatch
        let mut db_batch = WriteBatch::default();
        
        // Add all key-value operations
        for (key, value_opt) in batch.operations {
            match value_opt {
                Some(value) => db_batch.put(key, value),
                None => db_batch.delete(key),
            }
        }
        
        // Add Merkle tree updates to the batch
        self.merkle_tree.write_updates_to_batch(&mut db_batch, &self.merkle_cf.clone());
        
        // Write the batch to RocksDB
        self.db.write(db_batch)
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        self.db.get(key)
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        // Update the Merkle tree
        self.merkle_tree.delete(key.as_ref());
        
        // Create a batch with the delete operation and Merkle tree updates
        let mut batch = WriteBatch::default();
        batch.delete(key);
        self.merkle_tree.write_updates_to_batch(&mut batch, &self.merkle_cf.clone());
        
        // Write the batch
        self.db.write(batch)
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>
    {
        // Update the Merkle tree
        self.merkle_tree.update(key.as_ref(), value.as_ref());
        
        // Create a batch with the put operation and Merkle tree updates
        let mut batch = WriteBatch::default();
        batch.put(key, value);
        self.merkle_tree.write_updates_to_batch(&mut batch, &self.merkle_cf);
        
        // Write the batch
        self.db.write(batch)
    }
}

impl BatchLike for MerkleizedRocksDBBatch {
    fn insert<K, V>(&mut self, key: K, value: V)
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.operations.push((key.as_ref().to_vec(), Some(value.as_ref().to_vec())));
    }

    fn remove<K>(&mut self, key: K)
    where
        K: AsRef<[u8]>,
    {
        self.operations.push((key.as_ref().to_vec(), None));
    }
}

impl MerkleizedRocksDBBatch {
    /// Create a new batch
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }
}

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
#[derive(Clone)]
pub struct SparseMerkleTree {
    /// Root hash of the tree
    root: Hash,
    /// In-memory cache of nodes
    nodes: std::collections::HashMap<Hash, Node>,
    /// Pending updates to be written to the database
    pending_updates: std::collections::HashMap<Vec<u8>, Option<Vec<u8>>>,
}

impl SparseMerkleTree {
    /// Create a new empty sparse Merkle tree
    pub fn new() -> Self {
        Self {
            root: [0; HASH_SIZE], // Empty tree root
            nodes: std::collections::HashMap::new(),
            pending_updates: std::collections::HashMap::new(),
        }
    }
    
    /// Create a new sparse Merkle tree with a specific root
    pub fn with_root(root: Hash) -> Self {
        Self {
            root,
            nodes: std::collections::HashMap::new(),
            pending_updates: std::collections::HashMap::new(),
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
    pub fn write_updates_to_batch(&mut self, batch: &mut WriteBatch, cf: &Arc<ColumnFamily>) {
        // Write the new root
        batch.put_cf(cf.as_ref(), "merkle_root", &self.root);
        
        // Write all new or updated nodes
        for (node_hash, node) in &self.nodes {
            match node {
                Node::Internal { left, right } => {
                    let mut data = Vec::with_capacity(2 * HASH_SIZE);
                    data.extend_from_slice(left);
                    data.extend_from_slice(right);
                    batch.put_cf(cf.as_ref(), node_hash, &data);
                },
                Node::Leaf { key, value_hash } => {
                    let mut data = Vec::with_capacity(key.len() + HASH_SIZE + 4);
                    data.extend_from_slice(&(key.len() as u32).to_le_bytes());
                    data.extend_from_slice(key);
                    data.extend_from_slice(value_hash);
                    batch.put_cf(cf.as_ref(), node_hash, &data);
                },
                Node::Empty => {
                    // Empty nodes are not stored
                }
            }
        }
        
        // Clear pending updates
        self.pending_updates.clear();
        
        // Clear the in-memory node cache if it's getting too large
        if self.nodes.len() > 10_000 {
            self.nodes.clear();
        }
    }
    
    /// Generate a Merkle proof for a key
    pub fn generate_proof(&self, key: &[u8]) -> Vec<Hash> {
        let path = self.key_to_path(key);
        let mut proof = Vec::new();
        self.generate_proof_internal(self.root, &path, 0, &mut proof);
        proof
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
        use blake2::{Blake2b512, Digest};
        
        let mut hasher = Blake2b512::new();
        hasher.update(key);
        let result = hasher.finalize();
        
        let mut key_hash = [0; 32];
        key_hash.copy_from_slice(&result[0..32]);
        key_hash
    }
    
    /// Hash a value
    fn hash_value(&self, value: &[u8]) -> Hash {
        use blake2::{Blake2b512, Digest};
        
        let mut hasher = Blake2b512::new();
        hasher.update(value);
        let result = hasher.finalize();
        
        let mut value_hash = [0; HASH_SIZE];
        value_hash.copy_from_slice(&result);
        value_hash
    }
    
    /// Hash two child nodes
    fn hash_children(&self, left: &Hash, right: &Hash) -> Hash {
        use blake2::{Blake2b512, Digest};
        
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
        use blake2::{Blake2b512, Digest};
        
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_merkleized_adapter_basic() {
        let dir = tempdir().unwrap();
        let db_path = dir.path();
        
        // Create a new adapter
        let mut adapter = MerkleizedRocksDBRuntimeAdapter::new(db_path).unwrap();
        
        // Initial state root should be empty
        assert_eq!(adapter.state_root(), [0; HASH_SIZE]);
        
        // Insert a key-value pair
        adapter.put(b"test_key", b"test_value").unwrap();
        
        // State root should be updated
        assert_ne!(adapter.state_root(), [0; HASH_SIZE]);
        
        // Get the value back
        let value = adapter.get(b"test_key").unwrap().unwrap();
        assert_eq!(value, b"test_value");
        
        // Delete the key
        adapter.delete(b"test_key").unwrap();
        
        // Value should be gone
        assert!(adapter.get(b"test_key").unwrap().is_none());
    }
    
    #[test]
    fn test_merkleized_adapter_batch() {
        let dir = tempdir().unwrap();
        let db_path = dir.path();
        
        // Create a new adapter
        let mut adapter = MerkleizedRocksDBRuntimeAdapter::new(db_path).unwrap();
        
        // Create a batch
        let mut batch = MerkleizedRocksDBBatch::new();
        
        // Add some operations
        batch.insert(b"key1", b"value1");
        batch.insert(b"key2", b"value2");
        batch.insert(b"key3", b"value3");
        
        // Write the batch
        adapter.write(batch).unwrap();
        
        // Check that all keys were written
        assert_eq!(adapter.get(b"key1").unwrap().unwrap(), b"value1");
        assert_eq!(adapter.get(b"key2").unwrap().unwrap(), b"value2");
        assert_eq!(adapter.get(b"key3").unwrap().unwrap(), b"value3");
        
        // Create another batch to delete a key
        let mut batch = MerkleizedRocksDBBatch::new();
        batch.remove(b"key2");
        
        // Write the batch
        adapter.write(batch).unwrap();
        
        // Check that the key was deleted
        assert!(adapter.get(b"key2").unwrap().is_none());
    }
}
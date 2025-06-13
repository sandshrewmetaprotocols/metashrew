use anyhow::{anyhow, Result};
use rocksdb::{DB, WriteBatch};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::collections::HashMap;
use std::cmp::Ordering;

// Prefixes for different types of keys in the database
pub const SMT_NODE_PREFIX: &str = "smt:node:";
pub const SMT_ROOT_PREFIX: &str = "smt:root:";
pub const SMT_LEAF_PREFIX: &str = "smt:leaf:";
pub const HEIGHT_INDEX_PREFIX: &str = "smt:height:";
pub const HEIGHT_TREE_PREFIX: &str = "height_tree:";
pub const HEIGHT_LIST_SUFFIX: &str = ":heights";
pub const MODIFIED_KEYS_PREFIX: &str = "modified_keys:";

// Default empty hash (represents empty nodes)
const EMPTY_NODE_HASH: [u8; 32] = [0; 32];

/// SMT node types
#[derive(Debug, Clone)]
pub enum SMTNode {
    Internal {
        left_child: [u8; 32],
        right_child: [u8; 32],
    },
    Leaf {
        key: Vec<u8>,
        value_index: [u8; 32], // Reference to the latest value
    },
}

/// Helper functions for SMT operations
pub struct SMTHelper {
    db: Arc<DB>,
}

impl SMTHelper {
    pub fn new(db: Arc<DB>) -> Self {
        Self { db }
    }

    /// Hash a key to get a fixed-length path
    pub fn hash_key(key: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.finalize().into()
    }

    /// Hash a value to get a fixed-length reference
    pub fn hash_value(value: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(value);
        hasher.finalize().into()
    }

    /// Hash a node to get its identifier
    pub fn hash_node(node: &SMTNode) -> [u8; 32] {
        let mut hasher = Sha256::new();
        match node {
            SMTNode::Internal { left_child, right_child } => {
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
            SMTNode::Internal { left_child, right_child } => {
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
                if data.len() != 65 { // 1 byte type + 32 bytes left + 32 bytes right
                    return Err(anyhow!("Invalid internal node data length"));
                }
                
                let mut left_child = [0u8; 32];
                let mut right_child = [0u8; 32];
                
                left_child.copy_from_slice(&data[1..33]);
                right_child.copy_from_slice(&data[33..65]);
                
                Ok(SMTNode::Internal { left_child, right_child })
            }
            1 => {
                // Leaf node
                if data.len() < 5 { // 1 byte type + 4 bytes key length
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

    /// Get a node from the database
    pub fn get_node(&self, node_hash: &[u8; 32]) -> Result<Option<SMTNode>> {
        if node_hash == &EMPTY_NODE_HASH {
            return Ok(None);
        }
        
        let node_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(node_hash)).into_bytes();
        match self.db.get(&node_key)? {
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
                SMTNode::Leaf { key, .. } => {
                    // Check if this is the leaf we're looking for
                    if Self::hash_key(&key) == key_hash {
                        return Ok(Some(node));
                    } else {
                        // Hash collision (extremely unlikely)
                        return Ok(None);
                    }
                }
                SMTNode::Internal { left_child, right_child } => {
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
    pub fn collect_path_nodes(&self, root: [u8; 32], key_hash: [u8; 32]) -> Result<Vec<(bool, SMTNode)>> {
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
                SMTNode::Internal { left_child, right_child } => {
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
    pub fn compute_smt_updates(&self, key: &[u8], value: &[u8], current_root: [u8; 32], height: u32) 
        -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut updates = Vec::new();
        let key_hash = Self::hash_key(key);
        
        // 1. Store the value with height annotation (old format)
        let value_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), height).into_bytes();
        let value_hash = Self::hash_value(value);
        updates.push((value_key, value.to_vec()));
        
        // 1.1 Store the value in the height-indexed BST structure
        let height_indexed_smt = HeightIndexedSMT::new(self.db.clone());
        let bst_updates = height_indexed_smt.put_with_height(key, value, height)?;
        updates.extend(bst_updates);
        
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
            let root_key = format!("{}:{}", SMT_ROOT_PREFIX, height).into_bytes();
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
                SMTNode::Internal { left_child, right_child } => {
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
                    let node_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(new_hash)).into_bytes();
                    updates.push((node_key, Self::serialize_node(&new_node)));
                    
                    new_nodes.push((depth, new_hash));
                    depth -= 1;
                }
            }
        }
        
        // 5. Update the root
        let new_root = new_nodes.last().unwrap().1;
        let root_key = format!("{}:{}", SMT_ROOT_PREFIX, height).into_bytes();
        updates.push((root_key, new_root.to_vec()));
        
        Ok(updates)
    }

    /// Compute the new root after applying updates
    pub fn compute_new_root(&self, current_root: [u8; 32], kvs: &[(Vec<u8>, Vec<u8>)], height: u32) 
        -> Result<[u8; 32]> {
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
    pub fn get_value_at_height(&self, key: &[u8], value_hash: [u8; 32], height: u32) 
        -> Result<Option<Vec<u8>>> {
        // First try using the height-indexed BST for efficient lookup
        let height_indexed_smt = HeightIndexedSMT::new(self.db.clone());
        if let Some(value) = height_indexed_smt.get_at_height(key, height)? {
            return Ok(Some(value));
        }
        
        // Fall back to the old method if not found in the BST
        // Try to get the value at the exact height first
        let exact_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), height).into_bytes();
        if let Ok(Some(value)) = self.db.get(&exact_key) {
            return Ok(Some(value));
        }
        
        // If not found, find the closest height less than the requested height
        let mut target_height = height;
        while target_height > 0 {
            target_height -= 1;
            let value_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), target_height).into_bytes();
            if let Ok(Some(value)) = self.db.get(&value_key) {
                return Ok(Some(value));
            }
        }
        
        // If still not found, try to get by value hash
        let value_key = format!("{}:{}", "value:", hex::encode(value_hash)).into_bytes();
        match self.db.get(&value_key)? {
            Some(value) => Ok(Some(value)),
            None => Ok(None),
        }
    }
    
    /// Track keys modified at a specific height
    pub fn track_modified_keys(&self, keys: &[Vec<u8>], height: u32) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut updates = Vec::new();
        let modified_keys_key = format!("{}:{}", MODIFIED_KEYS_PREFIX, height).into_bytes();
        
        // Serialize the list of keys
        let mut data = Vec::new();
        for key in keys {
            // Write key length (4 bytes)
            data.extend_from_slice(&(key.len() as u32).to_le_bytes());
            // Write key
            data.extend_from_slice(key);
        }
        
        updates.push((modified_keys_key, data));
        Ok(updates)
    }
    
    /// Get keys modified at a specific height
    pub fn get_keys_modified_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>> {
        let modified_keys_key = format!("{}:{}", MODIFIED_KEYS_PREFIX, height).into_bytes();
        
        match self.db.get(&modified_keys_key)? {
            Some(data) => {
                let mut keys = Vec::new();
                let mut i = 0;
                
                while i < data.len() {
                    if i + 4 <= data.len() {
                        let mut len_bytes = [0u8; 4];
                        len_bytes.copy_from_slice(&data[i..i+4]);
                        let key_len = u32::from_le_bytes(len_bytes) as usize;
                        i += 4;
                        
                        if i + key_len <= data.len() {
                            keys.push(data[i..i+key_len].to_vec());
                            i += key_len;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                
                Ok(keys)
            },
            None => Ok(Vec::new()),
        }
    }
    
    /// Get all keys affected by a reorg
    pub fn get_keys_affected_by_reorg(&self, fork_height: u32, current_height: u32) -> Result<Vec<Vec<u8>>> {
        let mut affected_keys = Vec::new();
        
        // For each height from fork_height+1 to current_height
        for height in (fork_height + 1)..=current_height {
            // Get all keys modified at this height
            let keys = self.get_keys_modified_at_height(height)?;
            affected_keys.extend(keys);
        }
        
        // Remove duplicates
        affected_keys.sort();
        affected_keys.dedup();
        
        Ok(affected_keys)
    }
    
    /// Compute delete updates for a key
    pub fn compute_delete_updates(&self, key: &[u8], current_root: [u8; 32], height: u32) 
        -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        // This is a simplified implementation
        // In a real implementation, we would need to update the SMT to remove the key
        // For now, we'll just mark the key as deleted at this height
        let mut updates = Vec::new();
        let delete_marker = Vec::new(); // Empty value indicates deletion
        
        let value_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), height).into_bytes();
        updates.push((value_key, delete_marker));
        
        // We would also need to update the SMT root
        // For now, we'll just use the current root
        let root_key = format!("{}:{}", SMT_ROOT_PREFIX, height).into_bytes();
        updates.push((root_key, current_root.to_vec()));
        
        Ok(updates)
    }
}
/// Height-Indexed Binary Search Tree for efficient historical state queries
pub struct HeightIndexedSMT {
    db: Arc<DB>,
}

impl HeightIndexedSMT {
    pub fn new(db: Arc<DB>) -> Self {
        Self { db }
    }
    
    /// Store a value with height indexing
    pub fn put_with_height(&self, key: &[u8], value: &[u8], height: u32) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut updates = Vec::new();
        
        // 1. Store the actual value with height in the key
        let height_key = format!("{}:{}:{}", 
            HEIGHT_TREE_PREFIX, 
            hex::encode(key), 
            height
        ).into_bytes();
        updates.push((height_key, value.to_vec()));
        
        // 2. Update the height list for binary search
        self.update_height_list(&mut updates, key, height)?;
        
        // 3. Track this key as modified at this height
        let smt_helper = SMTHelper::new(self.db.clone());
        let modified_keys_updates = smt_helper.track_modified_keys(&[key.to_vec()], height)?;
        updates.extend(modified_keys_updates);
        
        Ok(updates)
    }
    
    /// Update the height list for a key
    fn update_height_list(&self, updates: &mut Vec<(Vec<u8>, Vec<u8>)>, key: &[u8], height: u32) -> Result<()> {
        let heights_key = format!("{}:{}", hex::encode(key), HEIGHT_LIST_SUFFIX).into_bytes();
        
        // Get existing heights
        let mut heights = match self.db.get(&heights_key)? {
            Some(data) => {
                let mut heights = Vec::new();
                let mut i = 0;
                while i < data.len() {
                    if i + 4 <= data.len() {
                        let mut height_bytes = [0u8; 4];
                        height_bytes.copy_from_slice(&data[i..i+4]);
                        heights.push(u32::from_le_bytes(height_bytes));
                    }
                    i += 4;
                }
                heights
            },
            None => Vec::new(),
        };
        
        // Add new height if not already present
        if !heights.contains(&height) {
            heights.push(height);
            heights.sort(); // Keep sorted for binary search
            
            // Serialize heights back to bytes
            let mut data = Vec::with_capacity(heights.len() * 4);
            for h in heights {
                data.extend_from_slice(&h.to_le_bytes());
            }
            
            updates.push((heights_key, data));
        }
        
        Ok(())
    }
    
    /// Get a value at a specific height using binary search
    pub fn get_at_height(&self, key: &[u8], target_height: u32) -> Result<Option<Vec<u8>>> {
        // 1. Get the height list
        let heights_key = format!("{}:{}", hex::encode(key), HEIGHT_LIST_SUFFIX).into_bytes();
        
        let heights = match self.db.get(&heights_key)? {
            Some(data) => {
                let mut heights = Vec::new();
                let mut i = 0;
                while i < data.len() {
                    if i + 4 <= data.len() {
                        let mut height_bytes = [0u8; 4];
                        height_bytes.copy_from_slice(&data[i..i+4]);
                        heights.push(u32::from_le_bytes(height_bytes));
                    }
                    i += 4;
                }
                heights
            },
            None => return Ok(None), // No history for this key
        };
        
        // 2. Binary search for the closest height less than or equal to target_height
        let closest_height = match heights.binary_search_by(|&h| {
            if h <= target_height {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }) {
            Ok(idx) => heights[idx],
            Err(idx) => {
                if idx == 0 {
                    return Ok(None); // No value before target_height
                }
                heights[idx - 1] // Get the closest height less than target_height
            }
        };
        
        // 3. Get the value at the closest height
        let height_key = format!("{}:{}:{}", 
            HEIGHT_TREE_PREFIX, 
            hex::encode(key), 
            closest_height
        ).into_bytes();
        
        self.db.get(&height_key).map_err(|e| anyhow!(e))
    }
    
    /// Migrate existing data to the height-indexed structure
    pub fn migrate_data(&self, start_height: u32, end_height: u32) -> Result<()> {
        let smt_helper = SMTHelper::new(self.db.clone());
        let mut batch = WriteBatch::default();
        
        // For each height
        for height in start_height..=end_height {
            // Get all keys modified at this height
            let modified_keys_key = format!("{}:{}", HEIGHT_INDEX_PREFIX, height).into_bytes();
            
            if let Ok(Some(keys_data)) = self.db.get(&modified_keys_key) {
                let mut i = 0;
                let mut modified_keys = Vec::new();
                
                while i < keys_data.len() {
                    if i + 4 <= keys_data.len() {
                        let mut len_bytes = [0u8; 4];
                        len_bytes.copy_from_slice(&keys_data[i..i+4]);
                        let key_len = u32::from_le_bytes(len_bytes) as usize;
                        i += 4;
                        
                        if i + key_len <= keys_data.len() {
                            let key = keys_data[i..i+key_len].to_vec();
                            modified_keys.push(key.clone());
                            
                            // Get the value at this height
                            let value_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(&key), height).into_bytes();
                            
                            if let Ok(Some(value)) = self.db.get(&value_key) {
                                // Store in the new format
                                let updates = self.put_with_height(&key, &value, height)?;
                                
                                for (update_key, update_value) in updates {
                                    batch.put(&update_key, &update_value);
                                }
                            }
                            
                            i += key_len;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                
                // Track modified keys in the new format
                let modified_keys_updates = smt_helper.track_modified_keys(&modified_keys, height)?;
                for (update_key, update_value) in modified_keys_updates {
                    batch.put(&update_key, &update_value);
                }
            }
        }
        
        // Apply all changes
        self.db.write(batch)?;
        
        Ok(())
    }
    
    /// Roll back to a specific height during reorg
    pub fn rollback_to_height(&self, fork_height: u32, current_height: u32) -> Result<()> {
        let smt_helper = SMTHelper::new(self.db.clone());
        
        // Get all keys affected by the reorg
        let affected_keys = smt_helper.get_keys_affected_by_reorg(fork_height, current_height)?;
        
        let mut batch = WriteBatch::default();
        
        // For each affected key
        for key in &affected_keys {
            // Get the value at the fork height
            let fork_value = match self.get_at_height(key, fork_height)? {
                Some(value) => value,
                None => Vec::new(), // Key didn't exist at fork height
            };
            
            // Update the SMT with the fork height value
            let fork_root = smt_helper.get_smt_root_at_height(fork_height)?;
            
            // Compute SMT updates to restore the state
            let updates = if fork_value.is_empty() {
                // Key didn't exist at fork height, so remove it
                smt_helper.compute_delete_updates(key, fork_root, fork_height)?
            } else {
                // Key existed with a different value, restore it
                smt_helper.compute_smt_updates(key, &fork_value, fork_root, fork_height)?
            };
            
            // Apply the updates to the batch
            for (update_key, update_value) in updates {
                batch.put(&update_key, &update_value);
            }
            
            // Update the height list to mark this key as not modified after fork_height
            let heights_key = format!("{}:{}", hex::encode(key), HEIGHT_LIST_SUFFIX).into_bytes();
            
            if let Some(heights_data) = self.db.get(&heights_key)? {
                let mut heights = Vec::new();
                let mut i = 0;
                
                while i < heights_data.len() {
                    if i + 4 <= heights_data.len() {
                        let mut height_bytes = [0u8; 4];
                        height_bytes.copy_from_slice(&heights_data[i..i+4]);
                        let height = u32::from_le_bytes(height_bytes);
                        
                        // Only keep heights up to fork_height
                        if height <= fork_height {
                            heights.push(height);
                        }
                        
                        i += 4;
                    } else {
                        break;
                    }
                }
                
                // Serialize heights back to bytes
                let mut new_heights_data = Vec::with_capacity(heights.len() * 4);
                for h in heights {
                    new_heights_data.extend_from_slice(&h.to_le_bytes());
                }
                
                batch.put(&heights_key, &new_heights_data);
            }
            
            /// Migrate existing data to the height-indexed BST structure
            pub fn migrate_to_height_indexed(&self, start_height: u32, end_height: u32) -> Result<()> {
                info!("Migrating data to height-indexed BST structure from height {} to {}",
                      start_height, end_height);
                
                let height_indexed_smt = HeightIndexedSMT::new(self.db.clone());
                height_indexed_smt.migrate_data(start_height, end_height)?;
                
                info!("Migration completed successfully");
                Ok(())
            }
        }
        
        // Delete height index entries for heights > fork_height
        for height in (fork_height + 1)..=current_height {
            let modified_keys_key = format!("{}:{}", MODIFIED_KEYS_PREFIX, height).into_bytes();
            batch.delete(&modified_keys_key);
            
            // Delete the stateroot for this height
            let root_key = format!("{}:{}", SMT_ROOT_PREFIX, height).into_bytes();
            batch.delete(&root_key);
        }
        
        // Apply all changes atomically
        self.db.write(batch)?;
        
        Ok(())
    }
}
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use crate::traits::KeyValueStoreLike;

// Prefixes for different types of keys in the database
pub const SMT_NODE_PREFIX: &str = "smt:node:";
pub const SMT_ROOT_PREFIX: &str = "smt:root:";
pub const SMT_LEAF_PREFIX: &str = "smt:leaf:";
pub const HEIGHT_INDEX_PREFIX: &str = "smt:height:";
pub const BST_PREFIX: &str = "bst:";
pub const BST_HEIGHT_PREFIX: &str = "bst:height:";
pub const KEYS_AT_HEIGHT_PREFIX: &str = "keys:height:";

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
pub struct SMTHelper<T: KeyValueStoreLike> {
    storage: T,
}

impl<T: KeyValueStoreLike> SMTHelper<T> {
    pub fn new(storage: T) -> Self {
        Self { storage }
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
            if let Some(root_data) = self.storage.get_immutable(&root_key).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
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
        match self.storage.get_immutable(&node_key).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
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
        
        // 1. Store the value with height annotation
        let value_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), height).into_bytes();
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
        // Try to get the value at the exact height first
        let exact_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), height).into_bytes();
        if let Some(value) = self.storage.get_immutable(&exact_key).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
            return Ok(Some(value));
        }
        
        // If not found, find the closest height less than the requested height
        let mut target_height = height;
        while target_height > 0 {
            target_height -= 1;
            let value_key = format!("{}:{}:{}", HEIGHT_INDEX_PREFIX, hex::encode(key), target_height).into_bytes();
            if let Some(value) = self.storage.get_immutable(&value_key).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
                return Ok(Some(value));
            }
        }
        
        // If still not found, try to get by value hash
        let value_key = format!("{}:{}", "value:", hex::encode(value_hash)).into_bytes();
        match self.storage.get_immutable(&value_key).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
            Some(value) => Ok(Some(value)),
            None => Ok(None),
        }
    }
    
    /// Store a key-value pair in the BST with height indexing
    pub fn bst_put(&mut self, key: &[u8], value: &[u8], height: u32) -> Result<()> {
        // Store the value with height annotation
        let height_key = format!("{}{}:{}", BST_HEIGHT_PREFIX, hex::encode(key), height).into_bytes();
        self.storage.put(&height_key, value).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
        
        // Track that this key was updated at this height
        self.track_key_at_height(key, height)?;
        
        Ok(())
    }

    /// Get the value of a key at a specific height using linear search
    pub fn bst_get_at_height(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>> {
        // Search backwards from the requested height to find the most recent value
        for h in (0..=height).rev() {
            let height_key = format!("{}{}:{}", BST_HEIGHT_PREFIX, hex::encode(key), h).into_bytes();
            if let Some(value) = self.storage.get_immutable(&height_key).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
                return Ok(Some(value));
            }
        }
        
        Ok(None)
    }
    
    /// Get the current (most recent) value of a key across all heights
    pub fn bst_get_current(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let heights = self.bst_get_heights_for_key(key)?;
        
        if heights.is_empty() {
            return Ok(None);
        }
        
        // Get the value at the highest height
        let highest_height = *heights.last().unwrap();
        let height_key = format!("{}{}:{}", BST_HEIGHT_PREFIX, hex::encode(key), highest_height).into_bytes();
        
        match self.storage.get_immutable(&height_key).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
            Some(value) => Ok(Some(value)),
            None => Ok(None),
        }
    }
    
    /// Get all heights at which a key was updated
    pub fn bst_get_heights_for_key(&self, key: &[u8]) -> Result<Vec<u32>> {
        let mut heights = Vec::new();
        let prefix = format!("{}{}:", BST_HEIGHT_PREFIX, hex::encode(key));
        
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
        let keys_key = format!("{}{}:{}", KEYS_AT_HEIGHT_PREFIX, height, hex::encode(key)).into_bytes();
        self.storage.put(&keys_key, b"").map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?; // Empty value, we just need the key
        Ok(())
    }
    
    /// Get all keys that were updated at a specific height
    pub fn get_keys_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>> {
        let mut keys = Vec::new();
        let prefix = format!("{}{}:", KEYS_AT_HEIGHT_PREFIX, height);
        
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
                let height_key = format!("{}{}:{}", BST_HEIGHT_PREFIX, hex::encode(key), height).into_bytes();
                self.storage.delete(&height_key).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
                
                // Also remove from keys-at-height tracking
                let keys_key = format!("{}{}:{}", KEYS_AT_HEIGHT_PREFIX, height, hex::encode(key)).into_bytes();
                self.storage.delete(&keys_key).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
            }
        }
        
        Ok(())
    }
    
    /// Rollback all keys to their state before a specific height
    pub fn bst_rollback_to_height(&mut self, target_height: u32) -> Result<()> {
        // Get all heights greater than target_height that have updates
        let mut heights_to_rollback = Vec::new();
        let prefix = format!("{}:", KEYS_AT_HEIGHT_PREFIX);
        
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
    pub fn bst_iterate_backwards(&self, key: &[u8], from_height: u32) -> Result<Vec<(u32, Vec<u8>)>> {
        let heights = self.bst_get_heights_for_key(key)?;
        let mut results = Vec::new();
        
        // Filter heights to only include those <= from_height and sort in descending order
        let mut filtered_heights: Vec<u32> = heights.into_iter()
            .filter(|&h| h <= from_height)
            .collect();
        filtered_heights.sort_by(|a, b| b.cmp(a)); // Descending order
        
        for height in filtered_heights {
            let height_key = format!("{}{}:{}", BST_HEIGHT_PREFIX, hex::encode(key), height).into_bytes();
            if let Some(value) = self.storage.get_immutable(&height_key).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))? {
                results.push((height, value));
            }
        }
        
        Ok(results)
    }
    
    /// Calculate and store the SMT state root for a specific height
    pub fn calculate_and_store_state_root(&mut self, height: u32) -> Result<[u8; 32]> {
        let prev_height = if height > 0 { height - 1 } else { 0 };
        let prev_root = self.get_smt_root_at_height(prev_height)?;
        
        // Get all keys that were updated at this height
        let updated_keys = self.get_keys_at_height(height)?;
        
        if updated_keys.is_empty() {
            // No updates at this height, return previous root
            let root_key = format!("{}:{}", SMT_ROOT_PREFIX, height).into_bytes();
            self.storage.put(&root_key, &prev_root).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
            return Ok(prev_root);
        }
        
        // Build a map of all current key-value pairs for SMT calculation
        let mut current_state = BTreeMap::new();
        
        // Get all keys that exist in the database up to this height
        let prefix = format!("{}:", BST_HEIGHT_PREFIX);
        
        let mut all_keys = std::collections::HashSet::new();
        // Get all keys with this prefix and extract original keys
        for (key, _) in self.storage.scan_prefix(prefix.as_bytes())? {
            let key_str = String::from_utf8_lossy(&key);
            if let Some(rest) = key_str.strip_prefix(&prefix) {
                if let Some(colon_pos) = rest.find(':') {
                    let hex_key = &rest[..colon_pos];
                    if let Ok(original_key) = hex::decode(hex_key) {
                        all_keys.insert(original_key);
                    }
                }
            }
        }
        
        // For each key, get its value at this height
        for key in all_keys {
            if let Ok(Some(value)) = self.bst_get_at_height(&key, height) {
                current_state.insert(key, value);
            }
        }
        
        // Calculate the new SMT root based on current state
        let new_root = self.compute_smt_root_from_state(&current_state)?;
        
        // Store the new root
        let root_key = format!("{}:{}", SMT_ROOT_PREFIX, height).into_bytes();
        self.storage.put(&root_key, &new_root).map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
        
        Ok(new_root)
    }
    
    /// Compute SMT root from a complete state map
    fn compute_smt_root_from_state(&self, state: &BTreeMap<Vec<u8>, Vec<u8>>) -> Result<[u8; 32]> {
        if state.is_empty() {
            return Ok(EMPTY_NODE_HASH);
        }
        
        // For a simple implementation, we'll hash all key-value pairs together
        // In a production SMT, this would build the actual tree structure
        let mut hasher = Sha256::new();
        
        for (key, value) in state.iter() {
            hasher.update(key);
            hasher.update(value);
        }
        
        Ok(hasher.finalize().into())
    }
    
    /// Get the current state root (most recent)
    pub fn get_current_state_root(&self) -> Result<[u8; 32]> {
        // Find the highest height with a stored root
        let prefix = format!("{}:", SMT_ROOT_PREFIX);
        
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
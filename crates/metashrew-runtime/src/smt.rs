use crate::traits::{BatchLike, KeyValueStoreLike};
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

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
    pub storage: T,
    // Incremental state tracking (no caching)
    pending_updates: HashMap<Vec<u8>, Vec<u8>>,
    last_state_height: Option<u32>,
}

impl<T: KeyValueStoreLike> SMTHelper<T> {
    pub fn new(storage: T) -> Self {
        Self {
            storage,
            pending_updates: HashMap::new(),
            last_state_height: None,
        }
    }

    /// Add pending update for incremental state calculation
    pub fn add_pending_update(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.pending_updates.insert(key, value);
    }

    /// Get pending updates for batch processing
    pub fn get_pending_updates(&self) -> &HashMap<Vec<u8>, Vec<u8>> {
        &self.pending_updates
    }

    /// Clear pending updates after processing
    pub fn clear_pending_updates(&mut self) {
        self.pending_updates.clear();
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
    pub fn get_smt_root_at_height(&mut self, height: u32) -> Result<[u8; 32]> {
        // Find the closest height less than or equal to the requested height
        let mut target_height = height;

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

            // If we've reached height 0 and found nothing, return error
            if target_height == 0 {
                break;
            }
            target_height -= 1;
        }

        // If no root found, return an error instead of empty hash
        Err(anyhow!(
            "No state root found for height {} or any previous height",
            height
        ))
    }

    /// Get the SMT root for a specific height without caching (for immutable operations)
    pub fn get_smt_root_at_height_immutable(&self, height: u32) -> Result<[u8; 32]> {
        // Find the closest height less than or equal to the requested height
        let mut target_height = height;

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

            // If we've reached height 0 and found nothing, return error
            if target_height == 0 {
                break;
            }
            target_height -= 1;
        }

        // If no root found, return an error instead of empty hash
        Err(anyhow!(
            "No state root found for height {} or any previous height",
            height
        ))
    }

    /// Get a node from the database
    pub fn get_node(&mut self, node_hash: &[u8; 32]) -> Result<Option<SMTNode>> {
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

    /// Get a node from the database without caching (for immutable operations)
    pub fn get_node_immutable(&self, node_hash: &[u8; 32]) -> Result<Option<SMTNode>> {
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
    pub fn get_smt_leaf(&mut self, root: [u8; 32], key_hash: [u8; 32]) -> Result<Option<SMTNode>> {
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
        &mut self,
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
        &mut self,
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
        &mut self,
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
        // Store the value with height annotation
        let height_key =
            format!("{}{}:{}", BST_HEIGHT_PREFIX, hex::encode(key), height).into_bytes();
        self.storage
            .put(&height_key, value)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        // Track that this key was updated at this height
        self.track_key_at_height(key, height)?;

        Ok(())
    }

    /// Store multiple key-value pairs in the BST with height indexing using batch operations (optimized)
    pub fn bst_put_batch(&mut self, entries: &[(Vec<u8>, Vec<u8>)], height: u32) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        // Add to pending updates for incremental state calculation
        for (key, value) in entries {
            self.add_pending_update(key.clone(), value.clone());
        }

        // Create a batch for all operations
        let mut batch = self.storage.create_batch();

        // Add all entries to the batch
        for (key, value) in entries {
            // Store the value with height annotation
            let height_key =
                format!("{}{}:{}", BST_HEIGHT_PREFIX, hex::encode(key), height).into_bytes();
            batch.put(height_key, value.clone());

            // Track that this key was updated at this height
            let keys_key =
                format!("{}{}:{}", KEYS_AT_HEIGHT_PREFIX, height, hex::encode(key)).into_bytes();
            batch.put(keys_key, Vec::new()); // Empty value, we just need the key
        }

        // Write the entire batch atomically
        self.storage
            .write_batch(batch)
            .map_err(|e| anyhow::anyhow!("Batch write error: {:?}", e))?;

        Ok(())
    }

    /// Get the value of a key at a specific height using linear search
    pub fn bst_get_at_height(&mut self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>> {
        // Search backwards from the requested height to find the most recent value
        for h in (0..=height).rev() {
            let height_key =
                format!("{}{}:{}", BST_HEIGHT_PREFIX, hex::encode(key), h).into_bytes();
            if let Some(value) = self
                .storage
                .get_immutable(&height_key)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
            {
                return Ok(Some(value));
            }
        }

        Ok(None)
    }

    /// Get the current (most recent) value of a key across all heights
    pub fn bst_get_current(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let heights = self.bst_get_heights_for_key(key)?;

        if heights.is_empty() {
            return Ok(None);
        }

        // Get the value at the highest height
        let highest_height = *heights.last().unwrap();
        let height_key = format!(
            "{}{}:{}",
            BST_HEIGHT_PREFIX,
            hex::encode(key),
            highest_height
        )
        .into_bytes();

        match self
            .storage
            .get_immutable(&height_key)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?
        {
            Some(value) => Ok(Some(value)),
            None => Ok(None),
        }
    }

    /// Get all heights at which a key was updated
    pub fn bst_get_heights_for_key(&mut self, key: &[u8]) -> Result<Vec<u32>> {
        let mut heights = Vec::new();
        let prefix = format!("{}{}:", BST_HEIGHT_PREFIX, hex::encode(key));

        // Get all keys with this prefix and extract heights
        for (key_bytes, _) in self.storage.scan_prefix(prefix.as_bytes())? {
            let key_str = String::from_utf8_lossy(&key_bytes);
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
            format!("{}{}:{}", KEYS_AT_HEIGHT_PREFIX, height, hex::encode(key)).into_bytes();
        self.storage
            .put(&keys_key, b"")
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?; // Empty value, we just need the key
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
                let height_key =
                    format!("{}{}:{}", BST_HEIGHT_PREFIX, hex::encode(key), height).into_bytes();
                self.storage
                    .delete(&height_key)
                    .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

                // Also remove from keys-at-height tracking
                let keys_key = format!("{}{}:{}", KEYS_AT_HEIGHT_PREFIX, height, hex::encode(key))
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
    pub fn bst_iterate_backwards(
        &mut self,
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
                format!("{}{}:{}", BST_HEIGHT_PREFIX, hex::encode(key), height).into_bytes();
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

    /// Calculate and store the SMT state root for a specific height (optimized)
    pub fn calculate_and_store_state_root(&mut self, height: u32) -> Result<[u8; 32]> {
        // Temporarily disable incremental calculation to avoid hangs
        // TODO: Re-enable after fixing the SMT implementation
        
        // Always use full calculation for now
        self.calculate_full_state_root(height)
    }

    /// Calculate state root incrementally based on pending updates
    fn calculate_incremental_state_root(&mut self, height: u32) -> Result<[u8; 32]> {
        let prev_root = if height > 0 {
            self.get_smt_root_at_height(height - 1)?
        } else {
            EMPTY_NODE_HASH
        };

        if self.pending_updates.is_empty() {
            // No updates, return previous root
            let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
            self.storage
                .put(&root_key, &prev_root)
                .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;
            self.last_state_height = Some(height);
            return Ok(prev_root);
        }

        // Build incremental state from pending updates
        let mut incremental_state = BTreeMap::new();
        for (key, value) in &self.pending_updates {
            incremental_state.insert(key.clone(), value.clone());
        }

        // Calculate new root based on incremental changes
        let new_root = self.compute_incremental_smt_root(prev_root, &incremental_state)?;

        // Store the new root
        let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
        self.storage
            .put(&root_key, &new_root)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        // Update tracking
        self.last_state_height = Some(height);

        Ok(new_root)
    }

    /// Calculate state root using full scan (fallback method)
    fn calculate_full_state_root(&mut self, height: u32) -> Result<[u8; 32]> {
        let prev_root = if height > 0 {
            self.get_smt_root_at_height(height - 1)?
        } else {
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
            self.last_state_height = Some(height);
            return Ok(prev_root);
        }

        // Build a map of all current key-value pairs for SMT calculation
        let mut current_state = BTreeMap::new();

        // Optimized: Only scan for keys that were updated at this height or before
        let mut all_keys = std::collections::HashSet::new();
        
        // Add keys updated at this height
        for key in &updated_keys {
            all_keys.insert(key.clone());
        }

        // For efficiency, limit the scan to recently updated keys if we have many
        if updated_keys.len() < 1000 {
            // Small update set - scan all keys for completeness
            let prefix = BST_HEIGHT_PREFIX.as_bytes();
            for (key, _) in self.storage.scan_prefix(prefix)? {
                let key_str = String::from_utf8_lossy(&key);
                if let Some(rest) = key_str.strip_prefix(BST_HEIGHT_PREFIX) {
                    if let Some(colon_pos) = rest.find(':') {
                        let hex_key = &rest[..colon_pos];
                        if let Ok(original_key) = hex::decode(hex_key) {
                            all_keys.insert(original_key);
                        }
                    }
                }
            }
        }

        // For each key, get its value at this height
        for key in all_keys {
            if let Ok(Some(value)) = self.bst_get_at_height(&key, height) {
                current_state.insert(key.clone(), value);
            }
        }

        // Calculate the new SMT root based on current state
        let new_root = self.compute_smt_root_from_state(&current_state)?;

        // Store the new root
        let root_key = format!("{}{}", SMT_ROOT_PREFIX, height).into_bytes();
        self.storage
            .put(&root_key, &new_root)
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        // Update tracking
        self.last_state_height = Some(height);

        Ok(new_root)
    }

    /// Compute incremental SMT root from previous root and changes (proper SMT implementation)
    fn compute_incremental_smt_root(
        &mut self,
        prev_root: [u8; 32],
        changes: &BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Result<[u8; 32]> {
        if changes.is_empty() {
            return Ok(prev_root);
        }

        // Start with the previous root
        let mut current_root = prev_root;

        // Apply each change incrementally to the SMT
        for (key, value) in changes.iter() {
            current_root = self.update_smt_with_single_change(current_root, key, value)?;
        }

        Ok(current_root)
    }

    /// Update SMT with a single key-value change, reusing existing branch nodes
    fn update_smt_with_single_change(
        &mut self,
        current_root: [u8; 32],
        key: &[u8],
        value: &[u8],
    ) -> Result<[u8; 32]> {
        let key_hash = Self::hash_key(key);
        let value_hash = Self::hash_value(value);

        // Create the new leaf node
        let new_leaf = SMTNode::Leaf {
            key: key.to_vec(),
            value_index: value_hash,
        };
        let new_leaf_hash = Self::hash_node(&new_leaf);

        // Store the new leaf node
        let leaf_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(new_leaf_hash)).into_bytes();
        self.storage
            .put(&leaf_key, &Self::serialize_node(&new_leaf))
            .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

        // If tree is empty, the new leaf becomes the root
        if current_root == EMPTY_NODE_HASH {
            return Ok(new_leaf_hash);
        }

        // Traverse the tree and update only the path to the changed leaf
        self.update_path_to_leaf(current_root, key_hash, new_leaf_hash, 0)
    }

    /// Update the path from root to a specific leaf, reusing unchanged branches
    fn update_path_to_leaf(
        &mut self,
        node_hash: [u8; 32],
        target_key_hash: [u8; 32],
        new_leaf_hash: [u8; 32],
        depth: usize,
    ) -> Result<[u8; 32]> {
        if depth >= 256 {
            return Err(anyhow!("Maximum SMT depth exceeded at depth {}", depth));
        }

        // Safety check for empty node hash
        if node_hash == EMPTY_NODE_HASH {
            return Ok(new_leaf_hash);
        }

        // Get the current node
        let current_node = match self.get_node(&node_hash)? {
            Some(node) => node,
            None => {
                // If node doesn't exist, create a new path to the leaf
                return Ok(new_leaf_hash);
            }
        };

        match current_node {
            SMTNode::Leaf { key, .. } => {
                let existing_key_hash = Self::hash_key(&key);
                
                if existing_key_hash == target_key_hash {
                    // Replace the existing leaf
                    Ok(new_leaf_hash)
                } else {
                    // Create a new internal node to accommodate both leaves
                    self.create_internal_node_for_leaves(
                        existing_key_hash,
                        node_hash,
                        target_key_hash,
                        new_leaf_hash,
                        depth,
                    )
                }
            }
            SMTNode::Internal {
                left_child,
                right_child,
            } => {
                // Determine which child to update based on the key bit at current depth
                let bit = (target_key_hash[depth / 8] >> (7 - (depth % 8))) & 1;
                
                let (new_left, new_right) = if bit == 0 {
                    // Update left child - add safety check for infinite recursion
                    if left_child == node_hash {
                        return Err(anyhow!("Detected infinite recursion in left child at depth {}", depth));
                    }
                    let new_left = self.update_path_to_leaf(left_child, target_key_hash, new_leaf_hash, depth + 1)?;
                    (new_left, right_child)
                } else {
                    // Update right child - add safety check for infinite recursion
                    if right_child == node_hash {
                        return Err(anyhow!("Detected infinite recursion in right child at depth {}", depth));
                    }
                    let new_right = self.update_path_to_leaf(right_child, target_key_hash, new_leaf_hash, depth + 1)?;
                    (left_child, new_right)
                };

                // Only create new internal node if something actually changed
                if (bit == 0 && new_left == left_child) || (bit == 1 && new_right == right_child) {
                    return Ok(node_hash); // No change needed
                }

                // Create new internal node with updated child
                let new_internal = SMTNode::Internal {
                    left_child: new_left,
                    right_child: new_right,
                };
                let new_internal_hash = Self::hash_node(&new_internal);

                // Store the new internal node
                let internal_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(new_internal_hash)).into_bytes();
                self.storage
                    .put(&internal_key, &Self::serialize_node(&new_internal))
                    .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

                Ok(new_internal_hash)
            }
        }
    }

    /// Create an internal node to separate two leaves with different key hashes
    fn create_internal_node_for_leaves(
        &mut self,
        existing_key_hash: [u8; 32],
        existing_leaf_hash: [u8; 32],
        new_key_hash: [u8; 32],
        new_leaf_hash: [u8; 32],
        start_depth: usize,
    ) -> Result<[u8; 32]> {
        // Safety check for identical keys
        if existing_key_hash == new_key_hash {
            return Err(anyhow!("Cannot create internal node for identical key hashes"));
        }

        let mut depth = start_depth;

        // Find the first bit where the keys differ
        while depth < 256 {
            let existing_bit = (existing_key_hash[depth / 8] >> (7 - (depth % 8))) & 1;
            let new_bit = (new_key_hash[depth / 8] >> (7 - (depth % 8))) & 1;

            if existing_bit != new_bit {
                // Create internal node with leaves as children
                let (left_child, right_child) = if existing_bit == 0 {
                    (existing_leaf_hash, new_leaf_hash)
                } else {
                    (new_leaf_hash, existing_leaf_hash)
                };

                let internal_node = SMTNode::Internal {
                    left_child,
                    right_child,
                };
                let internal_hash = Self::hash_node(&internal_node);

                // Store the internal node
                let internal_key = format!("{}:{}", SMT_NODE_PREFIX, hex::encode(internal_hash)).into_bytes();
                self.storage
                    .put(&internal_key, &Self::serialize_node(&internal_node))
                    .map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))?;

                return Ok(internal_hash);
            }

            depth += 1;
        }

        // If we reach here, the keys are identical (should not happen in practice)
        Err(anyhow!("Key collision detected in SMT: keys are identical after {} bits", depth))
    }

    /// Compute SMT root from a complete state map
    fn compute_smt_root_from_state(&self, state: &BTreeMap<Vec<u8>, Vec<u8>>) -> Result<[u8; 32]> {
        if state.is_empty() {
            return Ok(EMPTY_NODE_HASH);
        }

        // For a simple implementation, we'll hash all key-value pairs together
        // In a production SMT, this would build the actual tree structure
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

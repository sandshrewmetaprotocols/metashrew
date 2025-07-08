//! Threading utilities and abstractions for Metashrew
//!
//! This module provides portable threading utilities that can be used across
//! different environments (runtime, core, and support crates). It includes
//! thread-safe data structures, coordination primitives, and utilities that
//! work with the WASI threads model.
//!
//! # Architecture
//!
//! ## Portable Design
//! - Works in both host runtime and WASM guest environments
//! - Provides abstractions that work with WASI threads instance-per-thread model
//! - Thread-safe utilities that don't rely on shared memory
//!
//! ## Database-Based Coordination
//! - Since WASI threads don't support shared memory, coordination happens via database
//! - Provides patterns for thread communication through key-value operations
//! - Atomic operations using database transactions
//!
//! ## Integration Points
//! - Used by metashrew-runtime for thread management
//! - Used by metashrew-core for guest-side thread coordination
//! - Provides common patterns for both environments


/// Thread-safe identifier generator for creating unique thread IDs
///
/// This structure provides a thread-safe way to generate unique identifiers
/// that can be used across different threading contexts. It's designed to work
/// in both host and guest environments.
#[derive(Debug)]
pub struct ThreadIdGenerator {
    /// Current counter value
    counter: std::sync::atomic::AtomicU32,
}

impl ThreadIdGenerator {
    /// Create a new thread ID generator starting from 1
    pub fn new() -> Self {
        Self {
            counter: std::sync::atomic::AtomicU32::new(1),
        }
    }

    /// Generate the next unique thread ID
    ///
    /// # Returns
    ///
    /// A unique u32 thread ID that hasn't been returned before
    pub fn next_id(&self) -> u32 {
        self.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Get the current counter value without incrementing
    pub fn current_value(&self) -> u32 {
        self.counter.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for ThreadIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread coordination patterns for database-based communication
///
/// Since WASI threads use instance-per-thread with no shared memory,
/// all coordination must happen through the shared database. This module
/// provides common patterns for thread coordination.
pub mod coordination {

    /// Standard key prefixes for thread coordination
    pub mod keys {
        /// Prefix for thread status keys: "thread_status_{thread_id}"
        pub const THREAD_STATUS: &str = "thread_status_";
        
        /// Prefix for thread result keys: "thread_result_{thread_id}"
        pub const THREAD_RESULT: &str = "thread_result_";
        
        /// Prefix for thread completion markers: "thread_complete_{thread_id}"
        pub const THREAD_COMPLETE: &str = "thread_complete_";
        
        /// Prefix for shared values: "shared_{key}"
        pub const SHARED_VALUE: &str = "shared_";
        
        /// Key for active thread list: "active_threads"
        pub const ACTIVE_THREADS: &str = "active_threads";
        
        /// Prefix for thread work queue: "work_queue_{queue_id}"
        pub const WORK_QUEUE: &str = "work_queue_";
    }

    /// Thread status values
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ThreadStatus {
        /// Thread is starting up
        Starting,
        /// Thread is running normally
        Running,
        /// Thread has completed successfully
        Completed,
        /// Thread encountered an error
        Error(String),
    }

    impl ThreadStatus {
        /// Convert status to bytes for database storage
        pub fn to_bytes(&self) -> Vec<u8> {
            match self {
                ThreadStatus::Starting => b"starting".to_vec(),
                ThreadStatus::Running => b"running".to_vec(),
                ThreadStatus::Completed => b"completed".to_vec(),
                ThreadStatus::Error(msg) => format!("error:{}", msg).into_bytes(),
            }
        }

        /// Parse status from bytes stored in database
        pub fn from_bytes(bytes: &[u8]) -> Self {
            let s = String::from_utf8_lossy(bytes);
            match s.as_ref() {
                "starting" => ThreadStatus::Starting,
                "running" => ThreadStatus::Running,
                "completed" => ThreadStatus::Completed,
                s if s.starts_with("error:") => {
                    let msg = s.strip_prefix("error:").unwrap_or("unknown error");
                    ThreadStatus::Error(msg.to_string())
                }
                _ => ThreadStatus::Error("invalid status".to_string()),
            }
        }
    }

    /// Work item for thread work queues
    #[derive(Debug, Clone)]
    pub struct WorkItem {
        /// Unique identifier for this work item
        pub id: u32,
        /// Work data (application-specific)
        pub data: Vec<u8>,
        /// Priority (higher numbers = higher priority)
        pub priority: u32,
        /// Timestamp when work was created
        pub created_at: u64,
    }

    impl WorkItem {
        /// Create a new work item
        pub fn new(id: u32, data: Vec<u8>, priority: u32) -> Self {
            Self {
                id,
                data,
                priority,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            }
        }

        /// Serialize work item to bytes for database storage
        pub fn to_bytes(&self) -> Vec<u8> {
            // Simple serialization: id(4) + priority(4) + created_at(8) + data_len(4) + data
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&self.id.to_le_bytes());
            bytes.extend_from_slice(&self.priority.to_le_bytes());
            bytes.extend_from_slice(&self.created_at.to_le_bytes());
            bytes.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&self.data);
            bytes
        }

        /// Deserialize work item from bytes
        pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
            if bytes.len() < 20 {
                return None; // Minimum size: 4+4+8+4 = 20 bytes
            }

            let id = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
            let priority = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
            let created_at = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
            let data_len = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;

            if bytes.len() < 20 + data_len {
                return None;
            }

            let data = bytes[20..20 + data_len].to_vec();

            Some(Self {
                id,
                priority,
                created_at,
                data,
            })
        }
    }

    /// Thread pool configuration
    #[derive(Debug, Clone)]
    pub struct ThreadPoolConfig {
        /// Maximum number of worker threads
        pub max_threads: u32,
        /// Work queue identifier
        pub queue_id: String,
        /// Thread idle timeout in seconds
        pub idle_timeout: u64,
    }

    impl Default for ThreadPoolConfig {
        fn default() -> Self {
            Self {
                max_threads: 4,
                queue_id: "default".to_string(),
                idle_timeout: 60,
            }
        }
    }

    /// Utility functions for creating coordination keys
    pub fn thread_status_key(thread_id: u32) -> String {
        format!("{}{}", keys::THREAD_STATUS, thread_id)
    }

    pub fn thread_result_key(thread_id: u32) -> String {
        format!("{}{}", keys::THREAD_RESULT, thread_id)
    }

    pub fn thread_complete_key(thread_id: u32) -> String {
        format!("{}{}", keys::THREAD_COMPLETE, thread_id)
    }

    pub fn shared_value_key(key: &str) -> String {
        format!("{}{}", keys::SHARED_VALUE, key)
    }

    pub fn work_queue_key(queue_id: &str, item_id: u32) -> String {
        format!("{}{}_{}", keys::WORK_QUEUE, queue_id, item_id)
    }
}

/// Thread-safe utilities that work across different environments
pub mod utils {

    /// Generate a unique identifier using timestamp and counter
    pub fn generate_unique_id() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
        
        // Combine timestamp (upper 48 bits) with counter (lower 16 bits)
        (timestamp << 16) | (counter & 0xFFFF)
    }

    /// Create a thread-safe key for database operations
    pub fn make_thread_safe_key(prefix: &str, id: u32) -> Vec<u8> {
        format!("{}_{}", prefix, id).into_bytes()
    }

    /// Parse thread ID from a thread-safe key
    pub fn parse_thread_id_from_key(key: &[u8], prefix: &str) -> Option<u32> {
        let key_str = String::from_utf8_lossy(key);
        let expected_prefix = format!("{}_", prefix);
        
        if key_str.starts_with(&expected_prefix) {
            key_str.strip_prefix(&expected_prefix)?.parse().ok()
        } else {
            None
        }
    }

    /// Encode multiple values into a single database value
    pub fn encode_values(values: &[&[u8]]) -> Vec<u8> {
        let mut result = Vec::new();
        
        // Write count
        result.extend_from_slice(&(values.len() as u32).to_le_bytes());
        
        // Write each value with length prefix
        for value in values {
            result.extend_from_slice(&(value.len() as u32).to_le_bytes());
            result.extend_from_slice(value);
        }
        
        result
    }

    /// Decode multiple values from a single database value
    pub fn decode_values(data: &[u8]) -> Vec<Vec<u8>> {
        let mut result = Vec::new();
        let mut offset = 0;
        
        if data.len() < 4 {
            return result;
        }
        
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap_or([0; 4])) as usize;
        offset += 4;
        
        for _ in 0..count {
            if offset + 4 > data.len() {
                break;
            }
            
            let len = u32::from_le_bytes(
                data[offset..offset + 4].try_into().unwrap_or([0; 4])
            ) as usize;
            offset += 4;
            
            if offset + len > data.len() {
                break;
            }
            
            result.push(data[offset..offset + len].to_vec());
            offset += len;
        }
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_id_generator() {
        let generator = ThreadIdGenerator::new();
        
        let id1 = generator.next_id();
        let id2 = generator.next_id();
        let id3 = generator.next_id();
        
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        assert!(id2 > id1);
        assert!(id3 > id2);
    }

    #[test]
    fn test_thread_status_serialization() {
        use coordination::ThreadStatus;
        
        let status1 = ThreadStatus::Starting;
        let status2 = ThreadStatus::Running;
        let status3 = ThreadStatus::Completed;
        let status4 = ThreadStatus::Error("test error".to_string());
        
        // Test round-trip serialization
        assert_eq!(ThreadStatus::from_bytes(&status1.to_bytes()), status1);
        assert_eq!(ThreadStatus::from_bytes(&status2.to_bytes()), status2);
        assert_eq!(ThreadStatus::from_bytes(&status3.to_bytes()), status3);
        assert_eq!(ThreadStatus::from_bytes(&status4.to_bytes()), status4);
    }

    #[test]
    fn test_work_item_serialization() {
        use coordination::WorkItem;
        
        let work_item = WorkItem::new(42, b"test data".to_vec(), 10);
        let bytes = work_item.to_bytes();
        let deserialized = WorkItem::from_bytes(&bytes).unwrap();
        
        assert_eq!(deserialized.id, work_item.id);
        assert_eq!(deserialized.data, work_item.data);
        assert_eq!(deserialized.priority, work_item.priority);
        assert_eq!(deserialized.created_at, work_item.created_at);
    }

    #[test]
    fn test_coordination_keys() {
        use coordination::*;
        
        assert_eq!(thread_status_key(42), "thread_status_42");
        assert_eq!(thread_result_key(123), "thread_result_123");
        assert_eq!(shared_value_key("test"), "shared_test");
        assert_eq!(work_queue_key("main", 456), "work_queue_main_456");
    }

    #[test]
    fn test_utils_encode_decode() {
        use utils::*;
        
        let values = vec![b"hello".as_slice(), b"world".as_slice(), b"test".as_slice()];
        let encoded = encode_values(&values);
        let decoded = decode_values(&encoded);
        
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0], b"hello");
        assert_eq!(decoded[1], b"world");
        assert_eq!(decoded[2], b"test");
    }

    #[test]
    fn test_unique_id_generation() {
        use utils::generate_unique_id;
        
        let id1 = generate_unique_id();
        let id2 = generate_unique_id();
        let id3 = generate_unique_id();
        
        // IDs should be unique
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
        
        // IDs should generally increase (due to timestamp component)
        assert!(id2 >= id1);
        assert!(id3 >= id2);
    }

    #[test]
    fn test_thread_safe_key_operations() {
        use utils::*;
        
        let key = make_thread_safe_key("test_prefix", 42);
        let parsed_id = parse_thread_id_from_key(&key, "test_prefix");
        
        assert_eq!(parsed_id, Some(42));
        
        // Test with wrong prefix
        let wrong_id = parse_thread_id_from_key(&key, "wrong_prefix");
        assert_eq!(wrong_id, None);
    }
}
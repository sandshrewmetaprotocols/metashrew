//! Protocol Buffer definitions for Metashrew.
//!
//! This module contains the Protocol Buffer definitions used by Metashrew.

use protobuf::Message;

/// The metashrew module contains Protocol Buffer message definitions
pub mod metashrew {
    use protobuf::Message;
    
    /// KeyValueFlush is used to send key-value pairs to the host for storage
    #[derive(Clone, PartialEq, Message)]
    pub struct KeyValueFlush {
        #[prost(bytes, repeated, tag = "1")]
        pub list: ::std::vec::Vec<::std::vec::Vec<u8>>,
    }
    
    impl KeyValueFlush {
        /// Create a new KeyValueFlush message
        pub fn new() -> Self {
            Self {
                list: ::std::vec::Vec::new(),
            }
        }
        
        /// Add a key-value pair to the message
        pub fn add_pair(&mut self, key: Vec<u8>, value: Vec<u8>) {
            self.list.push(key);
            self.list.push(value);
        }
        
        /// Get the number of key-value pairs in the message
        pub fn pair_count(&self) -> usize {
            self.list.len() / 2
        }
    }
}

/// Utility functions for working with Protocol Buffers
pub mod utils {
    use anyhow::Result;
    use protobuf::Message;
    
    /// Serialize a Protocol Buffer message to bytes
    pub fn serialize<T: Message>(message: &T) -> Result<Vec<u8>> {
        Ok(message.write_to_bytes()?)
    }
    
    /// Deserialize bytes to a Protocol Buffer message
    pub fn deserialize<T: Message>(bytes: &[u8]) -> Result<T> {
        Ok(T::parse_from_bytes(bytes)?)
    }
}
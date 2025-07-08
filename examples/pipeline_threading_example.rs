//! Pipeline Threading Example
//!
//! This example demonstrates the complete pipeline threading interface for Metashrew,
//! showing how to spawn threads with custom entrypoints, load input data, process
//! protocol messages, and retrieve results from thread memory.
//!
//! # Pipeline Workflow
//!
//! 1. **Spawn Pipeline Thread**: Use `thread_spawn_pipeline("__pipeline", &input_data)`
//! 2. **Thread Loads Input**: Thread calls `host_len()` and `load_input()` to get data
//! 3. **Process Protocol Messages**: Thread processes the input data
//! 4. **Return Result Length**: Thread returns i32 indicating result data length
//! 5. **Read Thread Memory**: Host calls `read_thread_memory()` to get result data
//! 6. **Cleanup**: Host calls `thread_free()` to clean up thread resources

use metashrew_core::wasi_threads::{
    thread_spawn_pipeline, thread_wait_result, read_thread_memory, thread_free,
    host_len, load_input, ThreadError
};

/// Example protocol message structure
#[derive(Debug, Clone)]
pub struct ProtocolMessage {
    pub message_type: u8,
    pub data: Vec<u8>,
}

impl ProtocolMessage {
    /// Serialize protocol message to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::new();
        result.push(self.message_type);
        result.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        result.extend_from_slice(&self.data);
        result
    }

    /// Deserialize protocol message from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 5 {
            return None;
        }
        
        let message_type = bytes[0];
        let data_len = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
        
        if bytes.len() < 5 + data_len {
            return None;
        }
        
        let data = bytes[5..5 + data_len].to_vec();
        
        Some(ProtocolMessage {
            message_type,
            data,
        })
    }
}

/// Host-side pipeline execution example
///
/// This function demonstrates how to use the pipeline threading interface
/// from the host side to process protocol messages in a separate thread.
pub fn execute_pipeline_workflow() -> Result<Vec<u8>, ThreadError> {
    // 1. Prepare protocol messages for processing
    let messages = vec![
        ProtocolMessage {
            message_type: 1,
            data: b"Hello from message 1".to_vec(),
        },
        ProtocolMessage {
            message_type: 2,
            data: b"Hello from message 2".to_vec(),
        },
        ProtocolMessage {
            message_type: 3,
            data: b"Hello from message 3".to_vec(),
        },
    ];

    // 2. Serialize messages into input data
    let mut input_data = Vec::new();
    input_data.extend_from_slice(&(messages.len() as u32).to_le_bytes());
    
    for message in &messages {
        let message_bytes = message.to_bytes();
        input_data.extend_from_slice(&message_bytes);
    }

    println!("Prepared {} protocol messages ({} bytes total)", 
             messages.len(), input_data.len());

    // 3. Spawn pipeline thread with custom entrypoint
    let thread_id = thread_spawn_pipeline("__pipeline", &input_data)?;
    println!("Spawned pipeline thread with ID: {}", thread_id);

    // 4. Wait for thread completion (with 10 second timeout)
    let result_length = thread_wait_result(thread_id, 10000)?;
    println!("Thread completed, result length: {}", result_length);

    // 5. Read result data from thread memory
    let result_data = if result_length > 0 {
        read_thread_memory(thread_id, 0, result_length as u32)?
    } else {
        Vec::new()
    };

    println!("Retrieved {} bytes of result data", result_data.len());

    // 6. Clean up thread resources
    thread_free(thread_id)?;
    println!("Thread {} cleaned up successfully", thread_id);

    Ok(result_data)
}

/// Guest-side pipeline implementation
///
/// This function would be implemented in the WASM module and called
/// when a pipeline thread is spawned. It demonstrates the complete
/// pipeline workflow from the thread's perspective.
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{host_len, load_input};
/// use metashrew_core::{initialize, set, flush};
/// use std::sync::Arc;
///
/// #[no_mangle]
/// pub extern "C" fn __pipeline() -> i32 {
///     // Initialize the thread's cache system
///     initialize();
///     
///     // Get input data length from host
///     let input_len = host_len();
///     if input_len == 0 {
///         return 0; // No input data
///     }
///     
///     // Allocate buffer and load input data
///     let mut input_buffer = vec![0u8; input_len as usize];
///     let loaded = load_input(input_buffer.as_mut_ptr(), input_len);
///     
///     if loaded != input_len {
///         return -1; // Failed to load all input data
///     }
///     
///     // Process protocol messages
///     let result_data = process_protocol_messages(&input_buffer);
///     
///     // Store result data in memory at a known location (e.g., offset 0)
///     // In a real implementation, you would write result_data to memory
///     // and return the length so the host can read it back
///     
///     // For this example, return the length of processed data
///     result_data.len() as i32
/// }
/// ```
#[no_mangle]
pub extern "C" fn __pipeline() -> i32 {
    // This is a mock implementation for documentation purposes
    // In a real WASM environment, this would be implemented as shown above
    
    // Simulate processing and return result length
    1024
}

/// Process protocol messages and generate result data
///
/// This function demonstrates how to parse and process protocol messages
/// within the pipeline thread.
pub fn process_protocol_messages(input_data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut offset = 0;

    // Read number of messages
    if input_data.len() < 4 {
        return result;
    }
    
    let message_count = u32::from_le_bytes([
        input_data[0], input_data[1], input_data[2], input_data[3]
    ]);
    offset += 4;

    println!("Processing {} protocol messages", message_count);

    // Process each message
    for i in 0..message_count {
        if let Some(message) = ProtocolMessage::from_bytes(&input_data[offset..]) {
            println!("Processing message {}: type={}, data_len={}", 
                     i + 1, message.message_type, message.data.len());
            
            // Process the message (example: reverse the data)
            let processed_data: Vec<u8> = message.data.iter().rev().cloned().collect();
            
            // Add processed result to output
            result.push(message.message_type);
            result.extend_from_slice(&(processed_data.len() as u32).to_le_bytes());
            result.extend_from_slice(&processed_data);
            
            // Move to next message
            offset += 1 + 4 + message.data.len();
        } else {
            break;
        }
    }

    println!("Generated {} bytes of result data", result.len());
    result
}

/// Complete pipeline example with error handling
pub fn run_complete_pipeline_example() {
    println!("=== Pipeline Threading Example ===");
    
    match execute_pipeline_workflow() {
        Ok(result_data) => {
            println!("Pipeline completed successfully!");
            println!("Result data: {} bytes", result_data.len());
            
            // Parse and display results
            if !result_data.is_empty() {
                println!("First 64 bytes of result: {:?}", 
                         &result_data[..std::cmp::min(64, result_data.len())]);
            }
        }
        Err(e) => {
            println!("Pipeline failed: {:?}", e);
        }
    }
}

/// Example of batch pipeline processing
///
/// This demonstrates processing multiple pipeline jobs concurrently
/// and collecting all results.
pub fn run_batch_pipeline_example() -> Result<Vec<Vec<u8>>, ThreadError> {
    println!("=== Batch Pipeline Example ===");
    
    let batch_inputs = vec![
        vec![1, 2, 3, 4, 5],
        vec![10, 20, 30, 40, 50],
        vec![100, 200, 300, 400, 500],
    ];
    
    let mut thread_ids = Vec::new();
    
    // Spawn multiple pipeline threads
    for (i, input_data) in batch_inputs.iter().enumerate() {
        let thread_id = thread_spawn_pipeline("__pipeline", input_data)?;
        thread_ids.push(thread_id);
        println!("Spawned batch thread {} with ID: {}", i + 1, thread_id);
    }
    
    let mut results = Vec::new();
    
    // Collect results from all threads
    for (i, thread_id) in thread_ids.iter().enumerate() {
        println!("Waiting for batch thread {} (ID: {})", i + 1, thread_id);
        
        let result_length = thread_wait_result(*thread_id, 5000)?;
        let result_data = if result_length > 0 {
            read_thread_memory(*thread_id, 0, result_length as u32)?
        } else {
            Vec::new()
        };
        
        results.push(result_data);
        thread_free(*thread_id)?;
        
        println!("Batch thread {} completed: {} bytes", i + 1, result_length);
    }
    
    println!("All batch threads completed successfully!");
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_message_serialization() {
        let message = ProtocolMessage {
            message_type: 42,
            data: b"test data".to_vec(),
        };
        
        let bytes = message.to_bytes();
        let deserialized = ProtocolMessage::from_bytes(&bytes).unwrap();
        
        assert_eq!(deserialized.message_type, 42);
        assert_eq!(deserialized.data, b"test data");
    }
    
    #[test]
    fn test_process_protocol_messages() {
        let messages = vec![
            ProtocolMessage {
                message_type: 1,
                data: b"hello".to_vec(),
            },
        ];
        
        let mut input_data = Vec::new();
        input_data.extend_from_slice(&1u32.to_le_bytes());
        input_data.extend_from_slice(&messages[0].to_bytes());
        
        let result = process_protocol_messages(&input_data);
        assert!(!result.is_empty());
    }
}

fn main() {
    // Run the complete pipeline example
    run_complete_pipeline_example();
    
    // Run the batch pipeline example
    match run_batch_pipeline_example() {
        Ok(results) => {
            println!("Batch processing completed: {} results", results.len());
        }
        Err(e) => {
            println!("Batch processing failed: {:?}", e);
        }
    }
}
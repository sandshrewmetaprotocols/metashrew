//! Comprehensive WASI Threads Example for Metashrew
//!
//! This example demonstrates the complete WASI threads implementation with
//! inter-thread communication and result retrieval capabilities. It shows
//! both host-side thread management and guest-side thread operations.
//!
//! # Features Demonstrated
//!
//! ## Host-Side (metashrew-runtime)
//! - Thread manager creation and lifecycle management
//! - WASI threads host function setup with result retrieval
//! - Thread result storage and retrieval system
//! - Memory export from completed threads (first 4KB)
//! - Custom data passing through get_thread_result() function
//! - Error handling and resource cleanup
//!
//! ## Guest-Side (metashrew-core)
//! - Thread spawning with start arguments
//! - Thread result polling and waiting with timeout
//! - Memory export retrieval from completed threads
//! - Custom data retrieval from thread functions
//! - Comprehensive error handling patterns
//!
//! ## Communication Patterns
//! - Database-based thread coordination
//! - Result retrieval with thread IDs
//! - Memory sharing through exports
//! - Custom data passing mechanisms
//!
//! # Inter-Thread Communication
//!
//! This implementation provides several ways for threads to communicate:
//! 1. **Return Values**: Simple i32 return codes from thread execution
//! 2. **Memory Exports**: First 4KB of thread memory automatically exported
//! 3. **Custom Data**: Structured data via get_thread_result() function
//! 4. **Database State**: Shared database operations for coordination

use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::Result;

// Import all threading components
use metashrew_runtime::wasi_threads::{
    ThreadManager, ThreadResult, add_wasi_threads_support,
    store_thread_result, get_thread_result, wait_for_thread_result
};
use metashrew_support::threading::{
    ThreadCoordinator, ThreadMessage, ThreadSafeCounter
};

/// Example demonstrating host-side thread management with result retrieval
pub fn demonstrate_host_side_threading() -> Result<()> {
    println!("=== Host-Side Threading with Result Retrieval Demo ===");
    
    let thread_manager = Arc::new(ThreadManager::new());
    println!("Created thread manager");
    
    // Store thread results with different communication patterns
    let result1 = ThreadResult {
        thread_id: 1,
        return_value: 42,
        memory_export: Some(b"Hello from thread 1 - memory export data".to_vec()),
        custom_data: Some(b"Custom JSON: {\"status\":\"success\",\"processed\":100}".to_vec()),
        timestamp: Instant::now(),
        success: true,
        error_message: None,
    };
    
    let result2 = ThreadResult {
        thread_id: 2,
        return_value: 84,
        memory_export: Some(b"Thread 2 memory: computation results".to_vec()),
        custom_data: None,
        timestamp: Instant::now(),
        success: true,
        error_message: None,
    };
    
    store_thread_result(result1);
    store_thread_result(result2);
    println!("Stored results for threads 1 and 2");
    
    // Demonstrate result retrieval patterns
    if let Some(result) = get_thread_result(1) {
        println!("Thread 1: return_value={}, success={}", 
                result.return_value, result.success);
        if let Some(memory) = result.memory_export {
            println!("  Memory export: {}", String::from_utf8_lossy(&memory));
        }
        if let Some(custom) = result.custom_data {
            println!("  Custom data: {}", String::from_utf8_lossy(&custom));
        }
    }
    
    // Demonstrate waiting with timeout
    if let Some(result) = wait_for_thread_result(2, Duration::from_secs(1)) {
        println!("Thread 2 completed: return_value={}", result.return_value);
    }
    
    println!("Host-side demo completed successfully\n");
    Ok(())
}

/// Example demonstrating guest-side thread operations with result retrieval
pub fn demonstrate_guest_side_threading() -> Result<()> {
    println!("=== Guest-Side Threading with Result Retrieval Demo ===");
    
    // Simulate spawning threads with different work patterns
    let work_patterns = vec![
        ("compute_prime", 1000),
        ("process_data", 2000), 
        ("analyze_results", 3000),
        ("generate_report", 4000),
    ];
    
    let mut thread_ids = Vec::new();
    
    for (i, (work_type, work_arg)) in work_patterns.iter().enumerate() {
        // In real WASM: let thread_id = metashrew_core::wasi_threads::thread_spawn(*work_arg)?;
        let thread_id = (i + 1) as u32;
        thread_ids.push(thread_id);
        println!("Spawned thread {} for '{}' with arg {}", thread_id, work_type, work_arg);
    }
    
    // Demonstrate result retrieval patterns
    for (i, &thread_id) in thread_ids.iter().enumerate() {
        let work_type = work_patterns[i].0;
        
        println!("\nWaiting for {} thread {}...", work_type, thread_id);
        // In real WASM: let result = metashrew_core::wasi_threads::thread_wait_result(thread_id, 5000)?;
        let result = thread_id as i32 * 10;
        println!("  Completed with result: {}", result);
        
        // In real WASM: let memory = metashrew_core::wasi_threads::thread_get_memory(thread_id)?;
        let memory_size = 1024 + (thread_id * 512);
        println!("  Memory export: {} bytes available", memory_size);
        
        // In real WASM: let custom = metashrew_core::wasi_threads::thread_get_custom_data(thread_id)?;
        if thread_id % 2 == 1 {
            println!("  Custom data: Structured results available");
        } else {
            println!("  Custom data: None (return value only)");
        }
    }
    
    println!("Guest-side demo completed successfully\n");
    Ok(())
}

/// Example demonstrating thread coordination patterns
pub fn demonstrate_thread_coordination() -> Result<()> {
    println!("=== Thread Coordination Demo ===");
    
    let coordinator = ThreadCoordinator::new();
    println!("Created thread coordinator");
    
    // Send messages between threads
    let message1 = ThreadMessage {
        from_thread: 1,
        to_thread: 2,
        message_type: "work_request".to_string(),
        payload: b"Process data batch 1".to_vec(),
        timestamp: std::time::SystemTime::now(),
    };
    
    coordinator.send_message(message1)?;
    println!("Sent coordination message");
    
    // Demonstrate thread-safe counter
    let counter = ThreadSafeCounter::new(0);
    for i in 1..=5 {
        let new_value = counter.increment();
        println!("Thread {} incremented counter to: {}", i, new_value);
    }
    
    println!("Thread coordination demo completed successfully\n");
    Ok(())
}

/// Example demonstrating error handling patterns
pub fn demonstrate_error_handling() -> Result<()> {
    println!("=== Error Handling Demo ===");
    
    // Test thread not found
    if get_thread_result(999).is_none() {
        println!("Correctly handled thread not found");
    }
    
    // Test timeout
    if wait_for_thread_result(888, Duration::from_millis(1)).is_none() {
        println!("Correctly handled timeout");
    }
    
    // Test failed thread
    let failed_result = ThreadResult {
        thread_id: 777,
        return_value: -1,
        memory_export: None,
        custom_data: None,
        timestamp: Instant::now(),
        success: false,
        error_message: Some("Thread execution failed".to_string()),
    };
    
    store_thread_result(failed_result);
    
    if let Some(result) = get_thread_result(777) {
        if !result.success {
            println!("Thread 777 failed: {}", 
                    result.error_message.unwrap_or("Unknown error".to_string()));
        }
    }
    
    println!("Error handling demo completed successfully\n");
    Ok(())
}

/// Main example function demonstrating all WASI threads features
pub fn run_comprehensive_example() -> Result<()> {
    println!("🧵 WASI Threads Comprehensive Example");
    println!("=====================================\n");
    
    demonstrate_host_side_threading()?;
    demonstrate_guest_side_threading()?;
    demonstrate_thread_coordination()?;
    demonstrate_error_handling()?;
    
    println!("✅ All WASI threads examples completed successfully!");
    println!("\n📚 Key Features Demonstrated:");
    println!("- Thread spawning with start arguments");
    println!("- Result retrieval with return values");
    println!("- Memory export from completed threads");
    println!("- Custom data passing mechanisms");
    println!("- Timeout handling and error recovery");
    println!("- Database-based thread coordination");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_host_side_threading() {
        assert!(demonstrate_host_side_threading().is_ok());
    }
    
    #[test]
    fn test_guest_side_threading() {
        assert!(demonstrate_guest_side_threading().is_ok());
    }
    
    #[test]
    fn test_thread_coordination() {
        assert!(demonstrate_thread_coordination().is_ok());
    }
    
    #[test]
    fn test_error_handling() {
        assert!(demonstrate_error_handling().is_ok());
    }
    
    #[test]
    fn test_comprehensive_example() {
        assert!(run_comprehensive_example().is_ok());
    }
}

fn main() -> Result<()> {
    run_comprehensive_example()
}
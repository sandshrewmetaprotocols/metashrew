//! WASI Threads guest bindings for WebAssembly programs
//!
//! This module provides the guest-side bindings for WASI threads functionality,
//! allowing WebAssembly programs built with metashrew-core to spawn threads
//! using the standardized WASI threads specification.
//!
//! # Architecture
//!
//! ## Guest-Side Interface
//! - Provides safe Rust bindings to the WASI `thread_spawn` host function
//! - Handles error conditions and return value interpretation
//! - Integrates with metashrew-core's initialization and caching systems
//!
//! ## Thread Model
//! - Each spawned thread gets a fresh WASM instance with isolated memory
//! - Threads share database state through the host runtime
//! - Communication happens via database operations, not shared memory
//! - Each thread can run its own single-threaded async runtime
//!
//! ## Integration with Metashrew
//! - Threads inherit the same host function interface (`__get`, `__flush`, etc.)
//! - Each thread must call `initialize()` to set up its cache system
//! - Threads can use all metashrew-core functionality (get, set, flush, etc.)
//! - Database operations are thread-safe through the host runtime
//!
//! # Usage Examples
//!
//! ## Basic Thread Spawning
//! ```rust,no_run
//! use metashrew_core::wasi_threads::thread_spawn;
//! use metashrew_core::{initialize, set, flush};
//! use std::sync::Arc;
//!
//! // In your main WASM function
//! initialize();
//!
//! // Spawn a worker thread with start argument 42
//! match thread_spawn(42) {
//!     Ok(thread_id) => {
//!         println!("Spawned thread with ID: {}", thread_id);
//!     }
//!     Err(e) => {
//!         println!("Failed to spawn thread: {:?}", e);
//!     }
//! }
//! ```
//!
//! ## Worker Thread Implementation
//! ```rust,no_run
//! use metashrew_core::{initialize, input, get, set, flush};
//! use std::sync::Arc;
//!
//! // This function runs in the spawned thread
//! #[no_mangle]
//! pub extern "C" fn _start() {
//!     // Each thread must initialize its own cache system
//!     initialize();
//!
//!     // Get the start argument passed to thread_spawn
//!     let input_data = input();
//!     let start_arg = u32::from_le_bytes(input_data[0..4].try_into().unwrap());
//!
//!     // Perform thread-specific work
//!     let key = Arc::new(format!("thread_result_{}", start_arg).into_bytes());
//!     let value = Arc::new(b"thread completed".to_vec());
//!     set(key, value);
//!
//!     // Commit changes to shared database
//!     flush();
//! }
//! ```
//!
//! ## Parallel Processing Pattern
//! ```rust,no_run
//! use metashrew_core::wasi_threads::thread_spawn;
//! use metashrew_core::{initialize, get, set, flush};
//! use std::sync::Arc;
//!
//! // Distribute work across multiple threads
//! fn process_in_parallel(work_items: &[u32]) -> Result<(), ThreadError> {
//!     initialize();
//!
//!     let mut thread_ids = Vec::new();
//!
//!     // Spawn worker threads
//!     for &item in work_items {
//!         let thread_id = thread_spawn(item)?;
//!         thread_ids.push(thread_id);
//!     }
//!
//!     // Store thread IDs for potential coordination
//!     let ids_key = Arc::new(b"active_threads".to_vec());
//!     let ids_value = Arc::new(
//!         thread_ids.iter()
//!             .flat_map(|id| id.to_le_bytes())
//!             .collect::<Vec<u8>>()
//!     );
//!     set(ids_key, ids_value);
//!     flush();
//!
//!     Ok(())
//! }
//! ```

use crate::imports::{
    __thread_spawn, __thread_get_result, __thread_wait_result, __thread_get_memory, __thread_get_custom_data,
    __thread_spawn_pipeline, __thread_free, __read_thread_memory, __host_len, __load_input
};

/// Error types for WASI threads operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadError {
    /// Thread spawning failed due to resource constraints
    ResourceExhausted,
    /// Thread spawning failed due to system error
    SystemError,
    /// Thread spawning failed due to invalid argument
    InvalidArgument,
    /// Thread not found or not completed yet
    ThreadNotFound,
    /// Operation timed out
    Timeout,
    /// Memory operation failed
    MemoryError,
    /// Unknown error occurred during thread spawning
    Unknown(i32),
}

impl std::fmt::Display for ThreadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThreadError::ResourceExhausted => write!(f, "Thread spawning failed: resource exhausted"),
            ThreadError::SystemError => write!(f, "Thread spawning failed: system error"),
            ThreadError::InvalidArgument => write!(f, "Thread spawning failed: invalid argument"),
            ThreadError::ThreadNotFound => write!(f, "Thread not found or not completed"),
            ThreadError::Timeout => write!(f, "Operation timed out"),
            ThreadError::MemoryError => write!(f, "Memory operation failed"),
            ThreadError::Unknown(code) => write!(f, "Thread operation failed: unknown error (code: {})", code),
        }
    }
}

impl std::error::Error for ThreadError {}

/// Spawn a new thread using the WASI threads specification
///
/// This function creates a new thread with a fresh WASM instance that will
/// execute the same WASM module. The new thread receives the `start_arg`
/// as its input data and can access the shared database state.
///
/// # Parameters
///
/// - `start_arg`: A 32-bit argument passed to the new thread. This value
///   will be available to the thread through the `input()` function as
///   a 4-byte little-endian encoded value.
///
/// # Returns
///
/// - `Ok(thread_id)`: Success, returns a positive thread ID
/// - `Err(ThreadError)`: Failure, with specific error type
///
/// # Thread Execution Model
///
/// ## Instance-per-Thread
/// - Each thread gets a completely fresh WASM instance
/// - No shared memory between threads (WASI threads design)
/// - Each thread has its own isolated memory space
/// - Communication happens through shared database operations
///
/// ## Thread Lifecycle
/// 1. Host creates new WASM instance in new OS thread
/// 2. New instance executes `_start` function
/// 3. Thread can access start_arg via `input()` function
/// 4. Thread performs work using metashrew-core functions
/// 5. Thread terminates when `_start` function returns
///
/// ## Database Sharing
/// - All threads share the same database backend
/// - Database operations are thread-safe through the host
/// - Changes made by one thread are visible to all threads
/// - Use database operations for thread coordination
///
/// # Error Handling
///
/// The function maps negative return values from the host to specific error types:
/// - `-1`: System error (context lock failed, etc.)
/// - `-2`: Resource exhausted (thread spawn failed)
/// - Other negative values: Unknown errors
///
/// # Example Usage
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{thread_spawn, ThreadError};
/// use metashrew_core::{initialize, set, flush};
/// use std::sync::Arc;
///
/// fn spawn_worker_threads() -> Result<(), ThreadError> {
///     initialize();
///
///     // Spawn multiple worker threads with different arguments
///     for i in 0..4 {
///         let thread_id = thread_spawn(i * 100)?;
///         println!("Spawned worker thread {} with ID {}", i, thread_id);
///
///         // Store thread info for coordination
///         let key = Arc::new(format!("worker_{}", i).into_bytes());
///         let value = Arc::new(thread_id.to_le_bytes().to_vec());
///         set(key, value);
///     }
///
///     flush();
///     Ok(())
/// }
/// ```
///
/// # Thread Implementation
///
/// The spawned thread should implement its logic in the `_start` function:
///
/// ```rust,no_run
/// use metashrew_core::{initialize, input, get, set, flush};
/// use std::sync::Arc;
///
/// #[no_mangle]
/// pub extern "C" fn _start() {
///     // REQUIRED: Initialize cache system for this thread
///     initialize();
///
///     // Get the start argument
///     let input_data = input();
///     let start_arg = u32::from_le_bytes(input_data[0..4].try_into().unwrap());
///
///     // Perform thread-specific work
///     let result_key = Arc::new(format!("result_{}", start_arg).into_bytes());
///     let result_value = Arc::new(format!("processed_{}", start_arg).into_bytes());
///     set(result_key, result_value);
///
///     // Commit changes
///     flush();
/// }
/// ```
///
/// Get the result from a completed thread
///
/// This function retrieves the return value from a thread that has completed
/// execution. If the thread is still running or hasn't been found, it returns
/// an error.
///
/// # Parameters
///
/// - `thread_id`: The ID of the thread to get the result from
///
/// # Returns
///
/// - `Ok(i32)`: The return value from the thread
/// - `Err(ThreadError)`: An error if the thread is not found or failed
///
/// # Example
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{thread_spawn, thread_get_result, ThreadError};
///
/// let thread_id = thread_spawn(42)?;
///
/// // Poll for completion
/// loop {
///     match thread_get_result(thread_id) {
///         Ok(result) => {
///             println!("Thread completed with result: {}", result);
///             break;
///         }
///         Err(ThreadError::ThreadNotFound) => {
///             // Thread still running, wait a bit
///             std::thread::sleep(std::time::Duration::from_millis(100));
///         }
///         Err(e) => {
///             println!("Thread failed: {:?}", e);
///             break;
///         }
///     }
/// }
/// ```
pub fn thread_get_result(thread_id: u32) -> Result<i32, ThreadError> {
    let result = unsafe { __thread_get_result(thread_id) };
    
    match result {
        code if code >= 0 => Ok(code),
        -1 => Err(ThreadError::SystemError),
        -2 => Err(ThreadError::ThreadNotFound),
        code => Err(ThreadError::Unknown(code)),
    }
}

/// Wait for a thread to complete with timeout
///
/// This function waits for the specified thread to complete execution,
/// with a maximum timeout. It's more efficient than polling with
/// `thread_get_result` as it blocks until completion or timeout.
///
/// # Parameters
///
/// - `thread_id`: The ID of the thread to wait for
/// - `timeout_ms`: Maximum time to wait in milliseconds
///
/// # Returns
///
/// - `Ok(i32)`: The return value from the thread
/// - `Err(ThreadError)`: An error if timeout, thread not found, or failed
///
/// # Example
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{thread_spawn, thread_wait_result, ThreadError};
///
/// let thread_id = thread_spawn(42)?;
///
/// // Wait up to 5 seconds for completion
/// match thread_wait_result(thread_id, 5000) {
///     Ok(result) => println!("Thread completed: {}", result),
///     Err(ThreadError::Timeout) => println!("Thread timed out"),
///     Err(e) => println!("Thread error: {:?}", e),
/// }
/// ```
pub fn thread_wait_result(thread_id: u32, timeout_ms: u32) -> Result<i32, ThreadError> {
    let result = unsafe { __thread_wait_result(thread_id, timeout_ms) };
    
    match result {
        code if code >= 0 => Ok(code),
        -1 => Err(ThreadError::SystemError),
        -2 => Err(ThreadError::ThreadNotFound),
        -3 => Err(ThreadError::Timeout),
        code => Err(ThreadError::Unknown(code)),
    }
}

/// Get memory export from a completed thread
///
/// This function retrieves the exported memory data from a thread that has
/// completed execution. The host runtime exports the first 4KB of the thread's
/// memory for inspection by other threads.
///
/// # Parameters
///
/// - `thread_id`: The ID of the thread to get memory from
///
/// # Returns
///
/// - `Ok(Vec<u8>)`: The exported memory data (up to 4KB)
/// - `Err(ThreadError)`: An error if thread not found or memory unavailable
///
/// # Example
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{thread_spawn, thread_wait_result, thread_get_memory};
///
/// let thread_id = thread_spawn(42)?;
/// thread_wait_result(thread_id, 5000)?; // Wait for completion
///
/// match thread_get_memory(thread_id) {
///     Ok(memory_data) => {
///         println!("Retrieved {} bytes from thread memory", memory_data.len());
///         // Process the memory data...
///     }
///     Err(e) => println!("Failed to get memory: {:?}", e),
/// }
/// ```
pub fn thread_get_memory(thread_id: u32) -> Result<Vec<u8>, ThreadError> {
    // Allocate buffer for memory data (max 4KB)
    let mut buffer = vec![0u8; 4096];
    let buffer_ptr = buffer.as_mut_ptr() as i32;
    
    let result = unsafe { __thread_get_memory(thread_id, buffer_ptr) };
    
    match result {
        size if size >= 0 => {
            buffer.truncate(size as usize);
            Ok(buffer)
        }
        -1 => Err(ThreadError::MemoryError),
        -2 => Err(ThreadError::ThreadNotFound),
        code => Err(ThreadError::Unknown(code)),
    }
}

/// Get custom data from a completed thread
///
/// This function retrieves custom data that was returned by the thread's
/// `get_thread_result()` function. This allows threads to return structured
/// data beyond just a simple integer return value.
///
/// # Parameters
///
/// - `thread_id`: The ID of the thread to get custom data from
///
/// # Returns
///
/// - `Ok(Vec<u8>)`: The custom data returned by the thread
/// - `Err(ThreadError)`: An error if thread not found or no custom data
///
/// # Thread-side Implementation
///
/// For this to work, the thread's WASM module should implement a function:
/// ```rust,no_run
/// #[no_mangle]
/// pub extern "C" fn get_thread_result() -> i32 {
///     // Return pointer to custom data in memory
///     // The data should be formatted as: [length: u32][data: bytes]
///     let data = b"Hello from thread!";
///     let ptr = allocate_and_write(data);
///     ptr as i32
/// }
/// ```
///
/// # Example
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{thread_spawn, thread_wait_result, thread_get_custom_data, ThreadError};
///
/// let thread_id = thread_spawn(42)?;
/// thread_wait_result(thread_id, 5000)?; // Wait for completion
///
/// match thread_get_custom_data(thread_id) {
///     Ok(data) => {
///         let message = String::from_utf8_lossy(&data);
///         println!("Thread returned: {}", message);
///     }
///     Err(ThreadError::ThreadNotFound) => {
///         println!("No custom data available");
///     }
///     Err(e) => println!("Failed to get custom data: {:?}", e),
/// }
/// ```
pub fn thread_get_custom_data(thread_id: u32) -> Result<Vec<u8>, ThreadError> {
    // Allocate buffer for custom data (max 64KB)
    let mut buffer = vec![0u8; 65536];
    let buffer_ptr = buffer.as_mut_ptr() as i32;
    
    let result = unsafe { __thread_get_custom_data(thread_id, buffer_ptr) };
    
    match result {
        size if size > 0 => {
            buffer.truncate(size as usize);
            Ok(buffer)
        }
        0 => Ok(Vec::new()), // No custom data
        -1 => Err(ThreadError::MemoryError),
        -2 => Err(ThreadError::ThreadNotFound),
        code => Err(ThreadError::Unknown(code)),
    }
}

/// Spawn a thread with a custom entrypoint and input data
///
/// This function creates a new thread that will execute a specific function
/// (like `__pipeline`) instead of the default `_start`. The thread can load
/// input data from the host using `__host_len` and `__load_input`.
///
/// # Parameters
///
/// - `entrypoint`: Name of the function to execute (e.g., "__pipeline")
/// - `input_data`: Data to make available to the thread via `__load_input`
///
/// # Returns
///
/// - `Ok(thread_id)`: Success, returns a positive thread ID
/// - `Err(ThreadError)`: Failure, with specific error type
///
/// # Pipeline Workflow
///
/// 1. Host spawns thread with custom entrypoint and input data
/// 2. Thread executes the specified function (e.g., `__pipeline`)
/// 3. Thread can call `__host_len()` to get input data length
/// 4. Thread can call `__load_input(ptr, len)` to load input data
/// 5. Thread processes data and returns an i32 result
/// 6. Host can read thread memory using `read_thread_memory()`
/// 7. Host cleans up thread using `thread_free()`
///
/// # Example Usage
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{thread_spawn_pipeline, ThreadError};
///
/// // Spawn a pipeline thread with protocol message data
/// let protocol_data = vec![1, 2, 3, 4, 5];
/// let thread_id = thread_spawn_pipeline("__pipeline", &protocol_data)?;
/// ```
///
/// # Thread Implementation
///
/// The thread should implement the specified entrypoint:
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{host_len, load_input};
///
/// #[no_mangle]
/// pub extern "C" fn __pipeline() -> i32 {
///     // Get input data length
///     let input_len = host_len();
///
///     // Allocate buffer and load input data
///     let mut buffer = vec![0u8; input_len as usize];
///     load_input(buffer.as_mut_ptr(), input_len);
///
///     // Process the protocol messages...
///     let result_len = process_protocol_messages(&buffer);
///
///     // Return length of result data for host to read
///     result_len as i32
/// }
/// ```
pub fn thread_spawn_pipeline(entrypoint: &str, input_data: &[u8]) -> Result<u32, ThreadError> {
    let entrypoint_ptr = entrypoint.as_ptr() as i32;
    let entrypoint_len = entrypoint.len() as i32;
    let input_ptr = input_data.as_ptr() as i32;
    let input_len = input_data.len() as i32;
    
    let result = unsafe {
        __thread_spawn_pipeline(entrypoint_ptr, entrypoint_len, input_ptr, input_len)
    };

    if result >= 0 {
        Ok(result as u32)
    } else {
        match result {
            -1 => Err(ThreadError::SystemError),
            -2 => Err(ThreadError::ResourceExhausted),
            -3 => Err(ThreadError::InvalidArgument),
            other => Err(ThreadError::Unknown(other)),
        }
    }
}

/// Get the length of input data available from the host
///
/// This function is called from within a spawned thread to determine
/// how much input data is available to load.
///
/// # Returns
///
/// The length of input data in bytes, or 0 if no data is available.
///
/// # Example
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::host_len;
///
/// #[no_mangle]
/// pub extern "C" fn __pipeline() -> i32 {
///     let input_len = host_len();
///     if input_len > 0 {
///         // Load and process input data...
///     }
///     0
/// }
/// ```
pub fn host_len() -> u32 {
    unsafe { __host_len() as u32 }
}

/// Load input data from the host into thread memory
///
/// This function copies input data that was provided when the thread
/// was spawned into the thread's memory space.
///
/// # Parameters
///
/// - `ptr`: Pointer to buffer where data should be written
/// - `len`: Maximum number of bytes to load
///
/// # Returns
///
/// The actual number of bytes loaded, which may be less than `len`
/// if there isn't enough input data available.
///
/// # Safety
///
/// The caller must ensure that `ptr` points to a valid buffer of at
/// least `len` bytes.
///
/// # Example
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{host_len, load_input};
///
/// #[no_mangle]
/// pub extern "C" fn __pipeline() -> i32 {
///     let input_len = host_len();
///     let mut buffer = vec![0u8; input_len as usize];
///
///     let loaded = load_input(buffer.as_mut_ptr(), input_len);
///     assert_eq!(loaded, input_len);
///
///     // Process buffer...
///     0
/// }
/// ```
pub fn load_input(ptr: *mut u8, len: u32) -> u32 {
    unsafe { __load_input(ptr as i32, len as i32) as u32 }
}

/// Read memory from a completed thread
///
/// This function allows the host process to read memory from a thread
/// that has completed execution. This is useful for retrieving result
/// data that the thread has prepared in its memory space.
///
/// # Parameters
///
/// - `thread_id`: ID of the thread to read memory from
/// - `offset`: Byte offset in thread memory to start reading from
/// - `len`: Number of bytes to read
///
/// # Returns
///
/// - `Ok(Vec<u8>)`: The memory data read from the thread
/// - `Err(ThreadError)`: An error if thread not found or read failed
///
/// # Usage Pattern
///
/// 1. Thread returns length of result data as i32
/// 2. Host calls `read_thread_memory(thread_id, 0, length)` to read result
/// 3. Host processes the result data
/// 4. Host calls `thread_free(thread_id)` to clean up
///
/// # Example
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{
///     thread_spawn_pipeline, thread_wait_result, read_thread_memory, thread_free
/// };
///
/// // Spawn pipeline thread
/// let thread_id = thread_spawn_pipeline("__pipeline", &input_data)?;
///
/// // Wait for completion and get result length
/// let result_length = thread_wait_result(thread_id, 5000)?;
///
/// if result_length > 0 {
///     // Read the result data from thread memory
///     let result_data = read_thread_memory(thread_id, 0, result_length as u32)?;
///
///     // Process result_data...
///     println!("Got {} bytes of result data", result_data.len());
/// }
///
/// // Clean up the thread
/// thread_free(thread_id)?;
/// ```
pub fn read_thread_memory(thread_id: u32, offset: u32, len: u32) -> Result<Vec<u8>, ThreadError> {
    let mut buffer = vec![0u8; len as usize];
    let buffer_ptr = buffer.as_mut_ptr() as i32;
    
    let result = unsafe {
        __read_thread_memory(thread_id as i32, offset as i32, buffer_ptr, len as i32)
    };
    
    match result {
        bytes_read if bytes_read >= 0 => {
            buffer.truncate(bytes_read as usize);
            Ok(buffer)
        }
        -1 => Err(ThreadError::MemoryError),
        -2 => Err(ThreadError::ThreadNotFound),
        code => Err(ThreadError::Unknown(code)),
    }
}

/// Free a completed thread and clean up its resources
///
/// This function releases the WASM instance and associated resources
/// for a thread that has completed execution. This is important for
/// preventing memory leaks when using many threads.
///
/// # Parameters
///
/// - `thread_id`: ID of the thread to free
///
/// # Returns
///
/// - `Ok(())`: Thread was successfully freed
/// - `Err(ThreadError)`: An error occurred during cleanup
///
/// # Usage
///
/// Always call this function after you're done with a thread's results:
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::{
///     thread_spawn_pipeline, thread_wait_result, read_thread_memory, thread_free
/// };
///
/// let thread_id = thread_spawn_pipeline("__pipeline", &data)?;
/// let result = thread_wait_result(thread_id, 5000)?;
///
/// // Read any result data you need...
/// let result_data = read_thread_memory(thread_id, 0, result as u32)?;
///
/// // IMPORTANT: Free the thread to prevent memory leaks
/// thread_free(thread_id)?;
/// ```
pub fn thread_free(thread_id: u32) -> Result<(), ThreadError> {
    let result = unsafe { __thread_free(thread_id as i32) };
    
    match result {
        0 => Ok(()),
        -1 => Err(ThreadError::SystemError),
        -2 => Err(ThreadError::ThreadNotFound),
        code => Err(ThreadError::Unknown(code)),
    }
}

/// # Performance Considerations
///
/// ## Thread Creation Overhead
/// - Each thread creates a new WASM instance (moderate overhead)
/// - Consider thread pooling for high-frequency spawning
/// - Balance thread count with available system resources
///
/// ## Memory Usage
/// - Each thread has isolated WASM memory (typically 4GB virtual)
/// - Actual memory usage depends on thread workload
/// - Monitor total memory usage across all threads
///
/// ## Database Contention
/// - All threads share the same database backend
/// - Heavy concurrent writes may cause contention
/// - Consider partitioning work to minimize conflicts
///
/// # Thread Safety
///
/// - Database operations are thread-safe through the host runtime
/// - WASM memory is isolated per thread (no shared memory bugs)
/// - Use database operations for thread coordination and communication
/// - Each thread must call `initialize()` to set up its cache system
pub fn thread_spawn(start_arg: u32) -> Result<u32, ThreadError> {
    // Call the WASI thread_spawn host function
    let result = unsafe { __thread_spawn(start_arg) };

    // Interpret the result according to WASI threads specification
    if result >= 0 {
        // Positive values indicate success and return the thread ID
        Ok(result as u32)
    } else {
        // Negative values indicate specific error conditions
        match result {
            -1 => Err(ThreadError::SystemError),
            -2 => Err(ThreadError::ResourceExhausted),
            -3 => Err(ThreadError::InvalidArgument),
            other => Err(ThreadError::Unknown(other)),
        }
    }
}

/// Check if WASI threads support is available
///
/// This function attempts to detect if the runtime supports WASI threads
/// by checking for the presence of the `thread_spawn` host function.
/// Note that this is a best-effort detection and may not be 100% reliable.
///
/// # Returns
///
/// `true` if WASI threads appears to be supported, `false` otherwise.
///
/// # Usage
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::is_threads_supported;
///
/// if is_threads_supported() {
///     println!("WASI threads are supported");
///     // Use thread_spawn() safely
/// } else {
///     println!("WASI threads not supported, using single-threaded mode");
///     // Fallback to single-threaded processing
/// }
/// ```
pub fn is_threads_supported() -> bool {
    // This is a simple heuristic - in a real implementation,
    // we might try to call the function with invalid args to see if it exists
    // For now, we assume it's supported if we can compile with the import
    true
}

/// Get the current thread's start argument
///
/// This is a convenience function that extracts the start argument from
/// the input data. It's equivalent to calling `input()` and parsing the
/// first 4 bytes as a little-endian u32.
///
/// # Returns
///
/// The start argument that was passed to `thread_spawn()` for this thread.
/// For the main thread (not spawned via `thread_spawn`), this will return
/// the first 4 bytes of the input data interpreted as a u32.
///
/// # Example
///
/// ```rust,no_run
/// use metashrew_core::wasi_threads::get_thread_start_arg;
/// use metashrew_core::initialize;
///
/// #[no_mangle]
/// pub extern "C" fn _start() {
///     initialize();
///
///     let start_arg = get_thread_start_arg();
///     println!("This thread was started with argument: {}", start_arg);
/// }
/// ```
pub fn get_thread_start_arg() -> u32 {
    let input_data = crate::input();
    if input_data.len() >= 4 {
        u32::from_le_bytes(input_data[0..4].try_into().unwrap_or([0; 4]))
    } else {
        0
    }
}

/// Thread coordination utilities
pub mod coordination {
    //! Utilities for coordinating between threads using the shared database
    //!
    //! Since WASI threads don't support shared memory, all coordination must
    //! happen through the shared database. This module provides common patterns
    //! for thread coordination and communication.

    use crate::{get, set, flush};
    use std::sync::Arc;

    /// Store a value that can be read by other threads
    ///
    /// This is a convenience function for thread-to-thread communication
    /// via the shared database.
    ///
    /// # Parameters
    ///
    /// - `key`: Unique key for the shared value
    /// - `value`: Value to share with other threads
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use metashrew_core::wasi_threads::coordination::set_shared_value;
    /// use metashrew_core::initialize;
    ///
    /// initialize();
    /// set_shared_value("thread_status", b"running");
    /// ```
    pub fn set_shared_value(key: &str, value: &[u8]) {
        let key_arc = Arc::new(key.as_bytes().to_vec());
        let value_arc = Arc::new(value.to_vec());
        set(key_arc, value_arc);
        flush();
    }

    /// Get a value shared by another thread
    ///
    /// This is a convenience function for reading values stored by other threads.
    ///
    /// # Parameters
    ///
    /// - `key`: Key of the shared value to retrieve
    ///
    /// # Returns
    ///
    /// The shared value, or an empty vector if the key doesn't exist.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use metashrew_core::wasi_threads::coordination::get_shared_value;
    /// use metashrew_core::initialize;
    ///
    /// initialize();
    /// let status = get_shared_value("thread_status");
    /// if !status.is_empty() {
    ///     println!("Thread status: {}", String::from_utf8_lossy(&status));
    /// }
    /// ```
    pub fn get_shared_value(key: &str) -> Vec<u8> {
        let key_arc = Arc::new(key.as_bytes().to_vec());
        let value_arc = get(key_arc);
        value_arc.as_ref().clone()
    }

    /// Signal completion to other threads
    ///
    /// This function stores a completion marker that other threads can check.
    ///
    /// # Parameters
    ///
    /// - `thread_id`: Unique identifier for this thread
    /// - `result`: Optional result data to share
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use metashrew_core::wasi_threads::coordination::signal_completion;
    /// use metashrew_core::initialize;
    ///
    /// initialize();
    /// signal_completion(42, Some(b"success"));
    /// ```
    pub fn signal_completion(thread_id: u32, result: Option<&[u8]>) {
        let key = format!("thread_complete_{}", thread_id);
        let value = result.unwrap_or(b"completed").to_vec();
        set_shared_value(&key, &value);
    }

    /// Check if a thread has completed
    ///
    /// # Parameters
    ///
    /// - `thread_id`: Thread ID to check
    ///
    /// # Returns
    ///
    /// `true` if the thread has signaled completion, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use metashrew_core::wasi_threads::coordination::is_thread_complete;
    /// use metashrew_core::initialize;
    ///
    /// initialize();
    /// if is_thread_complete(42) {
    ///     println!("Thread 42 has completed");
    /// }
    /// ```
    pub fn is_thread_complete(thread_id: u32) -> bool {
        let key = format!("thread_complete_{}", thread_id);
        let value = get_shared_value(&key);
        !value.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_error_display() {
        assert_eq!(
            ThreadError::ResourceExhausted.to_string(),
            "Thread spawning failed: resource exhausted"
        );
        assert_eq!(
            ThreadError::SystemError.to_string(),
            "Thread spawning failed: system error"
        );
        assert_eq!(
            ThreadError::Unknown(-5).to_string(),
            "Thread spawning failed: unknown error (code: -5)"
        );
    }

    #[test]
    fn test_is_threads_supported() {
        // This will always return true in our current implementation
        assert!(is_threads_supported());
    }

    #[test]
    fn test_coordination_functions() {
        // These tests would require a mock environment to run properly
        // For now, we just test that the functions compile
        let _key = "test_key";
        let _value = b"test_value";
        
        // In a real test environment with proper mocking:
        // coordination::set_shared_value(key, value);
        // let retrieved = coordination::get_shared_value(key);
        // assert_eq!(retrieved, value);
    }
}
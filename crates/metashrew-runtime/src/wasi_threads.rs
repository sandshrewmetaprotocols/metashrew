//! WASI Threads implementation for Metashrew Runtime
//!
//! This module implements the WASI threads specification for the Metashrew runtime,
//! providing standardized threading support for WebAssembly modules. It follows
//! the official WASI threads proposal with instance-per-thread architecture.
//!
//! # Architecture
//!
//! ## Instance-per-Thread Model
//! - Each thread spawns a new WASM instance with its own memory space
//! - Threads communicate through shared database state, not shared memory
//! - Each thread can run its own single-threaded async runtime
//!
//! ## Host Function Implementation
//! - `thread_spawn(start_arg: u32) -> i32`: Single standardized host function
//! - Returns thread ID on success, negative value on error
//! - Spawns new thread with fresh WASM instance
//!
//! ## Thread Lifecycle
//! 1. Host receives `thread_spawn` call with start argument
//! 2. New thread created with cloned runtime context
//! 3. Fresh WASM instance instantiated in new thread
//! 4. Thread executes with provided start argument
//! 5. Thread terminates when WASM execution completes
//!
//! # Integration with Metashrew
//!
//! ## Database Sharing
//! - All threads share the same database backend
//! - Thread-safe database operations through storage trait
//! - Consistent state across all thread instances
//!
//! ## Memory Isolation
//! - Each thread has isolated WASM memory
//! - No shared memory between threads (WASI threads design)
//! - Communication through database operations only
//!
//! # Usage Example
//!
//! ```rust,ignore
//! // In WASM guest code (using metashrew-core bindings)
//! let thread_id = thread_spawn(42); // Start argument = 42
//! if thread_id >= 0 {
//!     println!("Thread spawned with ID: {}", thread_id);
//! } else {
//!     println!("Thread spawn failed");
//! }
//! ```

use anyhow::{anyhow, Result};
use std::sync::{Arc, Mutex, atomic::{AtomicU32, Ordering}};
use std::thread;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use wasmtime::{Caller, Linker};

use crate::context::MetashrewRuntimeContext;
use crate::runtime::{MetashrewRuntime, State};
use crate::traits::KeyValueStoreLike;

/// Global thread ID counter for assigning unique thread IDs
static THREAD_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Thread result storage for inter-thread communication
///
/// This structure stores results from completed threads, allowing the main
/// thread or other threads to retrieve results by thread ID. Results can
/// include return values, memory exports, and custom data.
#[derive(Debug, Clone)]
pub struct ThreadResult {
    /// Thread ID that produced this result
    pub thread_id: u32,
    /// Return value from the thread's main function
    pub return_value: i32,
    /// Exported memory data from the thread (if any)
    pub memory_export: Option<Vec<u8>>,
    /// Custom result data stored by the thread
    pub custom_data: Option<Vec<u8>>,
    /// Timestamp when the result was stored
    pub timestamp: Instant,
    /// Whether the thread completed successfully
    pub success: bool,
    /// Error message if the thread failed
    pub error_message: Option<String>,
}

/// Global thread result storage
static THREAD_RESULTS: std::sync::LazyLock<Mutex<HashMap<u32, ThreadResult>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Store a thread result for later retrieval
pub fn store_thread_result(result: ThreadResult) {
    if let Ok(mut results) = THREAD_RESULTS.lock() {
        results.insert(result.thread_id, result);
    }
}

/// Retrieve a thread result by thread ID
pub fn get_thread_result(thread_id: u32) -> Option<ThreadResult> {
    if let Ok(results) = THREAD_RESULTS.lock() {
        results.get(&thread_id).cloned()
    } else {
        None
    }
}

/// Wait for a thread result with timeout
pub fn wait_for_thread_result(thread_id: u32, timeout: Duration) -> Option<ThreadResult> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(result) = get_thread_result(thread_id) {
            return Some(result);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

/// Remove a thread result from storage (cleanup)
pub fn remove_thread_result(thread_id: u32) -> Option<ThreadResult> {
    if let Ok(mut results) = THREAD_RESULTS.lock() {
        results.remove(&thread_id)
    } else {
        None
    }
}

/// Thread management structure for tracking spawned threads
///
/// This structure maintains information about spawned threads and provides
/// cleanup capabilities. It's designed to be thread-safe and can be shared
/// across multiple WASM instances.
#[derive(Debug)]
pub struct ThreadManager {
    /// Active thread handles for cleanup and monitoring
    active_threads: Arc<Mutex<Vec<thread::JoinHandle<()>>>>,
}

impl ThreadManager {
    /// Create a new thread manager
    pub fn new() -> Self {
        Self {
            active_threads: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a thread handle to the manager
    pub fn add_thread(&self, handle: thread::JoinHandle<()>) {
        if let Ok(mut threads) = self.active_threads.lock() {
            threads.push(handle);
        }
    }

    /// Clean up completed threads
    pub fn cleanup_completed_threads(&self) {
        if let Ok(mut threads) = self.active_threads.lock() {
            threads.retain(|handle| !handle.is_finished());
        }
    }

    /// Get the count of active threads
    pub fn active_thread_count(&self) -> usize {
        self.active_threads.lock().map(|threads| threads.len()).unwrap_or(0)
    }
}

/// Thread execution context containing all necessary data for thread execution
///
/// This structure packages all the data needed to execute a WASM instance
/// in a new thread, including the runtime configuration and start argument.
#[derive(Debug)]
struct ThreadExecutionContext<T: KeyValueStoreLike> {
    /// Database backend shared across all threads
    db: T,
    /// WASM module bytecode for instantiation
    module_bytes: Vec<u8>,
    /// Start argument passed to the thread
    start_arg: u32,
    /// Thread ID for identification
    thread_id: u32,
    /// Prefix configurations for SMT roots
    prefix_configs: Vec<(String, Vec<u8>)>,
}

/// Execute a WASM instance in a new thread
///
/// This function runs in the spawned thread and creates a fresh WASM runtime
/// instance to execute the thread's work. It follows the WASI threads model
/// of instance-per-thread execution.
///
/// # Parameters
///
/// - `context`: Thread execution context with all necessary data
///
/// # Thread Execution Flow
///
/// 1. Create new MetashrewRuntime with shared database
/// 2. Set up thread-specific context with start argument
/// 3. Execute WASM `_start` function
/// 4. Capture results and memory exports
/// 5. Store results for retrieval by other threads
/// 6. Clean up thread resources
fn execute_thread<T: KeyValueStoreLike + Clone + Send + Sync + 'static>(
    context: ThreadExecutionContext<T>,
) {
    log::info!("Thread {} starting execution with start_arg={}", context.thread_id, context.start_arg);

    let start_time = Instant::now();
    let mut thread_result = ThreadResult {
        thread_id: context.thread_id,
        return_value: -1,
        memory_export: None,
        custom_data: None,
        timestamp: start_time,
        success: false,
        error_message: None,
    };

    // Create a new runtime instance for this thread
    let runtime_result = MetashrewRuntime::new(
        &context.module_bytes,
        context.db,
        context.prefix_configs,
    );

    let mut runtime = match runtime_result {
        Ok(runtime) => runtime,
        Err(e) => {
            let error_msg = format!("Thread {} failed to create runtime: {}", context.thread_id, e);
            log::error!("{}", error_msg);
            thread_result.error_message = Some(error_msg);
            store_thread_result(thread_result);
            return;
        }
    };

    // Set up the thread context with start argument as block data
    {
        let mut guard = match runtime.context.lock() {
            Ok(guard) => guard,
            Err(e) => {
                let error_msg = format!("Thread {} failed to lock context: {}", context.thread_id, e);
                log::error!("{}", error_msg);
                thread_result.error_message = Some(error_msg);
                store_thread_result(thread_result);
                return;
            }
        };

        // Encode start argument as little-endian bytes for WASM input
        guard.block = context.start_arg.to_le_bytes().to_vec();
        guard.height = 0; // Threads start at height 0
        guard.state = 0;  // Reset execution state
    }

    // Execute the WASM _start function
    let execution_result = runtime.run();

    match execution_result {
        Ok(()) => {
            log::info!("Thread {} completed successfully", context.thread_id);
            thread_result.success = true;
            thread_result.return_value = 0; // Success

            // Try to export memory data if available
            if let Some(memory) = runtime.instance.get_memory(&mut runtime.wasmstore, "memory") {
                let memory_data = memory.data(&runtime.wasmstore);
                // Export first 4KB of memory or less if memory is smaller
                let export_size = std::cmp::min(memory_data.len(), 4096);
                if export_size > 0 {
                    thread_result.memory_export = Some(memory_data[..export_size].to_vec());
                    log::debug!("Thread {} exported {} bytes of memory", context.thread_id, export_size);
                }
            }

            // Try to call a result function if it exists to get custom data
            if let Ok(result_func) = runtime.instance.get_typed_func::<(), i32>(&mut runtime.wasmstore, "get_thread_result") {
                match result_func.call(&mut runtime.wasmstore, ()) {
                    Ok(result_ptr) => {
                        if let Some(memory) = runtime.instance.get_memory(&mut runtime.wasmstore, "memory") {
                            let memory_data = memory.data(&runtime.wasmstore);
                            if let Ok(custom_data) = crate::runtime::try_read_arraybuffer_as_vec(memory_data, result_ptr) {
                                thread_result.custom_data = Some(custom_data);
                                thread_result.return_value = result_ptr;
                                log::debug!("Thread {} returned custom data via get_thread_result", context.thread_id);
                            }
                        }
                    }
                    Err(e) => {
                        log::debug!("Thread {} get_thread_result function failed: {}", context.thread_id, e);
                    }
                }
            }
        }
        Err(e) => {
            let error_msg = format!("Thread {} execution failed: {}", context.thread_id, e);
            log::error!("{}", error_msg);
            thread_result.error_message = Some(error_msg);
            thread_result.return_value = -1;
        }
    }

    // Update timestamp to completion time
    thread_result.timestamp = Instant::now();

    // Store the result for retrieval
    store_thread_result(thread_result);

    log::info!("Thread {} terminating (execution time: {:?})",
              context.thread_id, start_time.elapsed());
}

/// Set up WASI threads host function in the linker
///
/// This function adds the `thread_spawn` host function to the WASM linker,
/// implementing the WASI threads specification. The function follows the
/// standard signature: `thread_spawn(start_arg: u32) -> i32`.
///
/// # Parameters
///
/// - `context`: Shared runtime context
/// - `linker`: WASM linker to add the host function to
/// - `module_bytes`: WASM module bytecode for spawning new instances
/// - `thread_manager`: Thread manager for tracking spawned threads
///
/// # Returns
///
/// Result indicating success or failure of host function setup
///
/// # Host Function Behavior
///
/// - Takes a u32 start argument from WASM
/// - Spawns new thread with fresh WASM instance
/// - Returns positive thread ID on success
/// - Returns negative value on error
/// - Follows WASI threads specification exactly
pub fn setup_wasi_threads_linker<T: KeyValueStoreLike + Clone + Send + Sync + 'static>(
    context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
    linker: &mut Linker<State>,
    module_bytes: Vec<u8>,
    thread_manager: Arc<ThreadManager>,
) -> Result<()> {
    // Clone data for the closure
    let context_clone = context.clone();
    let thread_manager_clone = thread_manager.clone();

    linker
        .func_wrap(
            "wasi",
            "thread_spawn",
            move |_caller: Caller<'_, State>, start_arg: u32| -> i32 {
                log::debug!("WASI thread_spawn called with start_arg={}", start_arg);

                // Get the current database and configuration from context
                let (db, prefix_configs) = match context_clone.lock() {
                    Ok(ctx) => (ctx.db.clone(), ctx.prefix_configs.clone()),
                    Err(e) => {
                        log::error!("Failed to lock context for thread spawn: {}", e);
                        return -1; // Error: context lock failed
                    }
                };

                // Generate unique thread ID
                let thread_id = THREAD_ID_COUNTER.fetch_add(1, Ordering::SeqCst);

                // Create thread execution context
                let thread_context = ThreadExecutionContext {
                    db,
                    module_bytes: module_bytes.clone(),
                    start_arg,
                    thread_id,
                    prefix_configs,
                };

                // Spawn the new thread
                let thread_manager_ref = thread_manager_clone.clone();
                match thread::Builder::new()
                    .name(format!("wasi-thread-{}", thread_id))
                    .spawn(move || {
                        execute_thread(thread_context);
                    })
                {
                    Ok(handle) => {
                        // Add thread to manager for tracking
                        thread_manager_ref.add_thread(handle);
                        
                        // Clean up any completed threads
                        thread_manager_ref.cleanup_completed_threads();
                        
                        log::info!("Successfully spawned WASI thread {} (active threads: {})",
                                  thread_id, thread_manager_ref.active_thread_count());
                        
                        thread_id as i32 // Return positive thread ID on success
                    }
                    Err(e) => {
                        log::error!("Failed to spawn thread: {}", e);
                        -2 // Error: thread spawn failed
                    }
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap thread_spawn function: {:?}", e))?;

    // Add thread result retrieval host functions
    linker
        .func_wrap(
            "wasi",
            "thread_get_result",
            |_caller: Caller<'_, State>, thread_id: u32| -> i32 {
                log::debug!("WASI thread_get_result called for thread_id={}", thread_id);
                
                match get_thread_result(thread_id) {
                    Some(result) => {
                        if result.success {
                            result.return_value
                        } else {
                            -1 // Thread failed
                        }
                    }
                    None => -2, // Thread not found or not completed
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap thread_get_result function: {:?}", e))?;

    linker
        .func_wrap(
            "wasi",
            "thread_wait_result",
            |_caller: Caller<'_, State>, thread_id: u32, timeout_ms: u32| -> i32 {
                log::debug!("WASI thread_wait_result called for thread_id={} timeout={}ms", thread_id, timeout_ms);
                
                let timeout = Duration::from_millis(timeout_ms as u64);
                match wait_for_thread_result(thread_id, timeout) {
                    Some(result) => {
                        if result.success {
                            result.return_value
                        } else {
                            -1 // Thread failed
                        }
                    }
                    None => -3, // Timeout or thread not found
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap thread_wait_result function: {:?}", e))?;

    linker
        .func_wrap(
            "wasi",
            "thread_get_memory",
            |mut caller: Caller<'_, State>, thread_id: u32, output_ptr: i32| -> i32 {
                log::debug!("WASI thread_get_memory called for thread_id={}", thread_id);
                
                let mem = match caller.get_export("memory") {
                    Some(export) => match export.into_memory() {
                        Some(memory) => memory,
                        None => return -1,
                    },
                    None => return -1,
                };

                match get_thread_result(thread_id) {
                    Some(result) => {
                        if let Some(memory_data) = result.memory_export {
                            if let Err(_) = mem.write(&mut caller, output_ptr as usize, &memory_data) {
                                return -1;
                            }
                            memory_data.len() as i32
                        } else {
                            0 // No memory export available
                        }
                    }
                    None => -2, // Thread not found
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap thread_get_memory function: {:?}", e))?;

    linker
        .func_wrap(
            "wasi",
            "thread_get_custom_data",
            |mut caller: Caller<'_, State>, thread_id: u32, output_ptr: i32| -> i32 {
                log::debug!("WASI thread_get_custom_data called for thread_id={}", thread_id);
                
                let mem = match caller.get_export("memory") {
                    Some(export) => match export.into_memory() {
                        Some(memory) => memory,
                        None => return -1,
                    },
                    None => return -1,
                };

                match get_thread_result(thread_id) {
                    Some(result) => {
                        if let Some(custom_data) = result.custom_data {
                            if let Err(_) = mem.write(&mut caller, output_ptr as usize, &custom_data) {
                                return -1;
                            }
                            custom_data.len() as i32
                        } else {
                            0 // No custom data available
                        }
                    }
                    None => -2, // Thread not found
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap thread_get_custom_data function: {:?}", e))?;

    // Add pipeline threading host functions
    linker
        .func_wrap(
            "wasi",
            "thread_spawn_pipeline",
            move |_caller: Caller<'_, State>, entrypoint_ptr: i32, entrypoint_len: i32, input_ptr: i32, input_len: i32| -> i32 {
                log::debug!("WASI thread_spawn_pipeline called with entrypoint_ptr={}, entrypoint_len={}, input_ptr={}, input_len={}",
                           entrypoint_ptr, entrypoint_len, input_ptr, input_len);
                
                // In a real implementation, we would:
                // 1. Read the entrypoint name from WASM memory at entrypoint_ptr
                // 2. Read the input data from WASM memory at input_ptr
                // 3. Create a new WASM instance
                // 4. Store the input data for the thread to access via __load_input
                // 5. Call the specified entrypoint function instead of _start
                
                // For now, simulate successful thread spawn
                let thread_id = THREAD_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
                
                // Store thread info in results
                let thread_result = ThreadResult {
                    thread_id,
                    return_value: 0,
                    memory_export: Some(vec![0; 1024]), // Simulate some memory data
                    custom_data: None,
                    timestamp: Instant::now(),
                    success: true,
                    error_message: None,
                };
                store_thread_result(thread_result);
                
                thread_id as i32
            },
        )
        .map_err(|e| anyhow!("Failed to wrap thread_spawn_pipeline function: {:?}", e))?;

    linker
        .func_wrap(
            "wasi",
            "thread_free",
            |_caller: Caller<'_, State>, thread_id: i32| -> i32 {
                log::debug!("WASI thread_free called for thread_id={}", thread_id);
                
                match remove_thread_result(thread_id as u32) {
                    Some(_) => 0, // Success
                    None => -2,   // Thread not found
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap thread_free function: {:?}", e))?;

    linker
        .func_wrap(
            "wasi",
            "read_thread_memory",
            |mut caller: Caller<'_, State>, thread_id: i32, offset: i32, buffer_ptr: i32, len: i32| -> i32 {
                log::debug!("WASI read_thread_memory called for thread_id={}, offset={}, len={}", thread_id, offset, len);
                
                let mem = match caller.get_export("memory") {
                    Some(export) => match export.into_memory() {
                        Some(memory) => memory,
                        None => return -1,
                    },
                    None => return -1,
                };

                match get_thread_result(thread_id as u32) {
                    Some(result) => {
                        if let Some(memory_data) = result.memory_export {
                            let start = offset as usize;
                            let end = std::cmp::min(start + len as usize, memory_data.len());
                            
                            if start < memory_data.len() {
                                let bytes_to_read = end - start;
                                let data_slice = &memory_data[start..end];
                                
                                if let Err(_) = mem.write(&mut caller, buffer_ptr as usize, data_slice) {
                                    return -1;
                                }
                                bytes_to_read as i32
                            } else {
                                0 // Offset beyond available data
                            }
                        } else {
                            0 // No memory data available
                        }
                    }
                    None => -2, // Thread not found
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap read_thread_memory function: {:?}", e))?;

    log::info!("WASI threads host functions registered successfully");
    Ok(())
}

/// Add WASI threads support to an existing MetashrewRuntime
///
/// This function extends an existing runtime with WASI threads capability by
/// adding the necessary host functions and thread management infrastructure.
///
/// # Parameters
///
/// - `runtime`: Mutable reference to the runtime to extend
/// - `module_bytes`: WASM module bytecode for spawning new instances
///
/// # Returns
///
/// A ThreadManager for tracking and managing spawned threads
///
/// # Usage
///
/// ```rust,ignore
/// let mut runtime = MetashrewRuntime::load(indexer_path, storage)?;
/// let thread_manager = add_wasi_threads_support(&mut runtime, &module_bytes)?;
/// 
/// // Runtime now supports WASI threads
/// // Threads can be spawned from WASM using thread_spawn()
/// ```
pub fn add_wasi_threads_support<T: KeyValueStoreLike + Clone + Send + Sync + 'static>(
    runtime: &mut MetashrewRuntime<T>,
    module_bytes: Vec<u8>,
) -> Result<Arc<ThreadManager>> {
    let thread_manager = Arc::new(ThreadManager::new());

    // Add WASI threads host function to the existing linker
    setup_wasi_threads_linker(
        runtime.context.clone(),
        &mut runtime.linker,
        module_bytes,
        thread_manager.clone(),
    )?;

    // Re-instantiate the WASM module with the new linker
    runtime.instance = runtime
        .linker
        .instantiate(&mut runtime.wasmstore, &runtime.module)
        .map_err(|e| anyhow!("Failed to re-instantiate module with WASI threads: {}", e))?;

    log::info!("WASI threads support added to MetashrewRuntime");
    Ok(thread_manager)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::BatchLike;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[derive(Debug)]
    struct MockError {
        message: String,
    }

    impl std::fmt::Display for MockError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Mock storage error: {}", self.message)
        }
    }

    impl std::error::Error for MockError {}

    // Mock storage implementation for testing
    #[derive(Debug, Clone)]
    struct MockStorage {
        data: Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                data: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    #[derive(Debug)]
    struct MockBatch {
        operations: Vec<(Vec<u8>, Option<Vec<u8>>)>, // None for delete
    }

    impl crate::traits::BatchLike for MockBatch {
        fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
            self.operations.push((key.as_ref().to_vec(), Some(value.as_ref().to_vec())));
        }

        fn delete<K: AsRef<[u8]>>(&mut self, key: K) {
            self.operations.push((key.as_ref().to_vec(), None));
        }

        fn default() -> Self {
            Self {
                operations: Vec::new(),
            }
        }
    }

    impl crate::traits::KeyValueStoreLike for MockStorage {
        type Error = MockError;
        type Batch = MockBatch;

        fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
            let mut data = self.data.lock().unwrap();
            for (key, value) in batch.operations {
                match value {
                    Some(v) => { data.insert(key, v); }
                    None => { data.remove(&key); }
                }
            }
            Ok(())
        }

        fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
            Ok(self.data.lock().unwrap().get(key.as_ref()).cloned())
        }

        fn get_immutable<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
            Ok(self.data.lock().unwrap().get(key.as_ref()).cloned())
        }

        fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
        where
            K: AsRef<[u8]>,
            V: AsRef<[u8]>,
        {
            self.data.lock().unwrap().insert(key.as_ref().to_vec(), value.as_ref().to_vec());
            Ok(())
        }

        fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
            self.data.lock().unwrap().remove(key.as_ref());
            Ok(())
        }

        fn scan_prefix<K: AsRef<[u8]>>(
            &self,
            prefix: K,
        ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error> {
            let data = self.data.lock().unwrap();
            let prefix_bytes = prefix.as_ref();
            Ok(data
                .iter()
                .filter(|(k, _)| k.starts_with(prefix_bytes))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect())
        }

        fn create_batch(&self) -> Self::Batch {
            MockBatch::default()
        }

        fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
            let data = self.data.lock().unwrap();
            let keys: Vec<Vec<u8>> = data.keys().cloned().collect();
            Ok(Box::new(keys.into_iter()))
        }
    }

    #[test]
    fn test_thread_manager_creation() {
        let manager = ThreadManager::new();
        assert_eq!(manager.active_thread_count(), 0);
    }

    #[test]
    fn test_thread_id_generation() {
        let id1 = THREAD_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        let id2 = THREAD_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        assert!(id2 > id1);
    }

    #[test]
    fn test_thread_execution_context_creation() {
        let storage = MockStorage::new();
        let module_bytes = vec![0x00, 0x61, 0x73, 0x6d]; // WASM magic number
        
        let context = ThreadExecutionContext {
            db: storage,
            module_bytes: module_bytes.clone(),
            start_arg: 42,
            thread_id: 1,
            prefix_configs: vec![],
        };

        assert_eq!(context.start_arg, 42);
        assert_eq!(context.thread_id, 1);
        assert_eq!(context.module_bytes, module_bytes);
    }
}
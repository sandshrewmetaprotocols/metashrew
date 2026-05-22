//! Core WebAssembly runtime for executing Bitcoin indexers
//!
//! This module provides the main [`MetashrewRuntime`] struct that executes WebAssembly
//! modules for Bitcoin block processing. It implements the host side of the WASM
//! interface, providing functions that WASM modules can call to interact with the
//! database and retrieve blockchain data.
//!
//! # Architecture
//!
//! The runtime follows a generic design pattern where it's parameterized over a
//! storage type `T: KeyValueStoreLike`. This enables:
//!
//! - **Testing**: Use in-memory storage for fast unit tests
//! - **Production**: Use RocksDB for persistent, high-performance storage
//! - **Flexibility**: Support for future storage backends
//!
//! # Key Components
//!
//! ## WASM Execution Environment
//!
//! The runtime uses Wasmtime to execute WebAssembly modules with:
//! - **Deterministic execution**: Configured for reproducible results
//! - **Memory isolation**: Each block execution starts with fresh memory
//! - **Resource limits**: Configurable memory and execution limits
//! - **Host function bindings**: Provides database and I/O operations to WASM
//!
//! ## Host Functions
//!
//! The runtime provides these functions to WASM modules:
//! - `__host_len()`: Get input data length
//! - `__load_input(ptr)`: Load block data into WASM memory
//! - `__get(key_ptr, value_ptr)`: Read from database
//! - `__get_len(key_ptr)`: Get value length for a key
//! - `__flush(data_ptr)`: Write key-value pairs to database
//! - `__log(ptr)`: Output debug messages
//!
//! ## Execution Modes
//!
//! The runtime supports multiple execution modes:
//! - **Normal**: Standard block processing with database writes
//! - **View**: Read-only execution for querying state
//! - **Preview**: Isolated execution for testing block effects
//! - **Atomic**: Batch processing with rollback capability
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use metashrew_runtime::{MetashrewRuntime, traits::KeyValueStoreLike};
//! use std::path::PathBuf;
//!
//! async fn process_blocks<T: KeyValueStoreLike>(
//!     mut runtime: MetashrewRuntime<T>,
//!     block_data: &[u8],
//!     height: u32
//! ) -> anyhow::Result<()> {
//!     // Process a block
//!     runtime.process_block(height, block_data).await?;
//!
//!     Ok(())
//! }
//! ```

use anyhow::{anyhow, Context, Result};

use itertools::Itertools;
use prost::Message;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::sync::RwLock;
use wasmtime::{Caller, Linker, Store, StoreLimits, StoreLimitsBuilder};

use crate::context::MetashrewRuntimeContext;
use crate::traits::{BatchLike, KeyValueStoreLike};

/// Internal key used to store the current blockchain tip height
///
/// This key is used internally by the runtime to track the highest block
/// that has been successfully processed and committed to the database.
pub const TIP_HEIGHT_KEY: &'static str = "/__INTERNAL/tip-height";

/// Build a `wasmtime::Config` with the deterministic flags every
/// metashrew indexer engine MUST use.
///
/// This is the single source of truth: `MetashrewRuntime::{load,new}`
/// call it to build both the indexer engine and (with two extra
/// flags) the view engine. External crates that need to construct
/// their own engine for testing or specialty use cases should call
/// this rather than reach for `wasmtime::Config::default()` directly.
///
/// Flags set, with rationale:
/// - `cranelift_nan_canonicalization(true)` — float NaN payloads
///   canonicalize across hardware (x86-64 vs ARM, SSE/AVX
///   variants). Alkanes doesn't currently use floats, but nested
///   wasmi sub-instances inside an alkane *can*, and any future
///   wasm protocol might. Belt-and-suspenders.
/// - `relaxed_simd_deterministic(true)` — relaxed SIMD ops produce
///   different results on different CPUs without this flag. Same
///   risk class as floats.
/// - `memory_reservation(0x100000000)` — 4 GiB mmap'd up front so
///   `memory.grow` doesn't need to syscall mid-execution. Without
///   this, a host under memory pressure can fail `memory.grow`
///   mid-block, producing a different runtime error path than a
///   host with headroom — the same input wasm produces different
///   write paths.
/// - `memory_guard_size(0x10000)` — 64 KiB guard page so OOB
///   memory accesses trap predictably instead of corrupting host
///   memory.
/// - `memory_init_cow(false)` — disable copy-on-write so two
///   instances of the same module don't share underlying pages.
///   Defense against accidental cross-instance state contamination.
/// - `async_support(true)` — required because the indexer runs via
///   `instantiate_async` + `start.call_async`.
///
/// Flags intentionally NOT set on the indexer engine, but flipped
/// on the VIEW engine in `load()`/`new()`:
/// - `wasm_threads(true)` — view-only. Consensus-critical that the
///   indexer wasm cannot use shared memory + atomics; those
///   semantics are not deterministic across thread schedules.
/// - `consume_fuel(true)` — view-only. The indexer doesn't meter
///   per-instruction fuel (it metered per-block via FuelTank at
///   the alkanes layer).
pub fn indexer_config() -> wasmtime::Config {
    let mut config = wasmtime::Config::default();
    config.cranelift_nan_canonicalization(true);
    config.relaxed_simd_deterministic(true);
    config.memory_reservation(0x100000000);
    config.memory_guard_size(0x10000);
    config.memory_init_cow(false);
    config.async_support(true);
    config
}

fn lock_err<T>(err: std::sync::PoisonError<T>) -> anyhow::Error {
    anyhow!("Mutex lock error: {}", err)
}

fn try_into_vec<const N: usize>(bytes: [u8; N]) -> Result<Vec<u8>> {
    Vec::<u8>::try_from(bytes).map_err(|e| anyhow!("Failed to convert bytes to Vec: {:?}", e))
}

use crate::proto::metashrew::KeyValueFlush;

/// WASM execution state tracking for deterministic execution
///
/// This struct maintains the execution state for a single WASM instance,
/// including resource limits and failure tracking. It's designed to ensure
/// deterministic execution across different environments.
///
/// # Fields
///
/// - `limits`: Resource limits for WASM execution (memory, tables, instances)
/// - `had_failure`: Tracks whether any host function call failed during execution
///
/// # Deterministic Execution
///
/// The state is configured with maximum resource limits to ensure consistent
/// behavior across different environments. Memory is pre-allocated to avoid
/// non-deterministic growth patterns.
pub struct State {
    /// Resource limits for WASM execution
    ///
    /// Set to maximum values to ensure deterministic behavior by avoiding
    /// dynamic resource allocation during execution.
    limits: StoreLimits,

    /// Tracks execution failures in host functions
    ///
    /// When a host function encounters an error (e.g., database failure,
    /// memory access error), it sets this flag to signal the runtime
    /// that execution should be aborted.
    pub(crate) had_failure: bool,

    /// Captures the last error from a __flush atomic write so the runtime
    /// can surface ENOSPC / I/O errors instead of a generic "had failure"
    /// message. Read by `process_block_atomic` after WASM execution returns.
    pub(crate) last_flush_error: std::sync::Arc<std::sync::Mutex<Option<String>>>,

    /// Per-view-call thread registry. `Some(_)` only on view-runtime
    /// stores; `None` on indexer stores. The `__flush` syscall dispatcher
    /// pulls this out of `caller.data()` to spawn / join wasm threads.
    /// Indexer paths never construct this and the threading proto ops
    /// fail-closed (write empty response) when it's absent. Lives behind
    /// `Arc` so the spawn closure can clone it into the spawned task
    /// without borrowing the wasmtime store.
    pub(crate) view_threads: Option<std::sync::Arc<crate::view_threads::ViewThreadRegistry>>,
}

impl State {
    /// Create a new WASM execution state with maximum resource limits
    ///
    /// # Returns
    ///
    /// A new [`State`] instance configured for deterministic execution with:
    /// - Maximum memory allocation
    /// - Maximum table allocation
    /// - Maximum instance allocation
    /// - No execution failures initially
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let state = State::new();
    /// // State is ready for deterministic WASM execution
    /// ```
    pub fn new() -> Self {
        State {
            limits: StoreLimitsBuilder::new()
                .memories(usize::MAX)
                .tables(usize::MAX)
                .instances(usize::MAX)
                .build(),
            had_failure: false,
            last_flush_error: std::sync::Arc::new(std::sync::Mutex::new(None)),
            // Indexer paths construct State::new() and never touch this;
            // view paths replace it via `with_view_threads` before
            // installing the store. See `runtime::new_with_db_async_limited`.
            view_threads: None,
        }
    }

    /// Attach a view-thread registry. Called only from view-runtime
    /// construction (`new_with_db_async_limited`). Indexer stores never
    /// call this — their `view_threads` stays `None` so the `__flush`
    /// syscall dispatcher's thread ops fail-closed.
    ///
    /// Exposed `pub` for the integration test
    /// (`tests/view_threads.rs`) to build a view runtime without the
    /// full `MetashrewRuntime::new` 4 GB-pre-allocation path; production
    /// callers should not touch this directly.
    pub fn with_view_threads(
        mut self,
        registry: std::sync::Arc<crate::view_threads::ViewThreadRegistry>,
    ) -> Self {
        self.view_threads = Some(registry);
        self
    }
}

/// Core WebAssembly runtime for executing Bitcoin indexers
///
/// [`MetashrewRuntime`] is the main execution engine that runs WebAssembly modules
/// for Bitcoin block processing. It's generic over storage backends, enabling
/// flexible deployment scenarios from testing to production.
///
/// # Type Parameters
///
/// - `T`: Storage backend implementing [`KeyValueStoreLike`] + [`Clone`] + [`Send`] + [`Sync`]
///
/// # Architecture
///
/// The runtime maintains both synchronous and asynchronous execution engines:
/// - **Synchronous engine**: Used for block processing and preview operations
/// - **Asynchronous engine**: Used for view functions and cooperative yielding
///
/// # Key Components
///
/// ## Execution Context
/// - `context`: Shared state including database, block data, and execution height
/// - `wasmstore`: WASM execution store with resource limits and failure tracking
///
/// ## WASM Engines
/// - `engine`: Synchronous Wasmtime engine for block processing
/// - `async_engine`: Asynchronous Wasmtime engine for view functions
/// - `module`: Compiled WASM module for synchronous execution
/// - `async_module`: Compiled WASM module for asynchronous execution
///
/// ## Host Interface
/// - `linker`: Provides host functions to WASM modules
/// - `instance`: Instantiated WASM module ready for execution
///
/// # Execution Modes
///
/// ## Block Processing (`run`)
/// Normal block processing with database writes and state updates:
/// ```rust,ignore
/// runtime.run()?; // Process current block
/// ```
///
/// ## View Functions (`view`)
/// Read-only execution for querying historical state:
/// ```rust,ignore
/// let result = runtime.view("get_balance".to_string(), &input, height).await?;
/// ```
///
/// ## Preview Mode (`preview`)
/// Isolated execution for testing block effects without committing:
/// ```rust,ignore
/// let result = runtime.preview(&block_data, "view_function".to_string(), &input, height)?;
/// ```
///
/// ## Atomic Processing (`process_block_atomic`)
/// Batch processing with rollback capability:
/// ```rust,ignore
/// let atomic_result = runtime.process_block_atomic(height, &block_data, &block_hash).await?;
/// ```
///
/// # Memory Management
///
/// The runtime ensures deterministic execution through:
/// - **Memory isolation**: Fresh memory for each block execution
/// - **Resource limits**: Pre-allocated maximum memory to avoid growth
/// - **Automatic refresh**: Memory is reset after each block for consistency
///
/// # Thread Safety
///
/// All shared state is protected by [`Arc<Mutex<_>>`] for safe concurrent access.
/// The runtime can be safely shared across threads for parallel view operations.
///
/// # Example Usage
///
/// ```rust,ignore
/// use metashrew_runtime::{MetashrewRuntime, traits::KeyValueStoreLike};
/// use std::path::PathBuf;
///
/// async fn run_indexer<T: KeyValueStoreLike + Clone + Send + Sync + 'static>(
///     indexer_path: PathBuf,
///     storage: T,
///     block_data: &[u8],
///     height: u32
/// ) -> anyhow::Result<()> {
///     // Load the runtime with WASM indexer
///     let mut runtime = MetashrewRuntime::load(indexer_path, storage)?;
///
///     // Process a block
///     runtime.process_block(height, block_data).await?;
///
///     // Query the resulting state
///     let balance = runtime.view(
///         "get_balance".to_string(),
///         &address_bytes,
///         height
///     ).await?;
///
///     println!("Balance: {}", hex::encode(balance));
///     Ok(())
/// }
/// ```
pub struct MetashrewRuntime<T: KeyValueStoreLike> {
    /// Shared execution context containing database, block data, and state
    ///
    /// Protected by [`Arc<Mutex<_>>`] for thread-safe access across
    /// different execution modes and concurrent view operations.
    pub context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
    
    /// Synchronous Wasmtime engine for block processing
    ///
    /// Configured for deterministic execution with:
    /// - NaN canonicalization for consistent floating point
    /// - Relaxed SIMD determinism
    /// - Static memory allocation
    pub engine: wasmtime::Engine,
    
    /// Asynchronous Wasmtime engine for view functions
    ///
    /// Supports cooperative yielding and fuel-based execution limits
    /// for long-running view operations that need to yield control.
    pub async_engine: wasmtime::Engine,
    
    /// Compiled WASM module for asynchronous execution
    ///
    /// Used by view functions and other operations that need
    /// cooperative yielding and async execution.
    pub async_module: wasmtime::Module,
    
    /// Compiled WASM module for synchronous execution
    ///
    /// Used for block processing and other operations that
    /// need deterministic, non-yielding execution.
    pub module: wasmtime::Module,
    
    /// Host function linker providing database and I/O operations
    ///
    /// Binds host functions like `__get`, `__flush`, `__log` that
    /// WASM modules can call to interact with the database and runtime.
    pub linker: wasmtime::Linker<State>,
    
    /// Instantiated WASM module ready for execution
    ///
    /// Contains the loaded and linked WASM instance with all
    /// host functions bound and ready to execute.
    instance: Mutex<WasmInstance>,
}

struct WasmInstance {
    store: wasmtime::Store<State>,
    instance: wasmtime::Instance,
}

pub fn db_make_list_key(v: &Vec<u8>, index: u32) -> Result<Vec<u8>> {
    let mut entry = v.clone();
    let index_bits = try_into_vec(index.to_le_bytes())?;
    entry.extend(index_bits);
    Ok(entry)
}

pub fn db_make_length_key(key: &Vec<u8>) -> Result<Vec<u8>> {
    db_make_list_key(key, u32::MAX)
}

pub fn db_make_updated_key(key: &Vec<u8>) -> Vec<u8> {
    key.clone()
}

pub fn u32_to_vec(v: u32) -> Result<Vec<u8>> {
    try_into_vec(v.to_le_bytes())
}

pub fn try_read_arraybuffer_as_vec(data: &[u8], data_start: i32) -> Result<Vec<u8>> {
    if data_start < 4 || (data_start as usize) > data.len() {
        return Err(anyhow!("memory error: invalid data_start"));
    }

    // data_start points to the data portion, length is at data_start - 4
    // This matches metashrew-support export_bytes which returns pointer + 4
    let len_offset = (data_start as usize) - 4;
    let len = u32::from_le_bytes(data[len_offset..len_offset + 4].try_into().unwrap());

    let data_offset = data_start as usize;
    let end_offset = data_offset + (len as usize);

    if end_offset > data.len() {
        return Err(anyhow!("memory error: data extends beyond memory bounds"));
    }

    return Ok(Vec::<u8>::from(&data[data_offset..end_offset]));
}

pub fn read_arraybuffer_as_vec(data: &[u8], data_start: i32) -> Vec<u8> {
    match try_read_arraybuffer_as_vec(data, data_start) {
        Ok(v) => v,
        Err(_) => Vec::<u8>::new(),
    }
}

/// Write a v10 view-syscall response into wasm linear memory at
/// `response_ptr` using the framing `[u32 LE: len | bytes]`.
///
/// If the response wouldn't fit within `response_max` (including the
/// 4-byte length prefix), writes `[u32 LE: 0]` instead — wasm sees an
/// empty response and treats it as a miss / failure. This is the same
/// fail-closed contract as the CacheGet miss path so wasm can use a
/// single "len==0 → didn't get a value" branch for both real misses
/// and oversized misses.
///
/// Silently does nothing on `mem.write` failure — the call site is
/// view-only and a memory-write failure means the wasm gave us a bad
/// pointer; safer to let the wasm time out / return junk than to
/// crash the host process.
pub fn write_syscall_response(
    mem: &wasmtime::Memory,
    caller: &mut Caller<'_, State>,
    response_ptr: usize,
    response_max: usize,
    response: &[u8],
) {
    let needed = 4usize.saturating_add(response.len());
    if response_ptr == 0 || response_max < 4 || needed > response_max {
        // Write a 0-length marker if there's at least room for the
        // length prefix; otherwise drop silently.
        if response_ptr > 0 && response_max >= 4 {
            let _ = mem.write(&mut *caller, response_ptr, &0u32.to_le_bytes());
        }
        return;
    }
    let len_le = (response.len() as u32).to_le_bytes();
    if mem.write(&mut *caller, response_ptr, &len_le).is_err() {
        return;
    }
    let _ = mem.write(&mut *caller, response_ptr + 4, response);
}

// Legacy function removed

pub fn to_signed_or_trap<'a, T: TryInto<i32>>(_caller: &mut Caller<'_, State>, v: T) -> i32 {
    return match <T as TryInto<i32>>::try_into(v) {
        Ok(v) => v,
        Err(_) => {
            return i32::MAX;
        }
    };
}

pub fn to_usize_or_trap<'a, T: TryInto<usize>>(_caller: &mut Caller<'_, State>, v: T) -> usize {
    return match <T as TryInto<usize>>::try_into(v) {
        Ok(v) => v,
        Err(_) => {
            return usize::MAX;
        }
    };
}

impl<T: KeyValueStoreLike + Clone + Send + Sync + 'static> MetashrewRuntime<T> {
    /// Load and initialize a new MetashrewRuntime from a WASM indexer file
    ///
    /// This is the primary constructor that loads a WebAssembly indexer module
    /// and sets up the complete runtime environment for Bitcoin block processing.
    ///
    /// # Parameters
    ///
    /// - `indexer`: Path to the compiled WASM indexer module file
    /// - `store`: Storage backend implementing [`KeyValueStoreLike`]
    ///
    /// # Returns
    ///
    /// A fully initialized [`MetashrewRuntime`] ready for block processing
    ///
    /// # Configuration
    ///
    /// The runtime is configured for deterministic execution with:
    /// - **NaN canonicalization**: Ensures consistent floating point behavior
    /// - **Relaxed SIMD determinism**: Makes SIMD operations deterministic
    /// - **Static memory allocation**: Pre-allocates 4GB maximum memory
    /// - **Memory guards**: 64KB guard pages for memory safety
    /// - **Async support**: Separate engine for cooperative yielding
    ///
    /// # Host Functions
    ///
    /// Sets up the complete host function interface:
    /// - `__host_len()`: Get input data length
    /// - `__load_input(ptr)`: Load block data into WASM memory
    /// - `__get(key_ptr, value_ptr)`: Read from database
    /// - `__get_len(key_ptr)`: Get value length for a key
    /// - `__flush(data_ptr)`: Write key-value pairs to database
    /// - `__log(ptr)`: Output debug messages
    /// - `abort()`: Handle WASM abort calls
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - WASM module file cannot be loaded or parsed
    /// - Engine configuration fails
    /// - Module instantiation fails
    /// - Host function binding fails
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use metashrew_runtime::MetashrewRuntime;
    /// use std::path::PathBuf;
    ///
    /// // Load runtime with RocksDB storage
    /// let runtime = MetashrewRuntime::load(
    ///     PathBuf::from("indexer.wasm"),
    ///     my_storage_backend
    /// )?;
    /// ```
    pub async fn load(indexer: PathBuf, mut store: T) -> Result<Self> where <T as KeyValueStoreLike>::Batch: Send {
        // Both engines are built from the same deterministic-config base
        // (see `indexer_config`). The view engine adds wasm_threads +
        // consume_fuel; the indexer engine intentionally does NOT get
        // wasm_threads because it's consensus-critical that the indexer
        // wasm executes single-threaded.
        //
        // History: through 2026-05, callers (rockshrew-mono) constructed
        // the indexer engine themselves with only `async_support(true)`
        // set and passed it in via an `engine: wasmtime::Engine`
        // parameter on this function. The deterministic flags (NaN
        // canonicalization, memory_reservation 4 GiB, memory_init_cow
        // false, relaxed-SIMD-deterministic) only ever landed on the
        // view-side async_engine the function built internally. The
        // indexer engine — the one actually writing state — was bare
        // defaults. This is the leading hypothesis for the g vs h
        // 907-DIESEL drift at h=950299 (same image, same wasm, same
        // start time, divergent state).
        //
        // Fix: load() / new() now build BOTH engines internally from
        // `indexer_config()` so determinism is guaranteed-by-construction.
        // The `engine` parameter is gone.
        let config = indexer_config();
        let engine = wasmtime::Engine::new(&config)?;

        // View engine: same deterministic base, plus the v10 view-syscall
        // features (wasm threading for ThreadSpawn/ThreadJoin, fuel for
        // ViewLimits-style accounting).
        let mut async_config = config.clone();
        async_config.consume_fuel(true);
        async_config.wasm_threads(true);

        let async_engine = wasmtime::Engine::new(&async_config)?;
        let module = wasmtime::Module::from_file(&engine, indexer.clone().into_os_string())
            .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to load WASM module")?;
        let async_module = wasmtime::Module::from_file(&async_engine, indexer.into_os_string())
            .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to load WASM module")?;
        let mut linker = Linker::<State>::new(&engine);
        let mut wasmstore = Store::<State>::new(&engine, State::new());
        let tip_height = match store.get(&TIP_HEIGHT_KEY.as_bytes().to_vec()) {
            Ok(Some(bytes)) if bytes.len() >= 4 => {
                u32::from_le_bytes(bytes[..4].try_into().unwrap())
            }
            _ => 0,
        };
        let context = Arc::<RwLock<MetashrewRuntimeContext<T>>>::new(RwLock::<
            MetashrewRuntimeContext<T>,
        >::new(
            MetashrewRuntimeContext::new(store, tip_height, vec![]),
        ));
        {
            wasmstore.limiter(|state| &mut state.limits)
        }
        {
            Self::setup_linker(context.clone(), &mut linker).await
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup basic linker")?;
            Self::setup_linker_indexer(context.clone(), &mut linker).await
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup indexer linker")?;
            linker.define_unknown_imports_as_traps(&module)?;
        }
        let instance = linker
            .instantiate_async(&mut wasmstore, &module).await
            .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to instantiate WASM module")?;
        Ok(MetashrewRuntime {
            async_engine,
            engine,
            async_module,
            module,
            linker,
            context,
            instance: Mutex::new(WasmInstance { store: wasmstore, instance }),
        })
    }

    pub async fn new(indexer: &[u8], mut store: T) -> Result<Self> where <T as KeyValueStoreLike>::Batch: Send {
        // See `load()` for the rationale on internal engine construction.
        // Same deterministic-config base; same indexer/view split.
        let config = indexer_config();
        let engine = wasmtime::Engine::new(&config)?;

        let mut async_config = config.clone();
        async_config.consume_fuel(true);
        async_config.wasm_threads(true);

        let async_engine = wasmtime::Engine::new(&async_config)?;
        let module = wasmtime::Module::new(&engine, indexer)
            .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to load WASM module from bytes")?;
        let async_module = wasmtime::Module::new(&async_engine, indexer)
            .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to load async WASM module from bytes")?;
        let mut linker = Linker::<State>::new(&engine);
        let mut wasmstore = Store::<State>::new(&engine, State::new());
        let tip_height = match store.get(&TIP_HEIGHT_KEY.as_bytes().to_vec()) {
            Ok(Some(bytes)) if bytes.len() >= 4 => {
                u32::from_le_bytes(bytes[..4].try_into().unwrap())
            }
            _ => 0,
        };
        let context = Arc::<RwLock<MetashrewRuntimeContext<T>>>::new(RwLock::<
            MetashrewRuntimeContext<T>,
        >::new(
            MetashrewRuntimeContext::new(store, tip_height, vec![]),
        ));
        {
            wasmstore.limiter(|state| &mut state.limits)
        }
        {
            Self::setup_linker(context.clone(), &mut linker).await
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup basic linker")?;
            Self::setup_linker_indexer(context.clone(), &mut linker).await
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup indexer linker")?;
            linker.define_unknown_imports_as_traps(&module)?;
        }
        let instance = linker
            .instantiate_async(&mut wasmstore, &module).await
            .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to instantiate WASM module")?;

        // Force OS to commit all 4GB pages to verify memory is available
        // This ensures deterministic execution - runtime fails fast if 4GB not available
        let memory = instance.get_memory(&mut wasmstore, "memory")
            .ok_or_else(|| anyhow!("Failed to get WASM memory for pre-allocation"))?;
        Self::force_initial_memory_commit(&memory, &mut wasmstore)
            .context("Failed to pre-allocate 4GB WASM memory. \
                      WASM32 requires 4GB of available physical memory for deterministic execution.")?;

        Ok(MetashrewRuntime {
            async_engine,
            engine,
            async_module,
            module,
            linker,
            context,
            instance: Mutex::new(WasmInstance { store: wasmstore, instance }),
        })
    }

    /// Execute a block in preview mode with isolated database state
    ///
    /// Preview mode allows testing the effects of a block without committing
    /// changes to the main database. It creates an isolated copy of the database,
    /// processes the block, then executes a view function on the resulting state.
    ///
    /// # Parameters
    ///
    /// - `block`: Raw block data to process
    /// - `symbol`: Name of the view function to execute after block processing
    /// - `input`: Input data for the view function
    /// - `height`: Block height for processing context
    ///
    /// # Returns
    ///
    /// The result of executing the view function on the preview state
    ///
    /// # Process Flow
    ///
    /// 1. **Create isolated database**: Copy current database state
    /// 2. **Process block**: Execute `_start` function with block data
    /// 3. **Create view runtime**: Set up new runtime for view execution
    /// 4. **Execute view function**: Run the specified view function
    /// 5. **Return result**: Extract and return the view function output
    ///
    /// # Isolation Guarantees
    ///
    /// - Changes are made to a database copy, not the original
    /// - Original database state remains unchanged
    /// - Multiple previews can run concurrently
    /// - Preview state is discarded after execution
    ///
    /// # Use Cases
    ///
    /// - **Testing**: Validate block effects before committing
    /// - **Simulation**: Explore "what-if" scenarios
    /// - **Debugging**: Inspect intermediate state during development
    /// - **Analysis**: Query state changes without persistence
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Preview the effect of a block on account balances
    /// let balance_after = runtime.preview(
    ///     &block_data,
    ///     "get_balance".to_string(),
    ///     &address_bytes,
    ///     height
    /// )?;
    ///
    /// println!("Balance after block: {}", hex::encode(balance_after));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Database copy creation fails
    /// - Block processing fails during `_start` execution
    /// - View function is not found in the WASM module
    /// - View function execution fails
    /// - Memory access errors occur
    pub async fn preview(
        &self,
        block: &Vec<u8>,
        symbol: String,
        input: &Vec<u8>,
        height: u32,
    ) -> Result<Vec<u8>> where <T as KeyValueStoreLike>::Batch: Send {
        // Create preview context with isolated DB copy
                        let preview_db = {
                            let guard = self.context.read().unwrap();
                            guard.db.create_isolated_copy()
                        };
                
                        // Create a new runtime with preview db using the async engine
                        // Process the preview block at height + 1 to simulate adding it after the target height
                        let preview_height = height + 1;
                        
                        // Use new_with_db_indexer which sets up proper indexer linker for processing blocks
                        let runtime =
                            Self::new_with_db_indexer(preview_db, preview_height, self.async_engine.clone(), self.async_module.clone()).await?;
                        runtime.context.write().unwrap().block = block.clone();
                
                        // Execute block via _start to populate preview db
                        {
                            let mut instance_guard = runtime.instance.lock().await;
                        let WasmInstance { ref mut store, instance } = &mut *instance_guard;
                            let start = instance
                                .get_typed_func::<(), ()>(&mut *store, "_start")
                                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to get _start function for preview")?;
                
                            // Use call_async since we're using an async store
                            match start.call_async(&mut *store, ()).await {
                                Ok(_) => {
                                    let context_guard = runtime.context.read().unwrap();
                                    let had_failure = store.data().had_failure;
                                    let state = context_guard.state.load(std::sync::atomic::Ordering::SeqCst);
                                    if state != 1 && !had_failure {
                                        return Err(anyhow!("indexer exited unexpectedly during preview: state={}, had_failure={}", state, had_failure));
                                    }
                                    if had_failure {
                                        return Err(anyhow!("indexer had failure during preview execution: state={}", state));
                                    }
                                }
                                Err(e) => {
                                    log::error!("Preview _start execution failed: {:?}", e);
                                    return Err(anyhow::anyhow!("{}", e)).context("Error executing _start in preview");
                                },
                            }
                        }
                
                        // Create new runtime just for the view using the updated preview DB
                        // Query at the preview height to see the state after processing the preview block
                        let view_runtime = {
                            let preview_db = {
                                let ctx = runtime.context.read().unwrap();
                                ctx.db.clone()
                            };
                            let mut linker = Linker::<State>::new(&self.engine);
                            // Preview view shares the view-runtime contract:
                            // give it a thread registry so `__flush` syscalls
                            // can spawn / join. The registry is dropped with
                            // the Store at end-of-preview.
                            let preview_view_state = State::new().with_view_threads(
                                std::sync::Arc::new(crate::view_threads::ViewThreadRegistry::new()),
                            );
                            let mut wasmstore = Store::<State>::new(&self.engine, preview_view_state);
                            let view_context = Arc::<RwLock<MetashrewRuntimeContext<T>>>::new(RwLock::new(
                                MetashrewRuntimeContext::new(preview_db, preview_height, vec![]),
                            ));
                
                            wasmstore.limiter(|state| &mut state.limits);
                
                            Self::setup_linker(view_context.clone(), &mut linker).await
                                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup basic linker for preview view")?;
                            Self::setup_linker_view(
                                view_context.clone(),
                                &mut linker,
                                Some(self.module.clone()),
                            )
                            .await
                            .map_err(|e| anyhow::anyhow!("{}", e))
                            .context("Failed to setup view linker for preview")?;
                            linker.define_unknown_imports_as_traps(&self.module)?;
                
                            let instance = linker
                                .instantiate_async(&mut wasmstore, &self.module)
                                .await
                                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to instantiate WASM module for preview view")?;
                
                            MetashrewRuntime {
                                engine: self.engine.clone(),
                                async_engine: self.engine.clone(),
                                module: self.module.clone(),
                                async_module: self.module.clone(),
                                linker,
                                context: view_context,
                                instance: Mutex::new(WasmInstance { store: wasmstore, instance }),
                            }
                        };
                
                        // Set block to input for view
                        view_runtime.context.write().unwrap().block = input.clone();
                
                        // Execute view function
                        let result = {
                            let mut instance_guard = view_runtime.instance.lock().await;
                        let WasmInstance { ref mut store, instance } = &mut *instance_guard;
                            let func = instance
                                .get_typed_func::<(), i32>(&mut *store, symbol.as_str())
                                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to get view function")?;
                
                            // Use call_async since we're using an async store
                            let result = func
                                .call_async(&mut *store, ()).await
                                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to execute view function")?;
                
                            let memory = instance
                                .get_memory(&mut *store, "memory")
                                .ok_or_else(|| anyhow!("Failed to get memory for view result"))?;
                
                            // Get the final result
                            read_arraybuffer_as_vec(
                                memory.data(&*store),
                                result,
                            )
                        };
                        Ok(result)    }

    // Async version of preview for use with the view server
    pub async fn preview_async(
        &self,
        block: &Vec<u8>,
        symbol: String,
        input: &Vec<u8>,
        height: u32,
    ) -> Result<Vec<u8>> where <T as KeyValueStoreLike>::Batch: Send {
        // For now, just use the synchronous version
        // In the future, we can implement a fully async version if needed
        self.preview(block, symbol, input, height).await
    }

    /// Execute a view function to query historical blockchain state
    ///
    /// View functions provide read-only access to the blockchain state at any
    /// historical block height. They use the asynchronous engine with cooperative
    /// yielding to handle long-running queries without blocking.
    ///
    /// # Parameters
    ///
    /// - `symbol`: Name of the view function to execute
    /// - `input`: Input data for the view function (typically query parameters)
    /// - `height`: Block height to query (determines database state snapshot)
    ///
    /// # Returns
    ///
    /// The result of the view function execution as raw bytes
    ///
    /// # Execution Model
    ///
    /// - **Read-only**: No database modifications are allowed
    /// - **Historical**: Queries state at the specified block height
    /// - **Asynchronous**: Uses cooperative yielding for long operations
    /// - **Isolated**: Each view runs in its own WASM instance
    ///
    /// # State Access
    ///
    /// View functions access historical state through:
    /// - **Append-only lookups**: Height-indexed lookups on append-only data
    /// - **Immutable snapshots**: Consistent view of state at target height
    /// - **Efficient indexing**: Optimized for historical range queries
    ///
    /// # Cooperative Yielding
    ///
    /// The async engine provides:
    /// - **Fuel limits**: Prevents infinite loops and resource exhaustion
    /// - **Yield intervals**: Periodic yielding for responsive execution
    /// - **Cancellation**: Ability to abort long-running queries
    ///
    /// # Use Cases
    ///
    /// - **Balance queries**: Get account balances at specific heights
    /// - **Transaction history**: Query transaction effects over time
    /// - **State analysis**: Analyze protocol state evolution
    /// - **API endpoints**: Power JSON-RPC query interfaces
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Query account balance at a specific block height
    /// let balance = runtime.view(
    ///     "get_balance".to_string(),
    ///     &address_bytes,
    ///     height
    /// ).await?;
    ///
    /// // Query transaction count for an address
    /// let tx_count = runtime.view(
    ///     "get_transaction_count".to_string(),
    ///     &address_bytes,
    ///     height
    /// ).await?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - View function is not found in the WASM module
    /// - Input data is malformed or invalid
    /// - Database query fails or times out
    /// - WASM execution encounters an error
    /// - Memory access violations occur
        pub async fn view(&self, symbol: String, input: &Vec<u8>, height: u32) -> Result<Vec<u8>> {
            self.view_with_limits(symbol, input, height, None).await
        }

        /// Same as [`view`] but allows the caller to thread per-view
        /// [`wasmtime::StoreLimits`] (e.g. capping linear-memory growth at
        /// `--view-memory-mb`).
        ///
        /// v9.0.5-rc.2: under heavy view-call load the view path was
        /// previously the dominant memory-allocator in the indexer
        /// process — sustained mallocs across N concurrent stores OOM-killed
        /// the indexer thread. With a per-view memory cap, growths beyond
        /// the budget trap the WASM cleanly and surface as a
        /// `ResourceExhausted`-style error to the JSON-RPC caller instead
        /// of bringing down the process.
        ///
        /// **Important**: this only affects the view path. The indexer
        /// store (created by `new` / `new_with_db_indexer`) still uses
        /// unbounded `StoreLimits` so block-application can grow memory as
        /// needed for normal block processing.
        pub async fn view_with_limits(
            &self,
            symbol: String,
            input: &Vec<u8>,
            height: u32,
            store_limits: Option<StoreLimits>,
        ) -> Result<Vec<u8>> {
            let db = {
                let guard = self.context.read().unwrap();
                guard.db.clone()
            };

            // Create a new async runtime for the view, optionally with a
            // memory-capping StoreLimits.
            let view_runtime = Self::new_with_db_async_limited(
                db,
                height,
                self.async_engine.clone(),
                self.async_module.clone(),
                store_limits,
            )
            .await?;

            // Set the input as the block data
            view_runtime.context.write().unwrap().block = input.clone();

            // Set fuel for cooperative yielding
            let result = {
                let mut instance_guard = view_runtime.instance.lock().await;
                let WasmInstance { store, instance } = &mut *instance_guard;
                store.set_fuel(u64::MAX)?;
                store
                    .fuel_async_yield_interval(Some(10000))?;

                // Execute view function
                let func = instance
                    .get_typed_func::<(), i32>(&mut *store, symbol.as_str())
                    .map_err(|e| anyhow::anyhow!("{}", e)).with_context(|| format!("Failed to get view function '{}'", symbol))?;

                // Use async call
                let result = func
                    .call_async(&mut *store, ())
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e)).with_context(|| format!("Failed to execute view function '{}'", symbol))?;

                let memory = instance
                    .get_memory(&mut *store, "memory")
                    .ok_or_else(|| anyhow!("Failed to get memory for view result"))?;

                Ok(read_arraybuffer_as_vec(
                    memory.data(store),
                    result,
                ))
            };
            result
        }

    /// Force OS to commit all 4GB pages at initial instantiation to verify memory availability.
    ///
    /// This runs ONCE when the runtime is first created to ensure 4GB is available.
    /// If 4GB cannot be allocated, the runtime fails fast with a clear error message.
    ///
    /// This ensures deterministic execution - all nodes either:
    /// 1. Start successfully with 4GB committed, or
    /// 2. Fail immediately at startup with insufficient memory error
    ///
    /// Expected time: 100-500ms (one-time cost at startup)
    fn force_initial_memory_commit(memory: &wasmtime::Memory, store: &mut Store<State>) -> Result<()> {
        const WASM_PAGE_SIZE: usize = 65536; // 64KB
        const WASM_MAX_PAGES: u64 = 65536; // 4GB / 64KB = 65,536 pages

        log::info!("🔒 Pre-allocating 4GB WASM memory (one-time startup verification)...");
        let start = std::time::Instant::now();

        // First, check current memory size
        let initial_pages = memory.size(&*store);
        log::debug!("Initial WASM memory size: {} pages ({}MB)",
            initial_pages,
            (initial_pages as usize * WASM_PAGE_SIZE) / (1024 * 1024)
        );

        // Grow memory to maximum size (4GB)
        // This will fail if the host doesn't have 4GB available
        let pages_to_grow = WASM_MAX_PAGES.saturating_sub(initial_pages);
        if pages_to_grow > 0 {
            log::debug!("Growing memory by {} pages to reach 4GB maximum...", pages_to_grow);
            memory.grow(&mut *store, pages_to_grow)
                .map_err(|e| anyhow::anyhow!("{}", e)).with_context(|| {
                    format!(
                        "Failed to grow WASM memory from {} pages to {} pages (4GB total). \
                         This indicates insufficient physical memory available. \
                         WASM32 requires 4GB of available memory for deterministic execution.",
                        initial_pages,
                        WASM_MAX_PAGES
                    )
                })?;
            log::debug!("Memory grown successfully to {} pages (4GB)", WASM_MAX_PAGES);
        }

        // Now touch NEW pages (after initial_pages) to force OS to commit physical memory
        // CRITICAL: We ONLY touch pages that were newly allocated, NOT the initial pages
        // The initial pages contain WASM data sections and heap metadata that must be preserved
        log::debug!("Touching {} NEW pages (preserving {} initial pages with WASM data) to force physical memory commit...",
            pages_to_grow, initial_pages);

        // Use a zero buffer to touch pages - since these are new pages, writing zeros is fine
        // The initial pages already have proper WASM initialization and we must NOT overwrite them
        let zero_block = vec![0u8; 65536]; // Full 64KB page

        // Start from initial_pages (skip the initialized memory) and touch only NEW pages
        for page_num in initial_pages as usize..WASM_MAX_PAGES as usize {
            let offset = page_num * WASM_PAGE_SIZE;

            // Touch the page to force physical allocation (just write zeros to new pages)
            memory.write(&mut *store, offset, &zero_block)
                .map_err(|e| anyhow::anyhow!("{}", e)).with_context(|| {
                    format!(
                        "Failed to commit memory page {} of {} (offset 0x{:x}). \
                         This indicates insufficient physical memory available. \
                         WASM32 requires 4GB of available memory for deterministic execution.",
                        page_num + 1,
                        WASM_MAX_PAGES,
                        offset
                    )
                })?;

            // Log progress every 8192 pages (512MB)
            if page_num % 8192 == 0 && page_num > initial_pages as usize {
                let gb = (page_num * WASM_PAGE_SIZE) / (1024 * 1024 * 1024);
                log::debug!("  Committed {}GB of 4GB...", gb);
            }
        }

        let total_duration = start.elapsed();
        log::info!("✅ Memory pre-allocation complete in {:?}", total_duration);
        Ok(())
    }

    /// Fast memory reset between blocks by zeroing only used memory.
    ///
    /// This is MUCH faster than recreating the instance:
    /// - Current approach: 100-500ms per block (full recreate)
    /// - Optimized approach: 1-10ms per block (zero used memory only)
    ///
    /// Memory is already committed from initial instantiation, we just need to
    /// zero it out to ensure clean state for the next block.
    fn fast_memory_reset(memory: &wasmtime::Memory, store: &mut Store<State>) -> Result<()> {
        const WASM_PAGE_SIZE: usize = 65536; // 64KB

        // Get current memory size in WASM pages
        let current_pages = memory.size(&*store) as usize;

        log::debug!(
            "Resetting {} WASM pages ({}MB)",
            current_pages,
            (current_pages * WASM_PAGE_SIZE) / (1024 * 1024)
        );

        let start = std::time::Instant::now();
        let zero_page = vec![0u8; WASM_PAGE_SIZE];

        // Zero out all currently allocated pages
        for page_num in 0..current_pages {
            let offset = page_num * WASM_PAGE_SIZE;
            memory.write(&mut *store, offset, &zero_page)
                .with_context(|| format!("Failed to zero memory page {}", page_num))?;
        }

        log::debug!("Memory reset completed in {:?}", start.elapsed());
        Ok(())
    }

    pub async fn refresh_memory(&self) -> Result<()> {
        let mut instance_guard = self.instance.lock().await;

        // Log memory state before refresh
        let had_failure = instance_guard.store.data().had_failure;
        log::debug!("Memory refresh: had_failure={}", had_failure);

        // ALWAYS refresh memory for deterministic behavior between blocks
        // This ensures no WASM state persists between blocks, preventing non-determinism
        // We recreate the instance to ensure all data sections and heap are properly initialized
        // The physical 4GB allocation was already done at startup, so this is just reinitializing
        let mut wasmstore = Store::<State>::new(&self.engine, State::new());
        wasmstore.limiter(|state| &mut state.limits);
        let new_instance = self
            .linker
            .instantiate_async(&mut wasmstore, &self.module)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to instantiate module during memory refresh")?;

        *instance_guard = WasmInstance {
            store: wasmstore,
            instance: new_instance,
        };

        log::debug!("Memory refresh completed successfully");
        Ok(())
    }

    /// Execute the current block through the WASM indexer
    ///
    /// This is the core block processing method that executes the WASM module's
    /// `_start` function to process the current block data. It handles the complete
    /// block processing lifecycle including chain reorganization detection,
    /// execution, and memory cleanup.
    ///
    /// # Block Processing Flow
    ///
    /// 1. **Initialize state**: Reset execution state to 0 (starting)
    /// 2. **Handle reorgs**: Check for and handle chain reorganizations
    /// 3. **Execute WASM**: Call the `_start` function with current block data
    /// 4. **Validate completion**: Ensure indexer completed successfully (state = 1)
    /// 5. **Refresh memory**: Reset WASM memory for deterministic execution
    ///
    /// # Deterministic Execution
    ///
    /// The runtime ensures deterministic behavior through:
    /// - **Memory isolation**: Fresh WASM memory for each block
    /// - **State validation**: Strict execution state checking
    /// - **Error handling**: Consistent error propagation
    /// - **Resource limits**: Bounded execution resources
    ///
    /// # Chain Reorganization Handling
    ///
    /// Before processing, the method:
    /// - Compares context height with database tip height
    /// - Detects potential chain reorganizations
    /// - Handles rollback scenarios (implementation pending)
    ///
    /// # Memory Management
    ///
    /// After each block execution:
    /// - WASM memory is completely refreshed
    /// - Module instance is recreated
    /// - No state persists between blocks
    /// - Ensures consistent execution environment
    ///
    /// # State Validation
    ///
    /// The method validates that:
    /// - Indexer reaches completion state (state = 1)
    /// - No host function failures occurred
    /// - WASM execution completed without traps
    ///
    /// # Example Usage
    ///
    /// ```rust,ignore
    /// // Set block data in context first
    /// {
    ///     let mut guard = runtime.context.lock()?;
    ///     guard.block = block_data.to_vec();
    ///     guard.height = height;
    /// }
    ///
    /// // Process the block
    /// runtime.run()?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Chain reorganization handling fails
    /// - `_start` function is not found in WASM module
    /// - WASM execution traps or fails
    /// - Indexer exits without reaching completion state
    /// - Memory refresh fails after execution
    /// - Host function failures occur during execution
    pub async fn run(&self) -> Result<(), anyhow::Error> {
        let height = {
            let ctx = self.context.read().unwrap();
            ctx.state.store(0, std::sync::atomic::Ordering::SeqCst);
            ctx.height
        };
        log::info!("Starting block processing for height {}", height);

        let execution_result = {
            let mut instance_guard = self.instance.lock().await;
            let WasmInstance { ref mut store, instance } = &mut *instance_guard;
            // Clear any error captured by a previous block before this run.
            *store.data().last_flush_error.lock().unwrap() = None;
            let start = instance
                .get_typed_func::<(), ()>(&mut *store, "_start")
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to get _start function")?;

            // Note: Chain reorganization detection is now handled at the sync framework level
            // using proper block hash comparison, not at the runtime level

            // Use call_async since we're using an async store
            match start.call_async(&mut *store, ()).await {
                Ok(_) => {
                    let captured_err = store.data().last_flush_error.lock().unwrap().clone();
                    if let Some(msg) = captured_err {
                        log::error!("Block {} __flush atomic write failed: {}", height, msg);
                        Err(anyhow!("__flush atomic write failed: {}", msg))
                    } else if self.context.read().unwrap().state.load(std::sync::atomic::Ordering::SeqCst) != 1
                        && !store.data().had_failure
                    {
                        log::error!("Block {} indexer exited unexpectedly (state != 1 and no failure)", height);
                        Err(anyhow!("indexer exited unexpectedly"))
                    } else if store.data().had_failure {
                        log::error!("Block {} indexer host function reported failure", height);
                        Err(anyhow!("indexer host function reported failure"))
                    } else {
                        log::info!("Block {} WASM execution completed successfully", height);
                        Ok(())
                    }
                }
                Err(e) => {
                    log::error!("Block {} WASM execution failed: {:?}", height, e);
                    Err(anyhow::anyhow!("{}", e)).context("Error calling _start function")
                }
            }
        };

        // ALWAYS refresh memory after block execution for deterministic behavior
        // This ensures no WASM state persists between blocks
        if let Err(refresh_err) = self.refresh_memory().await {
            log::error!("Block {} failed to refresh memory after block execution: {}", height, refresh_err);
            // Return the refresh error as it's critical for deterministic execution
            return Err(refresh_err).context("Memory refresh failed after block execution");
        }

        log::info!("Block {} completed processing with memory refresh", height);
        execution_result
    }

    /// Handle chain reorganization by rolling back to the specified height
    ///
    /// **DEPRECATED**: This method is no longer used as reorg detection has been moved
    /// to the sync framework level where it can properly compare block hashes from the
    /// Bitcoin node. The sync framework uses proper reorg detection by comparing stored
    /// block hashes with actual block hashes from bitcoind RPC.
    ///
    /// This method is kept for backward compatibility but should not be called.
    #[deprecated(note = "Reorg detection moved to sync framework level")]
    pub async fn handle_reorg(&self) -> Result<()> {
        let (context_height, db_tip_height) = {
            let mut guard = self.context.write().unwrap();
            let db_tip = match guard.db.get(&TIP_HEIGHT_KEY.as_bytes().to_vec()) {
                Ok(Some(bytes)) if bytes.len() >= 4 => {
                    u32::from_le_bytes(bytes[..4].try_into().unwrap())
                }
                _ => 0,
            };
            (guard.height, db_tip)
        };

        if context_height > db_tip_height + 1 {
            return Err(anyhow!(
                "Block height {} is too far ahead of tip {}",
                context_height,
                db_tip_height
            ));
        }

        // Only trigger reorg if context height is strictly less than db tip height
        // If they're equal, we're reprocessing the same block (normal on restart)
        if context_height < db_tip_height {
            if context_height == 0 {
                log::warn!("Reorg at height 0 is not a standard rollback.");
                return Ok(());
            }
            let target_height = context_height - 1;
            log::info!(
                "Reorg detected: rolling back from {} to {}",
                db_tip_height,
                target_height
            );

            let mut db = self.context.read().unwrap().db.clone();
            let mut batch = db.create_batch();

            // Rollback all chains to the target height.
            crate::chain_entries::rollback_all_keys_to_batch(&db, &mut batch, target_height)?;

            // Update the runtime-side tip pointer.
            batch.put(
                &TIP_HEIGHT_KEY.as_bytes().to_vec(),
                &target_height.to_le_bytes(),
            );

            db.write(batch)
                .map_err(|e| anyhow!("Failed to write reorg batch: {}", e))?;
            log::info!("Reorg to height {} complete", target_height);
        }

        Ok(())
    }

    /// Get the value of a key at a specific block height using the append-only data structure.
    /// This function performs a binary search on the list of historical values for the key.
    pub async fn get_value_at_height(
        context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
        key: &Vec<u8>,
        height: u32,
    ) -> Result<Vec<u8>> {
        let db = {
            let guard = context.read().unwrap();
            guard.db.clone()
        };
        match crate::chain_entries::get_at_height(&db, key, height) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Ok(Vec::new()),
            Err(e) => Err(anyhow!("Append-only query error: {}", e)),
        }
    }

    /// Append a key to an update list
    pub fn db_append(
        _context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
        batch: &mut T::Batch,
        update_key: &Vec<u8>,
        key: &Vec<u8>,
    ) -> Result<()> {
        // Create a key for the update list
        let update_list_key = crate::key_utils::make_prefixed_key(b"updates:", update_key);

        // Add the key to the update list
        batch.put(&update_list_key, key.clone());

        Ok(())
    }

    /// Build a fresh view-runtime instance for a spawned view thread.
    ///
    /// Called from inside a `tokio::spawn` invoked by the `__flush`
    /// ThreadSpawn handler. We can't share the parent view's Store
    /// across threads (Store isn't Send for the duration of an in-flight
    /// call), so spawn = "build a child Store against the same Engine +
    /// Module, looks up the indirect-function-table entry, call it".
    ///
    /// For wasm built WITHOUT shared memory + atomics, the child runs in
    /// an isolated linear-memory + module-local-globals — no cross-thread
    /// data sharing. The cross-thread visibility you'd expect from a
    /// real thread requires the wasm to be built with `(memory (shared
    /// N M))` so the imports section binds a shared memory; the host's
    /// `wasm_threads(true)` engine flag accepts that.
    ///
    /// Returns the i32 return value of the spawned function, or
    /// `INVALID_EXIT_CODE` on any failure (instantiate, table lookup,
    /// trap). Failures are logged but never propagated as panics — the
    /// spawned task runs to completion under tokio::spawn and the join
    /// path reads the exit code.
    pub(crate) async fn run_spawned_view_thread_impl(
        engine: wasmtime::Engine,
        module: wasmtime::Module,
        context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
        fn_idx: u32,
        arg: u32,
    ) -> i32 {
        // Build a fresh linker for this child store. Re-runs the view
        // linker setup so __get / __get_len / __log / __flush are all
        // wired identically to the parent — including __flush itself,
        // which lets spawned threads recursively spawn (rare but legal).
        let mut linker = Linker::<State>::new(&engine);
        let mut state = State::new();
        state.view_threads = Some(std::sync::Arc::new(
            crate::view_threads::ViewThreadRegistry::new(),
        ));
        let mut store = Store::<State>::new(&engine, state);
        store.limiter(|s| &mut s.limits);
        // Fuel for cooperative yielding, matching the parent view.
        let _ = store.set_fuel(u64::MAX);
        let _ = store.fuel_async_yield_interval(Some(10000));

        if let Err(e) = Self::setup_linker(context.clone(), &mut linker).await {
            log::warn!("spawned view thread: setup_linker failed: {:?}", e);
            return crate::view_threads::INVALID_EXIT_CODE;
        }
        if let Err(e) =
            Self::setup_linker_view(context.clone(), &mut linker, Some(module.clone())).await
        {
            log::warn!("spawned view thread: setup_linker_view failed: {:?}", e);
            return crate::view_threads::INVALID_EXIT_CODE;
        }
        if let Err(e) = linker.define_unknown_imports_as_traps(&module) {
            log::warn!("spawned view thread: define_unknown_imports_as_traps failed: {:?}", e);
            return crate::view_threads::INVALID_EXIT_CODE;
        }

        let instance = match linker.instantiate_async(&mut store, &module).await {
            Ok(i) => i,
            Err(e) => {
                log::warn!("spawned view thread: instantiate failed: {:?}", e);
                return crate::view_threads::INVALID_EXIT_CODE;
            }
        };

        // Pull the indirect-function-table. Wasm built with the wasm32
        // ABI exports the table that backs `call_indirect` as
        // `__indirect_function_table`. If absent, this wasm wasn't built
        // with threading in mind — fail clean.
        let table = match instance
            .get_export(&mut store, "__indirect_function_table")
            .and_then(|e| e.into_table())
        {
            Some(t) => t,
            None => {
                log::warn!(
                    "spawned view thread: wasm has no __indirect_function_table export"
                );
                return crate::view_threads::INVALID_EXIT_CODE;
            }
        };

        let funcref = match table.get(&mut store, fn_idx as u64) {
            Some(wasmtime::Ref::Func(Some(f))) => f,
            _ => {
                log::warn!(
                    "spawned view thread: fn_idx {} not present in indirect table",
                    fn_idx
                );
                return crate::view_threads::INVALID_EXIT_CODE;
            }
        };

        // Try to call with the (i32) -> i32 ABI. If the actual table
        // entry has a different signature, wasmtime returns an error
        // from typed() and we fall back to a generic Val call.
        let typed: Result<wasmtime::TypedFunc<i32, i32>, _> = funcref.typed(&store);
        let result = match typed {
            Ok(tf) => tf.call_async(&mut store, arg as i32).await,
            Err(_) => {
                // Fall back to untyped: arg is i32, return is i32. We
                // require exactly one i32 in / one i32 out for the
                // thread-spawn ABI.
                let mut results = [wasmtime::Val::I32(0)];
                match funcref
                    .call_async(
                        &mut store,
                        &[wasmtime::Val::I32(arg as i32)],
                        &mut results,
                    )
                    .await
                {
                    Ok(()) => match results[0] {
                        wasmtime::Val::I32(v) => Ok(v),
                        _ => {
                            log::warn!(
                                "spawned view thread: fn_idx {} returned non-i32",
                                fn_idx
                            );
                            return crate::view_threads::INVALID_EXIT_CODE;
                        }
                    },
                    Err(e) => Err(e),
                }
            }
        };
        match result {
            Ok(v) => v,
            Err(e) => {
                log::warn!("spawned view thread: call failed: {:?}", e);
                crate::view_threads::INVALID_EXIT_CODE
            }
        }
    }

    pub async fn setup_linker(
        context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
        linker: &mut Linker<State>,
    ) -> Result<()> {
        let context_ref_len = context.clone();
        let context_ref_input = context.clone();

        linker
            .func_wrap(
                "env",
                "__host_len",
                move |_caller: Caller<'_, State>| -> i32 {
                    let ctx = context_ref_len.read().unwrap();
                    ctx.block.len() as i32 + 4
                },
            )
            .map_err(|e| anyhow!("Failed to wrap __host_len: {:?}", e))?;

        linker
            .func_wrap(
                "env",
                "__load_input",
                move |mut caller: Caller<'_, State>, data_start: i32| {
                    let mem = match caller.get_export("memory") {
                        Some(export) => match export.into_memory() {
                            Some(memory) => memory,
                            None => {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        },
                        None => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    };

                    let (input, height) = {
                        let ctx = context_ref_input.read().unwrap();
                        (ctx.block.clone(), ctx.height)
                    };

                    let input_clone = match try_into_vec(height.to_le_bytes()) {
                        Ok(mut v) => {
                            v.extend(input);
                            v
                        }
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    };

                    let sz = to_usize_or_trap(&mut caller, data_start);
                    if sz == usize::MAX {
                        panic!("FATAL: __load_input failed to convert data_start to usize");
                    }

                    mem.write(&mut caller, sz, input_clone.as_slice())
                        .expect("FATAL: __load_input memory write failed");
                },
            )
            .map_err(|e| anyhow!("Failed to wrap __load_input: {:?}", e))?;

        linker
            .func_wrap(
                "env",
                "__log",
                move |mut caller: Caller<'_, State>, data_start: i32| {
                    let mem = match caller.get_export("memory") {
                        Some(export) => match export.into_memory() {
                            Some(memory) => memory,
                            None => return,
                        },
                        None => return,
                    };

                    let data = mem.data(&caller);
                    let bytes = match try_read_arraybuffer_as_vec(data, data_start) {
                        Ok(v) => v,
                        Err(_) => return,
                    };

                    if let Ok(text) = std::str::from_utf8(&bytes) {
                        print!("{}", text);
                    }
                },
            )
            .map_err(|e| anyhow!("Failed to wrap __log: {:?}", e))?;

        linker
            .func_wrap(
                "env",
                "abort",
                move |mut caller: Caller<'_, State>, _: i32, _: i32, _: i32, _: i32| {
                    caller.data_mut().had_failure = true;
                },
            )
            .map_err(|e| anyhow!("Failed to wrap abort: {:?}", e))?;

        Ok(())
    }

pub async fn setup_linker_view(

        context: Arc<RwLock<MetashrewRuntimeContext<T>>>,

        linker: &mut Linker<State>,

        // v10 view-syscall: the spawn host needs engine + module to
        // re-instantiate the wasm in a child store. We accept them as
        // `Option` so existing call-sites that don't yet thread these
        // through (none in-tree, but kept for forward-compat) still
        // compile. When `None`, ThreadSpawn returns a sentinel and the
        // wasm sees an empty response (deterministic miss).
        spawn_module: Option<wasmtime::Module>,

    ) -> Result<()> {

        let context_get = context.clone();

        let context_get_len = context.clone();



                // v10 view-mode `__flush` dispatcher.
                //
                // Indexer mode treats `__flush(ptr)` as the batch-commit hook.
                // View mode has no write path, so the existing wasm `__flush`
                // call site is repurposed as a host-syscall dispatcher: the
                // wasm passes a serialized `ViewSyscall` protobuf at `ptr`, the
                // host decodes + dispatches into the LRU view-cache (CacheGet
                // / CachePut) or — once steps 4-5 land — thread spawn/join.
                //
                // The signature is unchanged on purpose: `__flush(i32) -> ()`.
                // No wasm ABI break. Wasm that does NOT know about the
                // syscall protocol still gets the legacy no-op semantic —
                // we recognize the protocol by successfully decoding the
                // payload as `ViewSyscall`; anything else (`KeyValueFlush`,
                // empty, garbage) falls through to no-op.
                //
                // Responses (CacheGet primarily) are written directly into
                // wasm linear memory at `response_ptr` as
                // `[u32 LE: response_len | response_bytes]`. Wasm
                // pre-allocates the buffer + encodes the (ptr, cap) in the
                // request.
                let context_view_flush = context.clone();
                let spawn_module_for_flush = spawn_module.clone();
                linker
                    .func_wrap_async(
                        "env",
                        "__flush",
                        move |mut caller: Caller<'_, State>, (encoded,): (i32,)| {
                            let context = context_view_flush.clone();
                            let spawn_module = spawn_module_for_flush.clone();
                            Box::new(async move {
                                use crate::view_syscall::{
                                    decode_thread_op, dispatch_view_syscall, SyscallResult,
                                    ThreadOp,
                                };

                                let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                                    Some(m) => m,
                                    None    => return,
                                };

                                // Read the proto payload + the response-buffer
                                // (ptr, max) from wasm memory. The immutable borrow
                                // via `mem.data()` must end before we call
                                // `mem.write(&mut caller, ...)` later — clone the
                                // bytes we need and let the scope drop.
                                let (payload, height) = {
                                    let data = mem.data(&caller);
                                    let payload = match try_read_arraybuffer_as_vec(data, encoded) {
                                        Ok(v) => v,
                                        Err(_) => return,
                                    };
                                    let height = context.read().unwrap().height;
                                    (payload, height)
                                };

                                // We need response_ptr/max from the proto, but we
                                // also let the dispatcher decode and run the op.
                                // Decode once here to extract the (ptr, max) tuple.
                                let (response_ptr, response_max) = {
                                    use prost::Message;
                                    use crate::proto::metashrew::ViewSyscall;
                                    match ViewSyscall::decode(payload.as_slice()) {
                                        Ok(s) => (s.response_ptr as usize, s.response_max as usize),
                                        Err(_) => return, // legacy no-op
                                    }
                                };

                                match dispatch_view_syscall(height, &payload) {
                                    SyscallResult::NotASyscall | SyscallResult::NoResponse => {
                                        // No memory write needed.
                                    }
                                    SyscallResult::Respond(value) => {
                                        write_syscall_response(
                                            &mem, &mut caller, response_ptr, response_max, &value,
                                        );
                                    }
                                    SyscallResult::UnsupportedOp => {
                                        // v10 step 4: the dispatcher returns
                                        // UnsupportedOp for thread ops because
                                        // those need the wasmtime caller +
                                        // registry, which the pure-function
                                        // dispatcher doesn't see. Try to decode
                                        // a thread op here; if found, run it
                                        // with the bits we have.
                                        match decode_thread_op(&payload) {
                                            Some(ThreadOp::Spawn(req)) => {
                                                let registry = caller
                                                    .data()
                                                    .view_threads
                                                    .clone();
                                                let response_bytes = match (registry, spawn_module.clone()) {
                                                    (Some(registry), Some(module)) => {
                                                        let engine = caller.engine().clone();
                                                        let ctx = context.clone();
                                                        // run_spawned_view_thread_blocking
                                                        // schedules a tokio::spawn_blocking
                                                        // task running its own current_thread
                                                        // runtime, dodging wasmtime's
                                                        // not-quite-Send setup_linker_view
                                                        // future. The returned JoinHandle is
                                                        // registered so wasm can join by id.
                                                        let handle = crate::view_threads::run_spawned_view_thread_blocking::<T>(
                                                            engine,
                                                            module,
                                                            ctx,
                                                            req.fn_idx,
                                                            req.arg,
                                                        );
                                                        let tid = registry.register(handle);
                                                        tid.to_le_bytes().to_vec()
                                                    }
                                                    _ => {
                                                        // No registry / no module
                                                        // available — fail closed
                                                        // by minting INVALID_THREAD_ID (0).
                                                        crate::view_threads::INVALID_THREAD_ID
                                                            .to_le_bytes()
                                                            .to_vec()
                                                    }
                                                };
                                                write_syscall_response(
                                                    &mem,
                                                    &mut caller,
                                                    response_ptr,
                                                    response_max,
                                                    &response_bytes,
                                                );
                                            }
                                            Some(ThreadOp::Join(req)) => {
                                                let registry = caller
                                                    .data()
                                                    .view_threads
                                                    .clone();
                                                let exit = match registry {
                                                    Some(r) => r.join(req.thread_id).await,
                                                    None => crate::view_threads::INVALID_EXIT_CODE,
                                                };
                                                write_syscall_response(
                                                    &mem,
                                                    &mut caller,
                                                    response_ptr,
                                                    response_max,
                                                    &exit.to_le_bytes(),
                                                );
                                            }
                                            None => {
                                                // Not a thread op either —
                                                // genuine unsupported op. Emit a
                                                // deterministic-miss marker.
                                                write_syscall_response(
                                                    &mem, &mut caller, response_ptr, response_max, &[],
                                                );
                                            }
                                        }
                                    }
                                }
                            })
                        },
                    )
                    .map_err(|e| anyhow!("Failed to wrap __flush: {:?}", e))?;



        linker

            .func_wrap_async(

                "env",

                "__get",

                move |mut caller: Caller<'_, State>, (key, value): (i32, i32)| {
                    let context_get = context_get.clone();

                    Box::new(async move {
                        let mem = match caller.get_export("memory") {
                            Some(export) => match export.into_memory() {
                                Some(memory) => memory,
                                None => {
                                    caller.data_mut().had_failure = true;
                                    return;
                                }
                            },
                            None => {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        };

                        let data = mem.data(&caller);
                        let height = context_get.clone().read().unwrap().height;



                        match try_read_arraybuffer_as_vec(data, key) {
                            Ok(key_vec) => {
                                // Use append-only store for historical queries in view functions
                                let lookup = Self::get_value_at_height(context_get.clone(), &key_vec, height).await;

                                match lookup {
                                    Ok(lookup) => {
                                        // CRITICAL: Memory write failures are FATAL to prevent silent state corruption
                                        mem.write(&mut caller, value as usize, lookup.as_slice())
                                            .expect("FATAL: view __get memory write failed - WASM memory bounds exceeded.");
                                    }
                                    Err(_) => {
                                        // Key not found, return empty
                                        // CRITICAL: Memory write failures are FATAL to prevent silent state corruption
                                        mem.write(&mut caller, value as usize, &[])
                                            .expect("FATAL: view __get memory write failed for empty value - WASM memory bounds exceeded.");
                                    }
                                }
                            }
                            Err(_) => {
                                let error_bits = u32_to_vec(i32::MAX.try_into().unwrap())
                                    .expect("FATAL: Failed to convert error code to bytes");
                                // CRITICAL: Memory write failures are FATAL to prevent silent state corruption
                                mem.write(
                                    &mut caller,
                                    (value - 4) as usize,
                                    error_bits.as_slice(),
                                )
                                .expect("FATAL: view __get memory write failed for error bits - WASM memory bounds exceeded.");
                            }

                        }

                    })

                },

            )

            .map_err(|e| anyhow!("Failed to wrap __get: {:?}", e))?;



        linker

            .func_wrap_async(

                "env",

                "__get_len",

                move |mut caller: Caller<'_, State>, (key,): (i32,)| {
                    let context_get_len = context_get_len.clone();

                    Box::new(async move {
                        let mem = match caller.get_export("memory") {
                            Some(export) => match export.into_memory() {
                                Some(memory) => memory,
                                None => return i32::MAX,

                            },

                            None => return i32::MAX,

                        };



                        let data = mem.data(&caller);

                        let height = context_get_len.clone().read().unwrap().height;



                        match try_read_arraybuffer_as_vec(data, key) {

                            Ok(key_vec) => {

                                // Use append-only store for historical queries in view functions

                                let lookup = Self::get_value_at_height(context_get_len.clone(), &key_vec, height).await;



                                match lookup {

                                    Ok(value) => value.len() as i32,

                                    Err(_) => 0,

                                }

                            }

                            Err(_) => i32::MAX,

                        }

                    })

                },

            )

            .map_err(|e| anyhow!("Failed to wrap __get_len: {:?}", e))?;



        Ok(())

    }    async fn new_with_db(
        db: T,
        height: u32,
        engine: wasmtime::Engine,
        module: wasmtime::Module,
    ) -> Result<MetashrewRuntime<T>> {
        let mut linker = Linker::<State>::new(&engine);
        let mut wasmstore = Store::<State>::new(&engine, State::new());
        let context = Arc::<RwLock<MetashrewRuntimeContext<T>>>::new(RwLock::<
            MetashrewRuntimeContext<T>,
        >::new(
            MetashrewRuntimeContext::new(db, height, vec![]),
        ));
        {
            wasmstore.limiter(|state| &mut state.limits)
        }
        {
            Self::setup_linker(context.clone(), &mut linker).await
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup basic linker")?;
            Self::setup_linker_preview(context.clone(), &mut linker).await
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup preview linker")?;
            linker.define_unknown_imports_as_traps(&module)?;
        }
        let instance = linker
            .instantiate_async(&mut wasmstore, &module)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to instantiate WASM module")?;
        Ok(MetashrewRuntime {
            engine: engine.clone(),
            async_engine: engine,
            module: module.clone(),
            async_module: module.clone(),
            linker,
            context,
            instance: Mutex::new(WasmInstance { store: wasmstore, instance }),
        })
    }

    async fn new_with_db_indexer(
        db: T,
        height: u32,
        engine: wasmtime::Engine,
        module: wasmtime::Module,
    ) -> Result<MetashrewRuntime<T>> where <T as KeyValueStoreLike>::Batch: Send {
        let mut linker = Linker::<State>::new(&engine);
        let mut wasmstore = Store::<State>::new(&engine, State::new());
        let context = Arc::<RwLock<MetashrewRuntimeContext<T>>>::new(RwLock::<
            MetashrewRuntimeContext<T>,
        >::new(
            MetashrewRuntimeContext::new(db, height, vec![]),
        ));
        {
            wasmstore.limiter(|state| &mut state.limits);
            // Set fuel for async execution if engine has fuel enabled
            // Use a high limit to avoid running out during block processing
            // We can't check if fuel is enabled, so just try to set it
            let _ = wasmstore.set_fuel(u64::MAX);
        }
        {
            Self::setup_linker(context.clone(), &mut linker).await
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup basic linker")?;
            Self::setup_linker_indexer(context.clone(), &mut linker).await
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup indexer linker")?;
            linker.define_unknown_imports_as_traps(&module)?;
        }
        let instance = linker
            .instantiate_async(&mut wasmstore, &module)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to instantiate WASM module")?;
        Ok(MetashrewRuntime {
            engine: engine.clone(),
            async_engine: engine,
            module: module.clone(),
            async_module: module.clone(),
            linker,
            context,
            instance: Mutex::new(WasmInstance { store: wasmstore, instance }),
        })
    }

    async fn new_with_db_async(
        db: T,
        height: u32,
        engine: wasmtime::Engine,
        module: wasmtime::Module,
    ) -> Result<MetashrewRuntime<T>> {
        Self::new_with_db_async_limited(db, height, engine, module, None).await
    }

    /// Build an async view runtime, optionally overriding the per-store
    /// `StoreLimits`.
    ///
    /// When `store_limits` is `Some`, the supplied limits (typically a
    /// memory-cap built from `ViewLimitsConfig::view_store_limits()`)
    /// replace the default unbounded limits on `State`. This is the v9.0.5-rc.2
    /// view-runtime memory isolation hook — see `view_limits.rs`.
    ///
    /// When `store_limits` is `None`, behaviour is identical to the
    /// pre-v9.0.5-rc.2 view path: a fresh `State` with `StoreLimits` set to
    /// `usize::MAX` everywhere.
    async fn new_with_db_async_limited(
        db: T,
        height: u32,
        engine: wasmtime::Engine,
        module: wasmtime::Module,
        store_limits: Option<StoreLimits>,
    ) -> Result<MetashrewRuntime<T>> {
        let mut linker = Linker::<State>::new(&engine);
        let mut state = State::new();
        if let Some(limits) = store_limits {
            state.limits = limits;
        }
        // v10 view-syscall: attach a fresh thread registry to this
        // store's State. The registry's lifetime is tied to the Store —
        // when the Store is dropped at end-of-view, the registry is
        // dropped and its Drop impl aborts any still-pending spawned
        // tasks. That's the cross-call cleanup guarantee.
        state = state.with_view_threads(std::sync::Arc::new(
            crate::view_threads::ViewThreadRegistry::new(),
        ));
        let mut wasmstore = Store::<State>::new(&engine, state);
        let context = Arc::<RwLock<MetashrewRuntimeContext<T>>>::new(RwLock::<
            MetashrewRuntimeContext<T>,
        >::new(
            MetashrewRuntimeContext::new(db, height, vec![]),
        ));
        {
            wasmstore.limiter(|state| &mut state.limits)
        }
        {
            Self::setup_linker(context.clone(), &mut linker).await
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup basic linker")?;
            Self::setup_linker_view(context.clone(), &mut linker, Some(module.clone())).await
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to setup view linker")?;
            linker.define_unknown_imports_as_traps(&module)?;
        }
        let instance = linker
            .instantiate_async(&mut wasmstore, &module)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to instantiate WASM module")?;
        Ok(MetashrewRuntime {
            engine: engine.clone(),
            async_engine: engine,
            module: module.clone(),
            async_module: module.clone(),
            linker,
            context,
            instance: Mutex::new(WasmInstance { store: wasmstore, instance }),
        })
    }

    pub async fn setup_linker_preview(
        context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
        linker: &mut Linker<State>,
    ) -> Result<()> {
        let context_ref = context.clone();
        let context_get = context.clone();
        let context_get_len = context.clone();

                linker

                    .func_wrap_async(

                        "env",

                        "__flush",

                        move |mut caller: Caller<'_, State>, (encoded,): (i32,)| {
                            let context_ref = context_ref.clone();

                            Box::new(async move {

                                let height = context_ref.clone().read().unwrap().height;

        

        

                                let mem = match caller.get_export("memory") {

                                    Some(export) => match export.into_memory() {

                                        Some(memory) => memory,

                                        None => {

                                            caller.data_mut().had_failure = true;

                                            return;

                                        }

                                    },

                                    None => {

                                        caller.data_mut().had_failure = true;

                                        return;

                                    }

                                };

                        let data = mem.data(&caller);
                        let encoded_vec = match try_read_arraybuffer_as_vec(data, encoded) {
                            Ok(v) => v,
                            Err(_) => {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        };

                        // For preview, we'll store directly in the database
                        let decoded = match KeyValueFlush::decode(&*encoded_vec) {
                            Ok(d) => d,
                            Err(_) => {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        };

                        let binding = context_ref.clone();
                        // Set state atomically before acquiring write lock
                        binding.read().unwrap().state.store(1, std::sync::atomic::Ordering::SeqCst);
                        let mut ctx = binding.write().unwrap();

                        // Use append-only store for preview operations with batching.
                        // Preview writes to an isolated DB clone, so we commit the
                        // batch immediately (no atomic-commit slot).
                        let mut batch = ctx.db.create_batch();

                        for (k, v) in decoded.list.iter().tuples() {
                            let k_owned = <Vec<u8> as Clone>::clone(k);
                            let v_owned = <Vec<u8> as Clone>::clone(v);

                            if let Err(_) = crate::chain_entries::append_value_to_batch(
                                &ctx.db,
                                &mut batch,
                                &k_owned,
                                &v_owned,
                                height,
                            ) {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        }

                        if let Err(_) = ctx.db.write(batch) {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    })
                },
            )
            .map_err(|e| anyhow!("Failed to wrap __flush: {:?}", e))?;

                                        linker
                                            .func_wrap_async(
                                                "env",
                                                "__get",
                                                move |mut caller: Caller<'_, State>, (key, value): (i32, i32)| {
                                                    let context_get = context_get.clone();
                                                    Box::new(async move {
                                                    let mem = match caller.get_export("memory") {
                                                        Some(export) => match export.into_memory() {
                                                            Some(memory) => memory,
                                                            None => {
                                                                caller.data_mut().had_failure = true;
                                                                return ();
                                                            }
                                                        },
                                                        None => {
                                                            caller.data_mut().had_failure = true;
                                                            return ();
                                                        }
                                                    };
                                                    let data = mem.data(&caller);
                                                        let height = context_get.clone().read().unwrap().height;
                                            match try_read_arraybuffer_as_vec(data, key) {
                                                Ok(key_vec) => {
                                                    // Use append-only store for historical queries in view functions
                                                    let lookup = Self::get_value_at_height(context_get.clone(), &key_vec, height).await;

                                                    match lookup {
                                                        Ok(lookup) => {
                                                            // CRITICAL: Memory write failures are FATAL to prevent silent state corruption
                                                            mem.write(&mut caller, value as usize, lookup.as_slice())
                                                                .expect("FATAL: preview __get memory write failed - WASM memory bounds exceeded.");
                                                        }
                                                        Err(_) => {
                                                            // Key not found, return empty
                                                            // CRITICAL: Memory write failures are FATAL to prevent silent state corruption
                                                            mem.write(&mut caller, value as usize, &[])
                                                                .expect("FATAL: preview __get memory write failed for empty value - WASM memory bounds exceeded.");
                                                        }
                                                    }
                                                }
                                                Err(_) => {
                                                    let error_bits = u32_to_vec(i32::MAX.try_into().unwrap())
                                                        .expect("FATAL: Failed to convert error code to bytes");
                                                    // CRITICAL: Memory write failures are FATAL to prevent silent state corruption
                                                    mem.write(
                                                        &mut caller,
                                                        (value - 4) as usize,
                                                        error_bits.as_slice(),
                                                    )
                                                    .expect("FATAL: preview __get memory write failed for error bits - WASM memory bounds exceeded.");
                                                }
                                            }
                                                    })
                                                },
                                            )            .map_err(|e| anyhow!("Failed to wrap __get: {:?}", e))?;

        linker
            .func_wrap_async(
                "env",
                "__get_len",
                move |mut caller: Caller<'_, State>, (key,): (i32,)| {
                    let context_get_len = context_get_len.clone();
                    Box::new(async move {
                    let mem = match caller.get_export("memory") {
                        Some(export) => match export.into_memory() {
                            Some(memory) => memory,
                            None => return i32::MAX,
                        },
                        None => return i32::MAX,
                    };
                    let data = mem.data(&caller);
                        let height = context_get_len.clone().read().unwrap().height;

                        match try_read_arraybuffer_as_vec(data, key) {
                            Ok(key_vec) => {
                                // Use append-only store for historical queries in view functions
                                let lookup = Self::get_value_at_height(context_get_len.clone(), &key_vec, height).await;

                                match lookup {
                                    Ok(value) => value.len() as i32,
                                    Err(_) => 0,
                                }
                            }
                            Err(_) => i32::MAX,
                        }
                    })
                },
            )
            .map_err(|e| anyhow!("Failed to wrap __get_len: {:?}", e))?;

        Ok(())
    }

    pub async fn setup_linker_indexer(
        context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
        linker: &mut Linker<State>,
    ) -> Result<()> where <T as KeyValueStoreLike>::Batch: Send {
        let context_ref = context.clone();
        let context_get = context.clone();
        let context_get_len = context.clone();

        linker
            .func_wrap(
                "env",
                "__flush",
                move |mut caller: Caller<'_, State>, encoded: i32| {
                    let (height, mut db, block_hash, pending_slot) = {
                        let guard = context_ref.read().unwrap();
                        (
                            guard.height,
                            guard.db.clone(),
                            guard.current_block_hash.clone(),
                            guard.pending_atomic_batch.clone(),
                        )
                    };

                    let mem = match caller.get_export("memory") {
                        Some(export) => match export.into_memory() {
                            Some(memory) => memory,
                            None => {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        },
                        None => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    };

                    let data = mem.data(&caller);
                    let encoded_vec = match try_read_arraybuffer_as_vec(data, encoded) {
                        Ok(v) => v,
                        Err(_e) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    };

                    let decoded = match KeyValueFlush::decode(&*encoded_vec) {
                        Ok(d) => d,
                        Err(_e) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    };

                    let key_values: Vec<(Vec<u8>, Vec<u8>)> = decoded
                        .list
                        .iter()
                        .tuples()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();

                    for (k, v) in &key_values {
                        db.track_kv_update(k.clone(), v.clone());
                    }

                    // ATOMIC PATH (production block-apply, single-batch all-or-nothing):
                    // when `pending_atomic_batch` slot is armed by `process_block_atomic`,
                    // we BUILD the WASM batch but DON'T commit it — instead we serialize
                    // the batch bytes and stash them in the slot. `commit_atomic` will
                    // reconstruct, append metadata writes, and submit one
                    // `db.write_opt(batch, sync=true)`. ONE write, ONE fsync, true
                    // all-or-nothing.
                    //
                    // LEGACY PATH (`process_block()` no-block-hash, tests): when the slot
                    // is `None`, fall through to the "build and write immediately" helper.
                    // Preserves behavior for non-atomic callers.
                    let armed_for_atomic = pending_slot.lock().unwrap().is_some();
                    if armed_for_atomic {
                        match crate::chain_entries::build_block_write_batch(
                            &db,
                            height,
                            &key_values,
                            &block_hash,
                        ) {
                            Ok(batch) => {
                                use crate::traits::BatchLike;
                                let bytes = batch.to_bytes();
                                *pending_slot.lock().unwrap() = Some(bytes);
                            }
                            Err(e) => {
                                log::error!(
                                    "flush batch build failed at height {}: {:?} \
                                     (block NOT committed, indexer will retry on next pass)",
                                    height, e
                                );
                                *caller.data_mut().last_flush_error.lock().unwrap() =
                                    Some(format!("{:?}", e));
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        }
                    } else {
                        match crate::chain_entries::write_block_batch(
                            &mut db,
                            height,
                            &key_values,
                            &block_hash,
                        ) {
                            Ok(()) => {}
                            Err(e) => {
                                log::error!(
                                    "flush atomic write failed at height {}: {:?} \
                                     (likely ENOSPC or I/O error — block NOT committed, \
                                     indexer will retry on next pass)",
                                    height, e
                                );
                                *caller.data_mut().last_flush_error.lock().unwrap() =
                                    Some(format!("{:?}", e));
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        }
                    }

                    // Set completion state
                    context_ref.read().unwrap()
                        .state.store(1, std::sync::atomic::Ordering::SeqCst);
                },
            )
            .map_err(|e| anyhow!("Failed to wrap __flush: {:?}", e))?;

        linker
            .func_wrap(
                "env",
                "__get",
                move |mut caller: Caller<'_, State>, key: i32, value: i32| {
                    let mem = match caller.get_export("memory") {
                        Some(export) => match export.into_memory() {
                            Some(memory) => memory,
                            None => {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        },
                        None => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    };

                    let data = mem.data(&caller);
                    let key_vec_result = try_read_arraybuffer_as_vec(data, key);

                    let (height, db) = {
                        let guard = context_get.read().unwrap();
                        (guard.height, guard.db.clone())
                    };

                    match key_vec_result {
                        Ok(key_vec) => {
                            let lookup = {
                                let target_height = if height > 0 { height - 1 } else { 0 };
                                match crate::chain_entries::get_at_height(&db, &key_vec, target_height) {
                                    Ok(Some(v)) => Ok(v),
                                    Ok(None) => Ok(Vec::new()),
                                    Err(e) => Err(anyhow::anyhow!("Append-only query error: {}", e)),
                                }
                            };

                            match lookup {
                                Ok(v) => {
                                    mem.write(&mut caller, value as usize, v.as_slice())
                                        .expect("FATAL: __get memory write failed");
                                }
                                Err(_) => {
                                    mem.write(&mut caller, value as usize, &[])
                                        .expect("FATAL: __get memory write failed for empty value");
                                }
                            }
                        }
                        Err(_) => {
                            let error_bits = u32_to_vec(i32::MAX.try_into().unwrap())
                                .expect("FATAL: Failed to convert error code to bytes");
                            mem.write(&mut caller, (value - 4) as usize, error_bits.as_slice())
                                .expect("FATAL: __get memory write failed for error bits");
                        }
                    }
                },
            )
            .map_err(|e| anyhow!("Failed to wrap __get: {:?}", e))?;

        linker
            .func_wrap(
                "env",
                "__get_len",
                move |mut caller: Caller<'_, State>, key: i32| -> i32 {
                    let mem = match caller.get_export("memory") {
                        Some(export) => match export.into_memory() {
                            Some(memory) => memory,
                            None => return i32::MAX,
                        },
                        None => return i32::MAX,
                    };

                    let data = mem.data(&caller);
                    let key_vec_result = try_read_arraybuffer_as_vec(data, key);

                    let (db, height) = {
                        let ctx = context_get_len.read().unwrap();
                        (ctx.db.clone(), ctx.height)
                    };

                    match key_vec_result {
                        Ok(key_vec) => {
                            let target_height = if height > 0 { height - 1 } else { 0 };
                            match crate::chain_entries::get_at_height(&db, &key_vec, target_height) {
                                Ok(Some(v)) => v.len() as i32,
                                _ => 0,
                            }
                        }
                        Err(_) => i32::MAX,
                    }
                },
            )
            .map_err(|e| anyhow!("Failed to wrap __get_len: {:?}", e))?;

        Ok(())
    }

    /// Get all keys that were touched at a specific block height
    pub fn get_keys_touched_at_height(
        _context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
        _height: u32,
    ) -> Result<Vec<Vec<u8>>> {
        // For now, return an empty list
        // In a full implementation, we would scan the database for keys modified at this height
        Ok(Vec::new())
    }

    /// Iterate backwards through all values of a key from most recent update
    pub fn iterate_key_backwards(
        _context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
        _key: &Vec<u8>,
        _from_height: u32,
    ) -> Result<Vec<(u32, Vec<u8>)>> {
        // For now, return an empty list
        // In a full implementation, we would scan historical values for this key
        Ok(Vec::new())
    }

    /// Perform a complete rollback to a specific height
    pub fn rollback_to_height(
        _context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
        target_height: u32,
    ) -> Result<()> {
        // For now, just log the rollback
        // In a full implementation, we would need to restore database state
        log::info!("Rolling back to height {}", target_height);
        Ok(())
    }

    /// Get all heights at which a key was updated
    pub fn get_key_update_heights(
        _context: Arc<RwLock<MetashrewRuntimeContext<T>>>,
        _key: &Vec<u8>,
    ) -> Result<Vec<u32>> {
        // For now, return an empty list
        // In a full implementation, we would scan for all heights where this key was modified
        Ok(Vec::new())
    }

    /// Get the accumulated database operations as a serialized batch
    /// This collects all the operations that would be written to the database
    pub async fn get_accumulated_batch(&self) -> Result<Vec<u8>> {
        let db = {
            let guard = self.context.read().unwrap();
            guard.db.clone()
        };

        // For now, we'll return an empty batch since the current implementation
        // writes directly to the database during __flush
        // In a full atomic implementation, we would collect operations in a batch
        // and return the serialized batch data here

        // Create a batch and serialize it
        let _batch = db.create_batch();

        // For now, just return empty batch data
        // In a full implementation, we would serialize the batch operations
        Ok(Vec::new())
    }

    /// Process a block atomically and return all operations in a batch
    /// This is the atomic version that collects all operations without committing them.
    ///
    /// Critical invariant: this method DOES NOT commit anything to the database.
    /// It arms a single-batch slot on the context, runs the WASM module, and the
    /// WASM `__flush` handler builds-but-does-not-write the per-block batch into
    /// that slot. The serialized batch bytes are returned via
    /// `AtomicBlockResult::batch_data` for `StorageAdapter::commit_atomic` to
    /// reconstruct, append metadata writes, and commit in exactly one
    /// `db.write_opt(batch, sync=true)` call. ONE write, ONE fsync, all-or-nothing.
    pub async fn process_block_atomic(
        &self,
        height: u32,
        block_data: &[u8],
        block_hash: &[u8],
    ) -> Result<crate::traits::AtomicBlockResult> {
        // Arm the single-batch slot BEFORE setting context height/block, then
        // populate context. The WASM `__flush` host function checks this slot:
        // when armed (Some), it builds the batch and stashes bytes here instead
        // of committing — so `commit_atomic` can append metadata and commit
        // atomically.
        let pending_slot = {
            let guard = self.context.read().unwrap();
            guard.pending_atomic_batch.clone()
        };
        *pending_slot.lock().unwrap() = Some(Vec::new()); // armed; will be replaced by __flush
        {
            let mut guard = self.context.write().unwrap();
            guard.block = block_data.to_vec();
            guard.height = height;
            guard.current_block_hash = block_hash.to_vec();
            guard.state.store(0, std::sync::atomic::Ordering::SeqCst);
        }

        // Note: Chain reorganization detection is now handled at the sync framework level
        // using proper block hash comparison, not at the runtime level

        // Execute the WASM module
        let execution_result = {
            let mut instance_guard = self.instance.lock().await;
            let WasmInstance { store, instance } = &mut *instance_guard;
            // Reset any error captured from a previous block before this run.
            *store.data().last_flush_error.lock().unwrap() = None;
            let start = instance
                .get_typed_func::<(), ()>(&mut *store, "_start")
                .map_err(|e| anyhow::anyhow!("{}", e)).context("Failed to get _start function")?;

            // Use call_async since we're using an async store
            match start.call_async(&mut *store, ()).await {
                Ok(_) => {
                    let context_state = {
                        let guard = self.context.read().unwrap();
                        guard.state.load(std::sync::atomic::Ordering::SeqCst)
                    };

                    let captured_err = store.data().last_flush_error.lock().unwrap().clone();
                    if let Some(msg) = captured_err {
                        Err(anyhow!("__flush atomic write failed: {}", msg))
                    } else if context_state != 1 && !store.data().had_failure {
                        Err(anyhow!(
                            "indexer exited unexpectedly during atomic processing"
                        ))
                    } else if store.data().had_failure {
                        Err(anyhow!(
                            "indexer host function reported failure during atomic processing"
                        ))
                    } else {
                        Ok(())
                    }
                }
                Err(e) => Err(anyhow::anyhow!("{}", e)).context("Error calling _start function in atomic processing"),
            }
        };

        // Extract the batch data before memory refresh. The batch_data is the
        // SERIALIZED RocksDB-WriteBatch bytes that __flush built and stashed in
        // `pending_atomic_batch`. We DISARM the slot here (set to None) regardless
        // of success/failure so the retry loop in `Sync::process_block` starts
        // each attempt from a clean slate — process_block_atomic re-arms it on
        // the next call.
        let batch_data = match execution_result {
            Ok(_) => {
                let batch_data = {
                    let mut slot = pending_slot.lock().unwrap();
                    slot.take().unwrap_or_default()
                };

                log::info!(
                    "processed block {} atomically ({} batch bytes)",
                    height,
                    batch_data.len()
                );

                batch_data
            }
            Err(e) => {
                // Disarm slot on failure so the next retry starts clean.
                *pending_slot.lock().unwrap() = None;
                // ALWAYS refresh memory even on execution failure for deterministic behavior
                if let Err(refresh_err) = self.refresh_memory().await {
                    log::error!("Failed to refresh memory after failed atomic block execution: {}", refresh_err);
                }
                return Err(e);
            }
        };

        // ALWAYS refresh memory after block execution for deterministic behavior
        // This ensures no WASM state persists between blocks
        if let Err(refresh_err) = self.refresh_memory().await {
            log::error!("Failed to refresh memory after atomic block execution: {}", refresh_err);
            // Return the refresh error as it's critical for deterministic execution
            return Err(refresh_err).context("Memory refresh failed after atomic block execution");
        }

        log::debug!("Memory refreshed after atomic block execution for deterministic state isolation");

        // Return the atomic result
        Ok(crate::traits::AtomicBlockResult {
            batch_data,
            height,
            block_hash: block_hash.to_vec(),
        })
    }

    /// Process a block normally (non-atomic)
    pub async fn process_block(&self, height: u32, block_data: &[u8]) -> Result<()> {
        // Set the block data and height in context. The block_hash field is
        // cleared here because callers of the non-atomic path don't supply one;
        // __flush therefore skips the block-hash record write and the sync
        // framework's `store_block_hash` is what makes that record durable. The
        // height pointers are still bundled into __flush's atomic batch, so a
        // crash before `store_block_hash` runs leaves only the block-hash
        // record missing — recoverable via the same idempotent retry path.
        {
            let mut guard = self.context.write().unwrap();
            guard.block = block_data.to_vec();
            guard.height = height;
            guard.current_block_hash = Vec::new();
            guard.state.store(0, std::sync::atomic::Ordering::SeqCst);
        }

        // Execute the block processing - run() now handles memory refresh automatically
        self.run().await
    }
}

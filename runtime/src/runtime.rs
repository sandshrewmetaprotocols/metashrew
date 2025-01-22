use crate::memory_pool::MEMORY_POOL;
use anyhow::{anyhow, Context, Result};
use dashmap::DashMap; // For concurrent caching
use parking_lot::Mutex; // Better performance than std::sync::Mutex
use std::cell::RefCell;
use wasmtime::Caller;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use wasmtime::{Linker, Memory, Store, StoreLimits, StoreLimitsBuilder};

// Cache for frequently accessed values
lazy_static::lazy_static! {
    static ref KEY_CACHE: DashMap<(Vec<u8>, u32), Vec<u8>> = DashMap::with_capacity(10_000);
    static ref LENGTH_CACHE: DashMap<Vec<u8>, u32> = DashMap::with_capacity(10_000);
}

// Local thread cache size for batch operations
const LOCAL_CACHE_SIZE: usize = 1024;

// Type aliases for better readability
type SerBlock = Vec<u8>;
type WasmMemoryResult = Result<Memory>;
type ContextGuard<'a, T> = std::sync::MutexGuard<'a, MetashrewRuntimeContext<T>>;

/// Convert a type conversion error into an anyhow error
fn to_anyhow<T, E: std::fmt::Debug>(result: std::result::Result<T, E>) -> Result<T> {
    result.map_err(|e| anyhow!("Conversion error: {:?}", e))
}

/// Convert a byte array into a Vec<u8> with error handling
fn try_into_vec<const N: usize>(bytes: [u8; N]) -> Result<Vec<u8>> {
    Vec::<u8>::try_from(bytes).map_err(|e| anyhow!("Failed to convert bytes to Vec: {:?}", e))
}

/// Convert an i32 to usize for memory access, with trap on negative values
fn to_usize_or_trap(caller: &mut Caller<'_, State>, value: i32) -> usize {
    if value < 0 {
        caller.data_mut().had_failure = true;
        usize::MAX
    } else {
        value as usize
    }
}

/// Read and convert memory buffer to Vec<u8>
fn try_read_arraybuffer_as_vec(data: &[u8], data_start: i32) -> Result<Vec<u8>> {
    if data_start < 4 {
        return Err(anyhow!("Invalid memory access: data_start < 4"));
    }

    let len_start = (data_start - 4) as usize;
    let len_bytes = data
        .get(len_start..data_start as usize)
        .ok_or_else(|| anyhow!("Invalid memory range for length"))?;
    let len = u32::from_le_bytes(len_bytes.try_into()?);

    let end_idx = (data_start as u32 + len) as usize;
    data.get(data_start as usize..end_idx)
        .map(Vec::from)
        .ok_or_else(|| anyhow!("Invalid memory range for data"))
}

/// Read memory buffer with fallback to empty Vec
fn read_arraybuffer_as_vec(data: &[u8], data_start: i32) -> Vec<u8> {
    try_read_arraybuffer_as_vec(data, data_start).unwrap_or_default()
}

/// Trait for key-value store operations
pub trait KeyValueStoreLike: Clone + Send + Sync + 'static {
    type Error: std::fmt::Debug;
    type Batch: BatchLike;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error>;
    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error>;
    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error>;
    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>;
}

/// Trait for batch operations in key-value store
pub trait BatchLike {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V);
    fn default() -> Self;
}

/// State for WASM execution
#[derive(Default)]
pub struct State {
    limits: StoreLimits,
    had_failure: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            limits: StoreLimitsBuilder::new()
                .memories(usize::MAX)
                .tables(usize::MAX)
                .instances(usize::MAX)
                .build(),
            had_failure: false,
        }
    }
}

/// Runtime context for Metashrew operations
#[derive(Clone)]
pub struct MetashrewRuntimeContext<T: KeyValueStoreLike> {
    pub db: T,
    pub height: u32,
    pub block: SerBlock,
    pub state: u32,
}

impl<T: KeyValueStoreLike> MetashrewRuntimeContext<T> {
    fn new(db: T, height: u32, block: SerBlock) -> Self {
        Self {
            db,
            height,
            block,
            state: 0,
        }
    }
}

/// Key-value database utilities
mod db_utils {
    use super::*;

    // Pre-calculated byte arrays for common indices
    lazy_static::lazy_static! {
        static ref COMMON_INDICES: Vec<[u8; 4]> = {
            let mut v = Vec::with_capacity(256);
            for i in 0..256 {
                v.push((i as u32).to_le_bytes());
            }
            v
        };
    }

    #[inline]
    pub fn make_list_key(v: &[u8], index: u32) -> Result<Vec<u8>> {
        let mut entry = MEMORY_POOL.get(v.len() + 4);
        entry.extend_from_slice(v);

        // Use pre-calculated indices for common values
        if index < 256 {
            entry.extend_from_slice(&COMMON_INDICES[index as usize]);
        } else {
            entry.extend(try_into_vec(index.to_le_bytes())?);
        }

        Ok(entry)
    }

    #[inline]
    pub fn make_length_key(key: &[u8]) -> Result<Vec<u8>> {
        // Check cache first
        if let Some(_cached) = LENGTH_CACHE.get(key) {
            let mut entry = MEMORY_POOL.get(key.len() + 4);
            entry.extend_from_slice(key);
            entry.extend_from_slice(&u32::MAX.to_le_bytes());
            return Ok(entry);
        }

        make_list_key(key, u32::MAX)
    }

    #[inline]
    pub fn make_updated_key(key: &[u8]) -> Vec<u8> {
        let mut vec = MEMORY_POOL.get(key.len());
        vec.extend_from_slice(key);
        vec
    }

    #[inline]
    pub fn annotate_value(v: &[u8], block_height: u32) -> Result<Vec<u8>> {
        // Check cache for recently used values
        let cache_key = (v.to_vec(), block_height);
        if let Some(cached) = KEY_CACHE.get(&cache_key) {
            return Ok(cached.clone());
        }

        let mut entry = MEMORY_POOL.get(v.len() + 4);
        entry.extend_from_slice(v);
        entry.extend(try_into_vec(block_height.to_le_bytes())?);

        // Cache the result if it's not too large
        if v.len() < 1024 {
            KEY_CACHE.insert(cache_key, entry.clone());
        }

        Ok(entry)
    }

    // Helper to release memory back to the pool
    #[inline]
    pub fn release_vec(vec: Vec<u8>) {
        MEMORY_POOL.put(vec);
    }
}

/// Main Metashrew runtime implementation
pub struct MetashrewRuntime<T: KeyValueStoreLike> {
    context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
    engine: wasmtime::Engine,
    wasmstore: Store<State>,
    module: wasmtime::Module,
    linker: Linker<State>,
    instance: wasmtime::Instance,
}

impl<T: KeyValueStoreLike> MetashrewRuntime<T> {
    /// Setup common linker functions
    fn setup_linker(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        linker: &mut Linker<State>,
    ) -> Result<()> {
        let context_len = context.clone();
        let context_input = context.clone();
        
        linker
            .func_wrap(
                "env",
                "__host_len",
                move |mut _caller: Caller<'_, State>| -> i32 {
                    let ctx = context_len.lock();
                    ctx.block.len() as i32 + 4
                },
            )
            .context("Failed to wrap __host_len")?;

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
                        let ctx = context_input.lock();
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

                    let sz = match to_usize_or_trap(&mut caller, data_start) {
                        usize::MAX => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                        s => s,
                    };

                    if let Err(_) = mem.write(&mut caller, sz, input_clone.as_slice()) {
                        caller.data_mut().had_failure = true;
                    }
                },
            )
            .context("Failed to wrap __load_input")?;

        linker
            .func_wrap(
                "env",
                "__log",
                |mut caller: Caller<'_, State>, data_start: i32| {
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
            .context("Failed to wrap __log")?;

        linker
            .func_wrap(
                "env",
                "abort",
                |mut caller: Caller<'_, State>, _: i32, _: i32, _: i32, _: i32| {
                    caller.data_mut().had_failure = true;
                },
            )
            .context("Failed to wrap abort")?;

        Ok(())
    }

    /// Setup indexer-specific linker functions
    fn setup_linker_indexer(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        linker: &mut Linker<State>,
    ) -> Result<()> {
        let context_ref = context.clone();
        let context_get = context.clone();
        let context_get_len = context.clone();
        
        linker
            .func_wrap(
                "env",
                "__flush",
                move |mut caller: Caller<'_, State>, encoded: i32| {
                    let height = {
                        let ctx = context_ref.lock();
                        ctx.height
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
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    };

                    let mut batch = T::Batch::default();
                    if let Err(_) = Self::db_create_empty_update_list(&mut batch, height) {
                        caller.data_mut().had_failure = true;
                        return;
                    }

                    let chunks = encoded_vec.chunks(2);
                    for chunk in chunks {
                        if chunk.len() != 2 {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                        let k = &chunk[0..1];
                        let v = &chunk[1..2];
                        if let Err(_) = Self::db_append_annotated(
                            context_ref.clone(),
                            &mut batch,
                            k,
                            v,
                            height,
                        ) {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    }

                    {
                        let mut ctx = context_ref.lock();
                        ctx.state = 1;
                        if let Err(_) = ctx.db.write(batch) {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    }
                },
            )
            .context("Failed to wrap __flush")?;

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
                    let height = {
                        let ctx = context_get.lock();
                        ctx.height
                    };

                    match try_read_arraybuffer_as_vec(data, key) {
                        Ok(key_vec) => {
                            match Self::db_value_at_block(context_get.clone(), &key_vec, height) {
                                Ok(lookup) => {
                                    if let Err(_) = mem.write(&mut caller, value as usize, lookup.as_slice()) {
                                        caller.data_mut().had_failure = true;
                                    }
                                }
                                Err(_) => {
                                    caller.data_mut().had_failure = true;
                                }
                            }
                        }
                        Err(_) => {
                            if let Ok(error_bits) = try_into_vec(i32::MAX.to_le_bytes()) {
                                if let Err(_) = mem.write(
                                    &mut caller,
                                    (value - 4) as usize,
                                    error_bits.as_slice(),
                                ) {
                                    caller.data_mut().had_failure = true;
                                }
                            } else {
                                caller.data_mut().had_failure = true;
                            }
                        }
                    }
                },
            )
            .context("Failed to wrap __get")?;

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
                    let height = {
                        let ctx = context_get_len.lock();
                        ctx.height
                    };

                    match try_read_arraybuffer_as_vec(data, key) {
                        Ok(key_vec) => {
                            match Self::db_value_at_block(context_get_len.clone(), &key_vec, height) {
                                Ok(value) => value.len() as i32,
                                Err(_) => i32::MAX,
                            }
                        }
                        Err(_) => i32::MAX,
                    }
                },
            )
            .context("Failed to wrap __get_len")?;

        Ok(())
    }

    /// Setup view-specific linker functions
    fn setup_linker_view(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        linker: &mut Linker<State>,
    ) -> Result<()> {
        let context_get = context.clone();
        let context_get_len = context.clone();

        linker
            .func_wrap(
                "env",
                "__flush",
                move |_caller: Caller<'_, State>, _encoded: i32| {},
            )
            .context("Failed to wrap __flush")?;

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
                    let height = {
                        let ctx = context_get.lock();
                        ctx.height
                    };

                    match try_read_arraybuffer_as_vec(data, key) {
                        Ok(key_vec) => {
                            match Self::db_value_at_block(context_get.clone(), &key_vec, height) {
                                Ok(lookup) => {
                                    if let Err(_) = mem.write(&mut caller, value as usize, lookup.as_slice()) {
                                        caller.data_mut().had_failure = true;
                                    }
                                }
                                Err(_) => {
                                    caller.data_mut().had_failure = true;
                                }
                            }
                        }
                        Err(_) => {
                            if let Ok(error_bits) = try_into_vec(i32::MAX.to_le_bytes()) {
                                if let Err(_) = mem.write(
                                    &mut caller,
                                    (value - 4) as usize,
                                    error_bits.as_slice(),
                                ) {
                                    caller.data_mut().had_failure = true;
                                }
                            } else {
                                caller.data_mut().had_failure = true;
                            }
                        }
                    }
                },
            )
            .context("Failed to wrap __get")?;

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
                    let height = {
                        let ctx = context_get_len.lock();
                        ctx.height
                    };

                    match try_read_arraybuffer_as_vec(data, key) {
                        Ok(key_vec) => {
                            match Self::db_value_at_block(context_get_len.clone(), &key_vec, height) {
                                Ok(value) => value.len() as i32,
                                Err(_) => i32::MAX,
                            }
                        }
                        Err(_) => i32::MAX,
                    }
                },
            )
            .context("Failed to wrap __get_len")?;

        Ok(())
    }

    // The rest of the implementation...
    pub fn load(indexer: PathBuf, store: T) -> Result<Self> {
        let engine = wasmtime::Engine::default();
        let module =
            wasmtime::Module::from_file(&engine, indexer).context("Failed to load WASM module")?;

        let mut linker = Linker::<State>::new(&engine);
        let mut wasmstore = Store::<State>::new(&engine, State::new());
        let context = Arc::new(Mutex::new(MetashrewRuntimeContext::new(store, 0, vec![])));

        wasmstore.limiter(|state| &mut state.limits);

        Self::setup_linker(context.clone(), &mut linker).context("Failed to setup basic linker")?;
        Self::setup_linker_indexer(context.clone(), &mut linker)
            .context("Failed to setup indexer linker")?;
        linker.define_unknown_imports_as_traps(&module)?;

        let instance = linker
            .instantiate(&mut wasmstore, &module)
            .context("Failed to instantiate WASM module")?;

        Ok(Self {
            context,
            engine,
            wasmstore,
            module,
            linker,
            instance,
        })
    }

    /// Preview changes without committing them
    pub fn preview(
        &self,
        block: &[u8],
        symbol: &str,
        input: &[u8],
        height: u32,
    ) -> Result<Vec<u8>> {
        let preview_context = {
            let guard = self.context.lock();
            MetashrewRuntimeContext::new(guard.db.clone(), height, block.to_vec())
        };

        let mut linker = Linker::<State>::new(&self.engine);
        let mut wasmstore = Store::<State>::new(&self.engine, State::new());
        let context = Arc::new(Mutex::new(preview_context));

        wasmstore.limiter(|state| &mut state.limits);

        Self::setup_linker(context.clone(), &mut linker).context("Failed to setup basic linker")?;
        Self::setup_linker_indexer(context.clone(), &mut linker)
            .context("Failed to setup indexer linker")?;
        linker.define_unknown_imports_as_traps(&self.module)?;

        let instance = linker
            .instantiate(&mut wasmstore, &self.module)
            .context("Failed to instantiate module for preview")?;

        let start = instance
            .get_typed_func::<(), ()>(&mut wasmstore, "_start")
            .context("Failed to get _start function for preview")?;

        Self::execute_start(&start, &mut wasmstore, &context)?;

        // Set up for view call
        let mut view_linker = Linker::<State>::new(&self.engine);
        let mut view_store = Store::<State>::new(&self.engine, State::new());

        view_store.limiter(|state| &mut state.limits);

        Self::setup_linker(context.clone(), &mut view_linker)
            .context("Failed to setup basic linker for view")?;
        Self::setup_linker_view(context.clone(), &mut view_linker)
            .context("Failed to setup view linker")?;
        view_linker.define_unknown_imports_as_traps(&self.module)?;

        let view_instance = view_linker
            .instantiate(&mut view_store, &self.module)
            .context("Failed to instantiate module for view")?;

        context.lock().block = input.to_vec();

        let func = view_instance
            .get_typed_func::<(), i32>(&mut view_store, symbol)
            .context("Failed to get view function")?;

        let result = func
            .call(&mut view_store, ())
            .context("Failed to execute view function")?;

        let memory = view_instance
            .get_memory(&mut view_store, "memory")
            .ok_or_else(|| anyhow!("Failed to get memory for view result"))?;

        Ok(read_arraybuffer_as_vec(
            memory.data(&mut view_store),
            result,
        ))
    }

    /// Execute the WASM module
    pub fn run(&mut self) -> Result<()> {
        self.context.lock().state = 0;

        let start = self
            .instance
            .get_typed_func::<(), ()>(&mut self.wasmstore, "_start")
            .context("Failed to get _start function")?;

        self.handle_reorg()?;
        Self::execute_start(&start, &mut self.wasmstore, &self.context)
    }

    /// Execute the _start function with error handling
    fn execute_start(
        start: &wasmtime::TypedFunc<(), ()>,
        store: &mut Store<State>,
        context: &Arc<Mutex<MetashrewRuntimeContext<T>>>,
    ) -> Result<()> {
        start
            .call(&mut *store, ())
            .context("Error executing _start function")?;

        let guard = context.lock();
        if guard.state != 1 && !store.data().had_failure {
            return Err(anyhow!("indexer exited unexpectedly"));
        }

        Ok(())
    }

    /// Handle blockchain reorganization
    fn handle_reorg(&mut self) -> Result<()> {
        let context = self.context.clone();
        let height = context.lock().height;
        let latest = Self::check_latest_block_for_reorg(context.clone(), height)?;

        if latest == height {
            return Ok(());
        }

        let set = Self::db_updated_keys_for_block_range(context.clone(), height, latest)?;
        if !set.is_empty() {
            self.refresh_memory()?;
        }

        for key in &set {
            Self::db_rollback_key(context.clone(), key, height)?;
        }

        Ok(())
    }

    /// Refresh the WASM memory
    fn refresh_memory(&mut self) -> Result<()> {
        let mut wasmstore = Store::<State>::new(&self.engine, State::new());
        wasmstore.limiter(|state| &mut state.limits);

        self.instance = self
            .linker
            .instantiate(&mut wasmstore, &self.module)
            .context("Failed to instantiate module during memory refresh")?;

        self.wasmstore = wasmstore;
        Ok(())
    }

    /// Check for blockchain reorganization at a given height
    fn check_latest_block_for_reorg(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        height: u32,
    ) -> Result<u32> {
        use db_utils::*;
        let key = try_into_vec(height.to_le_bytes())?;
        let updated_key = make_updated_key(&key);
        let length_key = make_length_key(&updated_key)?;

        let result = context.lock()
            .db
            .get(&length_key)
            .map_err(|e| anyhow!("Database error: {:?}", e))?;

        match result {
            Some(_) => Self::check_latest_block_for_reorg(context.clone(), height + 1),
            None => Ok(height),
        }
    }

    /// Get the length of a key's value list
    fn db_length_at_key(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        length_key: &[u8],
    ) -> Result<u32> {
        let value = context.lock()
            .db
            .get(length_key)
            .map_err(|e| anyhow!("Database error: {:?}", e))?;

        match value {
            Some(v) => {
                let bytes: [u8; 4] = v
                    .try_into()
                    .map_err(|e| anyhow!("Invalid length value: {:?}", e))?;
                Ok(u32::from_le_bytes(bytes))
            }
            None => Ok(0),
        }
    }

    /// Get updated keys for a range of blocks
    fn db_updated_keys_for_block_range(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        from: u32,
        to: u32,
    ) -> Result<HashSet<Vec<u8>>> {
        let mut result = HashSet::new();
        for height in from..=to {
            result.extend(Self::db_updated_keys_for_block(context.clone(), height)?);
        }
        Ok(result)
    }

    /// Get updated keys for a specific block
    fn db_updated_keys_for_block(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        height: u32,
    ) -> Result<HashSet<Vec<u8>>> {
        use db_utils::*;

        // Use a thread-local storage for temporary vectors
        thread_local! {
            static LOCAL_VECS: RefCell<Vec<Vec<u8>>> = RefCell::new(Vec::with_capacity(LOCAL_CACHE_SIZE));
        }

        let key = try_into_vec(height.to_le_bytes())?;
        let updated_key = make_updated_key(&key);
        let length =
            Self::db_length_at_key(context.clone(), &make_length_key(&updated_key)?)? as i32;

        let mut set = HashSet::with_capacity(length as usize);
        let mut guard = context.lock();

        // Pre-allocate keys for the loop
        LOCAL_VECS.with(|vecs| {
            let mut vecs = vecs.borrow_mut();
            vecs.clear();

            // Pre-generate all list keys
            for i in 0..length {
                if let Ok(list_key) = make_list_key(&updated_key, i as u32) {
                    vecs.push(list_key);
                }
            }

            // Batch get from database
            for list_key in vecs.iter() {
                if let Ok(Some(value)) = guard.db.get(list_key) {
                    set.insert(value);
                }
            }

            // Return vectors to the pool
            for vec in vecs.drain(..) {
                release_vec(vec);
            }
        });

        release_vec(key);
        release_vec(updated_key);

        Ok(set)
    }

    /// Roll back a key's value to a specific block
    fn db_rollback_key(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        key: &[u8],
        to_block: u32,
    ) -> Result<()> {
        use db_utils::*;

        let length = Self::db_length_at_key(context.clone(), key)?;
        let mut index = length as i32 - 1;
        let mut end_length = length as i32;

        while index >= 0 {
            let list_key = make_list_key(key, index as u32)?;
            let db_value = context.lock()
                .db
                .get(&list_key)
                .map_err(|e| anyhow!("Database error: {:?}", e))?;

            if let Some(value) = db_value {
                if value.len() < 4 {
                    return Err(anyhow!("Invalid value length: {}", value.len()));
                }

                let value_height = {
                    let bytes: [u8; 4] = value.as_slice()[(value.len() - 4)..]
                        .try_into()
                        .map_err(|e| anyhow!("Invalid value height bytes: {:?}", e))?;
                    u32::from_le_bytes(bytes)
                };

                if to_block <= value_height {
                    context.lock()
                        .db
                        .delete(&list_key)
                        .map_err(|e| anyhow!("Database delete error: {:?}", e))?;
                    end_length -= 1;
                } else {
                    break;
                }
            } else {
                break;
            }
            index -= 1;
        }

        if end_length != length as i32 {
            Self::db_set_length(context.clone(), key, end_length as u32)?;
        }

        Ok(())
    }

    /// Set the length of a key's value list
    fn db_set_length(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        key: &[u8],
        length: u32,
    ) -> Result<()> {
        use db_utils::*;

        let length_key = make_length_key(key)?;
        let mut guard = context.lock();

        if length == 0 {
            guard
                .db
                .delete(&length_key)
                .map_err(|e| anyhow!("Failed to delete length key: {:?}", e))?;
            return Ok(());
        }

        let new_length_bits = try_into_vec((length + 1).to_le_bytes())?;
        guard
            .db
            .put(&length_key, &new_length_bits)
            .map_err(|e| anyhow!("Failed to update length: {:?}", e))?;

        Ok(())
    }

    /// Append an annotated value to a key's list
    fn db_append_annotated(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        batch: &mut T::Batch,
        key: &[u8],
        value: &[u8],
        block_height: u32,
    ) -> Result<()> {
        use db_utils::*;

        let length_key = make_length_key(key)?;
        let length = Self::db_length_at_key(context.clone(), &length_key)?;
        let entry = annotate_value(value, block_height)?;

        let entry_key = make_list_key(key, length)?;
        batch.put(&entry_key, &entry);

        let new_length_bits = try_into_vec((length + 1).to_le_bytes())?;
        batch.put(&length_key, &new_length_bits);

        Ok(())
    }

    /// Append a value to a key's list
    fn db_append(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        batch: &mut T::Batch,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        use db_utils::*;

        let length_key = make_length_key(key)?;
        let length = Self::db_length_at_key(context.clone(), &length_key)?;
        let entry_key = make_list_key(key, length)?;

        batch.put(&entry_key, value);

        let new_length_bits = try_into_vec((length + 1).to_le_bytes())?;
        batch.put(&length_key, &new_length_bits);

        Ok(())
    }

    /// Create an empty update list for a block
    fn db_create_empty_update_list(batch: &mut T::Batch, height: u32) -> Result<()> {
        use db_utils::*;

        let height_vec = try_into_vec(height.to_le_bytes())?;
        let updated_key = make_updated_key(&height_vec);
        let key = make_length_key(&updated_key)?;
        let value_vec = try_into_vec(0u32.to_le_bytes())?;
        batch.put(&key, &value_vec);

        Ok(())
    }

    /// Get a value from the database at a specific block height
    fn db_value_at_block(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        key: &[u8],
        height: u32,
    ) -> Result<Vec<u8>> {
        use db_utils::*;

        let length_key = make_length_key(key)?;
        let length = Self::db_length_at_key(context.clone(), &length_key)?;
        let mut index = length as i32 - 1;

        while index >= 0 {
            let list_key = make_list_key(key, index as u32)?;
            let value = match context.lock()
                .db
                .get(&list_key)
                .map_err(|e| anyhow!("Database error: {:?}", e))?
            {
                Some(v) => v,
                None => make_list_key(&[], 0)?,
            };

            if value.len() < 4 {
                return Err(anyhow!("Invalid value length: {}", value.len()));
            }

            let value_height = {
                let bytes: [u8; 4] = value.as_slice()[(value.len() - 4)..]
                    .try_into()
                    .map_err(|e| anyhow!("Invalid value height bytes: {:?}", e))?;
                u32::from_le_bytes(bytes)
            };

            if height >= value_height {
                let mut result = value;
                result.truncate(result.len() - 4);
                return Ok(result);
            }
            index -= 1;
        }
        Ok(vec![])
    }
}

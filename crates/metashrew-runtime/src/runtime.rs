use anyhow::{anyhow, Context, Result};
use itertools::Itertools;
use protobuf::Message;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use wasmtime::{Caller, Linker, Store, StoreLimits, StoreLimitsBuilder};

use crate::{RocksDBRuntimeAdapter, RocksDBBatch};
use crate::smt::SMTHelper;
use crate::optimized_bst::OptimizedBST;


pub trait BatchLike {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V);
    fn default() -> Self;
}
pub trait KeyValueStoreLike {
    type Error: std::fmt::Debug;
    type Batch: BatchLike;
    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error>;
    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error>;
    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error>;
    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>;
    
    // Optional method to track key-value updates
    // Default implementation does nothing
    fn track_kv_update(&mut self, _key: Vec<u8>, _value: Vec<u8>) {
        // Default implementation does nothing
    }
    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error>;
}


pub const TIP_HEIGHT_KEY: &'static str = "/__INTERNAL/tip-height";

fn lock_err<T>(err: std::sync::PoisonError<T>) -> anyhow::Error {
    anyhow!("Mutex lock error: {}", err)
}

fn try_into_vec<const N: usize>(bytes: [u8; N]) -> Result<Vec<u8>> {
    Vec::<u8>::try_from(bytes).map_err(|e| anyhow!("Failed to convert bytes to Vec: {:?}", e))
}

use crate::proto::metashrew::KeyValueFlush;

type SerBlock = Vec<u8>;

pub struct State {
    limits: StoreLimits,
    had_failure: bool,
}

impl State {
    pub fn new() -> Self {
        State {
            limits: StoreLimitsBuilder::new()
                .memories(usize::MAX)
                .tables(usize::MAX)
                .instances(usize::MAX)
                .build(),
            had_failure: false,
        }
    }
}

pub struct MetashrewRuntimeContext {
    pub db: RocksDBRuntimeAdapter,
    pub height: u32,
    pub block: SerBlock,
    pub state: u32,
}

impl Clone for MetashrewRuntimeContext {
    fn clone(&self) -> Self {
        return Self {
            db: self.db.clone(),
            height: self.height,
            block: self.block.clone(),
            state: self.state,
        };
    }
}

impl MetashrewRuntimeContext {
    fn new(db: RocksDBRuntimeAdapter, height: u32, block: SerBlock) -> Self {
        return Self {
            db,
            height,
            block,
            state: 0,
        };
    }
}

pub struct MetashrewRuntime {
    pub context: Arc<Mutex<MetashrewRuntimeContext>>,
    pub engine: wasmtime::Engine,
    pub async_engine: wasmtime::Engine,
    pub wasmstore: wasmtime::Store<State>,
    pub async_module: wasmtime::Module,
    pub module: wasmtime::Module,
    pub linker: wasmtime::Linker<State>,
    pub instance: wasmtime::Instance,
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
    if data_start < 4 {
        return Err(anyhow!("memory error"));
    }
    let len = u32::from_le_bytes(
        (data[((data_start - 4) as usize)..(data_start as usize)])
            .try_into()
            .unwrap(),
    );
    return Ok(Vec::<u8>::from(
        &data[(data_start as usize)..(((data_start as u32) + len) as usize)],
    ));
}

pub fn read_arraybuffer_as_vec(data: &[u8], data_start: i32) -> Vec<u8> {
    match try_read_arraybuffer_as_vec(data, data_start) {
        Ok(v) => v,
        Err(_) => Vec::<u8>::new(),
    }
}

pub fn db_annotate_value(v: &Vec<u8>, block_height: u32) -> Result<Vec<u8>> {
    let mut entry: Vec<u8> = v.clone();
    let height = try_into_vec(block_height.to_le_bytes())?;
    entry.extend(height);
    Ok(entry)
}

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

impl MetashrewRuntime {
    pub fn load(indexer: PathBuf, store: RocksDBRuntimeAdapter) -> Result<Self> {
        // Configure the engine with settings for deterministic execution
        let mut config = wasmtime::Config::default();
        // Enable NaN canonicalization for deterministic floating point operations
        config.cranelift_nan_canonicalization(true);
        // Make relaxed SIMD deterministic (or disable it if not needed)
        config.relaxed_simd_deterministic(true);
        // Allocate memory at maximum size to avoid non-deterministic memory growth
        config.static_memory_maximum_size(0x100000000); // 4GB max memory
        config.static_memory_guard_size(0x10000); // 64KB guard
        // Pre-allocate memory to maximum size
        config.memory_init_cow(false); // Disable copy-on-write to ensure consistent memory behavior
        
        // Configure async engine with the same deterministic settings
        let mut async_config = config.clone();
        async_config.consume_fuel(true);
        async_config.async_support(true);
        
        let engine = wasmtime::Engine::new(&config)?;
        let async_engine = wasmtime::Engine::new(&async_config)?;
        let module = wasmtime::Module::from_file(&engine, indexer.clone().into_os_string()).context("Failed to load WASM module")?;
        let async_module = wasmtime::Module::from_file(&async_engine, indexer.into_os_string()).context("Failed to load WASM module")?;
        let mut linker = Linker::<State>::new(&engine);
        let mut wasmstore = Store::<State>::new(&engine, State::new());
        let context = Arc::<Mutex<MetashrewRuntimeContext>>::new(Mutex::<
            MetashrewRuntimeContext,
        >::new(
            MetashrewRuntimeContext::new(store, 0, vec![]),
        ));
        {
            wasmstore.limiter(|state| &mut state.limits)
        }
        {
            Self::setup_linker(context.clone(), &mut linker)
                .context("Failed to setup basic linker")?;
            Self::setup_linker_indexer(context.clone(), &mut linker)
                .context("Failed to setup indexer linker")?;
            linker.define_unknown_imports_as_traps(&module)?;
        }
        let instance = linker.instantiate(&mut wasmstore, &module)
            .context("Failed to instantiate WASM module")?;
        Ok(MetashrewRuntime {
            wasmstore,
            async_engine,
            engine,
            async_module,
            module,
            linker,
            context,
            instance,
        })
    }

    pub fn preview(
        &self,
        block: &Vec<u8>,
        symbol: String,
        input: &Vec<u8>,
        height: u32,
    ) -> Result<Vec<u8>> {
        // Create preview context with wrapped DB
        let preview_db = {
            let guard = self.context.lock().map_err(lock_err)?;
            guard.db.clone()
        };

        // Create a new runtime with preview db
        let mut runtime = Self::new_with_db(preview_db, height, self.async_engine.clone(), self.async_module.clone())?;
        runtime.context.lock().map_err(lock_err)?.block = block.clone();

        // Execute block via _start to populate preview db
        let start = runtime.instance.get_typed_func::<(), ()>(&mut runtime.wasmstore, "_start")
            .context("Failed to get _start function for preview")?;
            
        match start.call(&mut runtime.wasmstore, ()) {
            Ok(_) => {
                let context_guard = runtime.context.lock().map_err(lock_err)?;
                if context_guard.state != 1 && !runtime.wasmstore.data().had_failure {
                    return Err(anyhow!("indexer exited unexpectedly during preview"));
                }
            }
            Err(e) => return Err(e).context("Error executing _start in preview"),
        }

        // Create new runtime just for the view using the same wrapped DB
        let mut view_runtime = {
            let context = runtime.context.lock().map_err(lock_err)?;
            Self::new_with_db(context.db.clone(), height, self.async_engine.clone(), self.async_module.clone())?
        };
        
        // Set block to input for view
        view_runtime.context.lock().map_err(lock_err)?.block = input.clone();
        
        // Execute view function
        let func = view_runtime.instance
            .get_typed_func::<(), i32>(&mut view_runtime.wasmstore, symbol.as_str())
            .context("Failed to get view function")?;
        
        let result = func.call(&mut view_runtime.wasmstore, ())
            .context("Failed to execute view function")?;
        
        let memory = view_runtime.instance
            .get_memory(&mut view_runtime.wasmstore, "memory")
            .ok_or_else(|| anyhow!("Failed to get memory for view result"))?;
        
        // Get the final result
        Ok(read_arraybuffer_as_vec(
            memory.data(&mut view_runtime.wasmstore),
            result,
        ))
    }
    
    // Async version of preview for use with the view server
    pub async fn preview_async(
        &self,
        block: &Vec<u8>,
        symbol: String,
        input: &Vec<u8>,
        height: u32,
    ) -> Result<Vec<u8>> {
        // For now, just use the synchronous version
        // In the future, we can implement a fully async version if needed
        self.preview(block, symbol, input, height)
    }
    
    pub async fn view(&self, symbol: String, input: &Vec<u8>, height: u32) -> Result<Vec<u8>> {
        let mut linker = Linker::<State>::new(&self.async_engine);
        let mut wasmstore = Store::<State>::new(&self.async_engine, State::new());
        
        let context = {
            let guard = self.context.lock().map_err(lock_err)?;
            Arc::<Mutex<MetashrewRuntimeContext>>::new(Mutex::new(guard.clone()))
        };
        
        {
            let mut guard = context.lock().map_err(lock_err)?;
            guard.height = height;
            guard.block = input.clone();
        }
        
        // Set fuel for cooperative yielding
        wasmstore.set_fuel(u64::MAX)?;
        wasmstore.fuel_async_yield_interval(Some(10000))?;
        
        {
            wasmstore.limiter(|state| &mut state.limits)
        }
        
        {
            Self::setup_linker(context.clone(), &mut linker)
                .context("Failed to setup basic linker for view")?;
            Self::setup_linker_view(context.clone(), &mut linker)
                .context("Failed to setup view linker")?;
            linker.define_unknown_imports_as_traps(&self.module)?;
        }
        
        // Use async instantiation
        let instance = linker.instantiate_async(&mut wasmstore, &self.async_module)
            .await
            .context("Failed to instantiate module for view")?;
            
        let func = instance
            .get_typed_func::<(), i32>(&mut wasmstore, symbol.as_str())
            .with_context(|| format!("Failed to get view function '{}'", symbol))?;
            
        // Use async call
        let result = func.call_async(&mut wasmstore, ())
            .await
            .with_context(|| format!("Failed to execute view function '{}'", symbol))?;
            
        let memory = instance
            .get_memory(&mut wasmstore, "memory")
            .ok_or_else(|| anyhow!("Failed to get memory for view result"))?;
            
        Ok(read_arraybuffer_as_vec(
            memory.data(&mut wasmstore),
            result,
        ))
    }

    pub fn refresh_memory(&mut self) -> Result<()> {
        let mut wasmstore = Store::<State>::new(&self.engine, State::new());
        wasmstore.limiter(|state| &mut state.limits);
        self.instance = self
            .linker
            .instantiate(&mut wasmstore, &self.module)
            .context("Failed to instantiate module during memory refresh")?;
        self.wasmstore = wasmstore;
        Ok(())
    }

    pub fn run(&mut self) -> Result<(), anyhow::Error> {
        self.context.lock().map_err(lock_err)?.state = 0;
        let start = self
            .instance
            .get_typed_func::<(), ()>(&mut self.wasmstore, "_start")
            .context("Failed to get _start function")?;
        
        // Handle any chain reorganizations before processing the block
        self.handle_reorg()?;
        
        match start.call(&mut self.wasmstore, ()) {
            Ok(_) => {
                if self.context.lock().map_err(lock_err)?.state != 1 && !self.wasmstore.data().had_failure {
                    return Err(anyhow!("indexer exited unexpectedly"));
                }
                Ok(())
            }
            Err(e) => Err(e).context("Error calling _start function"),
        }
    }

    /// Handle chain reorganization by rolling back BST updates to the specified height
    pub fn handle_reorg(&mut self) -> Result<()> {
        // Get the current context height and database tip height
        let (context_height, db_tip_height) = {
            let guard = self.context.lock().map_err(lock_err)?;
            let db_tip = match guard.db.db.get(crate::to_labeled_key(&TIP_HEIGHT_KEY.as_bytes().to_vec()))? {
                Some(bytes) => {
                    if bytes.len() >= 4 {
                        u32::from_le_bytes(bytes[..4].try_into().unwrap())
                    } else {
                        0
                    }
                },
                None => 0,
            };
            (guard.height, db_tip)
        };
        
        // If context height is ahead of or equal to db tip, no reorg needed
        if context_height >= db_tip_height {
            return Ok(());
        }
        
        // We need to rollback from db_tip_height to context_height
        log::info!("Handling reorg: rolling back from height {} to {}", db_tip_height, context_height);
        
        // Get the database from the context
        let db = {
            let guard = self.context.lock().map_err(lock_err)?;
            guard.db.clone()
        };
        
        // Create an OptimizedBST to handle the rollback efficiently
        let optimized_bst = OptimizedBST::new(db.db.clone());
        
        // Rollback all BST entries to the target height using optimized BST
        optimized_bst.rollback_to_height(context_height)?;
        
        // Also rollback using the legacy SMT helper for state root calculation
        let smt_helper = SMTHelper::new(db.db.clone());
        smt_helper.calculate_and_store_state_root(context_height)?;
        
        log::info!("Reorg completed: rolled back to height {}", context_height);
        
        Ok(())
    }
    
    /// Get the value of a key at a specific block height using optimized BST
    pub fn db_value_at_block(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
        key: &Vec<u8>,
        height: u32
    ) -> Result<Vec<u8>> {
        // Get the database from the context
        let db = {
            let guard = context.lock().map_err(lock_err)?;
            guard.db.clone()
        };
        
        // Create an OptimizedBST to work with the data
        let optimized_bst = OptimizedBST::new(db.db.clone());
        
        // Get the current height to determine if this is a current state query
        let current_height = {
            let guard = context.lock().map_err(lock_err)?;
            guard.height
        };
        
        // Use optimized access pattern based on query type
        let result = if height >= current_height {
            // Current state query - use O(1) lookup
            optimized_bst.get_current(key)?
        } else {
            // Historical query - uses O(1) check first, then binary search only if key exists
            optimized_bst.get_at_height(key, height)?
        };
        
        match result {
            Some(value) => {
                // Remove height annotation if present (last 4 bytes)
                if value.len() >= 4 {
                    Ok(value[..value.len()-4].to_vec())
                } else {
                    Ok(value)
                }
            },
            None => Ok(Vec::new())
        }
    }
    
    /// Create an empty update list for a specific block height
    pub fn db_create_empty_update_list(batch: &mut RocksDBBatch, height: u32) -> Result<()> {
        // Create a key for the update list
        let update_list_key = format!("updates:{}", height).into_bytes();
        
        // Create an empty update list
        batch.put(update_list_key, Vec::new());
        
        Ok(())
    }
    
    /// Append an annotated value to the database using optimized BST
    pub fn db_append_annotated(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
        batch: &mut RocksDBBatch,
        key: &Vec<u8>,
        value: &Vec<u8>,
        height: u32
    ) -> Result<()> {
        // Get the database from the context
        let db = {
            let guard = context.lock().map_err(lock_err)?;
            guard.db.clone()
        };
        
        // Create an OptimizedBST to work with the data
        let optimized_bst = OptimizedBST::new(db.db.clone());
        
        // Use the OptimizedBST to store the value (without height annotation for current state)
        // The OptimizedBST handles both current state and historical storage internally
        optimized_bst.put(key, value, height)?;
        
        // Also add to the regular batch for backward compatibility with height annotation
        let annotated_value = db_annotate_value(value, height)?;
        batch.put(key, annotated_value);
        
        Ok(())
    }
    
    /// Append a key to an update list
    pub fn db_append(
        _context: Arc<Mutex<MetashrewRuntimeContext>>,
        batch: &mut RocksDBBatch,
        update_key: &Vec<u8>,
        key: &Vec<u8>
    ) -> Result<()> {
        // Create a key for the update list
        let update_list_key = format!("updates:{}", hex::encode(update_key)).into_bytes();
        
        // Add the key to the update list
        batch.put(update_list_key, key.clone());
        
        Ok(())
    }

    pub fn setup_linker(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
        linker: &mut Linker<State>,
    ) -> Result<()> {
        let context_ref_len = context.clone();
        let context_ref_input = context.clone();
        
        linker
            .func_wrap(
                "env",
                "__host_len",
                move |mut _caller: Caller<'_, State>| -> i32 {
                    match context_ref_len.lock() {
                        Ok(ctx) => ctx.block.len() as i32 + 4,
                        Err(_) => i32::MAX, // Signal error
                    }
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

                    let (input, height) = match context_ref_input.lock() {
                        Ok(ctx) => (ctx.block.clone(), ctx.height),
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
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
                        caller.data_mut().had_failure = true;
                        return;
                    }

                    if let Err(_) = mem.write(&mut caller, sz, input_clone.as_slice()) {
                        caller.data_mut().had_failure = true;
                    }
                },
            )
            .map_err(|e| anyhow!("Failed to wrap __load_input: {:?}", e))?;

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
            .map_err(|e| anyhow!("Failed to wrap __log: {:?}", e))?;

        linker
            .func_wrap(
                "env",
                "abort",
                |mut caller: Caller<'_, State>, _: i32, _: i32, _: i32, _: i32| {
                    caller.data_mut().had_failure = true;
                },
            )
            .map_err(|e| anyhow!("Failed to wrap abort: {:?}", e))?;

        Ok(())
    }

    pub fn setup_linker_view(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
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
                    let height = match context_get.clone().lock() {
                        Ok(ctx) => ctx.height,
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
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
                            if let Ok(error_bits) = u32_to_vec(i32::MAX.try_into().unwrap()) {
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
                    let height = match context_get_len.clone().lock() {
                        Ok(ctx) => ctx.height,
                        Err(_) => return i32::MAX,
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
            .map_err(|e| anyhow!("Failed to wrap __get_len: {:?}", e))?;

        Ok(())
    }

    fn new_with_db(
        db: RocksDBRuntimeAdapter,
        height: u32,
        engine: wasmtime::Engine,
        module: wasmtime::Module,
    ) -> Result<MetashrewRuntime> {
        let mut linker = Linker::<State>::new(&engine);
        let mut wasmstore = Store::<State>::new(&engine, State::new());
        let context = Arc::<Mutex<MetashrewRuntimeContext>>::new(Mutex::<
            MetashrewRuntimeContext,
        >::new(
            MetashrewRuntimeContext::new(db, height, vec![]),
        ));
        {
            wasmstore.limiter(|state| &mut state.limits)
        }
        {
            MetashrewRuntime::setup_linker(context.clone(), &mut linker)
                .context("Failed to setup basic linker")?;
            MetashrewRuntime::setup_linker_preview(context.clone(), &mut linker)
                .context("Failed to setup preview linker")?;
            linker.define_unknown_imports_as_traps(&module)?;
        }
        let instance = linker.instantiate(&mut wasmstore, &module)
            .context("Failed to instantiate WASM module")?;
        Ok(MetashrewRuntime {
            wasmstore,
            engine: engine.clone(),
            async_engine: engine,
            module: module.clone(),
            async_module: module.clone(),
            linker,
            context,
            instance,
        })
    }

    fn setup_linker_preview(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
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
                    let height = match context_ref.clone().lock() {
                        Ok(ctx) => ctx.height,
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
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

                    // For preview, we'll store directly in the database
                    let decoded = match KeyValueFlush::parse_from_bytes(&encoded_vec) {
                        Ok(d) => d,
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    };

                    match context_ref.clone().lock() {
                        Ok(mut ctx) => {
                            ctx.state = 1;
                            // Write directly to the database
                            for (k, v) in decoded.list.iter().tuples() {
                                let k_owned = <Vec<u8> as Clone>::clone(k);
                                let v_owned = <Vec<u8> as Clone>::clone(v);
                                
                                let annotated = match db_annotate_value(&v_owned, height as u32) {
                                    Ok(v) => v,
                                    Err(_) => {
                                        caller.data_mut().had_failure = true;
                                        return;
                                    }
                                };

                                // Create an SMTHelper to work with the BST
                                let smt_helper = SMTHelper::new(ctx.db.db.clone());
                                
                                // Store in BST
                                if let Err(_) = smt_helper.bst_put(&k_owned, &annotated, height) {
                                    caller.data_mut().had_failure = true;
                                    return;
                                }
                            }
                        }
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    }
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
                    let height = match context_get.clone().lock() {
                        Ok(ctx) => ctx.height,
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
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
                            if let Ok(error_bits) = u32_to_vec(i32::MAX.try_into().unwrap()) {
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
                    let height = match context_get_len.clone().lock() {
                        Ok(ctx) => ctx.height,
                        Err(_) => return i32::MAX,
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
            .map_err(|e| anyhow!("Failed to wrap __get_len: {:?}", e))?;

        Ok(())
    }

    pub fn setup_linker_indexer(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
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
                    let height = match context_ref.clone().lock() {
                        Ok(ctx) => ctx.height,
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
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

                    let mut batch = RocksDBBatch::default();
                    if let Err(_) = Self::db_create_empty_update_list(&mut batch, height as u32) {
                        caller.data_mut().had_failure = true;
                        return;
                    }

                    let decoded = match KeyValueFlush::parse_from_bytes(&encoded_vec) {
                        Ok(d) => d,
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    };

                    for (k, v) in decoded.list.iter().tuples() {
                        let k_owned = <Vec<u8> as Clone>::clone(k);
                        let v_owned = <Vec<u8> as Clone>::clone(v);

                        // Track key-value updates using the trait method
                        {
                            // Create a longer-lived value for the context reference
                            let context_ref_clone = context_ref.clone();
                            let mut ctx_guard = match context_ref_clone.lock() {
                                Ok(guard) => guard,
                                Err(_) => {
                                    caller.data_mut().had_failure = true;
                                    return;
                                }
                            };
                            
                            // Track the original key-value pair before annotation
                            ctx_guard.db.track_kv_update(k_owned.clone(), v_owned.clone());
                        }

                        if let Err(_) = Self::db_append_annotated(
                            context_ref.clone(),
                            &mut batch,
                            &k_owned,
                            &v_owned,
                            height as u32,
                        ) {
                            caller.data_mut().had_failure = true;
                            return;
                        }

                        match u32_to_vec(height) {
                            Ok(update_key) => {
                                if let Err(_) = Self::db_append(context_ref.clone(), &mut batch, &update_key, &k_owned) {
                                    caller.data_mut().had_failure = true;
                                    return;
                                }
                            }
                            Err(_) => {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        }
                    }

                    log::debug!(
                        "saving {:?} k/v pairs for block {:?}",
                        decoded.list.len() / 2,
                        height
                    );

                    match context_ref.clone().lock() {
                        Ok(mut ctx) => {
                            ctx.state = 1;
                            if let Err(_) = ctx.db.write(batch) {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                            
                            // Create an SMTHelper to work with the BST
                            let smt_helper = SMTHelper::new(ctx.db.db.clone());
                            
                            // Calculate and store the state root for this height
                            if let Err(_) = smt_helper.calculate_and_store_state_root(height) {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        }
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    }
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
                    let height = match context_get.clone().lock() {
                        Ok(ctx) => ctx.height,
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    };

                    match key_vec_result {
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
                            if let Ok(error_bits) = u32_to_vec(i32::MAX.try_into().unwrap()) {
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
                    };
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
                    let height = match context_get_len.clone().lock() {
                        Ok(ctx) => ctx.height,
                        Err(_) => return i32::MAX,
                    };

                    match key_vec_result {
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
            .map_err(|e| anyhow!("Failed to wrap __get_len: {:?}", e))?;

        Ok(())
    }
    
    /// Get all keys that were touched at a specific block height
    pub fn get_keys_touched_at_height(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
        height: u32
    ) -> Result<Vec<Vec<u8>>> {
        let db = {
            let guard = context.lock().map_err(lock_err)?;
            guard.db.clone()
        };
        
        // Use optimized BST for efficient key lookup at height
        let optimized_bst = OptimizedBST::new(db.db.clone());
        optimized_bst.get_keys_at_height(height)
    }
    
    /// Iterate backwards through all values of a key from most recent update
    pub fn iterate_key_backwards(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
        key: &Vec<u8>,
        from_height: u32
    ) -> Result<Vec<(u32, Vec<u8>)>> {
        let db = {
            let guard = context.lock().map_err(lock_err)?;
            guard.db.clone()
        };
        
        // Use optimized BST for efficient backwards iteration
        let optimized_bst = OptimizedBST::new(db.db.clone());
        let results = optimized_bst.iterate_backwards(key, from_height)?;
        
        // Remove height annotations from values if present
        let mut clean_results = Vec::new();
        for (height, value) in results {
            let clean_value = if value.len() >= 4 {
                value[..value.len()-4].to_vec()
            } else {
                value
            };
            clean_results.push((height, clean_value));
        }
        
        Ok(clean_results)
    }
    
    /// Get the current state root (merkle root of entire state)
    pub fn get_current_state_root(
        context: Arc<Mutex<MetashrewRuntimeContext>>
    ) -> Result<[u8; 32]> {
        let db = {
            let guard = context.lock().map_err(lock_err)?;
            guard.db.clone()
        };
        
        let smt_helper = SMTHelper::new(db.db.clone());
        smt_helper.get_current_state_root()
    }
    
    /// Get the state root at a specific height
    pub fn get_state_root_at_height(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
        height: u32
    ) -> Result<[u8; 32]> {
        let db = {
            let guard = context.lock().map_err(lock_err)?;
            guard.db.clone()
        };
        
        let smt_helper = SMTHelper::new(db.db.clone());
        smt_helper.get_smt_root_at_height(height)
    }
    
    /// Perform a complete rollback to a specific height
    pub fn rollback_to_height(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
        target_height: u32
    ) -> Result<()> {
        let db = {
            let guard = context.lock().map_err(lock_err)?;
            guard.db.clone()
        };
        
        // Use optimized BST for efficient rollback
        let optimized_bst = OptimizedBST::new(db.db.clone());
        optimized_bst.rollback_to_height(target_height)?;
        
        // Also update legacy SMT for state root calculation
        let smt_helper = SMTHelper::new(db.db.clone());
        smt_helper.calculate_and_store_state_root(target_height)?;
        
        Ok(())
    }
    
    /// Get all heights at which a key was updated
    pub fn get_key_update_heights(
        context: Arc<Mutex<MetashrewRuntimeContext>>,
        key: &Vec<u8>
    ) -> Result<Vec<u32>> {
        let db = {
            let guard = context.lock().map_err(lock_err)?;
            guard.db.clone()
        };
        
        let smt_helper = SMTHelper::new(db.db.clone());
        smt_helper.bst_get_heights_for_key(key)
    }
}

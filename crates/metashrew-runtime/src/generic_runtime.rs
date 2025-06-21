//! Generic MetashrewRuntime implementation that works with any storage backend

use anyhow::{anyhow, Context, Result};
use itertools::Itertools;
use protobuf::Message;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use wasmtime::{Caller, Linker, Store, StoreLimits, StoreLimitsBuilder};

use crate::traits::{BatchLike, KeyValueStoreLike};
use crate::context::MetashrewRuntimeContext;
use crate::proto::metashrew::KeyValueFlush;

pub const TIP_HEIGHT_KEY: &'static str = "/__INTERNAL/tip-height";

fn lock_err<T>(err: std::sync::PoisonError<T>) -> anyhow::Error {
    anyhow!("Mutex lock error: {}", err)
}

fn try_into_vec<const N: usize>(bytes: [u8; N]) -> Result<Vec<u8>> {
    Vec::<u8>::try_from(bytes).map_err(|e| anyhow!("Failed to convert bytes to Vec: {:?}", e))
}

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

/// Generic MetashrewRuntime that works with any storage backend
pub struct MetashrewRuntime<T: KeyValueStoreLike> {
    pub context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
    pub engine: wasmtime::Engine,
    pub async_engine: wasmtime::Engine,
    pub wasmstore: wasmtime::Store<State>,
    pub async_module: wasmtime::Module,
    pub module: wasmtime::Module,
    pub linker: wasmtime::Linker<State>,
    pub instance: wasmtime::Instance,
}

impl<T: KeyValueStoreLike + Clone + Send + Sync + 'static> MetashrewRuntime<T> {
    pub fn load(indexer: PathBuf, store: T) -> Result<Self> {
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
        let context = Arc::<Mutex<MetashrewRuntimeContext<T>>>::new(Mutex::<
            MetashrewRuntimeContext<T>,
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

    pub async fn view(&self, symbol: String, input: &Vec<u8>, height: u32) -> Result<Vec<u8>> {
        let mut linker = Linker::<State>::new(&self.async_engine);
        let mut wasmstore = Store::<State>::new(&self.async_engine, State::new());
        
        let context = {
            let guard = self.context.lock().map_err(lock_err)?;
            Arc::<Mutex<MetashrewRuntimeContext<T>>>::new(Mutex::new(guard.clone()))
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

    pub async fn preview_async(&self, block_data: &Vec<u8>, symbol: String, _input: &Vec<u8>, height: u32) -> Result<Vec<u8>> {
        let mut linker = Linker::<State>::new(&self.async_engine);
        let mut wasmstore = Store::<State>::new(&self.async_engine, State::new());
        
        let context = {
            let guard = self.context.lock().map_err(lock_err)?;
            Arc::<Mutex<MetashrewRuntimeContext<T>>>::new(Mutex::new(guard.clone()))
        };
        
        {
            let mut guard = context.lock().map_err(lock_err)?;
            guard.height = height;
            guard.block = block_data.clone();
        }
        
        // Set fuel for cooperative yielding
        wasmstore.set_fuel(u64::MAX)?;
        wasmstore.fuel_async_yield_interval(Some(10000))?;
        
        {
            wasmstore.limiter(|state| &mut state.limits)
        }
        
        {
            Self::setup_linker(context.clone(), &mut linker)
                .context("Failed to setup basic linker for preview")?;
            Self::setup_linker_view(context.clone(), &mut linker)
                .context("Failed to setup view linker for preview")?;
            linker.define_unknown_imports_as_traps(&self.module)?;
        }
        
        // Use async instantiation
        let instance = linker.instantiate_async(&mut wasmstore, &self.async_module)
            .await
            .context("Failed to instantiate module for preview")?;
            
        let func = instance
            .get_typed_func::<(), i32>(&mut wasmstore, symbol.as_str())
            .with_context(|| format!("Failed to get preview function '{}'", symbol))?;
            
        // Use async call
        let result = func.call_async(&mut wasmstore, ())
            .await
            .with_context(|| format!("Failed to execute preview function '{}'", symbol))?;
            
        let memory = instance
            .get_memory(&mut wasmstore, "memory")
            .ok_or_else(|| anyhow!("Failed to get memory for preview result"))?;
            
        Ok(read_arraybuffer_as_vec(
            memory.data(&mut wasmstore),
            result,
        ))
    }

    pub fn setup_linker(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
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
                            match get_value_at_height(context_get.clone(), &key_vec, height) {
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
                            match get_value_at_height(context_get_len.clone(), &key_vec, height) {
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

                    let mut batch = T::Batch::default();

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

                        // Track key-value updates
                        {
                            let context_ref_clone = context_ref.clone();
                            let mut ctx_guard = match context_ref_clone.lock() {
                                Ok(guard) => guard,
                                Err(_) => {
                                    caller.data_mut().had_failure = true;
                                    return;
                                }
                            };
                            
                            ctx_guard.db.track_kv_update(k_owned.clone(), v_owned.clone());
                        }

                        // Add annotated value to batch
                        let annotated_value = match db_annotate_value(&v_owned, height as u32) {
                            Ok(v) => v,
                            Err(_) => {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        };
                        batch.put(k_owned, annotated_value);
                    }

                    // Add height update to batch
                    let key_bytes: Vec<u8> = TIP_HEIGHT_KEY.as_bytes().to_vec();
                    let height_bytes: Vec<u8> = (height + 1).to_le_bytes().to_vec();
                    batch.put(key_bytes, height_bytes);

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
                            match get_value_at_height(context_get.clone(), &key_vec, height) {
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
                            match get_value_at_height(context_get_len.clone(), &key_vec, height) {
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
}

// Helper functions
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
        Ok(v) => {
            // Add sanity check for reasonable data sizes
            if v.len() > 100000 { // 100KB limit for view function results
                log::warn!("View function returned suspiciously large data ({} bytes), truncating to empty", v.len());
                Vec::<u8>::new()
            } else {
                v
            }
        },
        Err(_) => {
            // If the standard format fails, try to read directly from the pointer
            // This handles cases where the WASM function returns a raw pointer
            if data_start >= 0 && (data_start as usize) < data.len() {
                // Try to read a length prefix at the pointer location
                if (data_start as usize + 4) <= data.len() {
                    let len_bytes = &data[data_start as usize..(data_start as usize + 4)];
                    if let Ok(len_array) = len_bytes.try_into() {
                        let len = u32::from_le_bytes(len_array) as usize;
                        let start = data_start as usize + 4;
                        // More restrictive sanity check - blocktracker should be small
                        if start + len <= data.len() && len > 0 && len < 10000 { // 10KB limit
                            return data[start..start + len].to_vec();
                        } else if len >= 10000 {
                            log::warn!("View function attempted to return {} bytes, which exceeds reasonable limit", len);
                        }
                    }
                }
            }
            Vec::<u8>::new()
        }
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

pub fn u32_to_vec(v: u32) -> Result<Vec<u8>> {
    try_into_vec(v.to_le_bytes())
}

/// Generic function to get value at a specific height
/// This is a simplified version that works with any storage backend
pub fn get_value_at_height<T: KeyValueStoreLike>(
    context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
    key: &Vec<u8>,
    _height: u32
) -> Result<Vec<u8>> {
    // For now, just get the current value
    // In a full implementation, this would use BST/SMT structures
    let mut guard = context.lock().map_err(lock_err)?;
    match guard.db.get(key) {
        Ok(Some(value)) => {
            // Remove height annotation if present (last 4 bytes)
            if value.len() >= 4 {
                Ok(value[..value.len()-4].to_vec())
            } else {
                Ok(value)
            }
        },
        Ok(None) => Ok(Vec::new()),
        Err(e) => Err(anyhow!("Database error: {:?}", e)),
    }
}
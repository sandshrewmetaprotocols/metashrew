use anyhow::{anyhow, Context, Result};
use itertools::Itertools;
//use rlp;
use protobuf::Message;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use wasmtime::{Caller, Linker, Store, StoreLimits, StoreLimitsBuilder};

fn lock_err<T>(err: std::sync::PoisonError<T>) -> anyhow::Error {
    anyhow!("Mutex lock error: {}", err)
}


fn try_into_vec<const N: usize>(bytes: [u8; N]) -> Result<Vec<u8>> {
    Vec::<u8>::try_from(bytes).map_err(|e| anyhow!("Failed to convert bytes to Vec: {:?}", e))
}

use crate::proto::metashrew::KeyValueFlush;

type SerBlock = Vec<u8>;
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
}

//const TIP_KEY: &[u8] = b"T";
//const HEADERS_CF: &str = "headers";

pub struct State {
    limits: StoreLimits,
    had_failure: bool,
}

#[derive(Debug)]
pub struct PreviewDBWrapper<T: KeyValueStoreLike + Clone> {
    underlying_db: T,
    overlay: std::collections::HashMap<Vec<u8>, Vec<u8>>,
}

impl<T: KeyValueStoreLike + Clone> Clone for PreviewDBWrapper<T> {
    fn clone(&self) -> Self {
        Self {
            underlying_db: self.underlying_db.clone(),
            overlay: self.overlay.clone(),
        }
    }
}

impl<T: KeyValueStoreLike + Clone> KeyValueStoreLike for PreviewDBWrapper<T> {
    type Error = T::Error;
    type Batch = T::Batch;

    fn write(&mut self, _batch: Self::Batch) -> Result<(), Self::Error> {
        // Write operations are captured in the overlay HashMap instead
        // We'll need to extract k/v pairs from the batch and store them
        Ok(())
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        // Check overlay first
        if let Some(value) = self.overlay.get(key.as_ref()) {
            return Ok(Some(value.clone()));
        }
        // Fall back to underlying db
        self.underlying_db.get(key)
    }

    fn delete<K: AsRef<[u8]>>(&mut self, _key: K) -> Result<(), Self::Error> {
        // For preview we don't need to implement delete
        Ok(())
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.overlay.insert(
            key.as_ref().to_vec(),
            value.as_ref().to_vec(),
        );
        Ok(())
    }
}

pub struct MetashrewRuntimeContext<T: KeyValueStoreLike + Clone> {
    pub db: T,
    pub height: u32,
    pub block: SerBlock,
    pub state: u32,
}

impl<T: KeyValueStoreLike + Clone> Clone for MetashrewRuntimeContext<T> {
    fn clone(&self) -> Self {
        return Self {
            db: self.db.clone(),
            height: self.height,
            block: self.block.clone(),
            state: self.state,
        };
    }
}

impl<T: KeyValueStoreLike + Clone> MetashrewRuntimeContext<T> {
    fn new(db: T, height: u32, block: SerBlock) -> Self {
        return Self {
            db: db,
            height: height,
            block: block,
            state: 0,
        };
    }
}

pub struct MetashrewRuntime<T: KeyValueStoreLike + Clone + 'static> {
    pub context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
    pub engine: wasmtime::Engine,
    pub async_engine: wasmtime::Engine,
    pub wasmstore: wasmtime::Store<State>,
    pub async_module: wasmtime::Module,
    pub module: wasmtime::Module,
    pub linker: wasmtime::Linker<State>,
    pub instance: wasmtime::Instance,
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

impl<T: KeyValueStoreLike> MetashrewRuntime<T>
where
    T: Sync + Send,
    T: Clone + 'static,
{
    pub fn load(indexer: PathBuf, store: T) -> Result<Self> {
        // Configure the engine with default settings
        let mut async_config = wasmtime::Config::default();
        async_config.consume_fuel(true);
        async_config.async_support(true);
        let config = wasmtime::Config::default();
        let engine = wasmtime::Engine::new(&config)?;
        let async_engine = wasmtime::Engine::new(&async_config)?;
        let module = wasmtime::Module::from_file(&engine, indexer.clone().into_os_string()).context("Failed to load WASM module")?;
        let async_module = wasmtime::Module::from_file(&async_engine, indexer.into_os_string()).context("Failed to load WASM module")?;
        let mut linker = Linker::<State>::new(&engine);
        let mut wasmstore = Store::<State>::new(&engine, State::new());
        let context = Arc::<Mutex<MetashrewRuntimeContext<T>>>::new(Mutex::<
            MetashrewRuntimeContext<T>,
        >::new(
            MetashrewRuntimeContext::<T>::new(store, 0, vec![]),
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
            PreviewDBWrapper {
                underlying_db: guard.db.clone(),
                overlay: std::collections::HashMap::new(),
            }
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

    pub fn db_create_empty_update_list(batch: &mut T::Batch, height: u32) -> Result<()> {
        let height_vec = u32_to_vec(height)?;
        let updated_key = db_make_updated_key(&height_vec);
        let key = db_make_length_key(&updated_key)?;
        let value_vec = u32_to_vec(0)?;
        batch.put(&key, &value_vec);
        Ok(())
    }
    pub fn run(&mut self) -> Result<(), anyhow::Error> {
        self.context.lock().map_err(lock_err)?.state = 0;
        let start = self
            .instance
            .get_typed_func::<(), ()>(&mut self.wasmstore, "_start")
            .context("Failed to get _start function")?;
        
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

    pub fn check_latest_block_for_reorg(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        height: u32,
    ) -> Result<u32> {
        let key = u32_to_vec(height as u32)?;
        let updated_key = db_make_updated_key(&key);
        let length_key = db_make_length_key(&updated_key)?;
        
        let result = context
            .lock()
            .map_err(lock_err)?
            .db
            .get(&length_key)
            .map_err(|e| anyhow!("Database error: {:?}", e))?;
            
        match result {
            Some(_v) => Self::check_latest_block_for_reorg(context.clone(), height + 1),
            None => Ok(height),
        }
    }

    pub fn db_length_at_key(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        length_key: &Vec<u8>,
    ) -> Result<u32> {
        let value = context
            .lock()
            .map_err(lock_err)?
            .db
            .get(length_key)
            .map_err(|e| anyhow!("Database error: {:?}", e))?;
            
        match value {
            Some(v) => {
                let bytes: [u8; 4] = v.try_into()
                    .map_err(|e| anyhow!("Invalid length value: {:?}", e))?;
                Ok(u32::from_le_bytes(bytes))
            }
            None => Ok(0),
        }
    }

    pub fn db_updated_keys_for_block(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        height: u32,
    ) -> Result<HashSet<Vec<u8>>> {
        let key = u32_to_vec(height)?;
        let updated_key = db_make_updated_key(&key);
        let length = Self::db_length_at_key(context.clone(), &updated_key)? as i32;
        let mut i: i32 = 0;
        let mut set: HashSet<Vec<u8>> = HashSet::<Vec<u8>>::new();
        
        while i < length {
            let list_key = db_make_list_key(&updated_key, i as u32)?;
            let value = context
                .lock()
                .map_err(lock_err)?
                .db
                .get(&list_key)
                .map_err(|e| anyhow!("Database error: {:?}", e))?
                .ok_or_else(|| anyhow!("Missing value for key at index {}", i))?;
                
            set.insert(value);
            i += 1;
        }
        Ok(set)
    }
    pub fn db_value_at_block(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        key: &Vec<u8>,
        height: u32,
    ) -> Result<Vec<u8>> {
        let length_key = db_make_length_key(key)?;
        let length = Self::db_length_at_key(context.clone(), &length_key)?;
        let mut index = length as i32 - 1;

        while index >= 0 {
            let list_key = db_make_list_key(key, index as u32)?;
            let value = match context
                .lock()
                .map_err(lock_err)?
                .db
                .get(&list_key)
                .map_err(|e| anyhow!("Database error: {:?}", e))? {
                    Some(v) => v,
                    None => db_make_list_key(&Vec::<u8>::new(), 0)?,
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
                let mut result = value.clone();
                result.truncate(value.len().saturating_sub(4));
                return Ok(result);
            }
            index -= 1;
        }
        Ok(vec![])
    }

    pub fn db_updated_keys_for_block_range(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        from: u32,
        to: u32,
    ) -> Result<HashSet<Vec<u8>>> {
        let mut i = from;
        let mut result: HashSet<Vec<u8>> = HashSet::<Vec<u8>>::new();
        while to >= i {
            result.extend(Self::db_updated_keys_for_block(context.clone(), i)?);
            i = i + 1;
        }
        Ok(result)
    }

    pub fn db_rollback_key(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        key: &Vec<u8>,
        to_block: u32,
    ) -> Result<()> {
        let length = Self::db_length_at_key(context.clone(), key)?;
        let mut index = length as i32 - 1;
        let mut end_length = length as i32;
        
        while index >= 0 {
            let list_key = db_make_list_key(key, index as u32)?;
            let db_value = context
                .lock()
                .map_err(lock_err)?
                .db
                .get(&list_key)
                .map_err(|e| anyhow!("Database error: {:?}", e))?;

            match db_value {
                Some(value) => {
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
                        context
                            .lock()
                            .map_err(lock_err)?
                            .db
                            .delete(&list_key)
                            .map_err(|e| anyhow!("Database delete error: {:?}", e))?;
                        end_length -= 1;
                    } else {
                        break;
                    }
                }
                None => break,
            }
            index -= 1;
        }
        
        if end_length != length as i32 {
            Self::db_set_length(context.clone(), key, end_length as u32)?;
        }
        
        Ok(())
    }

    pub fn db_set_length(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        key: &Vec<u8>,
        length: u32,
    ) -> Result<()> {
        let length_key = db_make_length_key(key)?;
        let mut guard = context.lock().map_err(lock_err)?;
        
        if length == 0 {
            guard.db.delete(&length_key)
                .map_err(|e| anyhow!("Failed to delete length key: {:?}", e))?;
            return Ok(());
        }
        
        let new_length_bits = u32_to_vec(length + 1)?;
        guard.db
            .put(&length_key, &new_length_bits)
            .map_err(|e| anyhow!("Failed to update length: {:?}", e))?;
            
        Ok(())
    }

    pub fn handle_reorg(&mut self) -> Result<()> {
        let context = self.context.clone();
        let height = { context.lock().map_err(lock_err)?.height };
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
    pub fn db_append_annotated(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        batch: &mut T::Batch,
        key: &Vec<u8>,
        value: &Vec<u8>,
        block_height: u32,
    ) -> Result<()> {
        let length_key = db_make_length_key(key)?;
        let length = Self::db_length_at_key(context.clone(), &length_key)?;
        let entry = db_annotate_value(value, block_height)?;

        let entry_key = db_make_list_key(key, length)?;
        batch.put(&entry_key, &entry);
        
        let new_length_bits = u32_to_vec(length + 1)?;
        batch.put(&length_key, &new_length_bits);
        
        Ok(())
    }
    pub fn db_append(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        batch: &mut T::Batch,
        key: &Vec<u8>,
        value: &Vec<u8>,
    ) -> Result<()> {
        let length_key = db_make_length_key(key)?;
        let length = Self::db_length_at_key(context.clone(), &length_key)?;
        let entry_key = db_make_list_key(key, length)?;
        
        batch.put(&entry_key, &value);
        
        let new_length_bits = u32_to_vec(length + 1)?;
        batch.put(&length_key, &new_length_bits);
        
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
    fn new_with_db<U: KeyValueStoreLike + Clone + Sync + Send + 'static>(
        db: U,
        height: u32,
        engine: wasmtime::Engine,
        module: wasmtime::Module,
    ) -> Result<MetashrewRuntime<U>> {
        let mut linker = Linker::<State>::new(&engine);
        let mut wasmstore = Store::<State>::new(&engine, State::new());
        let context = Arc::<Mutex<MetashrewRuntimeContext<U>>>::new(Mutex::<
            MetashrewRuntimeContext<U>,
        >::new(
            MetashrewRuntimeContext::<U>::new(db, height, vec![]),
        ));
        {
            wasmstore.limiter(|state| &mut state.limits)
        }
        {
            MetashrewRuntime::<U>::setup_linker(context.clone(), &mut linker)
                .context("Failed to setup basic linker")?;
            MetashrewRuntime::<U>::setup_linker_preview(context.clone(), &mut linker)
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

                    // For preview, we'll store directly in the HashMap overlay
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
                            // Write directly to the overlay HashMap
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

                                // Store in overlay
                                if let Err(_) = ctx.db.put(k_owned, annotated) {
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

                    debug!(
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
}

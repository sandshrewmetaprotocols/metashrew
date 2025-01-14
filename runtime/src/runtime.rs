use anyhow::{anyhow, Result};
use itertools::Itertools;
//use rlp;
use hex;
use protobuf::Message;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use wasmtime::{Caller, Linker, Store, StoreLimits, StoreLimitsBuilder};

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
    pub wasmstore: wasmtime::Store<State>,
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

pub fn db_make_list_key(v: &Vec<u8>, index: u32) -> Vec<u8> {
    let mut entry = v.clone();
    let index_bits: Vec<u8> = index.to_le_bytes().try_into().unwrap();
    entry.extend(index_bits);
    return entry;
}

pub fn db_make_length_key(key: &Vec<u8>) -> Vec<u8> {
    return db_make_list_key(key, u32::MAX);
}

pub fn db_make_updated_key(key: &Vec<u8>) -> Vec<u8> {
    return key.clone();
}

pub fn u32_to_vec(v: u32) -> Vec<u8> {
    return v.to_le_bytes().try_into().unwrap();
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

pub fn db_annotate_value(v: &Vec<u8>, block_height: u32) -> Vec<u8> {
    let mut entry: Vec<u8> = v.clone();
    let height: Vec<u8> = block_height.to_le_bytes().try_into().unwrap();
    entry.extend(height);
    return entry;
}

pub fn to_signed_or_trap<'a, T: TryInto<i32>>(caller: &mut Caller<'_, State>, v: T) -> i32 {
    return match <T as TryInto<i32>>::try_into(v) {
        Ok(v) => v,
        Err(_) => {
            return i32::MAX;
        }
    };
}

pub fn to_usize_or_trap<'a, T: TryInto<usize>>(caller: &mut Caller<'_, State>, v: T) -> usize {
    return match <T as TryInto<usize>>::try_into(v) {
        Ok(v) => v,
        Err(_) => {
            return usize::MAX;
        }
    };
}

impl<T: KeyValueStoreLike> MetashrewRuntime<T>
where
    T: KeyValueStoreLike,
    T: Sync + Send,
    T: Clone,
{
    pub fn load(indexer: PathBuf, store: T) -> Result<Self> {
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::from_file(&engine, indexer.into_os_string()).unwrap();
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
            Self::setup_linker(context.clone(), &mut linker);
            Self::setup_linker_indexer(context.clone(), &mut linker);
            linker.define_unknown_imports_as_traps(&module)?;
        }
        let instance = linker.instantiate(&mut wasmstore, &module).unwrap();
        return Ok(MetashrewRuntime {
            wasmstore: wasmstore,
            engine: engine,
            module: module,
            linker: linker,
            context: context,
            instance: instance,
        });
    }

    pub fn preview(
        &self,
        block: &Vec<u8>,
        symbol: String,
        input: &Vec<u8>,
        height: u32,
    ) -> Result<Vec<u8>> {
        // Create new isolated runtime with cloned context
        let mut preview_context = MetashrewRuntimeContext::new(
            self.context.lock().unwrap().db.clone(),
            height,
            block.clone(),
        );

        let mut linker = Linker::<State>::new(&self.engine);
        let mut wasmstore = Store::<State>::new(&self.engine, State::new());
        let context = Arc::<Mutex<MetashrewRuntimeContext<T>>>::new(Mutex::new(preview_context));

        {
            wasmstore.limiter(|state| &mut state.limits);
        }

        {
            Self::setup_linker(context.clone(), &mut linker);
            Self::setup_linker_indexer(context.clone(), &mut linker);
            linker.define_unknown_imports_as_traps(&self.module)?;
        }

        let instance = linker.instantiate(&mut wasmstore, &self.module)?;

        // Execute block via _start
        let start = instance.get_typed_func::<(), ()>(&mut wasmstore, "_start")?;
        match start.call(&mut wasmstore, ()) {
            Ok(_) => {
                if context.lock().unwrap().state != 1 && !wasmstore.data().had_failure {
                    return Err(anyhow!("indexer exited unexpectedly during preview"));
                }
            }
            Err(e) => return Err(e),
        }

        // Now set up for view call
        let mut view_linker = Linker::<State>::new(&self.engine);
        let mut view_store = Store::<State>::new(&self.engine, State::new());

        {
            view_store.limiter(|state| &mut state.limits);
        }

        {
            Self::setup_linker(context.clone(), &mut view_linker);
            Self::setup_linker_view(context.clone(), &mut view_linker);
            view_linker.define_unknown_imports_as_traps(&self.module)?;
        }

        let view_instance = view_linker.instantiate(&mut view_store, &self.module)?;

        // Execute view in same isolated context
        context.lock().unwrap().block = input.clone();

        let func = view_instance.get_typed_func::<(), i32>(&mut view_store, symbol.as_str())?;
        let result = func.call(&mut view_store, ())?;

        Ok(read_arraybuffer_as_vec(
            view_instance
                .get_memory(&mut view_store, "memory")
                .unwrap()
                .data(&mut view_store),
            result,
        ))
    }
    pub fn view(&self, symbol: String, input: &Vec<u8>, height: u32) -> Result<Vec<u8>> {
        let mut linker = Linker::<State>::new(&self.engine);
        let mut wasmstore = Store::<State>::new(&self.engine, State::new());
        let context = Arc::<Mutex<MetashrewRuntimeContext<T>>>::new(Mutex::new(
            self.context.clone().lock().unwrap().clone(),
        ));
        {
            (
                context.lock().unwrap().height,
                context.lock().unwrap().block,
            ) = (height, input.clone());
        }
        {
            wasmstore.limiter(|state| &mut state.limits)
        }
        {
            Self::setup_linker(context.clone(), &mut linker);
            Self::setup_linker_view(context.clone(), &mut linker);
            linker.define_unknown_imports_as_traps(&self.module)?;
        }
        let instance = linker.instantiate(&mut wasmstore, &self.module).unwrap();
        let func = instance
            .get_typed_func::<(), i32>(&mut wasmstore, symbol.as_str())
            .unwrap();
        let result = func.call(&mut wasmstore, ());
        return match result {
            Ok(v) => Ok(read_arraybuffer_as_vec(
                instance
                    .get_memory(&mut wasmstore, "memory")
                    .unwrap()
                    .data(&mut wasmstore),
                v,
            )),
            Err(e) => Err(e),
        };
    }
    pub fn refresh_memory(&mut self) {
        let mut wasmstore = Store::<State>::new(&self.engine, State::new());
        wasmstore.limiter(|state| &mut state.limits);
        self.instance = self
            .linker
            .instantiate(&mut wasmstore, &self.module)
            .unwrap();
        self.wasmstore = wasmstore;
    }

    pub fn db_create_empty_update_list(batch: &mut T::Batch, height: u32) {
        let height_vec: Vec<u8> = height.to_le_bytes().try_into().unwrap();
        let key: Vec<u8> = db_make_length_key(&db_make_updated_key(&height_vec));
        let value_vec: Vec<u8> = (0 as u32).to_le_bytes().try_into().unwrap();
        batch.put(&key, &value_vec);
    }
    pub fn run(&mut self) -> Result<(), anyhow::Error> {
        self.context.lock().unwrap().state = 0;
        let start = self
            .instance
            .get_typed_func::<(), ()>(&mut self.wasmstore, "_start")
            .unwrap();
        self.handle_reorg();
        match start.call(&mut self.wasmstore, ()) {
            Ok(_) => {
                if self.context.lock().unwrap().state != 1 && !self.wasmstore.data().had_failure {
                    return Err(anyhow!("indexer exited unexpectedly"));
                }
                return Ok(());
            }
            Err(e) => Err(e),
        }
    }

    pub fn check_latest_block_for_reorg(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        height: u32,
    ) -> u32 {
        let result = context
            .lock()
            .unwrap()
            .db
            .get(db_make_length_key(&db_make_updated_key(&u32_to_vec(
                height as u32,
            ))))
            .unwrap();
        match result {
            Some(_v) => Self::check_latest_block_for_reorg(context.clone(), height + 1),
            None => return height,
        }
    }

    pub fn db_length_at_key(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        length_key: &Vec<u8>,
    ) -> u32 {
        return match context.lock().unwrap().db.get(length_key).unwrap() {
            Some(v) => u32::from_le_bytes(v.try_into().unwrap()),
            None => 0,
        };
    }

    pub fn db_updated_keys_for_block(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        height: u32,
    ) -> HashSet<Vec<u8>> {
        let key: Vec<u8> = db_make_updated_key(&u32_to_vec(height));
        let length: i32 = Self::db_length_at_key(context.clone(), &key) as i32;
        let mut i: i32 = 0;
        let mut set: HashSet<Vec<u8>> = HashSet::<Vec<u8>>::new();
        while i < length {
            set.insert(
                context
                    .lock()
                    .unwrap()
                    .db
                    .get(&db_make_list_key(&key, i as u32))
                    .unwrap()
                    .unwrap(),
            );
            i = i + 1;
        }
        return set;
    }
    pub fn db_value_at_block(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        key: &Vec<u8>,
        height: u32,
    ) -> Vec<u8> {
        let length: i32 = Self::db_length_at_key(context.clone(), &db_make_length_key(key))
            .try_into()
            .unwrap();
        let mut index: i32 = length - 1;
        while index >= 0 {
            let value: Vec<u8> = match context
                .lock()
                .unwrap()
                .db
                .get(db_make_list_key(key, index.try_into().unwrap()))
                .unwrap()
            {
                Some(v) => v,
                None => db_make_list_key(&Vec::<u8>::new(), 0),
            };

            let value_height: u32 =
                u32::from_le_bytes(value.as_slice()[(value.len() - 4)..].try_into().unwrap());
            if height >= value_height.try_into().unwrap() {
                let mut result = value.clone();
                result.truncate(value.len().saturating_sub(4));
                /*
                panic!("exit");
                */
                return result;
            }
            index -= 1;
        }
        return vec![];
    }

    pub fn db_updated_keys_for_block_range(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        from: u32,
        to: u32,
    ) -> HashSet<Vec<u8>> {
        let mut i = from;
        let mut result: HashSet<Vec<u8>> = HashSet::<Vec<u8>>::new();
        while to >= i {
            result.extend(Self::db_updated_keys_for_block(context.clone(), i));
            i = i + 1;
        }
        return result;
    }

    pub fn db_rollback_key(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        key: &Vec<u8>,
        to_block: u32,
    ) {
        let length: i32 = Self::db_length_at_key(context.clone(), &key)
            .try_into()
            .unwrap();
        let mut index: i32 = length - 1;
        let mut end_length: i32 = length;
        while index >= 0 {
            let list_key = db_make_list_key(key, index.try_into().unwrap());
            let _ = match context.lock().unwrap().db.get(&list_key).unwrap() {
                Some(value) => {
                    let value_height: u32 = u32::from_le_bytes(
                        value.as_slice()[(value.len() - 4)..].try_into().unwrap(),
                    );
                    if to_block <= value_height.try_into().unwrap() {
                        context.lock().unwrap().db.delete(&list_key).unwrap();
                        end_length = end_length - 1;
                    } else {
                        break;
                    }
                }
                None => {
                    break;
                }
            };
            index -= 1;
        }
        if end_length != length {
            Self::db_set_length(context.clone(), key, end_length as u32);
        }
    }

    pub fn db_set_length(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        key: &Vec<u8>,
        length: u32,
    ) {
        let length_key = db_make_length_key(key);
        if length == 0 {
            context.lock().unwrap().db.delete(&length_key).unwrap();
            return;
        }
        let new_length_bits: Vec<u8> = (length + 1).to_le_bytes().try_into().unwrap();
        context
            .lock()
            .unwrap()
            .db
            .put(&length_key, &new_length_bits)
            .unwrap();
    }

    pub fn handle_reorg(&mut self) {
        let context: Arc<Mutex<MetashrewRuntimeContext<T>>> = self.context.clone();
        let height = { context.lock().unwrap().height };
        let latest: u32 = Self::check_latest_block_for_reorg(context.clone(), height);
        if latest == height {
            return;
        }
        let set: HashSet<Vec<u8>> =
            Self::db_updated_keys_for_block_range(context.clone(), height, latest);
        if set.len() != 0 {
            self.refresh_memory();
        }
        for key in set.iter() {
            Self::db_rollback_key(context.clone(), &key, height);
        }
    }

    pub fn setup_linker(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        linker: &mut Linker<State>,
    ) {
        let context_ref_len = context.clone();
        let context_ref_input = context.clone();
        linker
            .func_wrap(
                "env",
                "__host_len",
                move |mut _caller: Caller<'_, State>| -> i32 {
                    return context_ref_len.lock().unwrap().block.len() as i32 + 4;
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "env",
                "__load_input",
                move |mut caller: Caller<'_, State>, data_start: i32| {
                    let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
                    let (input, height) = {
                        let ref_copy = context_ref_input.clone();
                        let ctx = ref_copy.lock().unwrap();
                        (ctx.block.clone(), ctx.height)
                    };
                    let mut input_clone: Vec<u8> =
                        <Vec<u8> as TryFrom<[u8; 4]>>::try_from(height.to_le_bytes()).unwrap();
                    input_clone.extend(input.clone());
                    let sz: usize = to_usize_or_trap(&mut caller, data_start);
                    if sz == usize::MAX {
                        caller.data_mut().had_failure = true;
                    }
                    let _ = mem.write(&mut caller, sz, input_clone.as_slice());
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "env",
                "__log",
                |mut caller: Caller<'_, State>, data_start: i32| {
                    let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
                    let data = mem.data(&caller);
                    let bytes = match try_read_arraybuffer_as_vec(data, data_start) {
                        Ok(v) => v,
                        Err(_) => {
                            return;
                        }
                    };
                    print!("{}", std::str::from_utf8(bytes.as_slice()).unwrap());
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "env",
                "abort",
                |mut caller: Caller<'_, State>, _: i32, _: i32, _: i32, _: i32| {
                    caller.data_mut().had_failure = true;
                    return;
                },
            )
            .unwrap();
    }
    pub fn db_append_annotated(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        batch: &mut T::Batch,
        key: &Vec<u8>,
        value: &Vec<u8>,
        block_height: u32,
    ) {
        let length_key = db_make_length_key(key);
        let length: u32 = Self::db_length_at_key(context.clone(), &length_key);
        let entry = db_annotate_value(value, block_height);

        let entry_key: Vec<u8> = db_make_list_key(key, length);
        batch.put(&entry_key, &entry);
        let new_length_bits: Vec<u8> = (length + 1).to_le_bytes().try_into().unwrap();
        batch.put(&length_key, &new_length_bits);
    }
    pub fn db_append(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        batch: &mut T::Batch,
        key: &Vec<u8>,
        value: &Vec<u8>,
    ) {
        let length_key = db_make_length_key(key);
        let length: u32 = Self::db_length_at_key(context.clone(), &length_key);
        let entry_key: Vec<u8> = db_make_list_key(key, length);
        batch.put(&entry_key, &value);
        let new_length_bits: Vec<u8> = (length + 1).to_le_bytes().try_into().unwrap();
        batch.put(&length_key, &new_length_bits);
    }
    pub fn setup_linker_view(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        linker: &mut Linker<State>,
    ) {
        let context_get = context.clone();
        let context_get_len = context.clone();
        linker
            .func_wrap(
                "env",
                "__flush",
                move |_caller: Caller<'_, State>, _encoded: i32| {},
            )
            .unwrap();
        linker
            .func_wrap(
                "env",
                "__get",
                move |mut caller: Caller<'_, State>, key: i32, value: i32| {
                    let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
                    let data = mem.data(&caller);
                    let height = {
                        let val = context_get.clone().lock().unwrap().height;
                        val
                    };
                    let key_vec_result = try_read_arraybuffer_as_vec(data, key);
                    match key_vec_result {
                        Ok(key_vec) => {
                            let lookup =
                                Self::db_value_at_block(context_get.clone(), &key_vec, height);
                            mem.write(&mut caller, value as usize, lookup.as_slice())
                                .unwrap();
                        }
                        Err(e) => {
                            mem.write(
                                &mut caller,
                                (value - 4) as usize,
                                <[u8; 4] as TryInto<Vec<u8>>>::try_into(i32::MAX.to_le_bytes())
                                    .unwrap()
                                    .as_slice(),
                            )
                            .unwrap();
                        }
                    };
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "env",
                "__get_len",
                move |mut caller: Caller<'_, State>, key: i32| -> i32 {
                    let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
                    let data = mem.data(&caller);
                    let key_vec_result = try_read_arraybuffer_as_vec(data, key);
                    let height = {
                        let val = context_get_len.clone().lock().unwrap().height;
                        val
                    };
                    match key_vec_result {
                        Ok(key_vec) => {
                            let value =
                                Self::db_value_at_block(context_get_len.clone(), &key_vec, height);
                            return value.len() as i32;
                        }
                        Err(_) => {
                            return i32::MAX;
                        }
                    };
                },
            )
            .unwrap();
    }
    pub fn setup_linker_indexer(
        context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
        linker: &mut Linker<State>,
    ) {
        let context_ref = context.clone();
        let context_get = context.clone();
        let context_get_len = context.clone();
        linker
            .func_wrap(
                "env",
                "__flush",
                move |mut caller: Caller<'_, State>, encoded: i32| {
                    let height = {
                        let val = context_ref.clone().lock().unwrap().height;
                        val
                    };
                    let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
                    let data = mem.data(&caller);
                    let encoded_vec = match try_read_arraybuffer_as_vec(data, encoded) {
                        Ok(v) => v,
                        Err(_) => {
                            return;
                        }
                    };
                    let mut batch = T::Batch::default();
                    let _ = Self::db_create_empty_update_list(&mut batch, height as u32);
                    let decoded: KeyValueFlush =
                        KeyValueFlush::parse_from_bytes(&encoded_vec).unwrap();

                    for (k, v) in decoded.list.iter().tuples() {
                        let k_owned = <Vec<u8> as Clone>::clone(k);
                        let v_owned = <Vec<u8> as Clone>::clone(v);
                        Self::db_append_annotated(
                            context_ref.clone(),
                            &mut batch,
                            &k_owned,
                            &v_owned,
                            height as u32,
                        );
                        let update_key: Vec<u8> =
                            <Vec<u8> as TryFrom<[u8; 4]>>::try_from((height as u32).to_le_bytes())
                                .unwrap();
                        Self::db_append(context_ref.clone(), &mut batch, &update_key, &k_owned);
                    }
                    debug!(
                        "saving {:?} k/v pairs for block {:?}",
                        decoded.list.len() / 2,
                        height
                    );
                    context_ref.clone().lock().unwrap().state = 1;
                    context_ref.clone().lock().unwrap().db.write(batch).unwrap();
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "env",
                "__get",
                move |mut caller: Caller<'_, State>, key: i32, value: i32| {
                    let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
                    let data = mem.data(&caller);
                    let key_vec_result = try_read_arraybuffer_as_vec(data, key);
                    let height = {
                        let val = context_get.clone().lock().unwrap().height;
                        val
                    };
                    match key_vec_result {
                        Ok(key_vec) => {
                            let lookup =
                                Self::db_value_at_block(context_get.clone(), &key_vec, height);
                            let _ = mem.write(&mut caller, value as usize, lookup.as_slice());
                        }
                        Err(e) => {
                            mem.write(
                                &mut caller,
                                (value - 4) as usize,
                                <[u8; 4] as TryInto<Vec<u8>>>::try_into(i32::MAX.to_le_bytes())
                                    .unwrap()
                                    .as_slice(),
                            )
                            .unwrap();
                        }
                    };
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "env",
                "__get_len",
                move |mut caller: Caller<'_, State>, key: i32| -> i32 {
                    let mem = caller.get_export("memory").unwrap().into_memory().unwrap();
                    let data = mem.data(&caller);
                    let key_vec_result = try_read_arraybuffer_as_vec(data, key);
                    let height = {
                        let val = context_get_len.clone().lock().unwrap().height;
                        val
                    };
                    match key_vec_result {
                        Ok(key_vec) => {
                            let value =
                                Self::db_value_at_block(context_get_len.clone(), &key_vec, height);
                            return value.len() as i32;
                        }
                        Err(_) => {
                            return i32::MAX;
                        }
                    };
                },
            )
            .unwrap();
    }
}

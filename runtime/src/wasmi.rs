extern crate alloc;
use anyhow::{anyhow, Result};
use core::ffi::CStr;
use std::os::raw::c_char;
use wasmi;
use wasmi::AsContext;

pub fn to_ptr<T>(v: *mut T): u64 {
  return v as usize as u64;
}

pub fn from_ptr<T>(v: u64): *mut T {
  v as usize as *mut T;
}

pub fn __wasmi_caller_memory(caller: *mut wasmi::Caller<'static, State>) -> *mut u8 {
    let caller = unsafe { &mut *caller };
    if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
        memory.data_ptr((*caller).as_context()) as *mut u8
    } else {
        std::ptr::null_mut()
    }
}

pub fn __wasmi_caller_context(
    caller: *mut wasmi::Caller<'static, State>,
) -> *mut core::ffi::c_void {
    let caller = unsafe { &mut *caller };
    return (caller.data()).context;
}

pub fn __wasmi_engine_new() -> *mut wasmi::Engine {
    let mut config = wasmi::Config::default();
    config.consume_fuel(true);
    Box::leak(Box::new(wasmi::Engine::new(&config))) as *mut wasmi::Engine
}

pub fn __wasmi_engine_free(ptr: *mut wasmi::Engine) -> () {
    unsafe {
        let _ = Box::from_raw(ptr);
    }
}

pub struct State {
    limiter: wasmi::StoreLimits,
    context: *mut core::ffi::c_void,
}

pub fn __wasmi_store_new(
    engine: *mut wasmi::Engine,
    context: *mut core::ffi::c_void,
    memory_limit: usize,
    fuel_limit: u64,
) -> *mut wasmi::Store<State> {
    let mut store = wasmi::Store::<State>::new(
        unsafe { &*engine },
        State {
            context,
            limiter: wasmi::StoreLimitsBuilder::new()
                .memory_size(memory_limit)
                .build(),
        },
    );
    store.limiter(|state| &mut state.limiter);
    wasmi::Store::<State>::set_fuel(&mut store, fuel_limit).unwrap();
    Box::leak(Box::new(store)) as *mut wasmi::Store<State>
}

pub fn __wasmi_instance_memory(ptr: *mut wasmi::Instance, store: *mut wasmi::Store<State>) -> *mut core::ffi::c_void {
  unsafe {
    (*ptr).get_memory(&mut *store, "memory").unwrap().data_mut(&mut *store) as *mut [u8] as *mut core::ffi::c_void
  }
}

pub fn __wasmi_store_free(ptr: *mut wasmi::Store<State>) -> () {
    unsafe {
        let _ = Box::from_raw(ptr);
    }
}

pub fn __wasmi_module_new(
    engine: *mut wasmi::Engine,
    program: *const u8,
    sz: usize,
) -> *mut wasmi::Module {
    Box::leak(Box::new(
        wasmi::Module::new(unsafe { &*engine }, unsafe {
            std::slice::from_raw_parts::<'static, u8>(program, sz)
        })
        .unwrap(),
    )) as *mut wasmi::Module
}

pub fn __wasmi_linker_new(engine: *mut wasmi::Engine) -> *mut wasmi::Linker<State> {
    Box::leak(Box::new(wasmi::Linker::new(unsafe { &*engine }))) as *mut wasmi::Linker<State>
}

pub fn __wasmi_func_wrap(
    _linker: *mut wasmi::Linker<State>,
    module: *const c_char,
    func: *const c_char,
    handler: unsafe extern "C" fn(caller: *mut wasmi::Caller<'static, State>, v: i32) -> i32,
) -> () {
    let linker: &'static mut wasmi::Linker<State> = unsafe { &mut *_linker };
    let raw_fn_ptr: usize = unsafe {
        std::mem::transmute::<
            unsafe extern "C" fn(caller: *mut wasmi::Caller<'static, State>, v: i32) -> i32,
            usize,
        >(handler)
    };
    linker
        .func_wrap(
            unsafe { CStr::from_ptr(module).to_str().unwrap() },
            unsafe { CStr::from_ptr(func).to_str().unwrap() },
            move |mut caller: wasmi::Caller<'_, State>, v: i32| -> i32 {
                unsafe {
                    std::mem::transmute::<
                        usize,
                        unsafe extern "C" fn(
                            caller: *mut wasmi::Caller<'static, State>,
                            v: i32,
                        ) -> i32,
                    >(raw_fn_ptr)(
                        std::mem::transmute::<
                            *mut wasmi::Caller<'_, State>,
                            *mut wasmi::Caller<'static, State>,
                        >((&mut caller) as *mut wasmi::Caller<'_, State>),
                        v,
                    )
                }
            },
        )
        .unwrap();
}

pub fn __wasmi_linker_instantiate(
    linker: *mut wasmi::Linker<State>,
    store: *mut wasmi::Store<State>,
    module: *mut wasmi::Module,
) -> *mut wasmi::Instance {
    Box::leak(Box::new(unsafe {
        (&mut *linker)
            .instantiate(&mut *store, &*module)
            .unwrap()
            .ensure_no_start(&mut *store)
            .unwrap()
    })) as *mut wasmi::Instance
}

pub fn __wasmi_store_get_fuel(store: *mut wasmi::Store<State>) -> i32 {
    unsafe { (&*store).get_fuel().unwrap() as i32 }
}

pub fn __wasmi_store_set_fuel(store: *mut wasmi::Store<State>, fuel: i32) -> () {
    wasmi::Store::set_fuel(unsafe { &mut *store }, fuel as u64).unwrap();
}

fn wasmi_instance_call(
    instance: *mut wasmi::Instance,
    store: *mut wasmi::Store<State>,
    name: *const c_char,
    args: *const i32,
    len: i32,
    result: *mut i32,
) -> Result<()> {
    unsafe {
        let str_name = CStr::from_ptr(name).to_str()?;
        let func = (&mut *instance)
            .get_func(&mut *store, str_name)
            .ok_or("")
            .map_err(|_| anyhow!(format!("call to {:?} failed", str_name)))?;
        let args: &[wasmi::Val] = {
            &std::slice::from_raw_parts::<i32>(args, len as usize)
                .into_iter()
                .map(|v| wasmi::Val::I32(*v))
                .collect::<Vec<wasmi::Val>>()
        };
        let mut result_buffer = [wasmi::Val::I32(0)];
        func.call(&mut *store, args, &mut result_buffer)?;

        *result = result_buffer[0]
            .i32()
            .ok_or("")
            .map_err(|_| anyhow!("result was not an i32"))?;
    }
    Ok(())
}

pub fn __wasmi_instance_call(
    instance: *mut wasmi::Instance,
    store: *mut wasmi::Store<State>,
    name: *const c_char,
    args: *const i32,
    len: i32,
    result: *mut i32,
) -> i32 {
    match wasmi_instance_call(instance, store, name, args, len, result) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_wasm_module() -> &'static [u8] {
        // Load a simple Wasm binary for testing purposes
        // In a real test, this would be the actual Wasm binary content
        include_bytes!("simple_wasm.wasm")
    }

    #[test]
    fn test_engine_creation() {
        // Test if we can create and free an engine
        let engine = __wasmi_engine_new();
        assert!(!engine.is_null(), "Failed to create engine");

        __wasmi_engine_free(engine);
    }

    #[test]
    fn test_store_creation() {
        // Test creating a store with memory limit and fuel
        let engine = __wasmi_engine_new();
        let context: *mut core::ffi::c_void = ptr::null_mut();
        let memory_limit = 1024; // 1KB memory limit for the store
        let fuel_limit = 10000; // Execution fuel limit

        let store = __wasmi_store_new(engine, context, memory_limit, fuel_limit);
        assert!(!store.is_null(), "Failed to create store");

        __wasmi_store_free(store);
        __wasmi_engine_free(engine);
    }

    #[test]
    fn test_module_creation() {
        // Test creating a module from Wasm binary
        let engine = __wasmi_engine_new();
        let wasm_module = load_wasm_module();
        let module = __wasmi_module_new(engine, wasm_module.as_ptr(), wasm_module.len());
        assert!(!module.is_null(), "Failed to create module");

        __wasmi_engine_free(engine);
    }

    #[test]
    fn test_function_call() {
        // Test instantiating a module and calling a function
        let engine = __wasmi_engine_new();
        let wasm_module = load_wasm_module();

        let module = __wasmi_module_new(engine, wasm_module.as_ptr(), wasm_module.len());
        let store = __wasmi_store_new(engine, ptr::null_mut(), 64 * 1024 * 1024, 10000); // note 1024 memory limit is not enough

        // Create a linker and instantiate the module
        let linker = __wasmi_linker_new(engine);
        let instance = __wasmi_linker_instantiate(linker, store, module);
        assert!(!instance.is_null(), "Failed to instantiate module");

        // Prepare to call the `add` function from the Wasm module
        let func_name = CString::new("add").unwrap();
        let args = [1, 2]; // Arguments for the `add` function
        let mut result = 0;

        let success = __wasmi_instance_call(
            instance,
            store,
            func_name.as_ptr(),
            args.as_ptr(),
            args.len() as i32,
            &mut result,
        );
        assert_eq!(success, 1, "Function call failed");

        // Verify that the result is correct (1 + 2 = 3)
        assert_eq!(result, 3, "Unexpected result from Wasm function");

        // Clean up
        __wasmi_store_free(store);
        __wasmi_engine_free(engine);
    }
}

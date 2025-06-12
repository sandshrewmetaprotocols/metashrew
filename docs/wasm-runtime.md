# WASM Runtime

Metashrew's core functionality is built around a WebAssembly (WASM) runtime that enables deterministic execution of custom indexers. This document explains the WASM runtime architecture, host interface, memory management, and configuration details.

## WebAssembly Host Interface

The WebAssembly host interface defines the contract between Metashrew (the host) and the custom indexers (WASM modules). This interface enables communication and data exchange between the two environments.

### Host Functions

Metashrew provides the following host functions to WASM modules:

1. **`__host_len()`**: Returns the length of the input data (block height + serialized block).
   ```rust
   fn __host_len() -> i32;
   ```

2. **`__load_input(ptr: i32)`**: Loads input data into WASM memory at the specified pointer.
   ```rust
   fn __load_input(ptr: i32);
   ```

3. **`__log(ptr: i32)`**: Writes UTF-8 encoded text to stdout for debugging.
   ```rust
   fn __log(ptr: i32);
   ```

4. **`__flush(ptr: i32)`**: Commits key-value pairs to the database.
   ```rust
   fn __flush(ptr: i32);
   ```

5. **`__get(key_ptr: i32, value_ptr: i32)`**: Reads a value for a key from the database.
   ```rust
   fn __get(key_ptr: i32, value_ptr: i32) -> i32;
   ```

6. **`__get_len(ptr: i32)`**: Gets the length of a value for a key in the database.
   ```rust
   fn __get_len(ptr: i32) -> i32;
   ```

### WASM Module Exports

WASM modules must export the following functions:

1. **`_start()`**: Main entry point for block processing.
   ```rust
   fn _start();
   ```

2. **Custom view functions**: Named exports for querying indexed data.
   ```rust
   fn view_function_name(input_ptr: i32, input_len: i32) -> i32;
   ```

### Memory Layout

Data passed between the host and WASM modules follows AssemblyScript's ArrayBuffer memory layout:

- 4 bytes for length (little-endian u32)
- Followed by actual data bytes

Example in Rust:
```rust
fn write_to_memory(memory: &mut [u8], ptr: usize, data: &[u8]) {
    let len = data.len() as u32;
    memory[ptr..ptr+4].copy_from_slice(&len.to_le_bytes());
    memory[ptr+4..ptr+4+data.len()].copy_from_slice(data);
}
```

## Deterministic Execution

Deterministic execution is critical for blockchain applications to ensure consistent results across different environments. Metashrew achieves determinism through several mechanisms:

### 1. Wasmtime Configuration

Wasmtime is configured with specific settings to ensure deterministic execution:

```rust
let mut config = Config::new();
config.cranelift_opt_level(OptLevel::Speed);
config.consume_fuel(true);
config.epoch_interruption(false);
config.dynamic_memory_guard(false);
config.static_memory_maximum_size(4 * 1024 * 1024 * 1024); // 4GB
config.static_memory_guard(false);
config.wasm_reference_types(false);
config.wasm_multi_value(false);
config.wasm_bulk_memory(true);
config.wasm_simd(false);
config.wasm_threads(false);
config.wasm_multi_memory(false);
config.wasm_memory64(false);
```

### 2. Fuel Metering

Fuel metering prevents infinite loops and ensures consistent resource usage:

```rust
// Set a fuel limit for the WASM module
store.add_fuel(1_000_000_000)?;
```

### 3. Controlled Environment

The WASM module runs in a controlled environment with:

- No access to system time or random number generators
- No network access
- No file system access
- No threading or concurrency primitives

### 4. Deterministic APIs

All APIs provided to the WASM module are deterministic:

- Database operations are deterministic
- Input data is deterministic (block data)
- No external state is accessible

## Memory Management

Memory management is a critical aspect of the WASM runtime, as it handles data transfer between the host and WASM modules.

### Memory Allocation

Memory is allocated in the WASM module and accessed by the host:

```rust
// Get the memory export from the WASM module
let memory = instance.get_memory(&mut store, "memory")
    .ok_or_else(|| anyhow!("Failed to find memory export"))?;

// Access the memory
let data = memory.data(&mut store);
```

### Data Transfer

Data is transferred between the host and WASM module through memory:

1. **Host to WASM**:
   ```rust
   fn load_input(&mut self, ptr: i32) -> Result<()> {
       let memory = self.instance.get_memory(&mut self.store, "memory")
           .ok_or_else(|| anyhow!("Failed to find memory export"))?;
       
       let data = memory.data_mut(&mut self.store);
       let ptr = ptr as usize;
       
       // Write length (4 bytes)
       let len = self.context.lock()?.block.len() as u32;
       data[ptr..ptr+4].copy_from_slice(&len.to_le_bytes());
       
       // Write data
       data[ptr+4..ptr+4+len as usize].copy_from_slice(&self.context.lock()?.block);
       
       Ok(())
   }
   ```

2. **WASM to Host**:
   ```rust
   fn get_string(&mut self, ptr: i32) -> Result<String> {
       let memory = self.instance.get_memory(&mut self.store, "memory")
           .ok_or_else(|| anyhow!("Failed to find memory export"))?;
       
       let data = memory.data(&mut self.store);
       let ptr = ptr as usize;
       
       // Read length (4 bytes)
       let mut len_bytes = [0u8; 4];
       len_bytes.copy_from_slice(&data[ptr..ptr+4]);
       let len = u32::from_le_bytes(len_bytes) as usize;
       
       // Read data
       let string_data = &data[ptr+4..ptr+4+len];
       let string = String::from_utf8(string_data.to_vec())?;
       
       Ok(string)
   }
   ```

### Memory Limits

Memory limits are enforced to prevent resource exhaustion:

```rust
config.static_memory_maximum_size(4 * 1024 * 1024 * 1024); // 4GB
```

## Wasmtime Configuration

Metashrew uses Wasmtime as its WebAssembly runtime. Wasmtime is configured for optimal performance and determinism.

### Version

Metashrew uses Wasmtime version 15.0.1, which provides a good balance of features, performance, and stability.

### Compilation Strategy

Wasmtime uses Cranelift as its code generator, configured for speed:

```rust
config.cranelift_opt_level(OptLevel::Speed);
```

### Feature Configuration

Specific WebAssembly features are enabled or disabled to ensure determinism and compatibility:

```rust
// Enabled features
config.wasm_bulk_memory(true);

// Disabled features
config.wasm_reference_types(false);
config.wasm_multi_value(false);
config.wasm_simd(false);
config.wasm_threads(false);
config.wasm_multi_memory(false);
config.wasm_memory64(false);
```

### Security Settings

Security settings are configured to prevent potential vulnerabilities:

```rust
config.dynamic_memory_guard(false);
config.static_memory_guard(false);
```

## Block Processing Flow

The block processing flow in the WASM runtime follows these steps:

1. **Initialization**:
   ```rust
   let engine = Engine::new(&config)?;
   let module = Module::from_file(&engine, wasm_path)?;
   let mut store = Store::new(&engine, Context::default());
   let instance = linker.instantiate(&mut store, &module)?;
   ```

2. **Set Block Data**:
   ```rust
   context.block = block_data;
   context.height = height;
   ```

3. **Execute WASM Module**:
   ```rust
   let start = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
   start.call(&mut store, ())?;
   ```

4. **Handle Results**:
   ```rust
   if context.state != 1 && !store.data().had_failure {
       return Err(anyhow!("indexer exited unexpectedly"));
   }
   ```

## View Function Execution

View functions are executed in a similar way to block processing:

```rust
pub async fn view(&self, function_name: String, input: &[u8], height: u32) -> Result<Vec<u8>> {
    // Get a lock on the runtime
    let mut store = self.store.lock().await;
    
    // Set the height for historical queries
    store.data_mut().height = height;
    
    // Get the view function
    let view_func = self.instance.get_typed_func::<(i32, i32), i32>(&mut *store, &function_name)
        .map_err(|_| anyhow!("View function not found: {}", function_name))?;
    
    // Allocate memory for input
    let input_ptr = self.alloc(&mut *store, input.len() as i32)?;
    
    // Write input to memory
    self.write_memory(&mut *store, input_ptr, input)?;
    
    // Call the view function
    let result_ptr = view_func.call(&mut *store, (input_ptr, input.len() as i32))?;
    
    // Read the result from memory
    let result = self.read_memory(&mut *store, result_ptr)?;
    
    Ok(result)
}
```

## Error Handling

Error handling in the WASM runtime is designed to provide meaningful feedback while maintaining security:

```rust
fn run(&mut self) -> Result<()> {
    self.context.lock().map_err(lock_err)?.state = 0;
    let start = self
        .instance
        .get_typed_func::<(), ()>(&mut self.store, "_start")
        .context("Failed to get _start function")?;
    
    self.handle_reorg()?;
    
    match start.call(&mut self.store, ()) {
        Ok(_) => {
            if self.context.lock().map_err(lock_err)?.state != 1 && !self.store.data().had_failure {
                return Err(anyhow!("indexer exited unexpectedly"));
            }
            Ok(())
        }
        Err(e) => Err(e).context("Error calling _start function"),
    }
}
```

## Conclusion

Metashrew's WASM runtime provides a secure, deterministic environment for executing custom indexers. The WebAssembly host interface enables communication between the host and WASM modules, while careful configuration of Wasmtime ensures consistent results across different environments.

By understanding the WASM runtime, developers can build efficient, deterministic indexers that leverage Metashrew's capabilities while maintaining compatibility with the host interface.
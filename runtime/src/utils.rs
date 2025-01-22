use anyhow::Result;
use wasmtime::Caller;

use crate::State;

/// Convert to a signed integer or return trap value
pub fn to_signed_or_trap<'a, T: TryInto<i32>>(caller: &mut Caller<'_, State>, v: T) -> i32 {
    match <T as TryInto<i32>>::try_into(v) {
        Ok(v) => v,
        Err(_) => i32::MAX,
    }
}

/// Convert to usize or return trap value
pub fn to_usize_or_trap<'a, T: TryInto<usize>>(caller: &mut Caller<'_, State>, v: T) -> usize {
    match <T as TryInto<usize>>::try_into(v) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    }
}

/// Get a memory instance with error handling
pub fn get_memory<'a>(caller: &mut Caller<'_, State>, name: &str) -> Option<wasmtime::Memory> {
    caller
        .get_export(name)
        .and_then(|export| export.into_memory())
}

/// Try to write to memory with error handling
pub fn try_write_memory(
    memory: &wasmtime::Memory,
    caller: &mut Caller<'_, State>,
    offset: usize,
    data: &[u8],
) -> Result<()> {
    memory.write(caller, offset, data)
        .map_err(|e| anyhow::anyhow!("Memory write error: {:?}", e))
}
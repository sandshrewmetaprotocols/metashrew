//! Generic MetashrewRuntime that works with any storage backend

use anyhow::Result;

// Core modules
pub mod block_stm;
pub mod block_stm_scheduler;
pub mod chain_entries;
pub mod context;
pub mod helpers;
pub mod indexer_syscall;
pub mod key_utils;
pub mod length_cache;
pub mod proto;
pub mod rollback;
pub mod runtime;
pub mod traits;
pub mod view_cache;
pub mod view_limits;
pub mod view_syscall;
pub mod view_threads;


// Re-export core types and traits
pub use context::MetashrewRuntimeContext;
pub use runtime::{MetashrewRuntime, State, TIP_HEIGHT_KEY};
pub use length_cache::LengthCache;
pub use traits::{BatchLike, KVTrackerFn, KeyValueStoreLike};
pub use view_limits::{
    MemoryFloor, ViewAcquireError, ViewLimiter, ViewLimitsConfig, ViewPermit,
    DEFAULT_ACQUIRE_TIMEOUT, DEFAULT_MEMORY_FLOOR_REFRESH_MS, DEFAULT_VIEW_CONCURRENCY,
    DEFAULT_VIEW_MEMORY_FLOOR_MB, DEFAULT_VIEW_MEMORY_MB,
};

// Re-export v10 chain-entry primitives for downstream crates that pin
// against this module path (e.g. alkanes-v220-alpha).
pub use chain_entries::{
    append_value_to_batch, build_block_write_batch, chain_entry_key, decode_chain_length,
    decode_value_entry, deserialize_key_manifest, encode_chain_length, encode_value_entry,
    get_at_height, rollback_all_keys_to_batch, rollback_key_to_batch, serialize_key_manifest,
    write_block_batch, MANIFEST_PREFIX,
};

// Utility functions that are storage-backend agnostic
static mut _LABEL: Option<String> = None;

const TIMEOUT: u64 = 1500;

use std::{thread, time};

pub fn wait_timeout() {
    thread::sleep(time::Duration::from_millis(TIMEOUT));
}

pub fn set_label(s: String) -> () {
    unsafe {
        _LABEL = Some(s + "://");
    }
}

#[allow(static_mut_refs)]
pub fn get_label() -> &'static String {
    unsafe { _LABEL.as_ref().unwrap() }
}

#[allow(static_mut_refs)]
pub fn has_label() -> bool {
    unsafe { _LABEL.is_some() }
}

pub fn to_labeled_key(key: &Vec<u8>) -> Vec<u8> {
    if has_label() {
        let mut result: Vec<u8> = vec![];
        result.extend(get_label().as_str().as_bytes());
        result.extend(key);
        result
    } else {
        key.clone()
    }
}

/// Generic function to query height from any storage backend
pub async fn query_height<T: KeyValueStoreLike>(mut db: T, start_block: u32) -> Result<u32>
where
    T::Error: std::error::Error + Send + Sync + 'static,
{
    let height_key = TIP_HEIGHT_KEY.as_bytes().to_vec();
    let bytes = match db
        .get(&to_labeled_key(&height_key))
        .map_err(|e| anyhow::anyhow!("Database error: {:?}", e))?
    {
        Some(v) => v,
        None => {
            return Ok(start_block);
        }
    };
    if bytes.len() == 0 {
        return Ok(start_block);
    }
    let bytes_ref: &[u8] = &bytes;
    Ok(u32::from_le_bytes(bytes_ref.try_into().unwrap()))
}

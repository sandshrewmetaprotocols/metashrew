//! v10 versioned-chain primitives for height-indexed key/value storage.
//!
//! Every WASM-indexed key in the database is stored as an append-only chain of
//! `{key}/0`, `{key}/1`, … entries, with `{key}/length` recording how many
//! updates exist. Each entry is encoded as `[u32 LE height | raw value bytes]`,
//! so a binary search over the chain returns the value at any historical
//! height — the read path used by every view RPC.
//!
//! This module replaces the v9 `"{height}:{hex_value}"` UTF-8 + hex encoding
//! that lived in the (now-deleted) SMT helper. The v10 binary encoding is
//! ~50% smaller per entry and skips the per-read string/hex parse, but the
//! chain structure and binary-search semantics are unchanged.
//!
//! # Database key layout
//!
//! - `{key}/length` — u32 LE, number of updates for `{key}`
//! - `{key}/{i}`    — i-th update, encoded as `[u32 LE height | value bytes]`
//! - `/__INTERNAL/keys-at-height/{height}` — per-block manifest of keys touched
//!   by the block, used by the fast-rollback path to avoid a full DB scan
//!
//! # Why this lives in a dedicated module
//!
//! The encoders, decoders, the manifest helpers, and the per-block batch
//! builder are the load-bearing primitives the runtime + rollback path
//! actually use. They were previously co-located with a 2500-line Sparse
//! Merkle Tree implementation that was dead code — the SMT was a no-op in
//! the hot path (`new_root = prev_root` unconditionally) and the only
//! reads were SMT-root metadata that nothing consumed. Future per-table
//! root calculation happens inside the WASM indexer, not here.

use crate::traits::{BatchLike, KeyValueStoreLike};
use anyhow::{anyhow, Result};
use std::collections::HashMap;

/// Per-height key manifest prefix. The manifest at
/// `/__INTERNAL/keys-at-height/{height}` lists every key written by block
/// `{height}`, and the rollback path reads it to scope work to just the
/// affected keys instead of scanning the entire database.
pub const MANIFEST_PREFIX: &str = "/__INTERNAL/keys-at-height/";

/// Serialize a list of keys into a compact binary format for per-height manifests.
/// Format: `[u32 LE: num_keys][u32 LE: key1_len][key1_bytes][u32 LE: key2_len][key2_bytes]...`
pub fn serialize_key_manifest(keys: &[&[u8]]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(keys.len() as u32).to_le_bytes());
    for key in keys {
        buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
        buf.extend_from_slice(key);
    }
    buf
}

/// Deserialize a per-height key manifest back into a list of keys.
pub fn deserialize_key_manifest(data: &[u8]) -> Vec<Vec<u8>> {
    if data.len() < 4 {
        return Vec::new();
    }
    let num_keys = u32::from_le_bytes(data[0..4].try_into().unwrap_or([0; 4])) as usize;
    let mut keys = Vec::with_capacity(num_keys);
    let mut offset = 4;
    for _ in 0..num_keys {
        if offset + 4 > data.len() {
            break;
        }
        let key_len =
            u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4])) as usize;
        offset += 4;
        if offset + key_len > data.len() {
            break;
        }
        keys.push(data[offset..offset + key_len].to_vec());
        offset += key_len;
    }
    keys
}

/// v10 versioned-chain entry encoding: `[u32 LE: height | value_bytes]`.
#[inline]
pub fn encode_value_entry(height: u32, value: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + value.len());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(value);
    buf
}

/// Decode a v10 versioned-chain entry: returns `(height, value_slice)`.
#[inline]
pub fn decode_value_entry(entry: &[u8]) -> Result<(u32, &[u8])> {
    if entry.len() < 4 {
        return Err(anyhow!(
            "v10 chain entry too short: {} bytes (need ≥4)",
            entry.len()
        ));
    }
    let mut h = [0u8; 4];
    h.copy_from_slice(&entry[..4]);
    Ok((u32::from_le_bytes(h), &entry[4..]))
}

/// Encode a chain length (u32 LE).
#[inline]
pub fn encode_chain_length(n: u32) -> [u8; 4] {
    n.to_le_bytes()
}

/// Decode a chain length. Missing/short bytes decode as 0 to match the
/// "no length key → empty chain" convention.
#[inline]
pub fn decode_chain_length(bytes: &[u8]) -> u32 {
    if bytes.len() < 4 {
        return 0;
    }
    let mut b = [0u8; 4];
    b.copy_from_slice(&bytes[..4]);
    u32::from_le_bytes(b)
}

/// Build the `{key}/{index}` entry key for a chain. Centralized so call sites
/// don't reinvent `i.to_string().as_bytes()`.
#[inline]
pub fn chain_entry_key(key: &[u8], index: u32) -> Vec<u8> {
    let idx_str = index.to_string();
    let mut k = Vec::with_capacity(key.len() + 1 + idx_str.len());
    k.extend_from_slice(key);
    k.push(b'/');
    k.extend_from_slice(idx_str.as_bytes());
    k
}

/// Append a single `(key, value)` update at `height` to an existing batch
/// using the v10 append-only encoding.
///
/// Reads the current chain length from `storage` (an immutable read), then
/// stages two writes into `batch`:
///   - `{key}/{length}` ← `[u32 LE height | value]`
///   - `{key}/length`   ← `length + 1` (u32 LE)
///
/// The caller is responsible for committing `batch`. Used by the preview
/// `__flush` host function which writes per-call instead of per-block.
pub fn append_value_to_batch<T: KeyValueStoreLike>(
    storage: &T,
    batch: &mut T::Batch,
    key: &[u8],
    value: &[u8],
    height: u32,
) -> Result<()> {
    let length_key = [key, b"/length"].concat();
    let current_length = match storage
        .get_immutable(&length_key)
        .map_err(|e| anyhow!("Storage error: {:?}", e))?
    {
        Some(length_bytes) => decode_chain_length(&length_bytes),
        None => 0,
    };
    let update_key = chain_entry_key(key, current_length);
    batch.put(&update_key, encode_value_entry(height, value));
    batch.put(&length_key, encode_chain_length(current_length + 1));
    Ok(())
}

/// Binary-search the v10 append-only chain for `key` and return its value at
/// `height` (i.e. the most recent update at-or-before `height`), or `None`
/// if no such update exists.
///
/// This is the canonical read path used by view RPCs and by the runtime's
/// `__get`/`__get_len` host functions.
pub fn get_at_height<T: KeyValueStoreLike>(
    storage: &T,
    key: &[u8],
    height: u32,
) -> Result<Option<Vec<u8>>> {
    let length_key = [key, b"/length"].concat();
    let length = match storage
        .get_immutable(&length_key)
        .map_err(|e| anyhow!("Storage error: {:?}", e))?
    {
        Some(length_bytes) => decode_chain_length(&length_bytes),
        None => return Ok(None),
    };
    if length == 0 {
        return Ok(None);
    }

    let mut left = 0;
    let mut right = length;
    let mut best_value: Option<Vec<u8>> = None;

    while left < right {
        let mid = (left + right) / 2;
        let update_key = chain_entry_key(key, mid);
        match storage
            .get_immutable(&update_key)
            .map_err(|e| anyhow!("Storage error: {:?}", e))?
        {
            Some(update_data) => {
                let (update_height, value_bytes) = decode_value_entry(&update_data)?;
                if update_height <= height {
                    best_value = Some(value_bytes.to_vec());
                    left = mid + 1;
                } else {
                    right = mid;
                }
            }
            None => return Err(anyhow!("Missing update at index {}", mid)),
        }
    }
    Ok(best_value)
}

/// Build the complete per-block write batch from `key_values` WITHOUT
/// committing it. The returned batch contains:
///
///   1. The per-key append-only writes (`{key}/{length}` payload +
///      incremented `{key}/length` counter) for every update in `key_values`.
///   2. The per-block key manifest at `/__INTERNAL/keys-at-height/{height}`
///      (only when `key_values` is non-empty), which the fast-rollback path
///      reads to scope its work.
///   3. The runtime-side tip pointer at `/__INTERNAL/tip-height` ← `height`
///      (u32 LE). This is the same key the sync-framework's metadata layer
///      writes to in `commit_atomic`; co-residency in the same batch is
///      intentional so the runtime tip stays consistent with WASM-side state.
///
/// The block-hash and indexed-height-pointer metadata records are intentionally
/// NOT staged here — those are owned by `StorageAdapter::commit_atomic`, which
/// appends them to this batch (after deserialization) and submits the whole
/// thing in one `db.write_opt(batch, sync=true)` call. That single-batch
/// design is what makes block-apply truly all-or-nothing on a process crash.
///
/// `_block_hash` is accepted for API symmetry with the sync-framework boundary
/// but is intentionally unused here for the same reason.
pub fn build_block_write_batch<T: KeyValueStoreLike>(
    storage: &T,
    height: u32,
    key_values: &[(Vec<u8>, Vec<u8>)],
    _block_hash: &[u8],
) -> Result<T::Batch> {
    let mut batch = storage.create_batch();

    // Track lengths within this batch to handle multiple updates to the same
    // key correctly — without this we'd read the stale on-disk length each
    // iteration and clobber earlier updates with overlapping indices.
    let mut key_lengths: HashMap<Vec<u8>, u32> = HashMap::new();

    for (key, value) in key_values {
        let length = if let Some(len) = key_lengths.get(key) {
            *len
        } else {
            let length_key = [key.as_slice(), b"/length".as_slice()].concat();
            match storage
                .get_immutable(&length_key)
                .map_err(|e| anyhow!("Storage error: {:?}", e))?
            {
                Some(length_bytes) => decode_chain_length(&length_bytes),
                None => 0,
            }
        };

        let update_key = chain_entry_key(key, length);
        batch.put(&update_key, encode_value_entry(height, value));

        let new_length = length + 1;
        let length_key = [key.as_slice(), b"/length".as_slice()].concat();
        batch.put(&length_key, encode_chain_length(new_length));
        key_lengths.insert(key.clone(), new_length);
    }

    // Per-height manifest of modified keys, for fast manifest-driven rollback.
    if !key_values.is_empty() {
        let mut unique_keys: Vec<&[u8]> = key_values.iter().map(|(k, _)| k.as_slice()).collect();
        unique_keys.sort_unstable();
        unique_keys.dedup();
        let manifest_key = format!("{}{}", MANIFEST_PREFIX, height).into_bytes();
        let manifest_value = serialize_key_manifest(&unique_keys);
        batch.put(&manifest_key, &manifest_value);
    }

    // Runtime-side tip pointer (read by view/preview paths).
    batch.put(
        &crate::runtime::TIP_HEIGHT_KEY.as_bytes().to_vec(),
        &height.to_le_bytes(),
    );

    Ok(batch)
}

/// Build and immediately write the per-block batch.
///
/// Used by the legacy non-atomic `process_block()` path and by non-RocksDB
/// backends that can't ship serialized batch bytes through the
/// runtime/storage-adapter boundary. The atomic production path builds the
/// batch via [`build_block_write_batch`] and ships it through
/// `AtomicBlockResult::batch_data` to `commit_atomic` instead.
pub fn write_block_batch<T: KeyValueStoreLike>(
    storage: &mut T,
    height: u32,
    key_values: &[(Vec<u8>, Vec<u8>)],
    block_hash: &[u8],
) -> Result<()> {
    let batch = build_block_write_batch(storage, height, key_values, block_hash)?;
    storage
        .write(batch)
        .map_err(|e| anyhow!("Storage error: {:?}", e))?;
    Ok(())
}

/// Roll back a key's append-only chain to its state at-or-before
/// `target_height`. Adds the necessary delete/put operations to `batch`.
///
/// Iterates the chain, keeping entries with `height <= target_height` and
/// staging deletes for the rest, then updates the `{key}/length` counter
/// to the new compacted length (or deletes it entirely if no entries remain).
///
/// Note: this leaves the surviving entries at their original indices — it
/// does NOT compact them down. The fast-rollback path
/// (`rollback_with_manifests`) walks the chain tail and trims in place,
/// which matches binary-search semantics; this helper is the slow-path
/// fallback used when manifests are missing.
pub fn rollback_key_to_batch<T: KeyValueStoreLike>(
    storage: &T,
    batch: &mut T::Batch,
    key: &[u8],
    target_height: u32,
) -> Result<()> {
    let length_key = [key, b"/length"].concat();
    if let Some(length_bytes) = storage
        .get_immutable(&length_key)
        .map_err(|e| anyhow!("Storage error: {:?}", e))?
    {
        let length = decode_chain_length(&length_bytes);

        let mut new_length = 0;
        for i in 0..length {
            let update_key = chain_entry_key(key, i);
            if let Some(update_data) = storage
                .get_immutable(&update_key)
                .map_err(|e| anyhow!("Storage error: {:?}", e))?
            {
                let (update_height, _value) = decode_value_entry(&update_data)?;
                if update_height <= target_height {
                    new_length = i + 1;
                } else {
                    batch.delete(&update_key);
                }
            }
        }

        batch.put(&length_key, encode_chain_length(new_length));
    }
    Ok(())
}

/// Roll back ALL keys to their state at-or-before `target_height`, staging
/// the operations into `batch`. Walks `{key}/length` keys via prefix scan
/// to discover every chain, then delegates per-key work to
/// [`rollback_key_to_batch`].
///
/// This is the slow-path rollback used when per-height manifests are
/// missing (e.g. blocks indexed before manifests existed). The
/// manifest-driven `rollback_with_manifests` in
/// [`crate::rollback`] is the fast path the production reorg code prefers.
pub fn rollback_all_keys_to_batch<T: KeyValueStoreLike>(
    storage: &T,
    batch: &mut T::Batch,
    target_height: u32,
) -> Result<()> {
    let length_suffix = b"/length";
    let mut keys_to_rollback = Vec::new();

    for (stored_key, _) in storage
        .scan_prefix(b"")
        .map_err(|e| anyhow!("Storage error: {:?}", e))?
    {
        if stored_key.ends_with(length_suffix) {
            let original_key = &stored_key[..stored_key.len() - length_suffix.len()];
            keys_to_rollback.push(original_key.to_vec());
        }
    }

    for key in keys_to_rollback {
        rollback_key_to_batch(storage, batch, &key, target_height)?;
    }
    Ok(())
}

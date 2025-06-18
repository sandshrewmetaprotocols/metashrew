#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use metashrew_core::{export_bytes, flush, get, input, set};

fn index() {
    let input_data = input();
    if input_data.len() < 4 {
        return;
    }
    let height = u32::from_le_bytes(input_data[0..4].try_into().unwrap());
    let block_bytes = &input_data[4..];
    let key = format!("/blocks/{}", height).into_bytes();
    set(Arc::new(key), Arc::new(block_bytes.to_vec()));
    flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    index();
}

#[unsafe(no_mangle)]
pub extern "C" fn view_block_by_height() -> i32 {
    let height_bytes = input();
    if height_bytes.len() != 4 {
        return export_bytes(Vec::new());
    }
    let height = u32::from_be_bytes(height_bytes.try_into().unwrap());
    let key = format!("/blocks/{}", height).into_bytes();
    let block_bytes_arc = get(Arc::new(key));
    let block_bytes: &Vec<u8> = &*block_bytes_arc;
    export_bytes(block_bytes.clone())
}

use bitcoin;
use metashrew_core::{get, index_pointer::{IndexPointer, KeyValuePointer}};
use metashrew_core::{println, stdout};
use std::fmt::Write;
use std::io::Cursor;
use std::sync::Arc;

#[metashrew_core::main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    for i in 0..height {
        let key = format!("/oom_test/{}/{}", height, i);
        let mut pointer = IndexPointer::from_keyword(&key);
        let value_to_set = block.to_vec();
        pointer.set(Arc::new(value_to_set));
    }
    let stats = metashrew_core::lru_cache_stats();
    println!("stats {:?}", stats);

    Ok(())
}

#[metashrew_core::view]
pub fn read_intensive_view(input: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut index_bytes = Cursor::new(input.to_vec());
    let height = metashrew_core::utils::consume_sized_int::<u32>(&mut index_bytes)?;
    let index = metashrew_core::utils::consume_sized_int::<u32>(&mut index_bytes)?;

    let key = format!("/oom_test/{}/{}", height, index);
    let pointer = IndexPointer::from_keyword(&key);
    let value = pointer.get();

    Ok(value.as_ref().clone())
}

#[metashrew_core::view]
pub fn get_cache_stats_view(_input: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let stats = metashrew_core::lru_cache_stats();
    let stats_string = format!("{:?}", stats);
    Ok(stats_string.into_bytes())
}

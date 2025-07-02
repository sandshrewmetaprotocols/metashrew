use bitcoin;
use metashrew_core::{get, index_pointer::IndexPointer};
use metashrew_support::index_pointer::KeyValuePointer;
use std::io::Cursor;
use std::sync::Arc;

#[metashrew_core::main]
pub fn main(height: u32, block: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    // Store the block data
    let mut block_pointer = IndexPointer::from_keyword(format!("/blocks/{}", height).as_str());
    block_pointer.set(Arc::new(block.to_vec()));
    
    // Parse the block
    let parsed_block =
        metashrew_support::utils::consensus_decode::<bitcoin::Block>(&mut Cursor::new(block.to_vec()))?;
    
    // Update block tracker
    let mut tracker = IndexPointer::from_keyword("/blocktracker");
    let mut new_tracker = tracker.get().as_ref().clone();
    new_tracker.extend((&[parsed_block.header.block_hash()[0]]).to_vec());
    tracker.set(Arc::new(new_tracker));
    
    // Add a simple test value to ensure something gets flushed
    let mut test_pointer = IndexPointer::from_keyword(format!("/test/{}", height).as_str());
    test_pointer.set(Arc::new(b"test_value".to_vec()));
    
    Ok(())
}

#[metashrew_core::view]
pub fn getblock(input: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut height_bytes = Cursor::new(input.to_vec());
    let height = metashrew_support::utils::consume_sized_int::<u32>(&mut height_bytes)?;
    let key = format!("/blocks/{}", height).into_bytes();
    let block_bytes_arc = get(Arc::new(key));
    let block_bytes: &Vec<u8> = &*block_bytes_arc;
    
    Ok(block_bytes.clone())
}

#[metashrew_core::view]
pub fn blocktracker(input: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let tracker_data = IndexPointer::from_keyword("/blocktracker")
        .get()
        .as_ref()
        .clone();
    
    Ok(tracker_data)
}

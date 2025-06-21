//! Debug test to investigate view function issues

use anyhow::Result;
use memshrew_runtime::{MemStoreAdapter, MemStoreRuntime, KeyValueStoreLike};
use metashrew_support::utils;
use std::path::PathBuf;
use bitcoin::hashes::Hash;

#[tokio::test]
async fn test_view_function_debug() -> Result<()> {
    // Create runtime
    let wasm_path = PathBuf::from("./target/wasm32-unknown-unknown/release/metashrew_minimal.wasm");
    let mem_adapter = MemStoreAdapter::new();
    let mut runtime = MemStoreRuntime::load(wasm_path, mem_adapter)?;
    
    // Create a simple test block
    let test_block = super::block_builder::create_test_block(
        0,
        bitcoin::BlockHash::all_zeros(),
        b"test_block_0",
    );
    let block_bytes = utils::consensus_encode(&test_block)?;
    
    // Process the block
    {
        let mut context = runtime.context.lock().unwrap();
        context.block = block_bytes;
        context.height = 0;
    }
    
    runtime.run()?;
    
    // Check direct database access first
    let direct_blocktracker = {
        let adapter = &runtime.context.lock().unwrap().db;
        let key = b"/blocktracker".to_vec();
        let raw_value = adapter.get_immutable(&key)?.unwrap_or_default();
        println!("Direct database raw value: {} bytes: {:?}", raw_value.len(), raw_value);
        
        // Remove height annotation if present (last 4 bytes)
        if raw_value.len() >= 4 {
            let data_portion = raw_value[..raw_value.len()-4].to_vec();
            println!("Direct database data portion: {} bytes: {:?}", data_portion.len(), data_portion);
            data_portion
        } else {
            raw_value
        }
    };
    
    // Now test the view function
    let view_input = vec![]; // blocktracker view function takes no input
    let view_result = runtime.view("blocktracker".to_string(), &view_input, 0).await?;
    
    println!("View function result: {} bytes", view_result.len());
    if view_result.len() <= 100 {
        println!("View function data: {:?}", view_result);
    } else {
        println!("View function data (first 20 bytes): {:?}", &view_result[..20]);
        println!("View function data (last 20 bytes): {:?}", &view_result[view_result.len()-20..]);
    }
    
    // Compare results
    if view_result.len() <= 100 {
        assert_eq!(view_result, direct_blocktracker, "View function should match direct database access");
    } else {
        println!("WARNING: View function returned {} bytes, which seems incorrect", view_result.len());
        println!("Expected {} bytes from direct database access", direct_blocktracker.len());
    }
    
    Ok(())
}
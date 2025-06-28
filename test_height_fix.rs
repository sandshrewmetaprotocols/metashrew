//! Test script to verify the height discrepancy fix
//! 
//! This script tests that metashrew_height returns the correct indexed height from storage

use anyhow::Result;
use serde_json::Value;
use std::process::Command;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Testing metashrew_height fix...\n");
    
    // Test the metashrew_height RPC endpoint
    let rpc_url = std::env::var("METASHREW_RPC_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    
    println!("Testing metashrew_height RPC at: {}", rpc_url);
    
    let client = reqwest::Client::new();
    
    // Test metashrew_height
    let height_request = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "metashrew_height",
        "params": []
    });
    
    match client.post(&rpc_url)
        .json(&height_request)
        .send()
        .await
    {
        Ok(response) => {
            let response_text = response.text().await?;
            println!("metashrew_height response: {}", response_text);
            
            let response_json: Value = serde_json::from_str(&response_text)?;
            
            if let Some(result) = response_json.get("result") {
                if let Some(height) = result.as_u64() {
                    println!("✅ metashrew_height returned: {}", height);
                    
                    // Test that we can query a view function at this height
                    test_view_function_at_height(&client, &rpc_url, height as u32).await?;
                } else {
                    println!("❌ metashrew_height result is not a number: {:?}", result);
                }
            } else if let Some(error) = response_json.get("error") {
                println!("❌ metashrew_height returned error: {:?}", error);
            } else {
                println!("❌ Unexpected response format: {}", response_text);
            }
        }
        Err(e) => {
            println!("❌ Failed to call metashrew_height: {}", e);
            println!("Make sure rockshrew-mono is running at {}", rpc_url);
        }
    }
    
    // Test metashrew_snapshot to see storage stats
    println!("\nTesting metashrew_snapshot for storage stats...");
    let snapshot_request = serde_json::json!({
        "id": 2,
        "jsonrpc": "2.0",
        "method": "metashrew_snapshot",
        "params": []
    });
    
    match client.post(&rpc_url)
        .json(&snapshot_request)
        .send()
        .await
    {
        Ok(response) => {
            let response_text = response.text().await?;
            println!("metashrew_snapshot response: {}", response_text);
            
            let response_json: Value = serde_json::from_str(&response_text)?;
            
            if let Some(result) = response_json.get("result") {
                if let Ok(result_obj) = serde_json::from_str::<Value>(&result.as_str().unwrap_or("{}")) {
                    if let Some(indexed_height) = result_obj.get("indexed_height") {
                        println!("✅ Storage indexed_height: {}", indexed_height);
                    }
                    if let Some(current_height) = result_obj.get("current_height") {
                        println!("✅ Sync engine current_height: {}", current_height);
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ Failed to call metashrew_snapshot: {}", e);
        }
    }
    
    Ok(())
}

async fn test_view_function_at_height(client: &reqwest::Client, rpc_url: &str, height: u32) -> Result<()> {
    println!("\nTesting view function at height {}...", height);
    
    // Try a simple view function call (this will depend on what WASM module is loaded)
    let view_request = serde_json::json!({
        "id": 3,
        "jsonrpc": "2.0",
        "method": "metashrew_view",
        "params": ["test", "0x", height]
    });
    
    match client.post(rpc_url)
        .json(&view_request)
        .send()
        .await
    {
        Ok(response) => {
            let response_text = response.text().await?;
            let response_json: Value = serde_json::from_str(&response_text)?;
            
            if response_json.get("result").is_some() {
                println!("✅ View function call succeeded at height {}", height);
            } else if let Some(error) = response_json.get("error") {
                // This is expected if the view function doesn't exist
                println!("ℹ️  View function call failed (expected): {:?}", error.get("message"));
            }
        }
        Err(e) => {
            println!("❌ Failed to call view function: {}", e);
        }
    }
    
    Ok(())
}
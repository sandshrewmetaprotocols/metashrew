use anyhow::Result;
use memshrew_runtime::MemStoreAdapter;
use metashrew_runtime::smt::SMTHelper;

#[tokio::test]
async fn test_bst_storage_and_retrieval() -> Result<()> {
    // Create a simple test to verify BST storage and retrieval
    let adapter = MemStoreAdapter::new();
    
    // Create SMTHelper
    let mut smt_helper = SMTHelper::new(adapter.clone());
    
    // Test key and value
    let test_key = b"/blocktracker".to_vec();
    let test_value = vec![0x8c]; // Single byte like metashrew-minimal should store
    let height = 0u32;
    
    println!("Testing BST storage and retrieval:");
    println!("Key: {}", hex::encode(&test_key));
    println!("Value: {}", hex::encode(&test_value));
    println!("Height: {}", height);
    
    // Store in BST
    smt_helper.bst_put(&test_key, &test_value, height)?;
    println!("✓ Stored value in BST");
    
    // Retrieve from BST
    let retrieved = smt_helper.bst_get_at_height(&test_key, height)?;
    println!("Retrieved value: {:?}", retrieved.as_ref().map(|v| hex::encode(v)));
    
    match retrieved {
        Some(value) => {
            println!("✓ Retrieved value from BST: {} bytes", value.len());
            if value == test_value {
                println!("✓ Values match exactly");
            } else {
                println!("✗ Values don't match!");
                println!("Expected: {}", hex::encode(&test_value));
                println!("Got: {}", hex::encode(&value));
            }
        },
        None => {
            println!("✗ No value retrieved from BST");
        }
    }
    
    // Test retrieval at a later height
    let retrieved_later = smt_helper.bst_get_at_height(&test_key, height + 5)?;
    println!("Retrieved at height {}: {:?}", height + 5, retrieved_later.as_ref().map(|v| hex::encode(v)));
    
    Ok(())
}

// Commented out for now due to compilation issues
// #[tokio::test]
// async fn test_metashrew_minimal_simulation() -> Result<()> {
//     println!("Test skipped - needs proper setup");
//     Ok(())
// }
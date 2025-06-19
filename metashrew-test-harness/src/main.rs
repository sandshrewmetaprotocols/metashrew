use anyhow::Result;
use metashrew_test_harness::MetashrewTestHarness;
use memshrew_store::MemStore;

fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();

    // Path to the WASM binary
    let wasm_path = "target/wasm32-unknown-unknown/release/metashrew_minimal.wasm";

    // Create a new test harness
    let mut harness = MetashrewTestHarness::new(wasm_path, MemStore::new())?;

    // Process a few blocks
    harness.process_block(b"block 1")?;
    harness.process_block(b"block 2")?;
    harness.process_block(b"block 3")?;

    // Verify the chain length
    assert_eq!(harness.chain.len(), 3);

    // Verify the block content
    assert_eq!(harness.get_block_by_height(0), Some(&b"block 1".to_vec()));
    assert_eq!(harness.get_block_by_height(1), Some(&b"block 2".to_vec()));
    assert_eq!(harness.get_block_by_height(2), Some(&b"block 3".to_vec()));

    // Verify the SMT root
    let root = harness.smt.get_smt_root_at_height(2)?;
    println!("SMT root before reorg: {}", hex::encode(root));

    // Simulate a reorg
    harness.remote_chain.push(b"block 1".to_vec());
    harness.remote_chain.push(b"block 2 prime".to_vec());
    harness.remote_chain.push(b"block 3 prime".to_vec());
    harness.reorg()?;

    // Verify the chain length after reorg
    assert_eq!(harness.chain.len(), 3);

    // Verify the block content after reorg
    assert_eq!(harness.get_block_by_height(0), Some(&b"block 1".to_vec()));
    assert_eq!(harness.get_block_by_height(1), Some(&b"block 2 prime".to_vec()));
    assert_eq!(harness.get_block_by_height(2), Some(&b"block 3 prime".to_vec()));

    // Verify the SMT root after reorg
    let root_after_reorg = harness.smt.get_smt_root_at_height(2)?;
    println!("SMT root after reorg: {}", hex::encode(root_after_reorg));

    // Verify view function after reorg
    let view_result = harness.runtime.view("view_block_by_height".to_string(), &2u32.to_be_bytes().to_vec(), 2).await?;
    assert_eq!(view_result, b"block 3 prime".to_vec());

    println!("All tests passed!");

    Ok(())
}
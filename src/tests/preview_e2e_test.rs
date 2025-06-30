//! End-to-end test for the `metashrew_preview` functionality.
//!
//! This test verifies that the `execute_preview` function can correctly
//! execute a view function against a provided block without modifying
//! the underlying database state.

use crate::in_memory_adapters::{InMemoryBitcoinNode, InMemoryRuntime};
use crate::test_utils::{create_minimal_indexer_wasm, get_minimal_wasm_bytes};
use metashrew_sync::{PreviewCall, RuntimeAdapter};

#[tokio::test]
async fn test_preview_e2e() {
    // 1. Setup
    let wasm_bytes = get_minimal_wasm_bytes();
    create_minimal_indexer_wasm();
    let runtime = InMemoryRuntime::new(&wasm_bytes);
    let mut adapter = InMemoryRuntime::new_runtime_adapter(runtime);
    let node = InMemoryBitcoinNode::new();

    // 2. Create a test block
    let block = node.get_block(1);

    // 3. Define the preview call
    let call = PreviewCall {
        block_data: block.data.clone(),
        function_name: "getblock".to_string(),
        input_data: 1u32.to_le_bytes().to_vec(),
        height: 1,
    };

    // 4. Execute the preview
    let preview_result = adapter.execute_preview(call).await.unwrap();

    // 5. Verify the preview result
    assert_eq!(preview_result.data, block.data, "Preview should return the correct block data");

    // 6. Verify that the database state was not modified
    // Attempt to retrieve the block from the actual database; it should not exist.
    let view_result = adapter.execute_view(metashrew_sync::ViewCall {
        function_name: "getblock".to_string(),
        input_data: 1u32.to_le_bytes().to_vec(),
        height: 1,
    }).await;

    // The view call should fail because the block was never indexed.
    assert!(view_result.is_err(), "Database state should not be modified by a preview call");
}
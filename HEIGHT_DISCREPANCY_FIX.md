# Height Discrepancy Fix

## Problem Analysis

The `metashrew_height` RPC method is returning 879999 when the index is actually synced to 902200+. This is caused by a discrepancy between two different height tracking mechanisms:

1. **Sync Engine's `current_height`** (atomic variable): Tracks the "next block to process" (stored as `height + 1`)
2. **Storage's `indexed_height`** (in RocksDB): Tracks the "last successfully indexed block" (stored as actual height)

## Root Cause

In [`rockshrew-mono/src/main.rs:486-494`](crates/rockshrew-mono/src/main.rs:486-494), the `metashrew_height` implementation uses:

```rust
let current_height = state.current_height.load(Ordering::SeqCst);
let height = current_height.saturating_sub(1); // Same logic as sync engine
```

This relies on the sync engine's atomic variable instead of the actual indexed height in storage. The sync engine's `current_height` can become out of sync with the storage's `indexed_height` due to:

- Race conditions between height updates
- Failures between sync engine height update and storage height update
- Restart scenarios where the sync engine reinitializes but storage retains the correct state

## Solution

The `metashrew_height` RPC should return the **actual indexed height from storage**, not the sync engine's internal tracking variable.

## Implementation

### 1. Update the `metashrew_height` handler in rockshrew-mono

Change the implementation to query the storage adapter directly:

```rust
} else if body.method == "metashrew_height" {
    // Use storage adapter to get the actual indexed height
    match state.storage.read().await.get_indexed_height().await {
        Ok(indexed_height) => {
            Ok(HttpResponse::Ok().json(serde_json::json!({
                "id": body.id,
                "result": indexed_height,
                "jsonrpc": "2.0"
            })))
        }
        Err(err) => Ok(HttpResponse::Ok().json(JsonRpcError {
            id: body.id,
            error: JsonRpcErrorObject {
                code: -32000,
                message: format!("Failed to get indexed height: {}", err),
                data: None,
            },
            jsonrpc: "2.0".to_string(),
        }))
    }
}
```

### 2. Update sync engine implementations

Both sync engines should also use storage-based height for consistency:

**In [`sync.rs:768-771`](crates/rockshrew-sync/src/sync.rs:768-771):**
```rust
async fn metashrew_height(&self) -> SyncResult<u32> {
    let storage = self.storage.read().await;
    storage.get_indexed_height().await
}
```

**In [`snapshot_sync.rs:653-656`](crates/rockshrew-sync/src/snapshot_sync.rs:653-656):**
```rust
async fn metashrew_height(&self) -> SyncResult<u32> {
    let storage = self.storage.read().await;
    storage.get_indexed_height().await
}
```

### 3. Ensure consistent height tracking

The sync engines should continue to use `current_height` for internal coordination (next block to process), but all external APIs should report the storage-based indexed height.

## Benefits

1. **Accuracy**: `metashrew_height` will always return the actual indexed height
2. **Consistency**: All height queries will be consistent with the database state
3. **Reliability**: Eliminates race conditions between sync engine and storage updates
4. **Restart Safety**: Height reporting will be correct even after restarts

## Testing

After applying the fix:

1. Run the debug script to verify storage indexed height: `cargo run --bin debug_height_discrepancy -- /path/to/db`
2. Call `metashrew_height` RPC and verify it matches the storage indexed height
3. Verify that view functions work correctly with the reported height
4. Test restart scenarios to ensure height consistency

## Files to Modify

1. `crates/rockshrew-mono/src/main.rs` - Update `metashrew_height` handler
2. `crates/rockshrew-sync/src/sync.rs` - Update `metashrew_height` implementation  
3. `crates/rockshrew-sync/src/snapshot_sync.rs` - Update `metashrew_height` implementation
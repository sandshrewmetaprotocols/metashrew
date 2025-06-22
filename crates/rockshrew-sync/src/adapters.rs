//! Real adapter implementations for production use

use async_trait::async_trait;
use metashrew_runtime::{KeyValueStoreLike, MetashrewRuntime};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    AtomicBlockResult, PreviewCall, RuntimeAdapter, RuntimeStats, SyncError, SyncResult, ViewCall,
    ViewResult,
};

/// Real runtime adapter that wraps MetashrewRuntime
pub struct MetashrewRuntimeAdapter<T: KeyValueStoreLike + Clone + Send + Sync + 'static> {
    runtime: Arc<Mutex<MetashrewRuntime<T>>>,
}

impl<T: KeyValueStoreLike + Clone + Send + Sync + 'static> MetashrewRuntimeAdapter<T> {
    pub fn new(runtime: MetashrewRuntime<T>) -> Self {
        Self {
            runtime: Arc::new(Mutex::new(runtime)),
        }
    }

    pub fn from_arc(runtime: Arc<Mutex<MetashrewRuntime<T>>>) -> Self {
        Self { runtime }
    }
}

impl<T: KeyValueStoreLike + Clone + Send + Sync + 'static> Clone for MetashrewRuntimeAdapter<T> {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
        }
    }
}

#[async_trait]
impl<T: KeyValueStoreLike + Clone + Send + Sync + 'static> RuntimeAdapter
    for MetashrewRuntimeAdapter<T>
{
    async fn process_block(&mut self, height: u32, block_data: &[u8]) -> SyncResult<()> {
        let mut runtime = self.runtime.lock().await;

        // Set block data and height in the runtime context
        {
            let mut context = runtime
                .context
                .lock()
                .map_err(|e| SyncError::Runtime(format!("Failed to lock context: {}", e)))?;
            context.block = block_data.to_vec();
            context.height = height;
            context.db.set_height(height);
        }

        // Execute the runtime
        if let Err(e) = runtime.run() {
            // Try to refresh memory and retry once
            if let Ok(_) = runtime.refresh_memory() {
                runtime.run().map_err(|retry_e| {
                    SyncError::Runtime(format!("Runtime execution failed after retry: {}", retry_e))
                })?;
            } else {
                return Err(SyncError::Runtime(format!(
                    "Runtime execution failed: {}",
                    e
                )));
            }
        }

        Ok(())
    }

    async fn process_block_atomic(
        &mut self,
        height: u32,
        block_data: &[u8],
        block_hash: &[u8],
    ) -> SyncResult<AtomicBlockResult> {
        let mut runtime = self.runtime.lock().await;

        // Set block data and height in the runtime context
        {
            let mut context = runtime
                .context
                .lock()
                .map_err(|e| SyncError::Runtime(format!("Failed to lock context: {}", e)))?;
            context.block = block_data.to_vec();
            context.height = height;
            context.db.set_height(height);
        }

        // Execute the runtime to collect all database operations
        if let Err(e) = runtime.run() {
            // Try to refresh memory and retry once
            if let Ok(_) = runtime.refresh_memory() {
                runtime.run().map_err(|retry_e| {
                    SyncError::Runtime(format!("Runtime execution failed after retry: {}", retry_e))
                })?;
            } else {
                return Err(SyncError::Runtime(format!(
                    "Runtime execution failed: {}",
                    e
                )));
            }
        }

        // Get the state root after processing
        let state_root = runtime
            .calculate_state_root()
            .map_err(|e| SyncError::Runtime(format!("Failed to calculate state root: {}", e)))?;

        // Get the accumulated database operations as a batch
        let batch_data = runtime
            .get_accumulated_batch()
            .map_err(|e| SyncError::Runtime(format!("Failed to get database batch: {}", e)))?;

        Ok(AtomicBlockResult {
            state_root,
            batch_data,
            height,
            block_hash: block_hash.to_vec(),
        })
    }

    async fn execute_view(&self, call: ViewCall) -> SyncResult<ViewResult> {
        let runtime = self.runtime.lock().await;

        let result = runtime
            .view(call.function_name, &call.input_data, call.height)
            .await
            .map_err(|e| SyncError::ViewFunction(format!("View function failed: {}", e)))?;

        Ok(ViewResult { data: result })
    }

    async fn execute_preview(&self, call: PreviewCall) -> SyncResult<ViewResult> {
        let runtime = self.runtime.lock().await;

        let result = runtime
            .preview_async(
                &call.block_data,
                call.function_name,
                &call.input_data,
                call.height,
            )
            .await
            .map_err(|e| SyncError::ViewFunction(format!("Preview function failed: {}", e)))?;

        Ok(ViewResult { data: result })
    }

    async fn get_state_root(&self, height: u32) -> SyncResult<Vec<u8>> {
        // This is a placeholder implementation for the generic adapter
        // The actual implementation should query the SMT state root from storage
        // For now, return an error to indicate that state root is not available
        // This prevents false positives in block processing checks
        Err(SyncError::Runtime(format!(
            "State root not available for height {} in generic adapter",
            height
        )))
    }

    async fn refresh_memory(&mut self) -> SyncResult<()> {
        let mut runtime = self.runtime.lock().await;

        runtime
            .refresh_memory()
            .map_err(|e| SyncError::Runtime(format!("Memory refresh failed: {}", e)))?;

        Ok(())
    }

    async fn is_ready(&self) -> bool {
        // MetashrewRuntime is always ready once created
        true
    }

    async fn get_stats(&self) -> SyncResult<RuntimeStats> {
        let runtime = self.runtime.lock().await;

        // Get memory usage from WASM instance if available
        // Note: We can't easily access memory size due to mutability constraints
        // This would require restructuring the MetashrewRuntime to provide this info
        let memory_usage_bytes = 0; // Placeholder for now

        // Get current height as blocks processed
        let blocks_processed = {
            let context = runtime
                .context
                .lock()
                .map_err(|e| SyncError::Runtime(format!("Failed to lock context: {}", e)))?;
            context.height
        };

        Ok(RuntimeStats {
            memory_usage_bytes,
            blocks_processed,
            last_refresh_height: Some(blocks_processed),
        })
    }
}

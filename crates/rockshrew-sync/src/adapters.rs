//! Real adapter implementations for production use

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use metashrew_runtime::{MetashrewRuntime, KeyValueStoreLike};

use crate::{
    RuntimeAdapter, RuntimeStats, SyncError, SyncResult, ViewCall, ViewResult, PreviewCall,
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
impl<T: KeyValueStoreLike + Clone + Send + Sync + 'static> RuntimeAdapter for MetashrewRuntimeAdapter<T> {
    async fn process_block(&mut self, height: u32, block_data: &[u8]) -> SyncResult<()> {
        let mut runtime = self.runtime.lock().await;
        
        // Set block data and height in the runtime context
        {
            let mut context = runtime.context.lock()
                .map_err(|e| SyncError::Runtime(format!("Failed to lock context: {}", e)))?;
            context.block = block_data.to_vec();
            context.height = height;
            context.db.set_height(height);
        }
        
        // Execute the runtime
        if let Err(e) = runtime.run() {
            // Try to refresh memory and retry once
            if let Ok(_) = runtime.refresh_memory() {
                runtime.run()
                    .map_err(|retry_e| SyncError::Runtime(format!("Runtime execution failed after retry: {}", retry_e)))?;
            } else {
                return Err(SyncError::Runtime(format!("Runtime execution failed: {}", e)));
            }
        }
        
        Ok(())
    }
    
    async fn execute_view(&self, call: ViewCall) -> SyncResult<ViewResult> {
        let runtime = self.runtime.lock().await;
        
        let result = runtime.view(call.function_name, &call.input_data, call.height)
            .await
            .map_err(|e| SyncError::ViewFunction(format!("View function failed: {}", e)))?;
        
        Ok(ViewResult { data: result })
    }
    
    async fn execute_preview(&self, call: PreviewCall) -> SyncResult<ViewResult> {
        let runtime = self.runtime.lock().await;
        
        let result = runtime.preview_async(&call.block_data, call.function_name, &call.input_data, call.height)
            .await
            .map_err(|e| SyncError::ViewFunction(format!("Preview function failed: {}", e)))?;
        
        Ok(ViewResult { data: result })
    }
    
    async fn get_state_root(&self, height: u32) -> SyncResult<Vec<u8>> {
        // For now, generate a deterministic state root based on height
        // In a full implementation, this would query the actual SMT state root
        // from the runtime after block processing
        let mut state_root = vec![0u8; 32];
        let height_bytes = height.to_le_bytes();
        
        // Create a deterministic pattern based on height
        for i in 0..32 {
            state_root[i] = height_bytes[i % 4].wrapping_add((i as u8).wrapping_mul(7));
        }
        
        Ok(state_root)
    }
    
    async fn refresh_memory(&mut self) -> SyncResult<()> {
        let mut runtime = self.runtime.lock().await;
        
        runtime.refresh_memory()
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
            let context = runtime.context.lock()
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
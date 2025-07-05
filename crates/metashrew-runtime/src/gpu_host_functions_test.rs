//! Test module for GPU host functions
//!
//! This module provides basic tests to verify that the `__call_vulkan` and `__load_vulkan`
//! host functions are properly integrated into the metashrew runtime.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::traits::KeyValueStoreLike;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// Simple error type for testing
    #[derive(Debug)]
    struct TestError(String);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestError: {}", self.0)
        }
    }

    impl std::error::Error for TestError {}

    /// Simple in-memory storage for testing
    #[derive(Clone, Debug)]
    struct TestStorage {
        data: Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
    }

    impl TestStorage {
        fn new() -> Self {
            Self {
                data: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    impl KeyValueStoreLike for TestStorage {
        type Batch = TestBatch;
        type Error = TestError;

        fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
            let data = self.data.lock().unwrap();
            Ok(data.get(key.as_ref()).cloned())
        }

        fn get_immutable<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
            let data = self.data.lock().unwrap();
            Ok(data.get(key.as_ref()).cloned())
        }

        fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
        where
            K: AsRef<[u8]>,
            V: AsRef<[u8]>,
        {
            let mut data = self.data.lock().unwrap();
            data.insert(key.as_ref().to_vec(), value.as_ref().to_vec());
            Ok(())
        }

        fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
            let mut data = self.data.lock().unwrap();
            data.remove(key.as_ref());
            Ok(())
        }

        fn scan_prefix<K: AsRef<[u8]>>(
            &self,
            prefix: K,
        ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error> {
            let data = self.data.lock().unwrap();
            let prefix_bytes = prefix.as_ref();
            let mut results = Vec::new();
            for (key, value) in data.iter() {
                if key.starts_with(prefix_bytes) {
                    results.push((key.clone(), value.clone()));
                }
            }
            Ok(results)
        }

        fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
            let data = self.data.lock().unwrap();
            let keys: Vec<Vec<u8>> = data.keys().cloned().collect();
            Ok(Box::new(keys.into_iter()))
        }

        fn create_batch(&self) -> Self::Batch {
            TestBatch::new()
        }

        fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
            let mut data = self.data.lock().unwrap();
            for (key, value) in batch.operations {
                match value {
                    Some(v) => { data.insert(key, v); },
                    None => { data.remove(&key); },
                }
            }
            Ok(())
        }

        fn create_isolated_copy(&self) -> Self {
            let data = self.data.lock().unwrap();
            let new_storage = TestStorage::new();
            {
                let mut new_data = new_storage.data.lock().unwrap();
                *new_data = data.clone();
            }
            new_storage
        }

        fn track_kv_update(&mut self, _key: Vec<u8>, _value: Vec<u8>) {
            // No-op for test storage
        }
    }

    #[derive(Debug)]
    struct TestBatch {
        operations: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    }

    impl TestBatch {
        fn new() -> Self {
            Self {
                operations: Vec::new(),
            }
        }
    }

    impl Default for TestBatch {
        fn default() -> Self {
            Self::new()
        }
    }

    impl crate::traits::BatchLike for TestBatch {
        fn default() -> Self {
            Self::new()
        }

        fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
            self.operations.push((key.as_ref().to_vec(), Some(value.as_ref().to_vec())));
        }

        fn delete<K: AsRef<[u8]>>(&mut self, key: K) {
            self.operations.push((key.as_ref().to_vec(), None));
        }
    }

    /// Simple test WASM module that just has the basic structure
    const TEST_WASM: &str = r#"
        (module
            (import "env" "__call_vulkan" (func $__call_vulkan (param i32) (result i32)))
            (import "env" "__load_vulkan" (func $__load_vulkan (param i32)))
            (import "env" "__host_len" (func $__host_len (result i32)))
            (import "env" "__load_input" (func $__load_input (param i32)))
            (memory (export "memory") 1)
            
            ;; Simple test function
            (func (export "test_gpu_functions") (result i32)
                ;; Just return 1 to indicate success
                (i32.const 1)
            )
            
            (func (export "_start"))
        )
    "#;

    #[test]
    fn test_gpu_host_functions_integration() {
        // Create test storage
        let storage = TestStorage::new();
        
        // Compile test WASM module
        let wasm_bytes = wat::parse_str(TEST_WASM).expect("Failed to parse WAT");
        
        // Create runtime with test WASM
        let runtime = MetashrewRuntime::new(&wasm_bytes, storage, vec![])
            .expect("Failed to create runtime");
        
        // The runtime should be created successfully with GPU host functions
        assert!(!runtime.disable_wasmtime_log);
        
        // Test that the context has the GPU result data field
        {
            let context = runtime.context.lock().unwrap();
            assert!(context.gpu_result_data.is_none()); // Should start as None
        }
    }

    #[test]
    fn test_execute_gpu_work_function() {
        // Test the execute_gpu_work function directly
        let test_input = b"Hello GPU!";
        let result = MetashrewRuntime::<TestStorage>::execute_gpu_work(test_input)
            .expect("GPU work execution should succeed");
        
        // Should return a non-empty result
        assert!(!result.is_empty());
        
        // Parse the JSON result
        let result_str = String::from_utf8_lossy(&result);
        
        // Try to parse as VulkanExecutionResult JSON
        if let Ok(vulkan_result) = serde_json::from_str::<crate::vulkan_runtime::VulkanExecutionResult>(&result_str) {
            // Should be successful (either GPU or CPU fallback)
            assert!(vulkan_result.success);
            
            // Should contain information about the input size in the output data
            let output_str = String::from_utf8_lossy(&vulkan_result.output_data);
            assert!(output_str.contains("10 bytes input"));
        } else {
            // Fallback: check if it's the old format (for backward compatibility)
            assert!(result_str.contains("10 bytes input"));
        }
    }

    #[test]
    fn test_gpu_result_data_storage() {
        let storage = TestStorage::new();
        let wasm_bytes = wat::parse_str(TEST_WASM).expect("Failed to parse WAT");
        let runtime = MetashrewRuntime::new(&wasm_bytes, storage, vec![])
            .expect("Failed to create runtime");
        
        // Test storing GPU result data in context
        {
            let mut context = runtime.context.lock().unwrap();
            let test_data = b"GPU result data".to_vec();
            context.gpu_result_data = Some(test_data.clone());
            
            // Verify data was stored
            assert_eq!(context.gpu_result_data.as_ref().unwrap(), &test_data);
        }
        
        // Test taking GPU result data from context
        {
            let mut context = runtime.context.lock().unwrap();
            let retrieved_data = context.gpu_result_data.take();
            
            // Verify data was retrieved and context is now empty
            assert!(retrieved_data.is_some());
            assert_eq!(retrieved_data.unwrap(), b"GPU result data");
            assert!(context.gpu_result_data.is_none());
        }
    }
}
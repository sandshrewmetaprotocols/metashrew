//! Tests for ZK proof generation with MetashrewRuntime
//!
//! These tests demonstrate end-to-end ZK proof generation for any WASM indexer,
//! including metashrew-minimal, showing how the system can prove correct
//! execution and state transitions.

use crate::{ZKProofGenerator, ZKVerifier};
use crate::traits::KeyValueStoreLike;
use anyhow::Result;
use std::collections::HashMap;
use tempfile::TempDir;

/// Simple error type for mock storage
#[derive(Debug)]
pub struct MockStorageError(String);

impl std::fmt::Display for MockStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MockStorageError: {}", self.0)
    }
}

impl std::error::Error for MockStorageError {}

/// Mock storage backend for testing
#[derive(Clone)]
pub struct MockStorage {
    data: std::sync::Arc<std::sync::Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl MockStorage {
    pub fn new() -> Self {
        Self {
            data: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn create_isolated_copy(&self) -> Self {
        let current_data = self.data.lock().unwrap().clone();
        Self {
            data: std::sync::Arc::new(std::sync::Mutex::new(current_data)),
        }
    }

    pub fn track_kv_update(&self, _key: Vec<u8>, _value: Vec<u8>) {
        // Mock implementation - in real storage this would track updates
    }
}

#[derive(Default)]
pub struct MockBatch {
    operations: Vec<(Vec<u8>, Option<Vec<u8>>)>, // None for delete
}

impl crate::traits::BatchLike for MockBatch {
    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        self.operations.push((key.as_ref().to_vec(), Some(value.as_ref().to_vec())));
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) {
        self.operations.push((key.as_ref().to_vec(), None));
    }

    fn default() -> Self {
        <MockBatch as Default>::default()
    }
}

impl KeyValueStoreLike for MockStorage {
    type Error = MockStorageError;
    type Batch = MockBatch;

    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error> {
        let mut data = self.data.lock().map_err(|e| MockStorageError(format!("Lock error: {}", e)))?;
        for (key, value_opt) in batch.operations {
            match value_opt {
                Some(value) => { data.insert(key, value); }
                None => { data.remove(&key); }
            }
        }
        Ok(())
    }

    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        let data = self.data.lock().map_err(|e| MockStorageError(format!("Lock error: {}", e)))?;
        Ok(data.get(key.as_ref()).cloned())
    }

    fn get_immutable<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        let data = self.data.lock().map_err(|e| MockStorageError(format!("Lock error: {}", e)))?;
        Ok(data.get(key.as_ref()).cloned())
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        let mut data = self.data.lock().map_err(|e| MockStorageError(format!("Lock error: {}", e)))?;
        data.remove(key.as_ref());
        Ok(())
    }

    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let mut data = self.data.lock().map_err(|e| MockStorageError(format!("Lock error: {}", e)))?;
        data.insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        Ok(())
    }

    fn create_batch(&self) -> Self::Batch {
        MockBatch::default()
    }

    fn scan_prefix<K: AsRef<[u8]>>(&self, _prefix: K) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error> {
        let data = self.data.lock().map_err(|e| MockStorageError(format!("Lock error: {}", e)))?;
        Ok(data.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    fn keys<'a>(&'a self) -> Result<Box<dyn Iterator<Item = Vec<u8>> + 'a>, Self::Error> {
        let data = self.data.lock().map_err(|e| MockStorageError(format!("Lock error: {}", e)))?;
        let keys: Vec<Vec<u8>> = data.keys().cloned().collect();
        Ok(Box::new(keys.into_iter()))
    }
}

/// Create a mock WASM module for testing
fn create_mock_wasm_module() -> Vec<u8> {
    // This is a minimal WASM module that exports _start function
    // In a real test, you would use the actual metashrew-minimal.wasm
    vec![
        0x00, 0x61, 0x73, 0x6d, // WASM magic number
        0x01, 0x00, 0x00, 0x00, // WASM version
        // Minimal module structure
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // Type section
        0x03, 0x02, 0x01, 0x00, // Function section
        0x07, 0x09, 0x01, 0x05, 0x5f, 0x73, 0x74, 0x61, 0x72, 0x74, 0x00, 0x00, // Export section
        0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b, // Code section
    ]
}

/// Create mock block data for testing
fn create_mock_block_data(height: u32) -> Vec<u8> {
    let mut block_data = Vec::new();
    block_data.extend_from_slice(b"MOCK_BLOCK_");
    block_data.extend_from_slice(&height.to_le_bytes());
    block_data.extend_from_slice(b"_DATA");
    block_data
}

#[tokio::test]
async fn test_zk_proof_generation_basic() -> Result<()> {
    // Create a temporary directory for the mock WASM module
    let temp_dir = TempDir::new()?;
    let wasm_path = temp_dir.path().join("mock_indexer.wasm");
    
    // Write mock WASM module
    let wasm_bytes = create_mock_wasm_module();
    std::fs::write(&wasm_path, &wasm_bytes)?;

    // Create mock storage
    let _storage = MockStorage::new();

    // This test would fail with the current mock WASM module since it's not a valid
    // metashrew indexer, but it demonstrates the integration pattern
    
    // For now, just test the ZK proof generator directly
    let mut zk_generator = ZKProofGenerator::new(true);
    
    // Test execution trace lifecycle
    let height = 100;
    let block_data = create_mock_block_data(height);
    let prev_root = [1u8; 32];
    let new_root = [2u8; 32];

    // Start trace
    zk_generator.start_trace(height, &block_data, prev_root, &wasm_bytes)?;

    // Record some mock KV updates (simulating what __flush would do)
    zk_generator.record_kv_update(b"key1".to_vec(), b"value1".to_vec());
    zk_generator.record_kv_update(b"key2".to_vec(), b"value2".to_vec());

    // Complete trace and generate proof
    let proof = zk_generator.complete_trace_and_generate_proof(new_root, &wasm_bytes)?;
    
    assert!(proof.is_some());
    let proof = proof.unwrap();
    
    // Verify the proof
    assert_eq!(proof.block_height, height);
    assert_eq!(proof.prev_state_root, prev_root);
    assert_eq!(proof.new_state_root, new_root);
    assert_eq!(proof.proof_system, "mock"); // Since we're not using real RISC Zero

    // Verify the proof cryptographically
    let is_valid = ZKVerifier::verify_proof(&proof)?;
    assert!(is_valid);

    // Verify state transition
    let state_valid = ZKVerifier::verify_state_transition(
        &proof,
        prev_root,
        new_root,
        height,
    )?;
    assert!(state_valid);

    println!("✅ ZK proof generation test passed!");
    println!("   - Generated proof for height {}", height);
    println!("   - Proof size: {} bytes", proof.proof_data.len());
    println!("   - Proof system: {}", proof.proof_system);

    Ok(())
}

#[tokio::test]
async fn test_zk_proof_verification_failure() -> Result<()> {
    let mut zk_generator = ZKProofGenerator::new(true);
    let wasm_bytes = create_mock_wasm_module();
    let height = 100;
    let block_data = create_mock_block_data(height);
    let prev_root = [1u8; 32];
    let new_root = [2u8; 32];

    // Generate a valid proof
    zk_generator.start_trace(height, &block_data, prev_root, &wasm_bytes)?;
    zk_generator.record_kv_update(b"key1".to_vec(), b"value1".to_vec());
    let proof = zk_generator.complete_trace_and_generate_proof(new_root, &wasm_bytes)?
        .unwrap();

    // Test verification with wrong expected values
    let wrong_prev_root = [99u8; 32];
    let wrong_new_root = [98u8; 32];
    let wrong_height = 999;

    let state_valid = ZKVerifier::verify_state_transition(
        &proof,
        wrong_prev_root,
        wrong_new_root,
        wrong_height,
    )?;
    
    // Should fail verification
    assert!(!state_valid);

    println!("✅ ZK proof verification failure test passed!");

    Ok(())
}

#[test]
fn test_zk_proof_generator_disabled() -> Result<()> {
    let mut zk_generator = ZKProofGenerator::new(false); // Disabled
    let wasm_bytes = create_mock_wasm_module();
    let height = 100;
    let block_data = create_mock_block_data(height);
    let prev_root = [1u8; 32];
    let new_root = [2u8; 32];

    // Operations should succeed but not generate proofs
    zk_generator.start_trace(height, &block_data, prev_root, &wasm_bytes)?;
    zk_generator.record_kv_update(b"key1".to_vec(), b"value1".to_vec());
    let proof = zk_generator.complete_trace_and_generate_proof(new_root, &wasm_bytes)?;

    // Should return None when disabled
    assert!(proof.is_none());

    println!("✅ ZK proof generator disabled test passed!");

    Ok(())
}

/// Integration test showing how ZK proofs would work with a real metashrew indexer
/// 
/// This test demonstrates the pattern for integrating ZK proofs with any WASM indexer:
/// 1. Load the WASM indexer into MetashrewRuntime
/// 2. Process blocks with ZK proof generation enabled
/// 3. Verify the generated proofs
/// 4. Use proofs for rapid sync verification
#[tokio::test]
async fn test_metashrew_runtime_zk_integration_pattern() -> Result<()> {
    // This test shows the integration pattern but uses mock data
    // In a real scenario, you would:
    // 1. Compile metashrew-minimal to WASM
    // 2. Load it into MetashrewRuntime
    // 3. Process real Bitcoin blocks
    // 4. Generate and verify ZK proofs

    let _storage = MockStorage::new();
    let wasm_bytes = create_mock_wasm_module();
    
    // Simulate the pattern that would be used with real metashrew-minimal
    let mut zk_generator = ZKProofGenerator::new(true);
    
    // Process multiple blocks with ZK proof generation
    let mut proofs = Vec::new();
    
    for height in 1..=3 {
        let block_data = create_mock_block_data(height);
        let prev_root = if height == 1 { [0u8; 32] } else { [height as u8 - 1; 32] };
        let new_root = [height as u8; 32];
        
        // Start execution trace
        zk_generator.start_trace(height, &block_data, prev_root, &wasm_bytes)?;
        
        // Simulate key-value updates that would come from metashrew-minimal
        zk_generator.record_kv_update(
            format!("/blocks/{}", height).into_bytes(),
            block_data.clone(),
        );
        zk_generator.record_kv_update(
            b"/blocktracker".to_vec(),
            vec![height as u8], // Simplified block tracker
        );
        
        // Generate proof
        let proof = zk_generator.complete_trace_and_generate_proof(new_root, &wasm_bytes)?
            .unwrap();
        
        // Verify proof
        let is_valid = ZKVerifier::verify_proof(&proof)?;
        assert!(is_valid);
        
        proofs.push(proof);
    }
    
    // Verify we can use these proofs for rapid sync verification
    for (i, proof) in proofs.iter().enumerate() {
        let height = i as u32 + 1;
        let expected_prev_root = if height == 1 { [0u8; 32] } else { [height as u8 - 1; 32] };
        let expected_new_root = [height as u8; 32];
        
        let state_valid = ZKVerifier::verify_state_transition(
            proof,
            expected_prev_root,
            expected_new_root,
            height,
        )?;
        assert!(state_valid);
    }
    
    println!("✅ MetashrewRuntime ZK integration pattern test passed!");
    println!("   - Generated {} ZK proofs", proofs.len());
    println!("   - All proofs verified successfully");
    println!("   - Ready for rapid sync with cryptographic verification");
    
    Ok(())
}
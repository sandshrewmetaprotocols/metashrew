//! View functionality for Metashrew.
//!
//! This module provides functionality for creating view functions in Metashrew indexer programs.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// A trait for types that can be used as view functions in Metashrew
pub trait ViewFunction<Input, Output> {
    /// Execute the view function with the given input
    fn execute(&self, input: Input) -> Result<Output>;
}

/// A trait for types that can be used as view functions with Protocol Buffer messages
pub trait ProtoViewFunction<Input, Output>
where
    Input: protobuf::Message,
    Output: protobuf::Message,
{
    /// Execute the view function with the given input
    fn execute_proto(&self, input: Input) -> Result<Output>;
}

/// A struct for building Metashrew view functions
pub struct MetashrewView<T> {
    view: T,
}

impl<T> MetashrewView<T> {
    /// Create a new MetashrewView
    pub fn new(view: T) -> Self {
        Self { view }
    }
}

impl<T, I, O> MetashrewView<T>
where
    T: ViewFunction<I, O>,
    I: for<'de> Deserialize<'de>,
    O: Serialize,
{
    /// Process a view request
    pub fn process_view(&self) -> Result<Vec<u8>> {
        // Load the input data
        let (_height, input_bytes) = crate::host::load_input()?;
        
        // Deserialize the input
        let input: I = serde_json::from_slice(&input_bytes)
            .map_err(|e| anyhow!("Failed to deserialize input: {}", e))?;
        
        // Execute the view function
        let output = self.view.execute(input)?;
        
        // Serialize the output
        let output_bytes = serde_json::to_vec(&output)
            .map_err(|e| anyhow!("Failed to serialize output: {}", e))?;
        
        Ok(output_bytes)
    }
}

/// A struct for building Metashrew view functions with Protocol Buffer messages
pub struct MetashrewProtoView<T> {
    view: T,
}

impl<T> MetashrewProtoView<T> {
    /// Create a new MetashrewProtoView
    pub fn new(view: T) -> Self {
        Self { view }
    }
}

impl<T, I, O> MetashrewProtoView<T>
where
    T: ProtoViewFunction<I, O>,
    I: protobuf::Message,
    O: protobuf::Message,
{
    /// Process a view request
    pub fn process_view(&self) -> Result<Vec<u8>> {
        // Load the input data
        let (_height, input_bytes) = crate::host::load_input()?;
        
        // Deserialize the input
        let input = I::parse_from_bytes(&input_bytes)
            .map_err(|e| anyhow!("Failed to deserialize input: {}", e))?;
        
        // Execute the view function
        let output = self.view.execute_proto(input)?;
        
        // Serialize the output
        let output_bytes = output.write_to_bytes()
            .map_err(|e| anyhow!("Failed to serialize output: {}", e))?;
        
        Ok(output_bytes)
    }
}

/// A helper function to return a view result
pub fn return_view_result(result: &[u8]) -> i32 {
    let len = result.len() as u32;
    let mut buffer = Vec::with_capacity(4 + result.len());
    buffer.extend_from_slice(&len.to_le_bytes());
    buffer.extend_from_slice(result);
    
    buffer.as_ptr() as i32
}

/// A helper function to parse view input
pub fn parse_view_input<T: for<'de> Deserialize<'de>>() -> Result<T> {
    let (_height, input_bytes) = crate::host::load_input()?;
    
    serde_json::from_slice(&input_bytes)
        .map_err(|e| anyhow!("Failed to deserialize input: {}", e))
}

/// A helper function to parse Protocol Buffer view input
pub fn parse_proto_view_input<T: protobuf::Message>() -> Result<T> {
    let (_height, input_bytes) = crate::host::load_input()?;
    
    T::parse_from_bytes(&input_bytes)
        .map_err(|e| anyhow!("Failed to deserialize input: {}", e))
}
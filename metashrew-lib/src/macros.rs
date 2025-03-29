//! Macros for Metashrew.
//!
//! This module provides macros for building Metashrew indexer programs.

/// Define a Metashrew indexer program.
///
/// This macro generates the necessary exports for a Metashrew indexer program,
/// including the `_start` function and any view functions.
///
/// # Example
///
/// ```
/// use metashrew_lib::metashrew_indexer;
/// use metashrew_lib::indexer::{Indexer, KeyValueStore};
/// use anyhow::Result;
///
/// struct MyIndexer {
///     store: KeyValueStore,
/// }
///
/// impl Indexer for MyIndexer {
///     fn index_block(&mut self, height: u32, block: &[u8]) -> Result<()> {
///         // Process the block and update the store
///         self.store.set(b"last_height".to_vec(), height.to_le_bytes().to_vec());
///         Ok(())
///     }
///
///     fn flush(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
///         Ok(self.store.pairs())
///     }
/// }
///
/// impl MyIndexer {
///     fn get_last_height(&self, _input: Vec<u8>) -> Result<Vec<u8>> {
///         // Return the last processed height
///         if let Some(height) = self.store.get(b"last_height") {
///             Ok(height.clone())
///         } else {
///             Ok(0u32.to_le_bytes().to_vec())
///         }
///     }
/// }
///
/// metashrew_indexer! {
///     struct MyIndexerProgram {
///         indexer: MyIndexer,
///         views: {
///             "get_last_height" => get_last_height(Vec<u8>) -> Vec<u8>,
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! metashrew_indexer {
    (
        struct $name:ident {
            indexer: $indexer_type:ty,
            views: {
                $($view_name:expr => $view_method:ident($input_type:ty) -> $output_type:ty),* $(,)?
            }
        }
    ) => {
        static mut INDEXER_INSTANCE: Option<$crate::MetashrewIndexer<$indexer_type>> = None;

        #[no_mangle]
        pub extern "C" fn _start() {
            unsafe {
                if INDEXER_INSTANCE.is_none() {
                    let indexer = <$indexer_type>::default();
                    INDEXER_INSTANCE = Some($crate::MetashrewIndexer::new(indexer));
                }

                if let Some(indexer) = &mut INDEXER_INSTANCE {
                    if let Err(e) = indexer.process_block() {
                        $crate::host::log(&format!("Error processing block: {}", e));
                    }
                }
            }
        }

        $(
            #[no_mangle]
            pub extern "C" fn $view_method() -> i32 {
                unsafe {
                    if INDEXER_INSTANCE.is_none() {
                        let indexer = <$indexer_type>::default();
                        INDEXER_INSTANCE = Some($crate::MetashrewIndexer::new(indexer));
                    }

                    if let Some(indexer) = &INDEXER_INSTANCE {
                        match $crate::view::parse_view_input::<$input_type>() {
                            Ok(input) => {
                                match indexer.indexer.$view_method(input) {
                                    Ok(output) => $crate::view::return_view_result(&output),
                                    Err(e) => {
                                        $crate::host::log(&format!("Error executing view function: {}", e));
                                        $crate::view::return_view_result(&[])
                                    }
                                }
                            },
                            Err(e) => {
                                $crate::host::log(&format!("Error parsing view input: {}", e));
                                $crate::view::return_view_result(&[])
                            }
                        }
                    } else {
                        $crate::view::return_view_result(&[])
                    }
                }
            }
        )*
    };
}

/// Define a Metashrew view function.
///
/// This macro generates a view function that can be exported from a Metashrew indexer program.
///
/// # Example
///
/// ```
/// use metashrew_lib::metashrew_view;
/// use serde::{Deserialize, Serialize};
/// use anyhow::Result;
///
/// #[derive(Deserialize)]
/// struct GetBalanceInput {
///     address: String,
/// }
///
/// #[derive(Serialize)]
/// struct GetBalanceOutput {
///     balance: u64,
/// }
///
/// metashrew_view! {
///     fn get_balance(input: GetBalanceInput) -> Result<GetBalanceOutput> {
///         // Query the database and return the balance
///         Ok(GetBalanceOutput { balance: 100 })
///     }
/// }
/// ```
#[macro_export]
macro_rules! metashrew_view {
    (
        fn $name:ident($input:ident: $input_type:ty) -> Result<$output_type:ty> $body:block
    ) => {
        #[no_mangle]
        pub extern "C" fn $name() -> i32 {
            match $crate::view::parse_view_input::<$input_type>() {
                Ok($input) => {
                    match (|| -> anyhow::Result<$output_type> $body)() {
                        Ok(output) => {
                            match serde_json::to_vec(&output) {
                                Ok(bytes) => $crate::view::return_view_result(&bytes),
                                Err(e) => {
                                    $crate::host::log(&format!("Error serializing output: {}", e));
                                    $crate::view::return_view_result(&[])
                                }
                            }
                        },
                        Err(e) => {
                            $crate::host::log(&format!("Error executing view function: {}", e));
                            $crate::view::return_view_result(&[])
                        }
                    }
                },
                Err(e) => {
                    $crate::host::log(&format!("Error parsing view input: {}", e));
                    $crate::view::return_view_result(&[])
                }
            }
        }
    };
}

/// Define a Metashrew program with Protocol Buffer schema for requests and responses.
///
/// This macro generates a Metashrew program with view functions that use Protocol Buffer
/// messages for requests and responses.
///
/// # Example
///
/// ```
/// use metashrew_lib::declare_indexer;
/// use metashrew_lib::indexer::{Indexer, KeyValueStore};
/// use anyhow::Result;
/// use protobuf::Message;
///
/// // Define Protocol Buffer messages
/// #[derive(Clone, PartialEq, Message)]
/// pub struct GetBalanceRequest {
///     #[prost(string, tag = "1")]
///     pub address: String,
/// }
///
/// #[derive(Clone, PartialEq, Message)]
/// pub struct GetBalanceResponse {
///     #[prost(uint64, tag = "1")]
///     pub balance: u64,
/// }
///
/// struct MyIndexer {
///     store: KeyValueStore,
/// }
///
/// impl Indexer for MyIndexer {
///     fn index_block(&mut self, height: u32, block: &[u8]) -> Result<()> {
///         // Process the block and update the store
///         Ok(())
///     }
///
///     fn flush(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
///         Ok(self.store.pairs())
///     }
/// }
///
/// declare_indexer! {
///     struct MyProtoProgram {
///         indexer: MyIndexer,
///         views: {
///             "get_balance" => {
///                 fn get_balance(&self, request: GetBalanceRequest) -> Result<GetBalanceResponse> {
///                     // Query the database and return the balance
///                     Ok(GetBalanceResponse { balance: 100 })
///                 }
///             }
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! declare_indexer {
    (
        struct $name:ident {
            indexer: $indexer_type:ty,
            views: {
                $($view_name:expr => {
                    fn $view_method:ident(&self, $request_param:ident: $request_type:ty) -> Result<$response_type:ty> $body:block
                }),* $(,)?
            }
        }
    ) => {
        static mut INDEXER_INSTANCE: Option<$crate::MetashrewIndexer<$indexer_type>> = None;

        #[no_mangle]
        pub extern "C" fn _start() {
            unsafe {
                if INDEXER_INSTANCE.is_none() {
                    let indexer = <$indexer_type>::default();
                    INDEXER_INSTANCE = Some($crate::MetashrewIndexer::new(indexer));
                }

                if let Some(indexer) = &mut INDEXER_INSTANCE {
                    if let Err(e) = indexer.process_block() {
                        $crate::host::log(&format!("Error processing block: {}", e));
                    }
                }
            }
        }

        $(
            #[no_mangle]
            pub extern "C" fn $view_method() -> i32 {
                unsafe {
                    if INDEXER_INSTANCE.is_none() {
                        let indexer = <$indexer_type>::default();
                        INDEXER_INSTANCE = Some($crate::MetashrewIndexer::new(indexer));
                    }

                    if let Some(indexer) = &INDEXER_INSTANCE {
                        let (_height, input_bytes) = match $crate::host::load_input() {
                            Ok(input) => input,
                            Err(e) => {
                                $crate::host::log(&format!("Error loading input: {}", e));
                                return $crate::view::return_view_result(&[]);
                            }
                        };

                        let $request_param = match <$request_type>::parse_from_bytes(&input_bytes) {
                            Ok(req) => req,
                            Err(e) => {
                                $crate::host::log(&format!("Error parsing request: {}", e));
                                return $crate::view::return_view_result(&[]);
                            }
                        };

                        match (|$request_param: $request_type| -> anyhow::Result<$response_type> {
                            indexer.indexer.$view_method($request_param)
                        })($request_param) {
                            Ok(response) => {
                                match response.write_to_bytes() {
                                    Ok(bytes) => $crate::view::return_view_result(&bytes),
                                    Err(e) => {
                                        $crate::host::log(&format!("Error serializing response: {}", e));
                                        $crate::view::return_view_result(&[])
                                    }
                                }
                            },
                            Err(e) => {
                                $crate::host::log(&format!("Error executing view function: {}", e));
                                $crate::view::return_view_result(&[])
                            }
                        }
                    } else {
                        $crate::view::return_view_result(&[])
                    }
                }
            }
        )*

        // Implement the ViewFunction and ProtoViewFunction traits for the indexer
        $(
            impl $crate::view::ProtoViewFunction<$request_type, $response_type> for $indexer_type {
                fn execute_proto(&self, input: $request_type) -> anyhow::Result<$response_type> {
                    self.$view_method(input)
                }
            }
        )*
    };
}

/// Define a native binary for a Metashrew indexer program.
///
/// This macro generates a native binary that can run a Metashrew indexer program
/// without requiring a WASM VM.
///
/// # Example
///
/// ```
/// use metashrew_lib::native_binary;
/// use metashrew_lib::indexer::{Indexer, KeyValueStore, NativeIndexer};
/// use anyhow::Result;
/// use std::collections::HashMap;
///
/// struct MyIndexer {
///     store: KeyValueStore,
/// }
///
/// impl Indexer for MyIndexer {
///     fn index_block(&mut self, height: u32, block: &[u8]) -> Result<()> {
///         // Process the block and update the store
///         Ok(())
///     }
///
///     fn flush(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
///         Ok(self.store.pairs())
///     }
/// }
///
/// impl Default for MyIndexer {
///     fn default() -> Self {
///         Self {
///             store: KeyValueStore::new(),
///         }
///     }
/// }
///
/// impl Clone for MyIndexer {
///     fn clone(&self) -> Self {
///         Self {
///             store: KeyValueStore::new(),
///         }
///     }
/// }
///
/// impl NativeIndexer for MyIndexer {
///     fn view_functions(&self) -> HashMap<String, Box<dyn metashrew_lib::indexer::ViewFunctionWrapper>> {
///         let mut map = HashMap::new();
///         // Add view functions here
///         map
///     }
/// }
///
/// native_binary! {
///     indexer: MyIndexer,
///     name: "my-indexer",
///     version: "0.1.0",
///     about: "My custom indexer",
/// }
/// ```
#[cfg(feature = "native")]
#[macro_export]
macro_rules! native_binary {
    (
        indexer: $indexer_type:ty,
        name: $name:expr,
        version: $version:expr,
        about: $about:expr,
    ) => {
        #[tokio::main]
        async fn main() -> anyhow::Result<()> {
            use $crate::native::{NativeRuntimeArgs, NativeRuntimeServer};
            use clap::Parser;
            use env_logger;
            
            // Initialize the logger
            env_logger::init();
            
            // Parse command line arguments
            let args = NativeRuntimeArgs::parse();
            
            // Create the indexer
            let indexer = <$indexer_type>::default();
            
            // Create the server
            let server = NativeRuntimeServer::new(indexer, args);
            
            // Run the server
            server.run().await?;
            
            Ok(())
        }
    };
}
# Architecture Overview

This document provides a comprehensive overview of the Metashrew architecture, including its components, data flow, and design patterns.

## System Components

Metashrew consists of several key components that work together to provide a flexible and powerful indexing framework:

### 1. Core Runtime (metashrew-runtime)

The core runtime is responsible for executing WASM modules and providing the host interface:

- **WASM Execution Environment**: Based on Wasmtime, configured for deterministic execution
- **Host Functions**: Provides functions for WASM modules to interact with the host system
- **Memory Management**: Handles memory allocation and data transfer between host and WASM
- **Error Handling**: Manages errors and exceptions during WASM execution

### 2. RocksDB Adapter (rockshrew-runtime)

The RocksDB adapter implements the key-value store interface for RocksDB:

- **Database Operations**: Handles read/write operations to RocksDB
- **Batch Processing**: Supports atomic batch operations
- **Labeled Namespaces**: Manages database namespaces with labels
- **Height-Indexed BST**: Implements efficient historical state querying

### 3. Indexer (rockshrew)

The indexer is responsible for fetching blocks and executing WASM modules:

- **Block Fetching**: Retrieves blocks from Bitcoin Core
- **WASM Module Loading**: Loads and initializes WASM modules
- **Block Processing**: Executes WASM modules for each block
- **Chain Reorganization Handling**: Detects and handles chain reorganizations

### 4. View Layer (rockshrew-view)

The view layer exposes a JSON-RPC API for querying indexed data:

- **JSON-RPC Server**: Implements the JSON-RPC 2.0 protocol
- **View Function Execution**: Executes view functions from WASM modules
- **Query Routing**: Routes queries to appropriate handlers
- **Result Formatting**: Formats query results according to JSON-RPC specification

### 5. Combined Binary (rockshrew-mono)

The combined binary integrates the indexer and view layer in a single process:

- **Unified Configuration**: Single configuration for both indexing and querying
- **Shared Resources**: Efficient resource sharing between components
- **Simplified Deployment**: Easier deployment and management
- **Improved Stability**: Avoids inter-process communication issues

### 6. Helper Libraries

Helper libraries provide utilities and abstractions for building indexers:

- **metashrew-core**: Core abstractions and macros for indexer development
- **metashrew-support**: Utilities for working with Bitcoin data structures
- **dynamodb-runtime**: Alternative implementation using DynamoDB

## Data Flow

The data flow in Metashrew follows a clear path from block retrieval to query processing:

### 1. Block Processing Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│             │     │             │     │             │     │             │
│  Bitcoin    │────►│  Metashrew  │────►│  WASM       │────►│  Database   │
│  Node       │     │  Indexer    │     │  Module     │     │  Updates    │
│             │     │             │     │             │     │             │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
```

1. **Block Retrieval**: The indexer fetches blocks from Bitcoin Core via RPC
2. **Block Preparation**: The block is prepared for processing (serialization, height annotation)
3. **WASM Execution**: The WASM module processes the block
4. **Database Updates**: Changes are written to the database in an atomic batch

### 2. Query Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│             │     │             │     │             │     │             │
│  Client     │────►│  JSON-RPC   │────►│  View       │────►│  Database   │
│  Application│     │  Server     │     │  Function   │     │  Query      │
│             │     │             │     │             │     │             │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
```

1. **Client Request**: A client sends a JSON-RPC request
2. **Request Parsing**: The server parses and validates the request
3. **View Function Execution**: The appropriate view function is executed
4. **Database Query**: The view function queries the database
5. **Response Formatting**: Results are formatted and returned to the client

### 3. Chain Reorganization Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│             │     │             │     │             │     │             │
│  Detect     │────►│  Identify   │────►│  Rollback   │────►│  Reprocess  │
│  Divergence │     │  Affected   │     │  State      │     │  New Blocks │
│             │     │  Keys       │     │  Changes    │     │             │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
```

1. **Divergence Detection**: The system detects a chain reorganization
2. **Key Identification**: Affected keys are identified
3. **State Rollback**: State changes are rolled back to the fork point
4. **Block Reprocessing**: New blocks are processed from the fork point

## Design Patterns

Metashrew employs several key design patterns that enable its flexibility and robustness:

### 1. WebAssembly Host Interface Pattern

The WebAssembly host interface defines a contract between Metashrew and WASM modules:

```
┌─────────────────────┐      ┌─────────────────────┐
│                     │      │                     │
│  WASM Module        │      │  Metashrew Host     │
│  (e.g., Indexer)    │◄────►│                     │
│                     │      │                     │
└─────────────────────┘      └─────────────────────┘
         │                              │
         │ Exports                      │ Provides
         │ - _start()                   │ - __host_len()
         │ - view functions             │ - __load_input()
         │                              │ - __log()
         │                              │ - __flush()
         │                              │ - __get()
         │                              │ - __get_len()
```

This pattern enables language-agnostic communication between the host and any WASM module that implements the interface.

### 2. Append-Only Database Pattern

The append-only database pattern ensures data integrity and enables historical queries:

- **Versioned Keys**: Each key maintains a list of values, indexed by position
- **Height Annotation**: Values are annotated with the block height they were created at
- **Length Tracking**: Special length keys track the number of values for each key
- **Update Tracking**: For each block height, a list of updated keys is maintained

### 3. Actor Model Pattern

Metashrew uses the actor model for concurrent processing:

- **Tokio Runtime**: Provides the asynchronous execution environment
- **Mutex-Protected State**: Ensures thread safety for shared state
- **Message Passing**: Components communicate through message passing
- **Task Isolation**: Each component runs in isolation, improving fault tolerance

### 4. Dependency Injection Pattern

The dependency injection pattern enables flexible storage backends:

```rust
pub trait KeyValueStoreLike {
    type Error: std::fmt::Debug;
    type Batch: BatchLike;
    fn write(&mut self, batch: Self::Batch) -> Result<(), Self::Error>;
    fn get<K: AsRef<[u8]>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Self::Error>;
    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error>;
    fn put<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>;
}
```

This pattern abstracts the storage interface from its implementation, allowing for different storage backends.

### 5. Command Pattern for RPC

The JSON-RPC interface uses the command pattern:

- **Request Encapsulation**: Each RPC request is encapsulated as a command object
- **Method Routing**: Commands are routed to the appropriate handler
- **Parameter Validation**: Parameters are validated before execution
- **Result Formatting**: Results are formatted according to the JSON-RPC specification

## Conclusion

The Metashrew architecture combines WebAssembly, append-only storage, and modular design to create a powerful framework for Bitcoin indexing and metaprotocol development. Its components work together to provide a flexible, efficient, and deterministic platform for processing blockchain data.

By understanding the architecture, developers can leverage Metashrew's capabilities to build sophisticated applications on Bitcoin without having to reinvent the wheel for core indexing functionality.
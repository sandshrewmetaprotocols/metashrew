# Metashrew System Patterns

## Core Architectural Patterns

Metashrew's architecture is built around several key design patterns that enable its flexibility and extensibility.

### 1. WebAssembly Host Interface Pattern

The core of Metashrew is its WebAssembly host interface, which defines a contract between the host (Metashrew) and the guest (WASM modules):

```
┌─────────────────────┐      ┌─────────────────────┐
│                     │      │                     │
│  WASM Module        │      │  Metashrew Host     │
│  (e.g., ALKANES-RS) │◄────►│                     │
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

#### Host Functions

1. **`__host_len()`**: Returns the length of the input data (block height + serialized block).
2. **`__load_input(ptr: i32)`**: Loads input data into WASM memory at the specified pointer.
3. **`__log(ptr: i32)`**: Writes UTF-8 encoded text to stdout for debugging.
4. **`__flush(ptr: i32)`**: Commits key-value pairs to the database.
5. **`__get(key_ptr: i32, value_ptr: i32)`**: Reads a value for a key from the database.
6. **`__get_len(ptr: i32)`**: Gets the length of a value for a key in the database.

#### Guest Functions

1. **`_start()`**: Main entry point for block processing.
2. **Custom view functions**: Named exports for querying indexed data.

#### Memory Layout

Data passed between host and guest follows AssemblyScript's ArrayBuffer memory layout:
- 4 bytes for length (little-endian u32)
- Followed by actual data bytes

This pattern enables language-agnostic communication between the host and any WASM module that implements the interface.

### 2. Height-Indexed Binary Search Tree Pattern

Metashrew uses a height-indexed binary search tree pattern for efficient state lookup and verification:

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│    Key      │────►│ Value + Height │   │    Key      │────►│ Value + Height │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
       │                                        │
       │                                        │
       ▼                                        ▼
┌─────────────┐                          ┌─────────────┐
│ Height-BST  │────►│ State at Height │  │ SMT Root    │────►│ State Root Hash │
└─────────────┘     └─────────────────┘  └─────────────┘     └─────────────────┘
```

Key characteristics:

1. **Height-Indexed BST**: Binary Search Tree indexed by height for efficient historical state lookup.
2. **Height Annotation**: Values are annotated with the block height they were created at.
3. **Sparse Merkle Tree**: SMT structure produces a state root at any block height.
4. **Binary Search Algorithm**: Finding the state at a specific height uses an efficient binary search algorithm.
5. **Key Prefixes**: Uses specific prefixes for different types of keys:
   - `bst:` for BST keys
   - `bst:height:` for height index keys
   - `smt:root:` for SMT root keys

Benefits:

1. **Historical Queries**: Enables querying the state at any historical block height with improved efficiency.
2. **Chain Reorganization Handling**: Simplifies rolling back state changes during reorgs.
3. **Audit Trail**: Provides a complete history of state changes with cryptographic verification.
4. **Consistency**: Ensures consistent state across parallel indexers.
5. **Verification**: State roots enable cryptographic verification of database state.
6. **Efficiency**: Binary search improves lookup performance for historical states from O(n) to O(log n).
7. **Snapshot Integration**: Enables efficient snapshot creation by tracking key-value changes at specific heights.

### 3. Actor Model Pattern

Metashrew uses the actor model for concurrent processing:

```
┌─────────────┐     ┌─────────────┐
│             │     │             │
│  Indexer    │────►│  Runtime    │
│  Actor      │     │  Actor      │
│             │     │             │
└─────────────┘     └─────────────┘
      │                   │
      │                   │
      ▼                   ▼
┌─────────────┐     ┌─────────────┐
│             │     │             │
│  RPC Server │     │  Database   │
│  Actor      │     │  Actor      │
│             │     │             │
└─────────────┘     └─────────────┘
```

Key components:

1. **Tokio Runtime**: Provides the asynchronous execution environment.
2. **Mutex-Protected State**: Ensures thread safety for shared state.
3. **Message Passing**: Components communicate through message passing rather than shared memory.
4. **Task Isolation**: Each component runs in isolation, improving fault tolerance.

This pattern enables Metashrew to handle concurrent operations efficiently, such as processing blocks while serving API requests.

### 4. Dependency Injection Pattern

Metashrew uses dependency injection for database backends:

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

This pattern:

1. **Abstracts Storage**: Separates the storage interface from its implementation.
2. **Enables Multiple Backends**: Allows for different storage backends (RocksDB, DynamoDB, etc.).
3. **Simplifies Testing**: Makes it easier to use mock implementations for testing.
4. **Promotes Modularity**: Keeps components loosely coupled.

### 5. Command Pattern for RPC

The JSON-RPC interface uses the command pattern:

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│             │     │             │     │             │
│  JSON-RPC   │────►│  Command    │────►│  Runtime    │
│  Request    │     │  Executor   │     │  Method     │
│             │     │             │     │             │
└─────────────┘     └─────────────┘     └─────────────┘
```

Key aspects:

1. **Request Encapsulation**: Each RPC request is encapsulated as a command object.
2. **Method Routing**: Commands are routed to the appropriate handler based on the method name.
3. **Parameter Validation**: Parameters are validated before execution.
4. **Result Formatting**: Results are formatted according to the JSON-RPC specification.

## Integration Patterns with ALKANES

ALKANES-RS integrates with Metashrew using several specific patterns:

### 1. Nested WASM Execution Pattern

ALKANES-RS implements its own WASM virtual machine within the Metashrew WASM environment:

```
┌───────────────────────────────────────────────┐
│ Metashrew WASM Environment                    │
│                                               │
│  ┌─────────────────────────────────────────┐  │
│  │ ALKANES-RS                              │  │
│  │                                         │  │
│  │  ┌─────────────────────────────────┐    │  │
│  │  │ ALKANES Smart Contract VM       │    │  │
│  │  │                                 │    │  │
│  │  │  ┌─────────────────────────┐    │    │  │
│  │  │  │ Smart Contract WASM     │    │    │  │
│  │  │  └─────────────────────────┘    │    │  │
│  │  └─────────────────────────────────┘    │  │
│  └─────────────────────────────────────────┘  │
└───────────────────────────────────────────────┘
```

This pattern:

1. **Enables Smart Contracts**: Allows ALKANES to execute smart contracts within its own environment.
2. **Provides Isolation**: Keeps contract execution isolated from the indexer.
3. **Implements Fuel Metering**: Prevents infinite loops and resource exhaustion.
4. **Manages Contract State**: Provides a controlled environment for contract state access.

### 2. Protocol Extension Pattern

ALKANES extends the protorunes protocol using a layered approach:

```
┌─────────────┐
│             │
│  Bitcoin    │
│  Blockchain │
│             │
└─────────────┘
      │
      │
      ▼
┌─────────────┐
│             │
│  Protorunes │
│  Protocol   │
│             │
└─────────────┘
      │
      │
      ▼
┌─────────────┐
│             │
│  ALKANES    │
│  Protocol   │
│             │
└─────────────┘
```

This pattern:

1. **Builds on Existing Standards**: Leverages the protorunes protocol as a foundation.
2. **Maintains Compatibility**: Ensures compatibility with the base protocol.
3. **Adds Specialized Functionality**: Extends the base protocol with DeFi capabilities.
4. **Preserves Core Behavior**: Doesn't modify the behavior of the underlying protocols.

### 3. View Function Pattern

ALKANES implements view functions for querying protocol state:

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│             │     │             │     │             │
│  Client     │────►│  JSON-RPC   │────►│  ALKANES    │
│  Request    │     │  Server     │     │  View Func  │
│             │     │             │     │             │
└─────────────┘     └─────────────┘     └─────────────┘
                                              │
                                              │
                                              ▼
                                        ┌─────────────┐
                                        │             │
                                        │  Database   │
                                        │  Query      │
                                        │             │
                                        └─────────────┘
```

Key aspects:

1. **Read-Only Access**: View functions provide read-only access to the protocol state.
2. **Historical Queries**: Support querying state at any historical block height.
3. **Typed Interfaces**: Use Protocol Buffers for type-safe parameter and result handling.
4. **Efficient Retrieval**: Optimize for common query patterns.

### 4. Table Relationship Pattern

ALKANES uses multiple tables with specific relationships:

```
┌─────────────────────┐     ┌─────────────────────┐
│                     │     │                     │
│  OUTPOINT_TO_RUNES  │────►│  RUNE_ID_TO_OUTPOINTS │
│                     │     │                     │
└─────────────────────┘     └─────────────────────┘
          │                           │
          │                           │
          ▼                           ▼
┌─────────────────────┐     ┌─────────────────────┐
│                     │     │                     │
│  OUTPOINTS_FOR_ADDRESS │  │  OUTPOINT_SPENDABLE_BY │
│                     │     │                     │
└─────────────────────┘     └─────────────────────┘
```

This pattern:

1. **Separates Concerns**: Different tables handle different aspects of the protocol state.
2. **Optimizes Queries**: Tables are structured for efficient querying of common patterns.
3. **Maintains Consistency**: Ensures consistency between related tables.
4. **Supports Complex Queries**: Enables complex queries through table joins.

## Data Flow Patterns

### 1. Block Processing Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│             │     │             │     │             │     │             │
│  Bitcoin    │────►│  Metashrew  │────►│  WASM       │────►│  Database   │
│  Node       │     │  Indexer    │     │  Module     │     │  Updates    │
│             │     │             │     │             │     │             │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
                                                                  │
                                                                  ▼
                                                           ┌─────────────┐
                                                           │             │
                                                           │  SMT Root   │
                                                           │  Calculation │
                                                           │             │
                                                           └─────────────┘
```

### 2. Query Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│             │     │             │     │             │     │             │
│  Client     │────►│  JSON-RPC   │────►│  View       │────►│  Database   │
│  Application│     │  Server     │     │  Function   │     │  Query      │
│             │     │             │     │             │     │             │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
```

### 3. Chain Reorganization Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│             │     │             │     │             │     │             │
│  Detect     │────►│  Binary     │────►│  Rollback   │────►│  Reprocess  │
│  Divergence │     │  Search for │     │  State to   │     │  New Blocks │
│             │     │  Height     │     │  Valid Root │     │  & Calculate│
│             │     │             │     │             │     │  New Roots  │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
```

The BST approach significantly improves this flow by:

1. **Efficient Height Location**: Using binary search to quickly find the divergence point
2. **Precise State Rollback**: Leveraging height-indexed keys to identify affected state
3. **Accurate Root Recalculation**: Recalculating state roots only for affected heights
4. **Snapshot Integration**: Using snapshots as checkpoints to avoid full reprocessing when possible

### 4. Snapshot System Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│             │     │             │     │             │     │             │
│  Track DB   │────►│  Create     │────►│  Compress   │────►│  Update     │
│  Changes    │     │  Snapshot   │     │  & Store    │     │  Repository │
│  Using BST  │     │  Metadata   │     │  Diff Files │     │  Index      │
│             │     │             │     │             │     │             │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
```

Key components of the snapshot system:

1. **BST Integration**: Uses the BST structure to efficiently track key-value changes at specific heights
2. **Height-Based Intervals**: Creates snapshots at configurable block height intervals
3. **Compressed Diffs**: Stores compressed key-value differences between snapshots
4. **State Root Verification**: Includes state roots for cryptographic verification
5. **Repository Structure**: Organizes snapshots in a repository structure for easy distribution
6. **Remote Sync**: Supports syncing from remote snapshot repositories
7. **WASM Module Key-Value Tracking**: Captures all ~40k key-value updates per block from the WASM module
8. **Callback-Based Tracking**: Uses a callback mechanism to track key-value updates from both direct database operations and batch operations

## Error Handling Patterns

### 1. Result Propagation

Metashrew uses Rust's `Result` type for error propagation:

```rust
fn run(&mut self) -> Result<(), anyhow::Error> {
    self.context.lock().map_err(lock_err)?.state = 0;
    let start = self
        .instance
        .get_typed_func::<(), ()>(&mut self.wasmstore, "_start")
        .context("Failed to get _start function")?;
    
    self.handle_reorg()?;
    
    match start.call(&mut self.wasmstore, ()) {
        Ok(_) => {
            if self.context.lock().map_err(lock_err)?.state != 1 && !self.wasmstore.data().had_failure {
                return Err(anyhow!("indexer exited unexpectedly"));
            }
            Ok(())
        }
        Err(e) => Err(e).context("Error calling _start function"),
    }
}
```

This pattern:

1. **Propagates Context**: Adds context to errors as they propagate up the call stack.
2. **Handles Failures Gracefully**: Attempts to recover from failures when possible.
3. **Provides Detailed Errors**: Gives detailed error information for debugging.

### 2. JSON-RPC Error Handling

The JSON-RPC server uses a standardized error format:

```rust
#[derive(Serialize)]
struct JsonRpcError {
    id: u32,
    error: JsonRpcErrorObject,
    jsonrpc: String,
}

#[derive(Serialize)]
struct JsonRpcErrorObject {
    code: i32,
    message: String,
    data: Option<String>,
}
```

This pattern:

1. **Standardizes Errors**: Uses standard JSON-RPC error codes and formats.
2. **Provides Context**: Includes detailed error messages and optional data.
3. **Maintains Compatibility**: Ensures compatibility with JSON-RPC clients.

## Conclusion

Metashrew's system patterns enable a flexible, extensible framework for Bitcoin indexing and metaprotocol development. The combination of WebAssembly, append-only storage, and modular architecture creates a powerful platform that can support complex applications like ALKANES while maintaining simplicity for developers.

The integration with ALKANES demonstrates how these patterns can be leveraged to build sophisticated DeFi applications on Bitcoin, showcasing the framework's capabilities and flexibility.
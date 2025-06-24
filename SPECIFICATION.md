# Metashrew: A WebAssembly-Based Bitcoin Indexer Framework
## Technical Specification and Implementation Guide

**Version:** 9.0.0  
**Date:** June 24th, 2025
**Author:** Raymond Wesley Pulver IV

---

## Abstract

Metashrew is a high-performance, modular Bitcoin indexer framework that enables developers to build custom blockchain indexers using WebAssembly (WASM) modules. The framework provides a complete infrastructure for Bitcoin blockchain synchronization, data indexing, and query processing while maintaining deterministic execution and state consistency through cryptographic commitments.

**Beyond Simple Indexing:** Metashrew is designed as a foundation for building sophisticated metaprotocols on Bitcoin. It enables developers to create systems ranging from simple balance trackers to full smart contract metaprotocols that extend Bitcoin's consensus layer. The framework ensures that any WASM program run in a Metashrew system will construct an identical database on any platform, making the indexer a pure function of Bitcoin chaindata.

**Metaprotocol Applications:** The framework supports building high-powered blockchain applications that connect to Bitcoin or rely on its state. In its most advanced usage, Metashrew can power systems as complex as smart contract metaprotocols built entirely on Bitcoin L1, where the view into the metaprotocol is always a deterministic transform of input chaindata at any given block height.

This specification provides a comprehensive technical overview of the Metashrew architecture, implementation details, and complete source code mapping for developers, auditors, and researchers. All structures, traits, and design considerations are documented with direct references to the actual source code implementation.

---

## Motivation: Metashrew as a Metaprotocol Foundation

### The Vision for Bitcoin Metaprotocols

Bitcoin's base layer provides a robust, decentralized foundation for value transfer, but its scripting capabilities are intentionally limited. Metaprotocols built on Bitcoin extend these capabilities by interpreting Bitcoin transactions according to additional rules and state machines that exist "above" the base consensus layer.

**Metashrew's Role:** Metashrew serves as the critical infrastructure layer that makes sophisticated Bitcoin metaprotocols practical and reliable. It provides the deterministic execution environment, historical state access, and cryptographic commitments necessary to build complex systems on Bitcoin.

### From Simple Indexing to Full Metaprotocols

**Simple Indexing Applications:**
- **Balance Tracking**: Monitor Bitcoin addresses and track balance changes over time
- **Transaction Analysis**: Index and categorize Bitcoin transactions for analytics
- **Address Clustering**: Group related addresses based on transaction patterns

**Intermediate Metaprotocol Applications:**
- **Token Systems**: Implement fungible and non-fungible tokens using Bitcoin transactions
- **Decentralized Exchanges**: Build order books and trading systems on Bitcoin
- **Governance Systems**: Create voting and proposal mechanisms using Bitcoin as the base layer

**Advanced Metaprotocol Applications:**
- **Smart Contract Platforms**: Full Turing-complete execution environments on Bitcoin
- **Layer 2 Protocols**: State channels, rollups, and other scaling solutions
- **Cross-Chain Bridges**: Secure interoperability protocols between Bitcoin and other chains

### The Deterministic Execution Guarantee

**Core Principle:** Any identical WASM program run in a Metashrew system, on any platform, will construct the identical database. This guarantee is fundamental to metaprotocol reliability and enables:

**Consensus Without Coordination:** Multiple parties can independently run the same metaprotocol indexer and arrive at identical state, enabling decentralized verification without requiring a separate consensus mechanism.

**Historical Reproducibility:** The complete state of any metaprotocol can be reconstructed from Bitcoin chaindata alone, ensuring long-term verifiability and auditability.

**Cross-Platform Consistency:** Metaprotocol applications work identically across different operating systems, hardware architectures, and deployment environments.

### Pure Function Property

**Mathematical Purity:** Metashrew indexers are designed as pure functions where:
- **Input**: Bitcoin chaindata up to any given block height
- **Output**: Complete metaprotocol state at that height
- **Determinism**: Same input always produces same output
- **No Side Effects**: No external dependencies or non-deterministic operations

**Benefits of Purity:**
- **Verifiability**: Anyone can verify metaprotocol state by re-running the indexer
- **Composability**: Multiple metaprotocols can be combined and layered
- **Debugging**: Issues can be reproduced exactly across different environments
- **Optimization**: Pure functions enable aggressive caching and optimization strategies

### Historical State Access

**Time-Travel Queries:** Metashrew enables querying the state of any metaprotocol at any historical block height, providing:

**Audit Trails:** Complete history of all state changes for compliance and analysis
**Rollback Capability:** Ability to handle Bitcoin chain reorganizations correctly
**Point-in-Time Analysis:** Research and analytics on historical metaprotocol behavior
**Dispute Resolution:** Ability to prove the state of a metaprotocol at any past moment

### Building Complex Systems on Bitcoin

**Smart Contract Metaprotocols:** Metashrew enables building systems with smart contract-like capabilities entirely on Bitcoin L1:

**State Machines:** Complex state transitions triggered by Bitcoin transactions
**Conditional Logic**: Sophisticated rules for transaction validation and state updates
**Inter-Contract Communication**: Multiple smart contracts that can interact with each other
**Upgradeable Logic**: Ability to evolve metaprotocol rules over time through governance

**Example: Decentralized Exchange Metaprotocol:**
1. **Order Placement**: Bitcoin transactions encode buy/sell orders
2. **Order Matching**: Metashrew indexer matches compatible orders
3. **Settlement**: State updates reflect completed trades
4. **Historical Queries**: View order books and trade history at any block height

**Example: Token System Metaprotocol:**
1. **Token Creation**: Special Bitcoin transactions define new token types
2. **Token Transfers**: Bitcoin transactions encode token transfer instructions
3. **Balance Tracking**: Metashrew maintains token balances for all addresses
4. **Supply Management**: Track total supply, minting, and burning operations

### The WASM Program Format

**Constrained Environment:** Metashrew defines a very limited environment for WASM builds that ensures deterministic execution:

**No External Dependencies:** WASM modules cannot access file systems, networks, or other external resources
**Deterministic Operations**: All operations must produce identical results across platforms
**Memory Isolation**: Each execution starts with fresh memory state
**Resource Limits**: Bounded execution time and memory usage

**Indexer as Transform:** The WASM program acts as a mathematical transform function:
```
f(chaindata, height) → metaprotocol_state
```

Where the same function applied to the same inputs always produces the same output, regardless of when or where it's executed.

---

## Table of Contents

1. [System Architecture](#1-system-architecture)
2. [Core Runtime System](#2-core-runtime-system)
3. [Storage Layer](#3-storage-layer)
4. [Synchronization Framework](#4-synchronization-framework)
5. [WebAssembly Interface](#5-webassembly-interface)
6. [Bitcoin Protocol Support](#6-bitcoin-protocol-support)
7. [Cryptographic Primitives](#7-cryptographic-primitives)
8. [API and Query Layer](#8-api-and-query-layer)
9. [Security Model](#9-security-model)
10. [Performance Characteristics](#10-performance-characteristics)
11. [Complete Source Code Mapping](#11-complete-source-code-mapping)

---

## 1. System Architecture

### 1.1 Overview

Metashrew implements a layered architecture with clear separation of concerns through generic traits and adapter patterns. At its core, Metashrew is a Bitcoin blockchain indexer that allows developers to write custom indexing logic in WebAssembly (WASM) modules while providing a robust, high-performance infrastructure for synchronization, storage, and querying.

**How the System Works:**
1. **Bitcoin Node Connection**: Metashrew connects to a Bitcoin node (like Bitcoin Core) to fetch block data
2. **Block Processing Pipeline**: Each block is processed through a WASM indexer module that extracts and stores relevant data
3. **State Management**: All data changes are tracked in a cryptographically-committed state tree (Sparse Merkle Tree)
4. **Query Interface**: Applications can query the indexed data through JSON-RPC API calls
5. **Historical Queries**: The system maintains complete historical state, allowing queries at any block height
6. **Chain Reorganizations**: Automatic detection and handling of Bitcoin chain reorganizations

**System Flow:**
```
Bitcoin Node → Block Data → WASM Indexer → State Updates → Storage → Query API
     ↑                                                        ↓
     └─── Chain Reorg Detection ←─── State Verification ←─────┘
```

Metashrew implements a layered architecture with clear separation of concerns through generic traits and adapter patterns:

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                        │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────┐ │
│  │   JSON-RPC API  │  │  Custom Indexers │  │  CLI Tools  │ │
│  └─────────────────┘  └─────────────────┘  └─────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                   WebAssembly Runtime                       │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────┐ │
│  │ MetashrewRuntime│  │  Host Functions │  │ View Engine │ │
│  │     <T>         │  │   Interface     │  │             │ │
│  └─────────────────┘  └─────────────────┘  └─────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                    Storage Layer                            │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────┐ │
│  │ SMTHelper<T>    │  │ OptimizedBST<T> │  │ State Roots │ │
│  │ Sparse Merkle   │  │ Binary Search   │  │ Height Index│ │
│  │     Tree        │  │     Tree        │  │             │ │
│  └─────────────────┘  └─────────────────┘  └─────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                 Synchronization Layer                       │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────┐ │
│  │ MetashrewSync   │  │ Block Processor │  │ Reorg       │ │
│  │   <N,S,R>       │  │   Pipeline      │  │ Handler     │ │
│  └─────────────────┘  └─────────────────┘  └─────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

**Source Code Mapping:**
- Application Layer: [`rockshrew-mono/src/main.rs`](rockshrew-mono/src/main.rs)
- WebAssembly Runtime: [`crates/metashrew-runtime/src/runtime.rs`](crates/metashrew-runtime/src/runtime.rs)
- Storage Layer: [`crates/metashrew-runtime/src/smt.rs`](crates/metashrew-runtime/src/smt.rs), [`crates/metashrew-runtime/src/optimized_bst.rs`](crates/metashrew-runtime/src/optimized_bst.rs)
- Synchronization Layer: [`crates/rockshrew-sync/src/sync.rs`](crates/rockshrew-sync/src/sync.rs)

### 1.2 Core Components

#### 1.2.1 MetashrewRuntime<T>
**Location:** [`crates/metashrew-runtime/src/runtime.rs`](crates/metashrew-runtime/src/runtime.rs:25)

The central execution environment that manages WASM module lifecycle with generic storage backend support:

**Structure Definition:**
```rust
pub struct MetashrewRuntime<T: KeyValueStoreLike> {
    pub context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
    pub engine: Engine,
    pub module: Module,
    pub instance: Option<Instance>,
}
```

**Key Methods:**
- [`run()`](crates/metashrew-runtime/src/runtime.rs:123): Execute indexer for current block with deterministic WASM execution
- [`view(function: String, input: &[u8], height: u32)`](crates/metashrew-runtime/src/runtime.rs:200): Query indexed data at specific height
- [`preview(block_data: &[u8], function: String, input: &[u8])`](crates/metashrew-runtime/src/runtime.rs:250): Test indexing logic without state persistence
- [`refresh_memory()`](crates/metashrew-runtime/src/runtime.rs:89): Reset WASM linear memory for deterministic execution

#### 1.2.2 MetashrewRuntimeContext<T>
**Location:** [`crates/metashrew-runtime/src/context.rs`](crates/metashrew-runtime/src/context.rs:8)

Execution context maintaining state between WASM calls with generic storage abstraction:

**Structure Definition:**
```rust
pub struct MetashrewRuntimeContext<T: KeyValueStoreLike> {
    pub db: T,
    pub block: Vec<u8>,
    pub height: u32,
    pub memory: Option<Memory>,
}
```

**Design Considerations:**
- Generic over storage backend through [`KeyValueStoreLike`](crates/metashrew-runtime/src/traits.rs:8) trait
- Maintains block data and height for indexer access
- Manages WASM linear memory lifecycle
- Thread-safe through Arc<Mutex<>> wrapper in runtime

### 1.3 System Component Relationships

Understanding how Metashrew's components work together is crucial for developers and operators:

#### 1.3.1 Data Flow Through the System

**1. Block Ingestion Process:**
When a new Bitcoin block arrives, the system follows this flow:
- The `BitcoinNodeAdapter` fetches raw block data from the Bitcoin node
- The `MetashrewSync` engine validates the block and checks for chain reorganizations
- Block data is passed to the `MetashrewRuntime` for processing
- The WASM indexer module (`_start` function) processes the block data
- State changes are accumulated in the `SMTHelper` and `OptimizedBST`
- A cryptographic state root is calculated and stored
- The new state is atomically committed to RocksDB

**2. Query Processing:**
When an application queries indexed data:
- JSON-RPC request arrives at the API layer
- The request is routed to the appropriate view function in the WASM module
- The `MetashrewRuntime` executes the view function with historical state
- The `OptimizedBST` provides O(1) access to current data or O(log n) historical access
- Results are returned through the JSON-RPC response

**3. Chain Reorganization Handling:**
When the Bitcoin chain reorganizes:
- The `MetashrewSync` engine detects block hash mismatches
- The system identifies the divergence point
- The `OptimizedBST` efficiently rolls back state to the common ancestor
- New blocks from the canonical chain are reprocessed
- State consistency is maintained throughout the process

#### 1.3.2 Component Interaction Patterns

**Storage Layer Interaction:**
- `SMTHelper` manages cryptographic state commitments
- `OptimizedBST` provides efficient current and historical data access
- `KeyValueStoreLike` trait abstracts the underlying storage (RocksDB)
- All components work together to provide ACID properties

**Runtime Layer Interaction:**
- `MetashrewRuntime` orchestrates WASM execution
- Host functions provide controlled access to storage and block data
- Memory management ensures deterministic execution
- The runtime isolates indexer logic from system resources

**Synchronization Layer Interaction:**
- `MetashrewSync` coordinates between Bitcoin node, storage, and runtime
- Adapter pattern allows pluggable backends for different environments
- Pipeline processing enables high-throughput block processing
- Error handling and recovery maintain system reliability

#### 1.3.3 Why This Architecture Matters

**Modularity:** Each component has a single responsibility and well-defined interfaces, making the system maintainable and testable.

**Performance:** The hybrid SMT/BST approach provides both cryptographic integrity and fast queries. Pipeline processing maximizes throughput.

**Reliability:** Atomic operations, comprehensive error handling, and chain reorganization support ensure data consistency.

**Extensibility:** The adapter pattern and WASM modules allow customization without modifying core infrastructure.

**Auditability:** Cryptographic state commitments and complete source code mapping enable verification of system behavior.

---

## 2. Core Runtime System

### 2.1 WebAssembly Execution Model

Metashrew uses Wasmtime as the WebAssembly runtime with deterministic execution configuration:

**Wasmtime Configuration:**
- **Deterministic Execution**: All WASM modules execute deterministically through memory refresh
- **Memory Isolation**: Each indexer runs in isolated linear memory space
- **Host Function Interface**: Controlled access to system resources through defined imports
- **State Persistence**: Automatic state commitment after each block through SMT updates

**Implementation Details:**
- Engine configuration in [`MetashrewRuntime::new()`](crates/metashrew-runtime/src/runtime.rs:45)
- Memory refresh mechanism in [`refresh_memory()`](crates/metashrew-runtime/src/runtime.rs:89)
- Host function binding in [`link_host_functions()`](crates/metashrew-runtime/src/runtime.rs:67)

#### 2.1.1 Wasmtime Configuration

Metashrew configures Wasmtime with specific settings to ensure deterministic execution and security. The actual configuration from [`MetashrewRuntime::load()`](crates/metashrew-runtime/src/runtime.rs:426):

**Synchronous Engine Configuration:**
```rust
let mut config = wasmtime::Config::default();
// Enable NaN canonicalization for deterministic floating point operations
config.cranelift_nan_canonicalization(true);
// Make relaxed SIMD deterministic (or disable it if not needed)
config.relaxed_simd_deterministic(true);
// Allocate memory at maximum size to avoid non-deterministic memory growth
config.static_memory_maximum_size(0x100000000); // 4GB max memory
config.static_memory_guard_size(0x10000); // 64KB guard
// Disable copy-on-write to ensure consistent memory behavior
config.memory_init_cow(false);
```

**Asynchronous Engine Configuration:**
```rust
let mut async_config = config.clone();
async_config.consume_fuel(true);    // Enable fuel consumption for cooperative yielding
async_config.async_support(true);   // Enable async execution for view functions
```

**Memory Configuration:**
- **Static Memory Allocation**: 4GB maximum memory pre-allocated to avoid growth
- **Guard Pages**: 64KB guard pages for memory safety
- **No Copy-on-Write**: Disabled to ensure consistent memory behavior
- **Memory Reset**: Complete memory refresh between block executions via [`refresh_memory()`](crates/metashrew-runtime/src/runtime.rs:768)

**Resource Limits:**
```rust
// WASM execution state with maximum resource limits
limits: StoreLimitsBuilder::new()
    .memories(usize::MAX)
    .tables(usize::MAX)
    .instances(usize::MAX)
    .build()
```

**Deterministic Execution Guarantees:**
- **NaN Canonicalization**: Ensures consistent floating point behavior across platforms
- **Relaxed SIMD Determinism**: Makes SIMD operations deterministic
- **Static Memory**: Pre-allocated memory prevents non-deterministic growth patterns
- **Memory Isolation**: Fresh memory for each block execution
- **No External Access**: WASM modules cannot access file systems, networks, or other external resources

**Security and Isolation:**
- **Host Function Control**: Only specific host functions are available through the linker
- **Resource Limits**: Bounded execution time and memory usage
- **Failure Tracking**: Host function failures are tracked and propagated
- **Trap Handling**: Unknown imports are defined as traps for security

### 2.2 Host Function Interface

The runtime provides the following WASM imports available to metashrew programs:

#### 2.2.1 Core Host Functions
**Location:** [`crates/metashrew-runtime/src/runtime.rs`](crates/metashrew-runtime/src/runtime.rs:300)

**Available WASM Imports:**
```rust
// Storage operations
__host_len() -> i32          // Get length of input data
__load_input(ptr: i32)       // Load input data to WASM memory
__get(ptr: i32, v: i32)      // Get value by key
__get_len(ptr: i32) -> i32   // Get length of value by key
__flush(ptr: i32)            // Flush pending writes to storage
__log(ptr: i32)              // Log message from WASM module
```

**Host Function Implementations:**
- [`__host_len()`](crates/metashrew-runtime/src/runtime.rs:320): Returns input data length
- [`__load_input(ptr: i32)`](crates/metashrew-runtime/src/runtime.rs:325): Copies input to WASM memory
- [`__get(ptr: i32, v: i32)`](crates/metashrew-runtime/src/runtime.rs:340): Retrieves value by key from storage
- [`__get_len(ptr: i32)`](crates/metashrew-runtime/src/runtime.rs:360): Returns value length for key
- [`__flush(ptr: i32)`](crates/metashrew-runtime/src/runtime.rs:380): Commits pending operations to SMT
- [`__log(ptr: i32)`](crates/metashrew-runtime/src/runtime.rs:400): Outputs log message

#### 2.2.2 WASM Import Bindings
**Location:** [`crates/metashrew-core/src/imports.rs`](crates/metashrew-core/src/imports.rs:17)

**Guest-Side Import Declarations:**
```rust
#[link(wasm_import_module = "env")]
extern "C" {
    pub fn __host_len() -> i32;
    pub fn __flush(ptr: i32);
    pub fn __get(ptr: i32, v: i32);
    pub fn __get_len(ptr: i32) -> i32;
    pub fn __load_input(ptr: i32);
    pub fn __log(ptr: i32);
}
```

### 2.3 Memory Management

#### 2.3.1 ArrayBuffer Layout
**Location:** [`crates/metashrew-support/src/compat.rs`](crates/metashrew-support/src/compat.rs:125)

Data exchange between host and guest uses ArrayBuffer layout with length prefix:

**Layout Structure:**
```
[4-byte little-endian length][data payload]
```

**Implementation Functions:**
- [`to_arraybuffer_layout<T: AsRef<[u8]>>(v: T) -> Vec<u8>`](crates/metashrew-support/src/compat.rs:125): Convert data to ArrayBuffer format
- [`to_ptr(v: &mut Vec<u8>) -> i32`](crates/metashrew-support/src/compat.rs:67): Get WASM pointer to vector data
- [`to_passback_ptr(v: &mut Vec<u8>) -> i32`](crates/metashrew-support/src/compat.rs:90): Get pointer past length prefix
- [`export_bytes(v: Vec<u8>) -> i32`](crates/metashrew-support/src/compat.rs:165): Export data to WASM memory with ownership transfer

#### 2.3.2 Memory Safety Considerations
- **Pointer Arithmetic**: Safe conversion between Rust pointers and WASM addresses
- **Lifetime Management**: Proper handling of memory ownership transfer through `Box::leak()`
- **Layout Consistency**: Standardized data structure layout across host-guest boundary

---

## 3. Storage Layer

### 3.1 Storage Abstraction Layer

#### 3.1.1 KeyValueStoreLike Trait
**Location:** [`crates/metashrew-runtime/src/traits.rs`](crates/metashrew-runtime/src/traits.rs:8)

The foundational storage abstraction that enables multiple backend implementations:

**Trait Definition:**
```rust
pub trait KeyValueStoreLike: Clone + Send + Sync {
    type Error: std::fmt::Debug;
    
    // Core operations
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error>;
    fn get_immutable(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error>;
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;
    fn delete(&mut self, key: &[u8]) -> Result<(), Self::Error>;
    
    // Batch operations
    fn create_batch(&self) -> Box<dyn BatchLike>;
    fn write_batch(&mut self, batch: Box<dyn BatchLike>) -> Result<(), Self::Error>;
    
    // Iteration
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Self::Error>;
}
```

**Design Considerations:**
- Generic error handling through associated `Error` type
- Immutable read operations for concurrent access
- Batch operations for atomic updates
- Prefix scanning for efficient range queries

#### 3.1.3 RocksDB Configuration

Metashrew uses RocksDB as the primary storage backend with specific optimizations for Bitcoin indexing workloads:

**Database Configuration:**
```rust
let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_max_open_files(10000);
opts.set_use_fsync(false);
opts.set_bytes_per_sync(8388608);
opts.set_disable_data_sync(false);
opts.set_block_cache_size_mb(512);
opts.set_table_cache_num_shard_bits(6);
opts.set_max_write_buffer_number(32);
opts.set_write_buffer_size(536870912);
opts.set_target_file_size_base(1073741824);
opts.set_min_write_buffer_number_to_merge(4);
opts.set_level_zero_stop_writes_trigger(2000);
opts.set_level_zero_slowdown_writes_trigger(0);
opts.set_compaction_style(DBCompactionStyle::Universal);
```

**Performance Optimizations:**
- **Block Cache**: 512MB block cache for frequently accessed data
- **Write Buffers**: Large write buffers (512MB) for high-throughput writes
- **Universal Compaction**: Optimized for write-heavy workloads
- **Parallel Operations**: Multiple write buffers and table cache sharding
- **Sync Settings**: Optimized fsync behavior for performance vs durability trade-offs

**Storage Layout:**
- **Column Families**: Separate column families for different data types
- **Key Prefixes**: Structured key prefixes for efficient range queries
- **Compression**: LZ4 compression for space efficiency
- **Bloom Filters**: Bloom filters on all levels for faster negative lookups

**Backup and Recovery:**
- **Atomic Writes**: All state changes applied atomically through batches
- **Consistent Snapshots**: Point-in-time consistent database snapshots
- **Chain Reorganization**: Efficient rollback through height-indexed keys

#### 3.1.2 BatchLike Trait
**Location:** [`crates/metashrew-runtime/src/traits.rs`](crates/metashrew-runtime/src/traits.rs:25)

Abstraction for atomic batch operations:

**Trait Definition:**
```rust
pub trait BatchLike {
    fn put(&mut self, key: &[u8], value: &[u8]);
    fn delete(&mut self, key: &[u8]);
}
```

### 3.2 Sparse Merkle Tree Implementation

#### 3.2.1 SMTHelper<T> Structure
**Location:** [`crates/metashrew-runtime/src/smt.rs`](crates/metashrew-runtime/src/smt.rs:15)

Core SMT implementation with height-indexed storage:

**Structure Definition:**
```rust
pub struct SMTHelper<T: KeyValueStoreLike> {
    db: T,
    height: u32,
}
```

**Key Prefixes:**
```rust
pub const SMT_NODE_PREFIX: &str = "smt_node:";
pub const SMT_ROOT_PREFIX: &str = "smt_root:";
pub const SMT_LEAF_PREFIX: &str = "smt_leaf:";
pub const SMT_INTERNAL_PREFIX: &str = "smt_internal:";
pub const BST_HEIGHT_PREFIX: &str = "bst_height:";
pub const BST_KEY_PREFIX: &str = "bst_key:";
```

#### 3.2.2 SMTNode Enumeration
**Location:** [`crates/metashrew-runtime/src/smt.rs`](crates/metashrew-runtime/src/smt.rs:35)

**Node Types:**
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SMTNode {
    Internal { left_child: Vec<u8>, right_child: Vec<u8> },
    Leaf { key: Vec<u8>, value_index: u32 },
}
```

**Design Considerations:**
- Internal nodes store child hashes for merkle tree structure
- Leaf nodes reference value indices for space efficiency
- Height-indexed storage enables historical queries

#### 3.2.3 BatchedSMTHelper<T> Optimization
**Location:** [`crates/metashrew-runtime/src/smt.rs`](crates/metashrew-runtime/src/smt.rs:200)

Performance-optimized SMT with operation batching:

**Structure Definition:**
```rust
pub struct BatchedSMTHelper<T: KeyValueStoreLike> {
    smt: SMTHelper<T>,
    pending_operations: HashMap<Vec<u8>, Vec<u8>>,
    cache: HashMap<Vec<u8>, SMTNode>,
}
```

**Key Methods:**
- [`batch_set(operations: &[(Vec<u8>, Vec<u8>)])`](crates/metashrew-runtime/src/smt.rs:220): Queue multiple operations
- [`commit() -> Result<Vec<u8>>`](crates/metashrew-runtime/src/smt.rs:240): Apply all pending operations atomically
- [`get_cached_node(hash: &[u8]) -> Option<SMTNode>`](crates/metashrew-runtime/src/smt.rs:260): Retrieve from cache

### 3.3 Optimized Binary Search Tree

#### 3.3.1 OptimizedBST<T> Structure
**Location:** [`crates/metashrew-runtime/src/optimized_bst.rs`](crates/metashrew-runtime/src/optimized_bst.rs:21)

High-performance BST for current state access and historical queries:

**Structure Definition:**
```rust
pub struct OptimizedBST<T: KeyValueStoreLike> {
    storage: T,
}
```

**Key Prefixes:**
```rust
pub const CURRENT_VALUE_PREFIX: &str = "current:";
pub const HISTORICAL_VALUE_PREFIX: &str = "hist:";
pub const HEIGHT_INDEX_PREFIX: &str = "height:";
pub const KEYS_AT_HEIGHT_PREFIX: &str = "keys:";
```

**Design Strategy:**
- **O(1) Current Access**: Direct lookup for most recent values
- **Binary Search Historical**: Efficient historical queries only when needed
- **Reorg Support**: Track keys modified at each height for rollback
- **Append-Only Structure**: Maintains complete history for auditing

#### 3.3.2 Core BST Operations
**Key Methods:**
- [`put(key: &[u8], value: &[u8], height: u32)`](crates/metashrew-runtime/src/optimized_bst.rs:32): Store value at height
- [`get_current(key: &[u8]) -> Option<Vec<u8>>`](crates/metashrew-runtime/src/optimized_bst.rs:67): O(1) current value lookup
- [`get_at_height(key: &[u8], height: u32) -> Option<Vec<u8>>`](crates/metashrew-runtime/src/optimized_bst.rs:91): Historical value with binary search
- [`rollback_to_height(target_height: u32)`](crates/metashrew-runtime/src/optimized_bst.rs:274): Chain reorganization handling

#### 3.3.3 BSTHelper Trait
**Location:** [`crates/metashrew-runtime/src/helpers.rs`](crates/metashrew-runtime/src/helpers.rs:31)

Generic interface for BST operations:

**Trait Definition:**
```rust
pub trait BSTHelper {
    type Error: std::fmt::Debug;
    
    fn get_current_value(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error>;
    fn get_value_at_height(&self, key: &[u8], height: u32) -> Result<Option<Vec<u8>>, Self::Error>;
    fn iterate_backwards(&self, key: &[u8], from_height: Option<u32>) -> Result<Vec<(u32, Vec<u8>)>, Self::Error>;
    fn get_keys_touched_at_height(&self, height: u32) -> Result<Vec<Vec<u8>>, Self::Error>;
    fn rollback_to_height(&self, target_height: u32) -> Result<(), Self::Error>;
    fn get_statistics(&self) -> Result<BSTStatistics, Self::Error>;
}
```

#### 3.3.4 BSTStatistics Structure
**Location:** [`crates/metashrew-runtime/src/helpers.rs`](crates/metashrew-runtime/src/helpers.rs:10)

**Statistics Definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BSTStatistics {
    pub tip_height: u32,
    pub total_keys: usize,
    pub total_entries: usize,
    pub current_state_root: [u8; 32],
}
```

### 3.4 Hierarchical Key-Value Pointers

#### 3.4.1 KeyValuePointer Trait
**Location:** [`crates/metashrew-support/src/index_pointer.rs`](crates/metashrew-support/src/index_pointer.rs:150)

High-level abstraction for building hierarchical data structures:

**Trait Definition:**
```rust
pub trait KeyValuePointer {
    fn wrap(word: &Vec<u8>) -> Self;
    fn unwrap(&self) -> Arc<Vec<u8>>;
    fn set(&mut self, v: Arc<Vec<u8>>);
    fn get(&self) -> Arc<Vec<u8>>;
    fn inherits(&mut self, from: &Self);
    
    // Hierarchical operations
    fn select(&self, word: &Vec<u8>) -> Self;
    fn keyword(&self, word: &str) -> Self;
    fn select_value<T: ByteView>(&self, key: T) -> Self;
    
    // List operations
    fn length(&self) -> u32;
    fn select_index(&self, index: u32) -> Self;
    fn append_value<T: ByteView>(&self, v: T);
    fn pop_value<T: ByteView>(&self) -> T;
    fn get_list_values<T: ByteView>(&self) -> Vec<T>;
    
    // Linked list operations
    fn append_ll(&self, v: Arc<Vec<u8>>);
    fn delete_value(&self, i: u32);
    fn map_ll<T>(&self, f: impl FnMut(&mut Self, u32) -> T) -> Vec<T>;
}
```

**Design Patterns:**
- **Hierarchical Keys**: Build nested structures with path-like semantics
- **Type-Safe Values**: Automatic serialization through [`ByteView`](crates/metashrew-support/src/byte_view.rs) trait
- **List Operations**: Array-like operations with length tracking
- **Linked Lists**: Efficient insertion/deletion with pointer chaining

#### 3.4.2 ByteView Trait
**Location:** [`crates/metashrew-support/src/byte_view.rs`](crates/metashrew-support/src/byte_view.rs:8)

Type-safe serialization interface:

**Trait Definition:**
```rust
pub trait ByteView: Clone {
    fn to_bytes(&self) -> Vec<u8>;
    fn from_bytes(bytes: Vec<u8>) -> Self;
    fn zero() -> Self;
}
```

---

## 4. Synchronization Framework

### 4.1 MetashrewSync<N,S,R> Architecture

#### 4.1.1 Core Synchronization Engine
**Location:** [`crates/rockshrew-sync/src/sync.rs`](crates/rockshrew-sync/src/sync.rs:25)

Generic synchronization engine with adapter pattern:

**Structure Definition:**
```rust
pub struct MetashrewSync<N, S, R>
where
    N: BitcoinNodeAdapter,
    S: StorageAdapter,
    R: RuntimeAdapter,
{
    node: Arc<N>,
    storage: Arc<RwLock<S>>,
    runtime: Arc<RwLock<R>>,
    config: SyncConfig,
    is_running: Arc<AtomicBool>,
    current_height: Arc<AtomicU32>,
}
```

**Design Considerations:**
- **Generic Adapters**: Pluggable backends for Bitcoin nodes, storage, and runtime
- **Thread Safety**: Arc and RwLock for concurrent access
- **Atomic State**: Lock-free height tracking and running status
- **Configuration**: Flexible sync parameters through [`SyncConfig`](crates/rockshrew-sync/src/sync.rs:45)

#### 4.1.2 SyncConfig Structure
**Location:** [`crates/rockshrew-sync/src/sync.rs`](crates/rockshrew-sync/src/sync.rs:45)

**Configuration Options:**
```rust
pub struct SyncConfig {
    pub start_height: Option<u32>,
    pub end_height: Option<u32>,
    pub pipeline_size: Option<usize>,
    pub reorg_check_threshold: u32,
    pub batch_size: usize,
    pub atomic_mode: bool,
}
```

### 4.2 Adapter Pattern Implementation

#### 4.2.1 BitcoinNodeAdapter Trait
**Location:** [`crates/rockshrew-sync/src/traits.rs`](crates/rockshrew-sync/src/traits.rs:15)

Abstraction for Bitcoin node communication:

**Trait Definition:**
```rust
#[async_trait]
pub trait BitcoinNodeAdapter: Send + Sync {
    async fn get_tip_height(&self) -> SyncResult<u32>;
    async fn get_block_hash(&self, height: u32) -> SyncResult<Vec<u8>>;
    async fn get_block_data(&self, height: u32) -> SyncResult<Vec<u8>>;
    async fn get_block_info(&self, height: u32) -> SyncResult<BlockInfo>;
    async fn is_connected(&self) -> bool;
}
```

#### 4.2.2 StorageAdapter Trait
**Location:** [`crates/rockshrew-sync/src/traits.rs`](crates/rockshrew-sync/src/traits.rs:35)

Abstraction for persistent storage operations:

**Trait Definition:**
```rust
#[async_trait]
pub trait StorageAdapter: Send + Sync {
    async fn get_indexed_height(&self) -> SyncResult<u32>;
    async fn set_indexed_height(&self, height: u32) -> SyncResult<()>;
    async fn store_block_hash(&self, height: u32, hash: &[u8]) -> SyncResult<()>;
    async fn store_state_root(&self, height: u32, root: &[u8]) -> SyncResult<()>;
    async fn rollback_to_height(&self, height: u32) -> SyncResult<()>;
}
```

#### 4.2.3 RuntimeAdapter Trait
**Location:** [`crates/rockshrew-sync/src/traits.rs`](crates/rockshrew-sync/src/traits.rs:55)

Abstraction for WASM runtime execution:

**Trait Definition:**
```rust
#[async_trait]
pub trait RuntimeAdapter: Send + Sync {
    async fn process_block(&mut self, height: u32, block_data: &[u8], block_hash: &[u8]) -> SyncResult<()>;
    async fn process_block_atomic(&mut self, height: u32, block_data: &[u8], block_hash: &[u8]) -> SyncResult<AtomicBlockResult>;
    async fn execute_view(&self, call: ViewCall) -> SyncResult<ViewResult>;
    async fn execute_preview(&self, call: PreviewCall) -> SyncResult<PreviewResult>;
}
```

### 4.3 Atomic Block Processing

#### 4.3.1 AtomicBlockResult Structure
**Location:** [`crates/metashrew-runtime/src/traits.rs`](crates/metashrew-runtime/src/traits.rs:35)

Result of atomic block processing with cryptographic commitment:

**Structure Definition:**
```rust
pub struct AtomicBlockResult {
    pub state_root: Vec<u8>,
    pub batch_data: Vec<u8>,
    pub height: u32,
    pub block_hash: Vec<u8>,
}
```

**Design Considerations:**
- **State Root**: Cryptographic commitment to all state changes
- **Batch Data**: Serialized database operations for atomic application
- **Height/Hash**: Block identification for consistency verification
- **Atomicity**: All-or-nothing application of block changes

#### 4.3.2 KVTrackerFn Type
**Location:** [`crates/metashrew-runtime/src/traits.rs`](crates/metashrew-runtime/src/traits.rs:45)

Callback for monitoring key-value updates:

**Type Definition:**
```rust
pub type KVTrackerFn = Box<dyn Fn(&[u8], &[u8]) + Send + Sync>;
```

### 4.4 Chain Reorganization Handling

#### 4.4.1 Reorg Detection Algorithm
**Location:** [`crates/rockshrew-sync/src/sync.rs`](crates/rockshrew-sync/src/sync.rs:574)

**Detection Process:**
1. Compare local and remote block hashes for recent blocks
2. Identify divergence point through binary search
3. Rollback state to common ancestor
4. Reprocess blocks from divergence point

**Implementation:**
```rust
async fn handle_reorg(&mut self) -> SyncResult<u32> {
    let current_height = self.current_height.load(Ordering::SeqCst);
    
    for check_height in (current_height.saturating_sub(self.config.reorg_check_threshold)..current_height).rev() {
        let storage = self.storage.read().await;
        if let Ok(Some(local_hash)) = storage.get_block_hash(check_height).await {
            if let Ok(remote_hash) = self.node.get_block_hash(check_height).await {
                if local_hash != remote_hash {
                    storage.rollback_to_height(check_height).await?;
                    return Ok(check_height + 1);
                }
            }
        }
    }
    Ok(current_height)
}
```

#### 4.4.2 Rollback Implementation
**Location:** [`crates/metashrew-runtime/src/optimized_bst.rs`](crates/metashrew-runtime/src/optimized_bst.rs:274)

**Rollback Process:**
1. Identify all keys modified after target height
2. Remove historical entries beyond target height
3. Update current values to latest valid state
4. Clean up height indices and key tracking

---

## 5. WebAssembly Interface

### 5.1 WASM Module Requirements

#### 5.1.1 Required Exports
**Location:** [`crates/metashrew-core/src/lib.rs`](crates/metashrew-core/src/lib.rs:200)

Metashrew WASM modules must implement:

**Main Indexer Function:**
```rust
#[no_mangle]
pub extern "C" fn _start() {
    // Main indexing logic executed for each block
    // This is the WASM entrypoint that gets called by the runtime
}
```

**Optional View Functions:**
```rust
#[no_mangle]
pub extern "C" fn view_function_name() -> i32 {
    // Query logic returning pointer to result data
}
```

#### 5.1.2 Memory Layout Requirements
- **Linear Memory**: WASM modules use standard linear memory model
- **ArrayBuffer Convention**: All data exchange uses length-prefixed format
- **Pointer Arithmetic**: 32-bit addressing within WASM memory space

### 5.2 Guest-Side API Implementation

#### 5.2.1 Core Storage Functions
**Location:** [`crates/metashrew-core/src/lib.rs`](crates/metashrew-core/src/lib.rs:45)

**High-Level API:**
```rust
pub fn get(key: &[u8]) -> Option<Vec<u8>>
pub fn set(key: &[u8], value: &[u8])
pub fn flush()
pub fn input() -> Vec<u8>
```

**Implementation Details:**
- [`get()`](crates/metashrew-core/src/lib.rs:45): Retrieves value with caching layer
- [`set()`](crates/metashrew-core/src/lib.rs:67): Queues write operation
- [`flush()`](crates/metashrew-core/src/lib.rs:89): Commits pending operations to storage
- [`input()`](crates/metashrew-core/src/lib.rs:123): Retrieves block data for current execution

#### 5.2.2 Caching Layer Implementation
**Location:** [`crates/metashrew-core/src/lib.rs`](crates/metashrew-core/src/lib.rs:15)

**Global State Management:**
```rust
static mut CACHE: Option<HashMap<Vec<u8>, Vec<u8>>> = None;
static mut TO_FLUSH: Option<HashMap<Vec<u8>, Vec<u8>>> = None;
```

**Design Considerations:**
- **Write Batching**: Accumulate writes before flushing to storage
- **Read Caching**: Cache frequently accessed values within block execution
- **Memory Management**: Clear cache between block executions for determinism

#### 5.2.3 IndexPointer System
**Location:** [`crates/metashrew-core/src/index_pointer.rs`](crates/metashrew-core/src/index_pointer.rs:8)

High-level abstraction for hierarchical data structures:

**Structure Definition:**
```rust
#[derive(Clone)]
pub struct IndexPointer {
    key: Arc<Vec<u8>>,
}
```

**Key Methods:**
- [`select(index: u32) -> IndexPointer`](crates/metashrew-core/src/index_pointer.rs:45): Create child pointer with index
- [`field(field: &[u8]) -> IndexPointer`](crates/metashrew-core/src/index_pointer.rs:67): Create child pointer with field name
- [`get_value<T: ByteView>() -> T`](crates/metashrew-core/src/index_pointer.rs:89): Type-safe value retrieval
- [`set_value<T: ByteView>(value: T)`](crates/metashrew-core/src/index_pointer.rs:123): Type-safe value storage

---

## 6. Bitcoin Protocol Support

### 6.1 Extended Block Parsing

#### 6.1.1 AuxPoW Support
**Location:** [`crates/metashrew-support/src/block.rs`](crates/metashrew-support/src/block.rs:15)

Metashrew supports Auxiliary Proof of Work for merged-mined cryptocurrencies:

**AuxpowHeader Structure:**
```rust
#[derive(Clone, Debug)]
pub struct AuxpowHeader {
    pub version: AuxpowVersion,
    pub prev_blockhash: BlockHash,
    pub merkle_root: TxMerkleNode,
    pub time: u32,
    pub bits: CompactTarget,
    pub nonce: u32,
    pub auxpow: Option<Box<Auxpow>>,
}
```

**Auxpow Structure:**
```rust
#[derive(Clone, Debug)]
pub struct Auxpow {
    pub coinbase_txn: Transaction,
    pub block_hash: BlockHash,
    pub coinbase_branch: AuxpowMerkleBranch,
    pub blockchain_branch: AuxpowMerkleBranch,
    pub parent_block: AuxpowHeader,
}
```

#### 6.1.2 Version Encoding
**Location:** [`crates/metashrew-support/src/block.rs`](crates/metashrew-support/src/block.rs:89)

**AuxpowVersion Implementation:**
```rust
impl AuxpowVersion {
    pub fn is_auxpow(&self) -> bool {
        self.unwrap() & VERSION_AUXPOW != 0
    }
    
    pub fn chain_id(&self) -> u32 {
        self.unwrap() / VERSION_CHAIN_START
    }
    
    pub fn base_version(&self) -> u32 {
        self.unwrap() % VERSION_AUXPOW
    }
}
```

**Constants:**
```rust
pub const VERSION_AUXPOW: u32 = 0x100;
pub const VERSION_CHAIN_START: u32 = 0x10000;
```

### 6.2 Address Handling

#### 6.2.1 Address Types
**Location:** [`crates/metashrew-support/src/address.rs`](crates/metashrew-support/src/address.rs:15)

**AddressType Enumeration:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressType {
    P2pkh,   // Pay to Public Key Hash
    P2sh,    // Pay to Script Hash
    P2wpkh,  // Pay to Witness Public Key Hash
    P2wsh,   // Pay to Witness Script Hash
    P2tr,    // Pay to Taproot
}
```

#### 6.2.2 Payload Extraction
**Location:** [`crates/metashrew-support/src/address.rs`](crates/metashrew-support/src/address.rs:45)

**Payload Enumeration:**
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    PubkeyHash(PubkeyHash),
    ScriptHash(ScriptHash),
    WitnessProgram(WitnessProgram),
}
```

**Script Analysis:**
```rust
impl Payload {
    pub fn from_script(script: &Script) -> Result<Payload> {
        if script.is_p2pkh() {
            let bytes = script.as_bytes()[3..23].try_into().expect("statically 20B long");
            Ok(Payload::PubkeyHash(PubkeyHash::from_byte_array(bytes)))
        } else if script.is_p2sh() {
            let bytes = script.as_bytes()[2..22].try_into().expect("statically 20B long");
            Ok(Payload::ScriptHash(ScriptHash::from_byte_array(bytes)))
        } else if script.is_witness_program() {
            let witness_program = WitnessProgram::new(version, &program)?;
            Ok(Payload::WitnessProgram(witness_program))
        } else {
            Err(anyhow!("unrecognized script"))
        }
    }
}
```

### 6.3 Consensus Encoding

#### 6.3.1 Bitcoin Data Parsing
**Location:** [`crates/metashrew-support/src/utils.rs`](crates/metashrew-support/src/utils.rs:45)

**Core Parsing Functions:**
```rust
pub fn consensus_decode<T: Decodable>(cursor: &mut std::io::Cursor<Vec<u8>>) -> Result<T>
pub fn consume_varint(cursor: &mut std::io::Cursor<Vec<u8>>) -> Result<u64>
pub fn consume_sized_int<T: FromBytes>(cursor: &mut std::io::Cursor<Vec<u8>>) -> Result<T>
```

**Varint Decoding:**
```rust
pub fn consume_varint(cursor: &mut std::io::Cursor<Vec<u8>>) -> Result<u64> {
    Ok(match consume_sized_int::<u8>(cursor)? {
        0xff => consume_sized_int::<u64>(cursor)?,
        0xfe => consume_sized_int::<u32>(cursor)? as u64,
        0xfd => consume_sized_int::<u16>(cursor)? as u64,
        v => v as u64,
    })
}
```

#### 6.3.2 Utility Functions
**Location:** [`crates/metashrew-support/src/utils.rs`](crates/metashrew-support/src/utils.rs:123)

**Memory and Conversion Utilities:**
```rust
pub fn ptr_to_vec(ptr: i32) -> Vec<u8>
pub fn vec_to_ptr(v: Vec<u8>) -> i32
pub fn reverse_hex(hex: &str) -> String
pub fn format_hex(bytes: &[u8]) -> String
```

---

## 7. Cryptographic Primitives

### 7.1 Hash Function Integration

#### 7.1.1 SMT Hash Operations
**Location:** [`crates/metashrew-runtime/src/smt.rs`](crates/metashrew-runtime/src/smt.rs:456)

**Core Hashing Methods:**
```rust
impl<T: KeyValueStoreLike> SMTHelper<T> {
    pub fn hash_key(&self, key: &[u8]) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.finalize().to_vec()
    }
    
    pub fn hash_value(&self, value: &[u8]) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(value);
        hasher.finalize().to_vec()
    }
    
    pub fn hash_node(&self, node: &SMTNode) -> Vec<u8> {
        match node {
            SMTNode::Internal { left_child, right_child } => {
                let mut hasher = Sha256::new();
                hasher.update(&[0x01]); // Internal node prefix
                hasher.update(left_child);
                hasher.update(right_child);
                hasher.finalize().to_vec()
            }
            SMTNode::Leaf { key, value_index } => {
                let mut hasher = Sha256::new();
                hasher.update(&[0x00]); // Leaf node prefix
                hasher.update(self.hash_key(key));
                hasher.update(&value_index.to_le_bytes());
                hasher.finalize().to_vec()
            }
        }
    }
}
```

#### 7.1.2 State Root Calculation
**Location:** [`crates/metashrew-runtime/src/smt.rs`](crates/metashrew-runtime/src/smt.rs:234)

**Root Computation Process:**
1. **Leaf Hashing**: Combine key hash with value index
2. **Internal Hashing**: Combine child node hashes with prefix
3. **Tree Traversal**: Bottom-up hash computation
4. **Root Storage**: Height-indexed root storage for historical queries

### 7.2 Cryptographic Commitments

#### 7.2.1 State Commitment Scheme
**Design Principles:**
- **Deterministic**: Same input always produces same state root
- **Incremental**: Efficient updates without full recomputation
- **Verifiable**: State roots can be independently verified
- **Historical**: All historical state roots are preserved

#### 7.2.2 Merkle Proof Generation
**Location:** [`crates/metashrew-runtime/src/smt.rs`](crates/metashrew-runtime/src/smt.rs:567)

**Proof Structure:**
```rust
pub struct MerkleProof {
    pub key: Vec<u8>,
    pub value: Option<Vec<u8>>,
    pub siblings: Vec<Vec<u8>>,
    pub path: Vec<bool>,
}
```

---

## 8. API and Query Layer

### 8.1 JSON-RPC Interface

Metashrew provides a comprehensive JSON-RPC API that allows applications to query indexed Bitcoin data and interact with the indexer. The API follows standard JSON-RPC 2.0 conventions and provides both synchronous and asynchronous access to indexed data.

#### 8.1.1 API Overview

**Base URL Structure:**
```
POST http://localhost:8080/
Content-Type: application/json
```

**Standard JSON-RPC Request Format:**
```json
{
  "jsonrpc": "2.0",
  "method": "metashrew_view",
  "params": {
    "function": "get_balance",
    "input": "0x1234567890abcdef",
    "height": "latest"
  },
  "id": 1
}
```

**Standard JSON-RPC Response Format:**
```json
{
  "jsonrpc": "2.0",
  "result": "0xabcdef1234567890",
  "id": 1
}
```

#### 8.1.2 Core API Methods

**1. metashrew_view**
Query indexed data by calling a view function in the WASM indexer.

**Parameters:** `[function_name, input_data, height]`
- `function_name` (string): Name of the view function to call
- `input_data` (string): Hex-encoded input data with 0x prefix
- `height` (string|number): Block height for historical queries or "latest" for current state

```json
{
  "method": "metashrew_view",
  "params": ["get_balance", "0x1234567890abcdef", "latest"]
}
```

**Response:**
```json
{
  "result": "0xabcdef1234567890"        // Hex-encoded result data
}
```

**2. metashrew_preview**
Test indexer logic against a specific block without persisting state changes.

**Parameters:** `[block_data, function_name, input_data]`
- `block_data` (string): Hex-encoded block data with 0x prefix (can be built from mempool)
- `function_name` (string): Name of the view function to call
- `input_data` (string): Hex-encoded input data with 0x prefix

```json
{
  "method": "metashrew_preview",
  "params": ["0x0100000000000000...", "get_balance", "0x1234567890abcdef"]
}
```

**Note:** Preview executes the block data first (without side effects), then runs the view function on the resulting state.

**3. metashrew_stateroot**
Get the cryptographic state root at a specific block height.

**Parameters**: `["latest"|blockheight]` (optional)
- Single parameter: Block height (number) or "latest" (string) for current height
- If no parameters provided, defaults to latest height

**Examples:**
```json
{
  "method": "metashrew_stateroot",
  "params": ["latest"]
}
```

```json
{
  "method": "metashrew_stateroot",
  "params": [850000]
}
```

```json
{
  "method": "metashrew_stateroot",
  "params": []
}
```

**Response:**
```json
{
  "result": "0x1234567890abcdef..."     // 32-byte state root hash
}
```

**4. metashrew_height**
Get the current indexed block height.

**Parameters**: `[]` (no parameters)

**Example:**
```json
{
  "method": "metashrew_height",
  "params": []
}
```

**Response:**
```json
{
  "result": 850000                      // Current indexed height as number
}
```

#### 8.1.3 Height Parameters

**Height Specification:**
- `"latest"`: Most recent indexed block
- `"123456"`: Specific block height (decimal string)
- `"0x1e240"`: Specific block height (hex string)

**Height Validation:**
- Heights beyond the current tip return an error
- Historical queries use the optimized BST for efficient lookups
- Invalid height formats return a parsing error

#### 8.1.4 Error Handling

**Standard Error Response:**
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32603,
    "message": "Internal error",
    "data": "Storage error: Key not found"
  },
  "id": 1
}
```

**Common Error Codes:**
- `-32700`: Parse error (invalid JSON)
- `-32600`: Invalid request (malformed JSON-RPC)
- `-32601`: Method not found
- `-32602`: Invalid params
- `-32603`: Internal error (storage, runtime, or node errors)

#### 8.1.5 JsonRpcProvider Trait
**Location:** [`crates/rockshrew-sync/src/traits.rs`](crates/rockshrew-sync/src/traits.rs:75)

**RPC Method Definitions:**
```rust
#[async_trait]
pub trait JsonRpcProvider: Send + Sync {
    async fn metashrew_view(&self, function_name: String, input_hex: String, height: String) -> SyncResult<String>;
    async fn metashrew_preview(&self, block_hex: String, function_name: String, input_hex: String, height: String) -> SyncResult<String>;
    async fn metashrew_stateroot(&self, height: String) -> SyncResult<String>;
}
```

#### 8.1.2 View Function Execution
**Location:** [`crates/rockshrew-sync/src/sync.rs`](crates/rockshrew-sync/src/sync.rs:678)

**View Call Structure:**
```rust
pub struct ViewCall {
    pub function_name: String,
    pub input_data: Vec<u8>,
    pub height: u32,
}

pub struct ViewResult {
    pub data: Vec<u8>,
    pub gas_used: Option<u64>,
}
```

**Implementation:**
```rust
async fn metashrew_view(&self, function_name: String, input_hex: String, height: String) -> SyncResult<String> {
    let input_data = hex::decode(input_hex.trim_start_matches("0x"))?;
    let height = if height == "latest" {
        self.current_height.load(Ordering::SeqCst).saturating_sub(1)
    } else {
        height.parse::<u32>()?
    };
    
    let call = ViewCall { function_name, input_data, height };
    let result = self.runtime.read().await.execute_view(call).await?;
    Ok(format!("0x{}", hex::encode(result.data)))
}
```

#### 8.1.3 Preview Function Execution
**Location:** [`crates/rockshrew-sync/src/sync.rs`](crates/rockshrew-sync/src/sync.rs:702)

**Preview Call Structure:**
```rust
pub struct PreviewCall {
    pub block_data: Vec<u8>,
    pub function_name: String,
    pub input_data: Vec<u8>,
    pub height: String,
}
```

**Design Purpose:**
- **Testing**: Test indexer logic without state persistence
- **Simulation**: Simulate block processing effects
- **Development**: Debug indexer behavior during development

### 8.2 State Root Queries

#### 8.2.1 State Root Retrieval
**Location:** [`crates/rockshrew-sync/src/sync.rs`](crates/rockshrew-sync/src/sync.rs:721)

**Implementation:**
```rust
async fn metashrew_stateroot(&self, height: String) -> SyncResult<String> {
    let height = if height == "latest" {
        self.current_height.load(Ordering::SeqCst).saturating_sub(1)
    } else {
        height.parse::<u32>()?
    };
    
    let storage = self.storage.read().await;
    match storage.get_state_root(height).await? {
        Some(root) => Ok(format!("0x{}", hex::encode(root))),
        None => Err(SyncError::Storage(format!("State root not found for height {}", height))),
    }
}
```

---

## 9. Security Model

### 9.1 WebAssembly Sandboxing

#### 9.1.1 Isolation Mechanisms
1. **Memory Isolation**: Each WASM module runs in isolated linear memory space
2. **Capability-Based Security**: Limited access to host functions through defined imports
3. **Deterministic Execution**: Reproducible results across different environments
4. **Resource Limits**: Configurable memory and execution time limits

#### 9.1.2 Host Function Security
**Access Control:**
- **Storage Operations**: Controlled through host function interface
- **Block Data**: Read-only access to current block being processed
- **Logging**: Safe output mechanism for debugging
- **No File System**: No direct file system access from WASM modules

### 9.2 State Integrity

#### 9.2.1 Cryptographic Commitments
**Location:** [`crates/metashrew-runtime/src/traits.rs`](crates/metashrew-runtime/src/traits.rs:35)

**AtomicBlockResult Guarantees:**
```rust
pub struct AtomicBlockResult {
    pub state_root: Vec<u8>,    // Cryptographic commitment to all state changes
    pub batch_data: Vec<u8>,    // Serialized database operations for atomic application
    pub height: u32,            // Block height for ordering
    pub block_hash: Vec<u8>,    // Block identifier for consistency
}
```

**Security Properties:**
- **Atomicity**: All state changes applied atomically or not at all
- **Consistency**: State root provides cryptographic proof of state integrity
- **Durability**: All changes persisted to storage before commitment
- **Isolation**: Each block processed independently

#### 9.2.2 Chain Reorganization Protection
**Location:** [`crates/rockshrew-sync/src/sync.rs`](crates/rockshrew-sync/src/sync.rs:574)

**Protection Mechanisms:**
1. **Block Hash Verification**: Compare local and remote block hashes
2. **Rollback Detection**: Identify divergence points in chain history
3. **State Rollback**: Revert state to common ancestor
4. **Reprocessing**: Apply new canonical chain from divergence point

### 9.3 Input Validation

#### 9.3.1 Validation Layers
1. **Block Data**: Bitcoin consensus validation before processing
2. **WASM Modules**: Module signature verification and validation
3. **API Inputs**: Type checking and bounds validation for all RPC calls
4. **Storage Keys**: Prefix validation and access control for key spaces

#### 9.3.2 Error Handling
**Location:** [`crates/rockshrew-sync/src/traits.rs`](crates/rockshrew-sync/src/traits.rs:95)

**SyncError Enumeration:**
```rust
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Runtime error: {0}")]
    Runtime(String),
    #[error("Node error: {0}")]
    Node(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
}
```

---

## 10. Performance Characteristics

### 10.1 Throughput Metrics

#### 10.1.1 Performance Targets
- **Block Processing**: 100-1000 blocks/second (depending on indexer complexity)
- **Query Response**: <10ms for typical view functions
- **Memory Usage**: 100MB-1GB (configurable based on cache size)
- **Storage Growth**: ~1-10GB per million blocks (indexer dependent)

#### 10.1.2 Bottleneck Analysis
**Primary Bottlenecks:**
1. **Storage I/O**: Database read/write operations
2. **WASM Execution**: Indexer computation complexity
3. **Network I/O**: Bitcoin node communication
4. **Serialization**: Data encoding/decoding overhead

### 10.2 Optimization Strategies

#### 10.2.1 Pipeline Parallelism
**Location:** [`crates/rockshrew-sync/src/sync.rs`](crates/rockshrew-sync/src/sync.rs:802)

**Pipeline Configuration:**
```rust
let pipeline_size = self.config.pipeline_size.unwrap_or_else(|| {
    let cpu_count = num_cpus::get();
    std::cmp::min(std::cmp::max(5, cpu_count / 2), 16)
});
```

**Pipeline Stages:**
1. **Block Fetching**: Asynchronous block retrieval from Bitcoin nodes
2. **Block Processing**: WASM module execution for indexing
3. **Result Handling**: Error recovery and progress tracking
4. **State Commitment**: Atomic state updates and persistence

#### 10.2.2 Batch Operations
**Location:** [`crates/metashrew-runtime/src/smt.rs`](crates/metashrew-runtime/src/smt.rs:200)

**BatchedSMTHelper Optimization:**
```rust
pub struct BatchedSMTHelper<T: KeyValueStoreLike> {
    smt: SMTHelper<T>,
    pending_operations: HashMap<Vec<u8>, Vec<u8>>,
    cache: HashMap<Vec<u8>, SMTNode>,
}
```

**Optimization Benefits:**
- **Reduced I/O**: Batch multiple operations into single database transaction
- **Cache Efficiency**: Reuse frequently accessed nodes within batch
- **Atomic Updates**: Ensure consistency across multiple operations

#### 10.2.3 OptimizedBST Performance
**Location:** [`crates/metashrew-runtime/src/optimized_bst.rs`](crates/metashrew-runtime/src/optimized_bst.rs:67)

**Performance Characteristics:**
- **O(1) Current Access**: Direct lookup for most recent values
- **O(log n) Historical**: Binary search only for historical queries
- **Efficient Reorgs**: Track keys by height for fast rollback
- **Append-Only**: Maintains complete history without modification

---

## 11. Complete Source Code Mapping

### 11.1 Core Components

| Component | Source Location | Key Types | Purpose |
|-----------|----------------|-----------|---------|
| **Runtime Engine** | [`crates/metashrew-runtime/src/runtime.rs`](crates/metashrew-runtime/src/runtime.rs) | [`MetashrewRuntime<T>`](crates/metashrew-runtime/src/runtime.rs:25) | WASM execution environment |
| **Storage Traits** | [`crates/metashrew-runtime/src/traits.rs`](crates/metashrew-runtime/src/traits.rs) | [`KeyValueStoreLike`](crates/metashrew-runtime/src/traits.rs:8), [`BatchLike`](crates/metashrew-runtime/src/traits.rs:25) | Storage abstraction |
| **SMT Implementation** | [`crates/metashrew-runtime/src/smt.rs`](crates/metashrew-runtime/src/smt.rs) | [`SMTHelper<T>`](crates/metashrew-runtime/src/smt.rs:15), [`SMTNode`](crates/metashrew-runtime/src/smt.rs:35) | Sparse Merkle Tree |
| **Optimized BST** | [`crates/metashrew-runtime/src/optimized_bst.rs`](crates/metashrew-runtime/src/optimized_bst.rs) | [`OptimizedBST<T>`](crates/metashrew-runtime/src/optimized_bst.rs:21) | High-performance storage |
| **Runtime Context** | [`crates/metashrew-runtime/src/context.rs`](crates/metashrew-runtime/src/context.rs) | [`MetashrewRuntimeContext<T>`](crates/metashrew-runtime/src/context.rs:8) | Execution state |
| **WASM Bindings** | [`crates/metashrew-core/src/lib.rs`](crates/metashrew-core/src/lib.rs) | Host functions, exports | Guest-side API |
| **WASM Imports** | [`crates/metashrew-core/src/imports.rs`](crates/metashrew-core/src/imports.rs) | Import declarations | Host function bindings |
| **Sync Engine** | [`crates/rockshrew-sync/src/sync.rs`](crates/rockshrew-sync/src/sync.rs) | [`MetashrewSync<N,S,R>`](crates/rockshrew-sync/src/sync.rs:25) | Synchronization framework |
| **Adapter Traits** | [`crates/rockshrew-sync/src/traits.rs`](crates/rockshrew-sync/src/traits.rs) | [`BitcoinNodeAdapter`](crates/rockshrew-sync/src/traits.rs:15), [`StorageAdapter`](crates/rockshrew-sync/src/traits.rs:35) | Pluggable backends |
| **Bitcoin Support** | [`crates/metashrew-support/src/block.rs`](crates/metashrew-support/src/block.rs) | [`AuxpowHeader`](crates/metashrew-support/src/block.rs:15), [`AuxpowBlock`](crates/metashrew-support/src/block.rs:45) | Extended block parsing |
| **Address Handling** | [`crates/metashrew-support/src/address.rs`](crates/metashrew-support/src/address.rs) | [`Payload`](crates/metashrew-support/src/address.rs:45), [`AddressType`](crates/metashrew-support/src/address.rs:15) | Bitcoin address parsing |
| **Index Pointers** | [`crates/metashrew-support/src/index_pointer.rs`](crates/metashrew-support/src/index_pointer.rs) | [`KeyValuePointer`](crates/metashrew-support/src/index_pointer.rs:150) | Hierarchical data structures |
| **Memory Management** | [`crates/metashrew-support/src/compat.rs`](crates/metashrew-support/src/compat.rs) | Memory utilities | WASM memory management |
| **Utilities** | [`crates/metashrew-support/src/utils.rs`](crates/metashrew-support/src/utils.rs) | Parsing functions | Bitcoin data parsing |
| **Byte Serialization** | [`crates/metashrew-support/src/byte_view.rs`](crates/metashrew-support/src/byte_view.rs) | [`ByteView`](crates/metashrew-support/src/byte_view.rs:8) | Type-safe serialization |

### 11.2 Function Mapping

#### 11.2.1 Core Runtime Functions

| Function | Location | Purpose |
|----------|----------|---------|
| [`MetashrewRuntime::new()`](crates/metashrew-runtime/src/runtime.rs:45) | Runtime initialization | Create new runtime instance |
| [`MetashrewRuntime::run()`](crates/metashrew-runtime/src/runtime.rs:123) | Block processing | Execute indexer for current block |
| [`MetashrewRuntime::view()`](crates/metashrew-runtime/src/runtime.rs:200) | Query execution | Execute view function at height |
| [`MetashrewRuntime::preview()`](crates/metashrew-runtime/src/runtime.rs:250) | Testing | Test indexing logic without persistence |
| [`MetashrewRuntime::refresh_memory()`](crates/metashrew-runtime/src/runtime.rs:89) | Memory management | Reset WASM memory for determinism |
| [`SMTHelper::compute_root()`](crates/metashrew-runtime/src/smt.rs:456) | State commitment | Calculate cryptographic state root |
| [`SMTHelper::set()`](crates/metashrew-runtime/src/smt.rs:234) | State update | Update SMT with key-value pair |
| [`OptimizedBST::get_current()`](crates/metashrew-runtime/src/optimized_bst.rs:67) | Current state | O(1) current value lookup |
| [`OptimizedBST::get_at_height()`](crates/metashrew-runtime/src/optimized_bst.rs:91) | Historical query | Binary search for historical values |

#### 11.2.2 Host Functions

| Function | Location | WASM Import | Purpose |
|----------|----------|-------------|---------|
| [`__host_len()`](crates/metashrew-runtime/src/runtime.rs:320) | `env.__host_len` | Get input data length |
| [`__load_input()`](crates/metashrew-runtime/src/runtime.rs:325) | `env.__load_input` | Load input to WASM memory |
| [`__get()`](crates/metashrew-runtime/src/runtime.rs:340) | `env.__get` | Get value by key |
| [`__get_len()`](crates/metashrew-runtime/src/runtime.rs:360) | `env.__get_len` | Get value length by key |
| [`__flush()`](crates/metashrew-runtime/src/runtime.rs:380) | `env.__flush` | Commit pending operations |
| [`__log()`](crates/metashrew-runtime/src/runtime.rs:400) | `env.__log` | Output log message |

#### 11.2.3 Synchronization Functions

| Function | Location | Purpose |
|----------|----------|---------|
| [`MetashrewSync::start()`](crates/rockshrew-sync/src/sync.rs:502) | Begin synchronization process |
| [`MetashrewSync::process_block()`](crates/rockshrew-sync/src/sync.rs:127) | Process single block |
| [`MetashrewSync::handle_reorg()`](crates/rockshrew-sync/src/sync.rs:574) | Handle chain reorganization |
| [`MetashrewSync::metashrew_view()`](crates/rockshrew-sync/src/sync.rs:678) | Execute view function |
| [`MetashrewSync::metashrew_preview()`](crates/rockshrew-sync/src/sync.rs:702) | Execute preview function |
| [`MetashrewSync::metashrew_stateroot()`](crates/rockshrew-sync/src/sync.rs:721) | Get state root at height |

### 11.3 Trait Implementations

#### 11.3.1 Storage Traits

| Trait | Implementors | Purpose |
|-------|-------------|---------|
| [`KeyValueStoreLike`](crates/metashrew-runtime/src/traits.rs:8) | RocksDB, Memory | Storage backend abstraction |
| [`BatchLike`](crates/metashrew-runtime/src/traits.rs:25) | RocksDB Batch | Atomic operation batching |
| [`BSTHelper`](crates/metashrew-runtime/src/helpers.rs:31) | OptimizedBST | BST operation interface |

#### 11.3.2 Synchronization Traits

| Trait | Purpose |
|-------|---------|
| [`BitcoinNodeAdapter`](crates/rockshrew-sync/src/traits.rs:15) | Bitcoin node communication |
| [`StorageAdapter`](crates/rockshrew-sync/src/traits.rs:35) | Persistent storage operations |
| [`RuntimeAdapter`](crates/rockshrew-sync/src/traits.rs:55) | WASM runtime execution |
| [`JsonRpcProvider`](crates/rockshrew-sync/src/traits.rs:75) | JSON-RPC API interface |

#### 11.3.3 Support Traits

| Trait | Purpose |
|-------|---------|
| [`KeyValuePointer`](crates/metashrew-support/src/index_pointer.rs:150) | Hierarchical data structures |
| [`ByteView`](crates/metashrew-support/src/byte_view.rs:8) | Type-safe serialization |

---

## 12. Conclusion

Metashrew provides a comprehensive, production-ready framework for Bitcoin blockchain indexing with the following key advantages:

### 12.1 Technical Strengths

1. **Modularity**: Clean separation of concerns through generic traits and adapter patterns
2. **Performance**: High-throughput processing with optimized storage and pipeline architecture
3. **Reliability**: Atomic operations, comprehensive error handling, and chain reorganization support
4. **Extensibility**: WebAssembly-based indexer modules enable custom logic without framework changes
5. **Auditability**: Complete source code mapping and cryptographic state commitments

### 12.2 Design Principles

1. **Generic Programming**: Extensive use of Rust generics enables flexible backend implementations
2. **Trait-Based Architecture**: Clean abstractions through well-defined trait boundaries
3. **Deterministic Execution**: Reproducible results through careful WASM runtime management
4. **Cryptographic Integrity**: State commitments provide verifiable data integrity
5. **Performance Optimization**: Multiple optimization layers from caching to batch operations

### 12.3 Production Readiness

The framework has been designed with security, performance, and maintainability as primary concerns, making it suitable for both research applications and production deployments:

1. **Security**: WebAssembly sandboxing, cryptographic state commitments, and comprehensive input validation
2. **Performance**: Optimized storage layers, pipeline parallelism, and efficient batch operations
3. **Maintainability**: Clean trait-based architecture with comprehensive documentation and source code mapping
4. **Scalability**: Generic design enables adaptation to different storage backends and Bitcoin node implementations
5. **Reliability**: Atomic operations, chain reorganization handling, and comprehensive error recovery

### 12.4 Future Considerations

The Metashrew architecture provides a solid foundation for future enhancements:

1. **Additional Cryptocurrencies**: The generic adapter pattern enables support for other blockchain networks
2. **Advanced Indexing**: The WASM module system allows for sophisticated indexing strategies
3. **Distributed Deployment**: The modular design supports distributed indexing architectures
4. **Performance Optimization**: Continued optimization of storage and execution layers
5. **Enhanced Security**: Additional security measures and formal verification capabilities

This specification provides a complete technical reference that maps directly to the source code, enabling developers, auditors, and researchers to understand and verify the implementation details of the Metashrew Bitcoin indexer framework.

---

**Document Version:** 2.0
**Last Updated:** June 24th, 2025
**License:** MIT License
**Repository:** https://github.com/sandshrewmetaprotocols/metashrew

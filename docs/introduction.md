# Introduction to Metashrew

## What is Metashrew?

Metashrew is a Bitcoin indexer framework with a WebAssembly (WASM) virtual machine. It's designed to simplify the development of Bitcoin metaprotocols and indexers by allowing developers to focus solely on the logic of processing individual blocks. The framework provides a portable calling convention compatible with AssemblyScript, C, Rust, or any language that can compile to WASM.

At its core, Metashrew enables developers to build custom indexers as WASM modules that can process Bitcoin blockchain data efficiently and deterministically. This approach allows for flexible and modular development of blockchain data processing applications without modifying the core indexing engine.

## Key Features and Benefits

### 1. WebAssembly-Based Execution

Metashrew uses WebAssembly as its execution environment, which provides several advantages:

- **Language Agnostic**: Write indexers in any language that compiles to WASM (Rust, AssemblyScript, C, etc.)
- **Sandboxed Execution**: WASM provides a secure, isolated execution environment
- **Deterministic Processing**: Ensures consistent results across different environments
- **Portable**: WASM modules can run on any platform that supports WebAssembly

### 2. Append-Only Database

Metashrew uses an append-only database design that:

- **Preserves Historical Data**: Never overwrites values, enabling historical queries
- **Supports Chain Reorganizations**: Can efficiently handle Bitcoin chain reorganizations
- **Maintains State Integrity**: Uses height annotation to track state changes
- **Enables Efficient Queries**: With the Height-Indexed BST structure, historical state queries are efficient

### 3. Modular Architecture

The framework is built with modularity in mind:

- **Separation of Concerns**: Core indexing engine is separate from custom indexing logic
- **Pluggable Storage Backends**: Support for different storage solutions (RocksDB, DynamoDB)
- **Flexible Deployment**: Can run as a single process or as separate indexer and view services

### 4. JSON-RPC API

Metashrew provides a comprehensive JSON-RPC API that:

- **Exposes Indexed Data**: Makes indexed data accessible via standard JSON-RPC calls
- **Supports View Functions**: Custom query functions defined in WASM modules
- **Enables Historical Queries**: Query the state at any historical block height
- **Integrates with Existing Tools**: Compatible with standard Bitcoin RPC clients

## Use Cases

Metashrew is particularly well-suited for the following use cases:

### 1. Bitcoin Metaprotocols

Metashrew provides an ideal foundation for building Bitcoin metaprotocols like:

- **Token Systems**: Implement token standards on Bitcoin (similar to Ordinals, BRC-20)
- **Smart Contract Platforms**: Build deterministic execution environments for Bitcoin
- **Decentralized Finance (DeFi)**: Create financial applications on Bitcoin
- **Digital Identity Systems**: Implement identity and reputation systems

### 2. Blockchain Data Indexing

Metashrew excels at custom blockchain data indexing:

- **Address Balances**: Track balances and transaction history for addresses
- **UTXO Indexing**: Create specialized UTXO indexes for specific applications
- **Script Pattern Analysis**: Identify and index specific Bitcoin script patterns
- **Metadata Extraction**: Extract and index metadata embedded in transactions

### 3. Analytics and Monitoring

The framework can be used for advanced analytics:

- **Network Statistics**: Track and analyze Bitcoin network metrics
- **Transaction Patterns**: Identify and analyze transaction patterns
- **Fee Estimation**: Build sophisticated fee estimation models
- **Market Analysis**: Analyze on-chain market indicators

## Why Metashrew?

Metashrew addresses several challenges in Bitcoin development:

1. **Complexity Reduction**: Abstracts away the complexities of Bitcoin block processing
2. **Development Speed**: Accelerates development of Bitcoin applications
3. **Flexibility**: Supports various languages and storage backends
4. **Scalability**: Designed for efficient processing of large blockchain datasets
5. **Determinism**: Ensures consistent results across different environments
6. **Modularity**: Enables reuse of components across different projects

By providing a robust framework for Bitcoin indexing and metaprotocol development, Metashrew empowers developers to build sophisticated applications on Bitcoin without having to reinvent the wheel for core indexing functionality.

## Getting Started

To start using Metashrew, proceed to the [Architecture Overview](./architecture.md) to understand the system's components and design, or jump directly to the [Development Guide](./development.md) if you're ready to build your first indexer.
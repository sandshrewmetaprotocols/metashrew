# Metashrew Documentation

Welcome to the Metashrew documentation. This guide will help you understand the Metashrew framework, its components, and how to use it effectively for building Bitcoin indexers and metaprotocols.

## Table of Contents

1. [Introduction to Metashrew](./introduction.md)
   - What is Metashrew?
   - Key Features and Benefits
   - Use Cases

2. [Architecture Overview](./architecture.md)
   - System Components
   - Data Flow
   - Design Patterns

3. [Database Structure](./database.md)
   - Append-Only Design
   - Height-Indexed BST
   - State Root Integrity
   - Chain Reorganization Handling

4. [WASM Runtime](./wasm-runtime.md)
   - WebAssembly Host Interface
   - Deterministic Execution
   - Memory Management
   - Wasmtime Configuration

5. [Helper Libraries](./helper-libraries.md)
   - metashrew-core
   - metashrew-support
   - Building Indexers

6. [JSON-RPC Interface](./json-rpc.md)
   - Available Methods
   - Request/Response Format
   - View Functions
   - Historical State Queries

7. [Using rockshrew-mono](./rockshrew-mono.md)
   - Command-Line Options
   - Configuration
   - Deployment Scenarios
   - Performance Tuning

8. [Development Guide](./development.md)
   - Setting Up Development Environment
   - Creating a Custom Indexer
   - Testing and Debugging
   - Best Practices

9. [Advanced Topics](./advanced-topics.md)
   - Snapshot System
   - SSH Tunneling
   - Custom Storage Backends
   - Performance Optimization

## Getting Started

If you're new to Metashrew, we recommend starting with the [Introduction](./introduction.md) and then proceeding to the [Architecture Overview](./architecture.md) to understand the system's design.

For developers looking to build indexers or metaprotocols, the [Helper Libraries](./helper-libraries.md) and [Development Guide](./development.md) sections will be most relevant.

For operators interested in deploying Metashrew, the [Using rockshrew-mono](./rockshrew-mono.md) section provides detailed information on configuration and operation.
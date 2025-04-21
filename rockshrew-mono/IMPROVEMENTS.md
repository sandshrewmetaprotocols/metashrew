# Rockshrew-Mono Improvements

This document outlines the improvements made to the rockshrew-mono component of the Metashrew project.

## SSH Tunneling for RPC Connections

### Overview

Added support for SSH tunneling when connecting to Bitcoin Core RPC servers. This allows for secure remote connections to Bitcoin nodes without exposing the RPC port directly to the internet.

### Features

- **SSH Tunneling URI Format**: Support for special URI formats that indicate SSH tunneling should be used:
  - `ssh2+http://user@ssh-host:ssh-port/target-host:target-port`
  - `ssh2+https://user@ssh-host:ssh-port/target-host:target-port`

- **SSH Config Integration**: Support for SSH config-based connections:
  - `ssh2+http://ssh-host-alias/target-host:target-port`
  - Uses your `~/.ssh/config` for connection details

- **Automatic Port Selection**: Automatically finds an available local port for the SSH tunnel

- **Secure Cleanup**: Automatically closes SSH tunnels when the application exits

- **SSL Certificate Validation**: Intelligently bypasses SSL certificate validation for localhost connections while maintaining security for actual domain names

### Usage Examples

1. **Direct SSH Tunneling**:
   ```
   rockshrew-mono --daemon-rpc-url ssh2+http://ubuntu@mainnet.sandshrew.io:2222/localhost:8332 --auth bitcoinrpc:password
   ```

2. **Using SSH Config**:
   ```
   rockshrew-mono --daemon-rpc-url ssh2+http://mainnet2/localhost:8332 --auth bitcoinrpc:password
   ```

3. **HTTPS with SSH Tunneling**:
   ```
   rockshrew-mono --daemon-rpc-url ssh2+https://mainnet2/localhost:8332 --auth bitcoinrpc:password
   ```

### Implementation Details

The SSH tunneling functionality is implemented in the `improvements.rs` module and integrated into the main application flow. It uses the standard SSH client available on the system to create the tunnel.

## Pipeline Architecture for Block Processing

### Overview

Implemented a multi-stage pipeline for fetching and processing blocks, significantly improving throughput by allowing parallel operations.

### Features

- **Parallel Block Fetching and Processing**: Separate threads for fetching blocks and processing them
- **Dynamic Pipeline Size**: Automatically configures pipeline size based on available CPU cores
- **Thread Isolation**: Dedicated threads for critical tasks with clear role separation
- **Improved Error Handling**: Better error recovery and reporting in the pipeline

### Implementation Details

The pipeline consists of:
1. A block fetcher task that retrieves blocks from the Bitcoin node
2. A block processor task that processes the blocks using the WASM module
3. A main task that coordinates the pipeline and handles results

## Memory Management Improvements

### Overview

Enhanced memory management to prevent out-of-memory errors and improve stability, especially for complex indexers like ALKANES.

### Features

- **Proactive Memory Monitoring**: Continuously monitors WASM memory usage
- **Preemptive Memory Refresh**: Refreshes memory before it reaches critical levels
- **Detailed Memory Statistics**: Logs detailed memory statistics for monitoring and debugging
- **Improved Error Recovery**: Better handling of memory-related errors

### Implementation Details

The system now monitors memory usage and performs preemptive memory refreshes when usage approaches configurable thresholds. This helps prevent out-of-memory errors that could crash the application.

## Dynamic RocksDB Configuration

### Overview

Implemented dynamic configuration of RocksDB based on available system resources for optimal performance.

### Features

- **CPU-Aware Configuration**: Adjusts RocksDB parameters based on available CPU cores
- **Optimized Write Buffers**: Configures write buffer size and count based on system capabilities
- **Balanced Background Jobs**: Sets appropriate number of background jobs for compaction

### Implementation Details

The system now detects available CPU cores and adjusts RocksDB configuration parameters accordingly, providing better performance across different hardware configurations.

## Error Handling and Logging Improvements

### Overview

Enhanced error handling and logging throughout the application for better debugging and stability.

### Features

- **Comprehensive Error Context**: More detailed error messages with context
- **Structured Logging**: Better structured logs with appropriate log levels
- **Recovery Mechanisms**: Improved recovery from transient errors
- **Detailed Memory Statistics**: Logs memory usage statistics at key points

### Implementation Details

Error handling has been improved throughout the codebase, with better context in error messages and more robust recovery mechanisms.
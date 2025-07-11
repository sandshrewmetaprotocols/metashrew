# GPU Parallelization Plan for Metashrew and ALKANES

## 1. Overview

This document outlines a plan to integrate GPU-based parallel processing into the Metashrew framework, specifically for ALKANES transactions. The goal is to significantly accelerate block processing by executing independent transactions concurrently on a GPU.

This change is triggered by a special `(8, 8)` prefix in an `alkanes-rs` `cellpack`, signaling that the transaction is a candidate for parallel execution.

The core challenges are:
1.  Identifying and partitioning non-conflicting transactions.
2.  Creating a secure and sandboxed GPU execution environment.
3.  Translating `alkanes-rs` business logic into a GPU-compatible format.
4.  Ensuring atomicity and correctness, including robust revert mechanisms.

## 2. Proposed Architecture: Two-Phase Block Processing

To accommodate GPU execution, we will introduce a two-phase processing model for each block.

```
flowchart TD
    A[Start Block Processing] --> B{Scan Txs for (8, 8) prefix};
    B --> C[Phase 1: Analysis & Partitioning (CPU)];
    C --> D[Phase 2: Execution (CPU/GPU)];
    D --> E[Commit State Changes];
    E --> F[End Block Processing];

    subgraph Phase 1
        C1[Static Analysis / Dry Run];
        C2[Generate Read/Write Sets];
        C3[Build UTXO Dependency Graph];
        C4[Build Storage Conflict Graph];
        C5[Partition Txs into Non-Conflicting Batches];
        C1 --> C2 --> C3 --> C4 --> C5;
    end

    subgraph Phase 2
        D1[Execute Standard Txs on CPU];
        D2[Dispatch Parallel Batches to GPU];
        D3[Execute Batches via Compute Shader];
        D4[Collect & Verify GPU Results];
        D1 & D2 --> D4;
        D2 --> D3 --> D4;
    end
```

### Phase 1: Analysis & Partitioning (CPU)

This phase happens before any state is modified.

1.  **Transaction Scanning**: Iterate through all transactions in the block. Identify standard transactions and parallel-candidate `(8, 8)` transactions.
2.  **Read/Write Set Generation**: For each `(8, 8)` transaction, perform a "dry run" execution in an instrumented WASM environment. This run will not modify state but will record every key that is read from (`__get`) and written to (`__flush`). This produces a `ReadSet` and a `WriteSet` for each transaction.
3.  **Dependency Validation**:
    *   **UTXO Dependencies**: Build a dependency graph for all transactions in the block. If any `(8, 8)` transaction consumes a UTXO created by another `(8, 8)` transaction in the same block, it is invalid and must be rejected or demoted to sequential CPU execution.
    *   **Storage Conflicts**: Build a conflict graph where an edge connects two `(8, 8)` transactions if:
        *   `TxA.WriteSet` overlaps with `TxB.WriteSet`.
        *   `TxA.WriteSet` overlaps with `TxB.ReadSet`.
        *   `TxB.WriteSet` overlaps with `TxA.ReadSet`.
4.  **Batch Partitioning**: Use the conflict graph to partition the `(8, 8)` transactions into the largest possible batches where no two transactions in a batch have a conflict. This is a graph coloring problem.

### Phase 2: Execution (CPU & GPU)

1.  **CPU Execution**: Process all standard (non-`8,8`) transactions sequentially, as is done currently.
2.  **GPU Dispatch**: For each partitioned batch of parallel transactions:
    *   Gather the union of all `ReadSet` values from the database.
    *   Serialize the transaction payloads and the required database state.
    *   Transfer this data to the GPU.
    *   Invoke the compute kernel.
3.  **Result Collection**:
    *   Transfer the results (the new `WriteSet` values) back from the GPU.
    *   Verify that no transaction failed or violated its access permissions.
4.  **State Commit**: Atomically commit the `WriteSet` from both the CPU and successful GPU executions to RocksDB in a single batch.

## 3. Metashrew Runtime Changes

### New Host Functions

The `metashrew-runtime` will need new host functions to support this workflow.

-   `__is_dry_run() -> i32`: Returns `1` if the current execution is a dry run for Read/Write set analysis, `0` otherwise.
-   `__record_get(key_ptr: i32)`: Called by the WASM module during a dry run to record a key read.
-   `__record_put(key_ptr: i32)`: Called by the WASM module during a dry run to record a key write.

The existing `__get` and `__flush` functions in `alkanes-rs` would be modified to call these new host functions when in a dry run.

### Indexer Core Logic (`rockshrew-mono`)

-   **Two-Phase Loop**: The main block processing loop must be refactored to implement the two-phase architecture.
-   **Instrumented WASM Instance**: A mechanism to run a WASM module in a sandboxed "dry run" mode that captures calls to `__record_get` and `__record_put`.
-   **Transaction Partitioner**: A new module implementing the conflict graph and partitioning logic.
-   **GPU Dispatcher**: A new module responsible for interfacing with the chosen GPU API (e.g., `wgpu`). This module will handle data serialization, buffer management, shader compilation, and kernel invocation.

## 4. `alkanes-rs` Changes

-   **Entry Point Modification**: The main `_start` function will first check for the `(8, 8)` prefix.
-   **Conditional Logic**:
    *   If the prefix is **not** found, execute the transaction normally.
    *   If the prefix **is** found, the transaction does nothing during the main block processing loop. Its logic is only executed via the GPU kernel.
-   **Dry Run Support**: The internal database access functions (`get`, `put`, `flush`) must be updated to check `__is_dry_run()` and call `__record_get`/`__record_put` accordingly.
-   **Kernel Logic Extraction**: The core business logic of a transaction must be written or translated into a format that can be executed on the GPU (see next section).

## 5. GPU Kernel Design

This is the most challenging part of the project. We cannot run Rust code directly on the GPU.

### Technology Choice: `wgpu`

-   **`wgpu`** is the recommended technology. It is a modern, Rust-native library that provides an abstraction over native GPU APIs (Vulkan, Metal, DX12). This ensures portability across different hardware vendors and operating systems.
-   The GPU code will be written in **WGSL (WebGPU Shading Language)**.

### Kernel Implementation Strategy

1.  **Translate `alkanes-rs` Logic to WGSL**: The core transaction logic (interpreting the `cellpack`, updating balances, modifying storage) must be re-implemented in a WGSL compute shader. This implies that the parallelizable operations must be limited to what can be expressed in WGSL.
2.  **Data-Parallel Approach**: The kernel will be designed to run one "thread" (invocation) per transaction in the batch.
3.  **Input/Output Buffers**:
    *   `tx_payloads`: A buffer containing all transaction data for the batch.
    *   `input_state`: A buffer containing the key-value data from the combined `ReadSet` of the batch. This is critical to minimize host-device communication.
    *   `state_map`: A lookup structure (e.g., a hash map implemented with buffers) to map keys to their offsets in the `input_state` buffer.
    *   `output_state`: A buffer where the kernel writes the new key-value pairs from the `WriteSet`.
    *   `status_flags`: A buffer where each thread writes a success/fail code.
4.  **In-Kernel State Access**:
    *   `get(key)` will be a function in the shader that looks up the key in the `state_map` and reads from the `input_state` buffer.
    *   `put(key, value)` will write to the `output_state` buffer.
5.  **Security and Validation**:
    *   The kernel will receive the specific `ReadSet` and `WriteSet` for each transaction.
    *   Any attempt by a transaction to read a key not in its `ReadSet` or write a key not in its `WriteSet` will cause it to write a "failure" code to its `status_flag` and terminate.
    *   The host will check these flags and discard the output for any failed transaction.

## 6. Phased Implementation Plan

This project should be approached as a research-heavy initiative and rolled out in phases.

1.  **Phase A: GPU Toolchain & PoC**:
    *   Integrate `wgpu` into the Metashrew project.
    *   Create a "hello world" compute shader that performs a simple calculation (e.g., adds two numbers) and is dispatched from the `rockshrew-mono` binary. This validates the entire toolchain.
2.  **Phase B: Read/Write Set Analysis**:
    *   Implement the instrumented "dry run" environment.
    *   Add the `__is_dry_run`, `__record_get`, and `__record_put` host functions.
    *   Modify `alkanes-rs` to use them and verify that it correctly logs all storage access for a given transaction.
3.  **Phase C: Partitioning and Scheduling**:
    *   Implement the conflict graph and partitioning logic.
    *   Refactor the main block processing loop to accommodate the two-phase model, but without actually calling the GPU yet. Log the created partitions to verify correctness.
4.  **Phase D: Kernel Development & Integration**:
    *   Translate a simplified subset of `alkanes-rs` transaction logic into a WGSL compute shader.
    *   Build the GPU dispatcher to manage data buffers and invoke the kernel.
    *   Integrate the full flow: Analysis -> Partitioning -> GPU Execution -> Result Verification.
5.  **Phase E: Testing and Optimization**:
    *   Develop a comprehensive test suite covering success cases, dependency conflicts, storage conflicts, and out-of-bounds access reverts.
    *   Benchmark performance and optimize data transfer and kernel execution.
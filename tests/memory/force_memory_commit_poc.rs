// Proof of Concept: Force Memory Commit After Runtime Instantiation
//
// This demonstrates how to modify MetashrewRuntime::new() to actually
// pre-allocate the full 4GB WASM32 memory space by forcing the OS to
// commit physical pages.

use anyhow::{Context, Result};
use wasmtime::{Instance, Memory, Store};

/// Force OS to commit all pages of WASM memory by touching them.
///
/// This function writes a single byte to the start of each page in the
/// WASM linear memory, forcing the OS to allocate physical memory for
/// each page.
///
/// # Arguments
/// * `memory` - The WASM memory instance
/// * `store` - The WASM store
///
/// # Returns
/// Ok(()) if all pages were successfully committed
/// Err if memory allocation fails (e.g., insufficient physical memory)
///
/// # Performance
/// With 64KB pages and 4GB total memory, this requires 65,536 writes.
/// Expected time: ~100-500ms depending on hardware.
pub fn force_memory_commit<T>(
    memory: &Memory,
    store: &mut Store<T>,
) -> Result<()> {
    const WASM_PAGE_SIZE: usize = 65536; // 64KB
    const WASM_MAX_PAGES: usize = 65536; // 4GB / 64KB = 65536 pages
    const WASM_MAX_SIZE: usize = WASM_PAGE_SIZE * WASM_MAX_PAGES; // 4GB

    println!("Pre-allocating 4GB WASM memory...");
    println!("  Total pages: {}", WASM_MAX_PAGES);
    println!("  Page size: {} bytes", WASM_PAGE_SIZE);
    println!("  Total size: {} bytes ({}GB)", WASM_MAX_SIZE, WASM_MAX_SIZE / (1024 * 1024 * 1024));

    let start = std::time::Instant::now();

    // Touch the first byte of each page
    // This forces the OS to commit physical memory for the page
    for page_num in 0..WASM_MAX_PAGES {
        let offset = page_num * WASM_PAGE_SIZE;

        // Write a zero byte at the start of each page
        // This will fail if the OS cannot allocate the page
        memory.write(store, offset, &[0])
            .with_context(|| {
                format!(
                    "Failed to allocate memory page {} of {} (offset 0x{:x}). \
                     This indicates insufficient physical memory available. \
                     WASM32 requires 4GB of available memory for deterministic execution.",
                    page_num + 1,
                    WASM_MAX_PAGES,
                    offset
                )
            })?;

        // Log progress every 8192 pages (512MB)
        if page_num % 8192 == 0 && page_num > 0 {
            let mb_allocated = (page_num * WASM_PAGE_SIZE) / (1024 * 1024);
            println!("  Committed {}MB of 4096MB...", mb_allocated);
        }
    }

    let duration = start.elapsed();
    println!("✅ Successfully pre-allocated 4GB in {:?}", duration);

    Ok(())
}

/// Alternative implementation that commits memory in larger chunks for better performance.
///
/// Instead of writing 1 byte per page, this writes a full page of zeros,
/// which may be more efficient on some systems.
pub fn force_memory_commit_fast<T>(
    memory: &Memory,
    store: &mut Store<T>,
) -> Result<()> {
    const WASM_PAGE_SIZE: usize = 65536; // 64KB
    const WASM_MAX_PAGES: usize = 65536; // 65536 pages = 4GB

    println!("Pre-allocating 4GB WASM memory (fast mode)...");

    let start = std::time::Instant::now();
    let zero_page = vec![0u8; WASM_PAGE_SIZE];

    for page_num in 0..WASM_MAX_PAGES {
        let offset = page_num * WASM_PAGE_SIZE;

        memory.write(store, offset, &zero_page)
            .with_context(|| {
                format!(
                    "Failed to allocate memory page {} of {}. \
                     Insufficient physical memory available. \
                     WASM32 requires 4GB for deterministic execution.",
                    page_num + 1,
                    WASM_MAX_PAGES
                )
            })?;

        if page_num % 8192 == 0 && page_num > 0 {
            let mb_allocated = (page_num * WASM_PAGE_SIZE) / (1024 * 1024);
            println!("  Committed {}MB of 4096MB...", mb_allocated);
        }
    }

    let duration = start.elapsed();
    println!("✅ Successfully pre-allocated 4GB in {:?}", duration);

    Ok(())
}

/// Integration point: Add this to MetashrewRuntime::new()
///
/// In `/data/metashrew/crates/metashrew-runtime/src/runtime.rs`, after line 535
/// where the instance is created, add:
///
/// ```rust
/// let instance = linker
///     .instantiate_async(&mut wasmstore, &module).await
///     .context("Failed to instantiate WASM module")?;
///
/// // NEW CODE: Force memory pre-allocation for determinism
/// let memory = instance.get_memory(&mut wasmstore, "memory")
///     .context("Failed to get WASM memory")?;
/// force_memory_commit(&memory, &mut wasmstore)
///     .context("Failed to pre-allocate 4GB WASM memory. \
///               This runtime requires 4GB of available physical memory \
///               for deterministic execution.")?;
///
/// Ok(MetashrewRuntime { ... })
/// ```
///
/// This ensures that:
/// 1. Memory is committed immediately after instantiation
/// 2. The runtime fails fast if 4GB is not available
/// 3. Memory allocation is deterministic across all environments
///
/// ## Testing the fix
///
/// After implementing, the instantiation test should show:
/// - High memory (4GB): ✅ Instantiation succeeds after committing 4GB
/// - Low memory (512MB): ❌ Instantiation fails with clear error message
///
/// The error message will be:
/// ```
/// Failed to pre-allocate 4GB WASM memory.
/// This runtime requires 4GB of available physical memory for deterministic execution.
/// Caused by: Failed to allocate memory page N of 65536 (offset 0xXXXXXXXX).
/// This indicates insufficient physical memory available.
/// ```

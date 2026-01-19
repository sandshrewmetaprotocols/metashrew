// Optimized Memory Management Strategy for Metashrew Runtime
//
// Problem: Current approach recreates the entire WASM instance for each block,
// which means force_commit would run every time (100-500ms per block overhead).
//
// Solution: Force commit ONCE at startup, then reuse the instance and just
// zero out the memory that was actually used between blocks.

use anyhow::{Context, Result};
use wasmtime::{Instance, Memory, Store};

/// Force OS to commit all pages of WASM memory at initial instantiation.
///
/// This runs ONCE when the runtime is first created to verify 4GB is available.
/// After this, we can safely reuse the instance knowing memory is committed.
pub fn force_initial_memory_commit<T>(
    memory: &Memory,
    store: &mut Store<T>,
) -> Result<()> {
    const WASM_PAGE_SIZE: usize = 65536; // 64KB
    const WASM_MAX_PAGES: usize = 65536; // 4GB / 64KB

    log::info!("🔒 Pre-allocating 4GB WASM memory (one-time startup check)...");
    let start = std::time::Instant::now();

    // Touch first byte of each page to force OS commit
    for page_num in 0..WASM_MAX_PAGES {
        let offset = page_num * WASM_PAGE_SIZE;
        memory.write(store, offset, &[0])
            .with_context(|| {
                format!(
                    "Failed to commit memory page {} of {}. \
                     WASM32 requires 4GB of available physical memory for deterministic execution.",
                    page_num + 1, WASM_MAX_PAGES
                )
            })?;

        if page_num % 8192 == 0 && page_num > 0 {
            let gb = (page_num * WASM_PAGE_SIZE) / (1024 * 1024 * 1024);
            log::debug!("  Committed {}GB of 4GB...", gb);
        }
    }

    log::info!("✅ Successfully pre-allocated 4GB in {:?}", start.elapsed());
    Ok(())
}

/// Fast memory reset between blocks by zeroing only used memory.
///
/// This is MUCH faster than recreating the instance since:
/// 1. We don't need to touch all 4GB (already committed)
/// 2. We only zero the memory actually used (typically <100MB per block)
/// 3. No instance recreation overhead
///
/// Expected time: ~1-10ms depending on memory used (vs 100-500ms for full recreate)
pub fn fast_memory_reset<T>(
    memory: &Memory,
    store: &mut Store<T>,
    used_pages: usize,
) -> Result<()> {
    const WASM_PAGE_SIZE: usize = 65536; // 64KB

    log::debug!("Resetting {} pages of WASM memory", used_pages);
    let start = std::time::Instant::now();

    // Zero out only the pages that were actually used
    let zero_page = vec![0u8; WASM_PAGE_SIZE];
    for page_num in 0..used_pages {
        let offset = page_num * WASM_PAGE_SIZE;
        memory.write(store, offset, &zero_page)?;
    }

    log::debug!("Memory reset completed in {:?}", start.elapsed());
    Ok(())
}

/// Track memory usage during WASM execution.
///
/// We need to know how much memory was actually used so we can
/// zero just that portion on reset.
pub struct MemoryUsageTracker {
    /// Maximum page accessed during execution (in WASM pages)
    max_page_used: usize,
}

impl MemoryUsageTracker {
    pub fn new() -> Self {
        Self { max_page_used: 0 }
    }

    /// Update the tracker when memory is accessed
    pub fn record_access(&mut self, byte_offset: usize) {
        const WASM_PAGE_SIZE: usize = 65536;
        let page = byte_offset / WASM_PAGE_SIZE;
        if page > self.max_page_used {
            self.max_page_used = page;
        }
    }

    /// Get the number of pages that need to be zeroed
    pub fn pages_to_reset(&self) -> usize {
        // Add some margin (e.g., 10% extra pages) to ensure we clear everything
        let margin = (self.max_page_used / 10).max(10);
        self.max_page_used + margin
    }

    pub fn reset(&mut self) {
        self.max_page_used = 0;
    }
}

/// Alternative: Use Wasmtime's memory.size() to determine how much to zero.
///
/// Wasmtime tracks the current memory size, so we can use that instead
/// of manual tracking.
pub fn fast_memory_reset_auto<T>(
    memory: &Memory,
    store: &mut Store<T>,
) -> Result<()> {
    const WASM_PAGE_SIZE: usize = 65536; // 64KB

    // Get current memory size in WASM pages (not OS pages)
    let current_pages = memory.size(store);

    log::debug!("Resetting {} WASM pages ({}MB)",
        current_pages,
        (current_pages as usize * WASM_PAGE_SIZE) / (1024 * 1024)
    );

    let start = std::time::Instant::now();
    let zero_page = vec![0u8; WASM_PAGE_SIZE];

    for page_num in 0..(current_pages as usize) {
        let offset = page_num * WASM_PAGE_SIZE;
        memory.write(store, offset, &zero_page)?;
    }

    log::debug!("Memory reset completed in {:?}", start.elapsed());
    Ok(())
}

// ============================================================================
// IMPLEMENTATION GUIDE
// ============================================================================

/// Modified refresh_memory() implementation for MetashrewRuntime
///
/// BEFORE (current):
/// ```rust,ignore
/// pub async fn refresh_memory(&self) -> Result<()> {
///     let mut instance_guard = self.instance.lock().await;
///
///     // Recreate entire instance (slow!)
///     let mut wasmstore = Store::<State>::new(&self.engine, State::new());
///     let new_instance = self.linker
///         .instantiate_async(&mut wasmstore, &self.module)
///         .await?;
///     *instance_guard = WasmInstance {
///         store: wasmstore,
///         instance: new_instance,
///     };
///     Ok(())
/// }
/// ```
///
/// AFTER (optimized):
/// ```rust,ignore
/// pub async fn refresh_memory(&self) -> Result<()> {
///     let mut instance_guard = self.instance.lock().await;
///     let WasmInstance { ref mut store, instance } = &mut *instance_guard;
///
///     // Just zero out the memory that was used
///     let memory = instance.get_memory(&mut *store, "memory")
///         .context("Failed to get WASM memory")?;
///
///     fast_memory_reset_auto(&memory, store)
///         .context("Failed to reset WASM memory")?;
///
///     // Also reset store state if needed
///     store.data_mut().had_failure = false;
///     store.data_mut().error_message = None;
///
///     Ok(())
/// }
/// ```

/// Modified new() implementation to add initial force commit
///
/// Add this after instance creation (line 535):
/// ```rust,ignore
/// let instance = linker
///     .instantiate_async(&mut wasmstore, &module).await
///     .context("Failed to instantiate WASM module")?;
///
/// // NEW: Force initial memory commit to verify 4GB available
/// let memory = instance.get_memory(&mut wasmstore, "memory")
///     .context("Failed to get WASM memory for pre-allocation")?;
/// force_initial_memory_commit(&memory, &mut wasmstore)
///     .context("Failed to pre-allocate 4GB WASM memory. \
///               WASM32 requires 4GB of available physical memory.")?;
///
/// Ok(MetashrewRuntime { ... })
/// ```

// ============================================================================
// PERFORMANCE COMPARISON
// ============================================================================

/// Current approach (recreate instance):
/// - Initial instantiation: 100-500ms (includes module loading, linking, instantiation)
/// - Per-block refresh: 100-500ms (full recreate)
/// - 100 blocks: 10-50 seconds of overhead
///
/// Optimized approach:
/// - Initial instantiation: 100-500ms (one time)
/// - Initial force_commit: 100-500ms (one time, verifies 4GB)
/// - Per-block reset: 1-10ms (zero used memory only)
/// - 100 blocks: 100ms-1s of overhead
///
/// **50-500x speedup for block processing!**

// ============================================================================
// ADDITIONAL BENEFITS
// ============================================================================

/// 1. **Deterministic memory state**: Always starts from zeros, no leftover data
/// 2. **Fast failure**: Fails at startup if 4GB not available (not during execution)
/// 3. **Better cache locality**: Reusing same memory addresses improves CPU cache hits
/// 4. **Lower memory fragmentation**: Not constantly allocating/deallocating 4GB
/// 5. **Simpler debugging**: Same memory address space across all blocks

// ============================================================================
// TESTING THE OPTIMIZATION
// ============================================================================

/// Before optimization (current):
/// ```bash
/// # Time to process 100 blocks
/// time cargo test --lib -- comprehensive_e2e_test --nocapture
/// # Expected: ~20-60 seconds (with memory refresh overhead)
/// ```
///
/// After optimization:
/// ```bash
/// # Time to process 100 blocks
/// time cargo test --lib -- comprehensive_e2e_test --nocapture
/// # Expected: ~0.5-2 seconds (minimal memory overhead)
/// ```

// ============================================================================
// SAFETY CONSIDERATIONS
// ============================================================================

/// **Important**: We MUST ensure ALL WASM state is cleared between blocks:
///
/// 1. Linear memory - zeroed by fast_memory_reset()
/// 2. Global variables - need to be reset manually if any exist
/// 3. Table entries - need to be cleared if using function tables
/// 4. Store data - reset State struct fields
///
/// The current full recreate handles all of these automatically.
/// With reuse, we need to explicitly reset each piece of state.
///
/// For metashrew, we need to reset:
/// - memory (covered by fast_memory_reset)
/// - store.data().had_failure
/// - store.data().error_message
/// - Any other State fields

// ============================================================================
// ROLLOUT STRATEGY
// ============================================================================

/// Phase 1: Add force_commit at initial instantiation
/// - Verify 4GB is available at startup
/// - Keep current refresh_memory() approach (full recreate)
/// - Test in production to ensure no issues
///
/// Phase 2: Switch to memory zeroing for refresh
/// - Replace full recreate with fast_memory_reset_auto()
/// - Add comprehensive tests to ensure state isolation
/// - Monitor for any non-determinism issues
///
/// Phase 3: Optimize further if needed
/// - Track actual memory usage and zero only that
/// - Consider memory pooling for frequently used sizes
/// - Profile and optimize the zeroing operation itself

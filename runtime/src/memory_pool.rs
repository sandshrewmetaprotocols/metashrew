use std::sync::Arc;
use parking_lot::Mutex;
use dashmap::DashMap;

/// Thread-safe memory pool for reusing Vec<u8> allocations
pub struct MemoryPool {
    // Use DashMap for better concurrent performance than HashMap with Mutex
    pools: DashMap<usize, Vec<Vec<u8>>>,
}

impl MemoryPool {
    pub fn new() -> Self {
        Self {
            pools: DashMap::new(),
        }
    }

    /// Get a vector from the pool or create a new one with the specified capacity
    pub fn get(&self, capacity: usize) -> Vec<u8> {
        let bucket_size = Self::get_bucket_size(capacity);
        if let Some(mut pool) = self.pools.get_mut(&bucket_size) {
            pool.pop().unwrap_or_else(|| Vec::with_capacity(bucket_size))
        } else {
            Vec::with_capacity(bucket_size)
        }
    }

    /// Return a vector to the pool for reuse
    pub fn put(&self, mut vec: Vec<u8>) {
        let bucket_size = Self::get_bucket_size(vec.capacity());
        vec.clear();
        self.pools.entry(bucket_size)
            .or_insert_with(Vec::new)
            .push(vec);
    }

    /// Get the standardized bucket size for a given capacity
    #[inline]
    fn get_bucket_size(capacity: usize) -> usize {
        // Round up to the next power of 2, with some common sizes optimized
        match capacity {
            0..=32 => 32,
            33..=64 => 64,
            65..=128 => 128,
            129..=256 => 256,
            257..=512 => 512,
            513..=1024 => 1024,
            1025..=2048 => 2048,
            _ => capacity.next_power_of_two(),
        }
    }
}

impl Default for MemoryPool {
    fn default() -> Self {
        Self::new()
    }
}

// Global memory pool
lazy_static::lazy_static! {
    pub static ref MEMORY_POOL: MemoryPool = MemoryPool::new();
}
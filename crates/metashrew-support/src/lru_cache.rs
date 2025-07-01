//! LRU Cache Implementation for Metashrew
//!
//! This module provides a memory-bounded LRU (Least Recently Used) cache system
//! designed to work with the stateful view functionality in Metashrew. The cache
//! provides a secondary caching layer between the in-memory cache and host calls,
//! allowing WASM programs to maintain persistent state across multiple invocations
//! while preventing unbounded memory growth.
//!
//! # Architecture
//!
//! The LRU cache sits between the existing CACHE and the host calls (__get/__get_len):
//!
//! ```text
//! get() -> CACHE -> LRU_CACHE -> __get/__get_len (host calls)
//! ```
//!
//! # Memory Management
//!
//! - **Memory Limit**: 1GB hard limit to prevent OOM crashes
//! - **LRU Eviction**: Automatically removes least recently used items when memory limit is reached
//! - **Precise Accounting**: Uses MemSize trait for accurate memory usage calculation
//! - **Thread Safety**: RwLock wrapper for safe concurrent access
//!
//! # Usage
//!
//! The LRU cache is designed to be used transparently by the existing cache system.
//! When stateful views are enabled, the cache lookup order becomes:
//!
//! 1. Check CACHE (immediate cache)
//! 2. Check LRU_CACHE (persistent cache)
//! 3. Fall back to host calls (__get/__get_len)
//! 4. Populate both CACHE and LRU_CACHE with retrieved value
//!
//! # Example
//!
//! ```rust,no_run
//! use metashrew_support::lru_cache::{initialize_lru_cache, get_lru_cache, set_lru_cache, clear_lru_cache};
//! use std::sync::Arc;
//!
//! // Initialize the LRU cache
//! initialize_lru_cache();
//!
//! // Store a value in the LRU cache
//! let key = Arc::new(b"my_key".to_vec());
//! let value = Arc::new(b"my_value".to_vec());
//! set_lru_cache(key.clone(), value.clone());
//!
//! // Retrieve a value from the LRU cache
//! if let Some(cached_value) = get_lru_cache(&key) {
//!     println!("Found cached value: {:?}", cached_value);
//! }
//! ```

use lru_mem::{LruCache, MemSize, HeapSize};
use std::sync::{Arc, RwLock};

/// Memory limit for the LRU cache (1GB)
const LRU_CACHE_MEMORY_LIMIT: usize = 1024 * 1024 * 1024; // 1GB

/// Cache allocation mode
#[derive(Debug, Clone, Copy)]
pub enum CacheAllocationMode {
    /// Indexer mode: allocate all memory to main LRU cache
    Indexer,
    /// View mode: allocate memory to height-partitioned and API caches
    View,
}

/// Current cache allocation mode
static CACHE_ALLOCATION_MODE: RwLock<CacheAllocationMode> = RwLock::new(CacheAllocationMode::Indexer);

/// Global LRU cache instance
///
/// This cache persists across multiple WASM invocations when stateful views are enabled.
/// It provides a memory-bounded secondary cache layer that sits between the immediate
/// cache (CACHE) and the host calls (__get/__get_len).
static LRU_CACHE: RwLock<Option<LruCache<CacheKey, CacheValue>>> = RwLock::new(None);

/// Global API cache for user-defined caching needs
///
/// This cache allows WASM programs to cache arbitrary data beyond just key-value store
/// lookups. It shares the same memory limit as the main LRU cache but uses a separate
/// namespace to avoid conflicts.
static API_CACHE: RwLock<Option<LruCache<ApiCacheKey, CacheValue>>> = RwLock::new(None);

/// Global height-partitioned cache for view functions
///
/// This cache partitions entries by block height, ensuring that view functions
/// only see cache entries for the specific height they are querying. This enables
/// proper archival state queries without side effects.
static HEIGHT_PARTITIONED_CACHE: RwLock<Option<LruCache<HeightPartitionedKey, CacheValue>>> = RwLock::new(None);

/// Current view height for height-partitioned caching
///
/// When set, get() operations will use height-partitioned caching instead of
/// the main LRU cache. This is used by view functions to ensure cache isolation.
static CURRENT_VIEW_HEIGHT: RwLock<Option<u32>> = RwLock::new(None);

/// Cache statistics for monitoring and debugging
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Total number of cache hits
    pub hits: u64,
    /// Total number of cache misses
    pub misses: u64,
    /// Current number of items in cache
    pub items: usize,
    /// Current memory usage in bytes
    pub memory_usage: usize,
    /// Number of items evicted due to memory pressure
    pub evictions: u64,
}

/// Global cache statistics
static CACHE_STATS: RwLock<CacheStats> = RwLock::new(CacheStats {
    hits: 0,
    misses: 0,
    items: 0,
    memory_usage: 0,
    evictions: 0,
});

/// Wrapper type for Arc<Vec<u8>> to implement MemSize
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheValue(pub Arc<Vec<u8>>);

impl From<Arc<Vec<u8>>> for CacheValue {
    fn from(arc: Arc<Vec<u8>>) -> Self {
        CacheValue(arc)
    }
}

impl From<CacheValue> for Arc<Vec<u8>> {
    fn from(val: CacheValue) -> Self {
        val.0
    }
}

impl HeapSize for CacheValue {
    fn heap_size(&self) -> usize {
        // Size of the Arc wrapper + size of the Vec + size of the data
        std::mem::size_of::<Arc<Vec<u8>>>() +
        std::mem::size_of::<Vec<u8>>() +
        self.0.len()
    }
}

/// Wrapper type for Arc<Vec<u8>> keys to implement MemSize
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(pub Arc<Vec<u8>>);

impl From<Arc<Vec<u8>>> for CacheKey {
    fn from(arc: Arc<Vec<u8>>) -> Self {
        CacheKey(arc)
    }
}

impl From<CacheKey> for Arc<Vec<u8>> {
    fn from(val: CacheKey) -> Self {
        val.0
    }
}

impl HeapSize for CacheKey {
    fn heap_size(&self) -> usize {
        // Size of the Arc wrapper + size of the Vec + size of the data
        std::mem::size_of::<Arc<Vec<u8>>>() +
        std::mem::size_of::<Vec<u8>>() +
        self.0.len()
    }
}

/// Wrapper type for String to implement MemSize for API cache
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApiCacheKey(pub String);

impl From<String> for ApiCacheKey {
    fn from(s: String) -> Self {
        ApiCacheKey(s)
    }
}

impl From<ApiCacheKey> for String {
    fn from(val: ApiCacheKey) -> Self {
        val.0
    }
}

impl HeapSize for ApiCacheKey {
    fn heap_size(&self) -> usize {
        std::mem::size_of::<String>() + self.0.len()
    }
}

/// Height-partitioned cache key combining height and original key
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeightPartitionedKey {
    pub height: u32,
    pub key: Arc<Vec<u8>>,
}

impl From<(u32, Arc<Vec<u8>>)> for HeightPartitionedKey {
    fn from((height, key): (u32, Arc<Vec<u8>>)) -> Self {
        HeightPartitionedKey { height, key }
    }
}

impl HeapSize for HeightPartitionedKey {
    fn heap_size(&self) -> usize {
        std::mem::size_of::<u32>() +
        std::mem::size_of::<Arc<Vec<u8>>>() +
        std::mem::size_of::<Vec<u8>>() +
        self.key.len()
    }
}

/// Initialize the LRU cache system
///
/// This function sets up the main LRU cache for key-value storage, the API cache
/// for user-defined caching, and the height-partitioned cache for view functions.
/// Memory allocation depends on the current cache allocation mode.
///
/// # Thread Safety
///
/// This function is thread-safe and can be called multiple times. Subsequent
/// calls will be no-ops if the cache is already initialized.
pub fn initialize_lru_cache() {
    let allocation_mode = *CACHE_ALLOCATION_MODE.read().unwrap();
    
    match allocation_mode {
        CacheAllocationMode::Indexer => {
            // Indexer mode: allocate all memory to main LRU cache
            {
                let mut cache = LRU_CACHE.write().unwrap();
                if cache.is_none() {
                    *cache = Some(LruCache::new(LRU_CACHE_MEMORY_LIMIT)); // All memory to main cache
                }
            }
            
            // Initialize other caches with minimal memory (they won't be used)
            {
                let mut api_cache = API_CACHE.write().unwrap();
                if api_cache.is_none() {
                    *api_cache = Some(LruCache::new(1024)); // Minimal allocation
                }
            }
            
            {
                let mut height_cache = HEIGHT_PARTITIONED_CACHE.write().unwrap();
                if height_cache.is_none() {
                    *height_cache = Some(LruCache::new(1024)); // Minimal allocation
                }
            }
        },
        CacheAllocationMode::View => {
            // View mode: allocate memory to height-partitioned and API caches
            {
                let mut cache = LRU_CACHE.write().unwrap();
                if cache.is_none() {
                    *cache = Some(LruCache::new(1024)); // Minimal allocation
                }
            }
            
            {
                let mut api_cache = API_CACHE.write().unwrap();
                if api_cache.is_none() {
                    *api_cache = Some(LruCache::new(LRU_CACHE_MEMORY_LIMIT / 2)); // Half for API cache
                }
            }
            
            {
                let mut height_cache = HEIGHT_PARTITIONED_CACHE.write().unwrap();
                if height_cache.is_none() {
                    *height_cache = Some(LruCache::new(LRU_CACHE_MEMORY_LIMIT / 2)); // Half for height-partitioned cache
                }
            }
        }
    }
}

/// Get a value from the LRU cache
///
/// This function retrieves a value from the LRU cache if it exists. The access
/// updates the LRU ordering, making the item more likely to be retained.
///
/// # Arguments
///
/// * `key` - The key to look up in the cache
///
/// # Returns
///
/// `Some(value)` if the key exists in the cache, `None` otherwise.
///
/// # Thread Safety
///
/// This function uses a read lock for cache access and upgrades to a write lock
/// only when updating the LRU ordering.
pub fn get_lru_cache(key: &Arc<Vec<u8>>) -> Option<Arc<Vec<u8>>> {
    let cache_key = CacheKey::from(key.clone());
    
    // Try to read with read lock first
    {
        let cache_guard = LRU_CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref() {
            // Check if key exists without updating LRU order
            if cache.contains(&cache_key) {
                drop(cache_guard); // Release read lock
                
                // Upgrade to write lock to update LRU order and get value
                let mut cache_guard = LRU_CACHE.write().unwrap();
                if let Some(cache) = cache_guard.as_mut() {
                    let result = cache.get(&cache_key).cloned().map(|v| v.into());
                    
                    // Update statistics
                    {
                        let mut stats = CACHE_STATS.write().unwrap();
                        if result.is_some() {
                            stats.hits += 1;
                        } else {
                            stats.misses += 1;
                        }
                        stats.items = cache.len();
                        stats.memory_usage = cache.mem_size();
                    }
                    
                    return result;
                }
            }
        }
    }
    
    // Update miss statistics
    {
        let mut stats = CACHE_STATS.write().unwrap();
        stats.misses += 1;
    }
    
    None
}

/// Set a value in the LRU cache
///
/// This function stores a key-value pair in the LRU cache. If the cache is full
/// and adding this item would exceed the memory limit, the least recently used
/// items will be evicted automatically.
///
/// # Arguments
///
/// * `key` - The key to store
/// * `value` - The value to associate with the key
///
/// # Memory Management
///
/// The cache automatically manages memory by evicting least recently used items
/// when the memory limit is approached. The eviction process is transparent to
/// the caller.
pub fn set_lru_cache(key: Arc<Vec<u8>>, value: Arc<Vec<u8>>) {
    let cache_key = CacheKey::from(key);
    let cache_value = CacheValue::from(value);
    
    let mut cache_guard = LRU_CACHE.write().unwrap();
    if let Some(cache) = cache_guard.as_mut() {
        let old_len = cache.len();
        let _ = cache.insert(cache_key, cache_value);
        
        // Update statistics
        {
            let mut stats = CACHE_STATS.write().unwrap();
            stats.items = cache.len();
            stats.memory_usage = cache.mem_size();
            
            // If cache size decreased, items were evicted
            if cache.len() < old_len {
                stats.evictions += (old_len - cache.len()) as u64;
            }
        }
    }
}

/// Clear the LRU cache
///
/// This function removes all items from the LRU cache, freeing all associated
/// memory. This is typically used for testing or when a complete cache reset
/// is needed.
///
/// # Warning
///
/// This operation cannot be undone. All cached data will be lost.
pub fn clear_lru_cache() {
    {
        let mut cache_guard = LRU_CACHE.write().unwrap();
        if let Some(cache) = cache_guard.as_mut() {
            cache.clear();
        }
    }
    
    {
        let mut api_cache_guard = API_CACHE.write().unwrap();
        if let Some(cache) = api_cache_guard.as_mut() {
            cache.clear();
        }
    }
    
    {
        let mut height_cache_guard = HEIGHT_PARTITIONED_CACHE.write().unwrap();
        if let Some(cache) = height_cache_guard.as_mut() {
            cache.clear();
        }
    }
    
    // Reset statistics
    {
        let mut stats = CACHE_STATS.write().unwrap();
        *stats = CacheStats::default();
    }
}

/// Get current cache statistics
///
/// This function returns a snapshot of the current cache statistics, including
/// hit/miss ratios, memory usage, and eviction counts. This is useful for
/// monitoring cache performance and tuning cache behavior.
///
/// # Returns
///
/// A `CacheStats` struct containing current cache metrics.
pub fn get_cache_stats() -> CacheStats {
    CACHE_STATS.read().unwrap().clone()
}

/// API Cache Functions
///
/// These functions provide a general-purpose caching API that WASM programs
/// can use to cache arbitrary data beyond just key-value store lookups.

/// Store a value in the API cache
///
/// This function allows WASM programs to cache arbitrary data using string keys.
/// The API cache shares the same memory limit as the main LRU cache but uses
/// a separate namespace to avoid conflicts.
///
/// # Arguments
///
/// * `key` - A string key to identify the cached value
/// * `value` - The value to cache (as bytes)
///
/// # Example
///
/// ```rust,no_run
/// use metashrew_support::lru_cache::api_cache_set;
/// use std::sync::Arc;
///
/// let computed_result = Arc::new(b"expensive_computation_result".to_vec());
/// api_cache_set("computation_key".to_string(), computed_result);
/// ```
pub fn api_cache_set(key: String, value: Arc<Vec<u8>>) {
    let cache_key = ApiCacheKey::from(key);
    let cache_value = CacheValue::from(value);
    
    let mut cache_guard = API_CACHE.write().unwrap();
    if let Some(cache) = cache_guard.as_mut() {
        let _ = cache.insert(cache_key, cache_value);
    }
}

/// Retrieve a value from the API cache
///
/// This function retrieves a previously cached value using its string key.
/// The access updates the LRU ordering for the item.
///
/// # Arguments
///
/// * `key` - The string key to look up
///
/// # Returns
///
/// `Some(value)` if the key exists in the cache, `None` otherwise.
///
/// # Example
///
/// ```rust,no_run
/// use metashrew_support::lru_cache::api_cache_get;
///
/// if let Some(cached_result) = api_cache_get("computation_key") {
///     println!("Found cached result: {:?}", cached_result);
/// } else {
///     println!("Cache miss, need to compute");
/// }
/// ```
pub fn api_cache_get(key: &str) -> Option<Arc<Vec<u8>>> {
    let cache_key = ApiCacheKey::from(key.to_string());
    
    // Try to read with read lock first
    {
        let cache_guard = API_CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref() {
            if cache.contains(&cache_key) {
                drop(cache_guard); // Release read lock
                
                // Upgrade to write lock to update LRU order and get value
                let mut cache_guard = API_CACHE.write().unwrap();
                if let Some(cache) = cache_guard.as_mut() {
                    return cache.get(&cache_key).cloned().map(|v| v.into());
                }
            }
        }
    }
    
    None
}

/// Remove a value from the API cache
///
/// This function removes a specific key-value pair from the API cache.
///
/// # Arguments
///
/// * `key` - The string key to remove
///
/// # Returns
///
/// `Some(value)` if the key existed and was removed, `None` if the key didn't exist.
pub fn api_cache_remove(key: &str) -> Option<Arc<Vec<u8>>> {
    let cache_key = ApiCacheKey::from(key.to_string());
    
    let mut cache_guard = API_CACHE.write().unwrap();
    if let Some(cache) = cache_guard.as_mut() {
        cache.remove(&cache_key).map(|v| v.into())
    } else {
        None
    }
}

/// Check if the LRU cache system is initialized
///
/// This function returns true if both the main LRU cache and API cache have
/// been initialized, false otherwise.
pub fn is_lru_cache_initialized() -> bool {
    let main_cache = LRU_CACHE.read().unwrap();
    let api_cache = API_CACHE.read().unwrap();
    main_cache.is_some() && api_cache.is_some()
}

/// Get the current memory usage of both caches combined
///
/// This function returns the total memory usage in bytes of both the main
/// LRU cache and the API cache.
pub fn get_total_memory_usage() -> usize {
    let mut total = 0;
    
    {
        let cache_guard = LRU_CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref() {
            total += cache.mem_size();
        }
    }
    
    {
        let cache_guard = API_CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref() {
            total += cache.mem_size();
        }
    }
    
    total
}

/// Set the current view height for height-partitioned caching
///
/// This function sets the current view height, which causes subsequent get()
/// operations to use height-partitioned caching instead of the main LRU cache.
/// This is used by view functions to ensure cache isolation by block height.
///
/// # Arguments
///
/// * `height` - The block height to use for partitioned caching
pub fn set_view_height(height: u32) {
    let mut current_height = CURRENT_VIEW_HEIGHT.write().unwrap();
    *current_height = Some(height);
}

/// Clear the current view height
///
/// This function clears the current view height, causing subsequent get()
/// operations to use the main LRU cache instead of height-partitioned caching.
/// This should be called at the end of view functions.
pub fn clear_view_height() {
    let mut current_height = CURRENT_VIEW_HEIGHT.write().unwrap();
    *current_height = None;
}

/// Get the current view height
///
/// Returns the current view height if set, None otherwise.
pub fn get_view_height() -> Option<u32> {
    *CURRENT_VIEW_HEIGHT.read().unwrap()
}

/// Get a value from the height-partitioned cache
///
/// This function retrieves a value from the height-partitioned cache for the
/// specified height and key. This is used internally when a view height is set.
///
/// # Arguments
///
/// * `height` - The block height for partitioning
/// * `key` - The key to look up
///
/// # Returns
///
/// `Some(value)` if the key exists in the cache for this height, `None` otherwise.
pub fn get_height_partitioned_cache(height: u32, key: &Arc<Vec<u8>>) -> Option<Arc<Vec<u8>>> {
    let cache_key = HeightPartitionedKey::from((height, key.clone()));
    
    // Try to read with read lock first
    {
        let cache_guard = HEIGHT_PARTITIONED_CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref() {
            if cache.contains(&cache_key) {
                drop(cache_guard); // Release read lock
                
                // Upgrade to write lock to update LRU order and get value
                let mut cache_guard = HEIGHT_PARTITIONED_CACHE.write().unwrap();
                if let Some(cache) = cache_guard.as_mut() {
                    let result = cache.get(&cache_key).cloned().map(|v| v.into());
                    
                    // Update statistics
                    {
                        let mut stats = CACHE_STATS.write().unwrap();
                        if result.is_some() {
                            stats.hits += 1;
                        } else {
                            stats.misses += 1;
                        }
                        stats.items = cache.len();
                        stats.memory_usage = cache.mem_size();
                    }
                    
                    return result;
                }
            }
        }
    }
    
    // Update miss statistics
    {
        let mut stats = CACHE_STATS.write().unwrap();
        stats.misses += 1;
    }
    
    None
}

/// Set a value in the height-partitioned cache
///
/// This function stores a key-value pair in the height-partitioned cache for
/// the specified height. This is used internally when a view height is set.
///
/// # Arguments
///
/// * `height` - The block height for partitioning
/// * `key` - The key to store
/// * `value` - The value to associate with the key
pub fn set_height_partitioned_cache(height: u32, key: Arc<Vec<u8>>, value: Arc<Vec<u8>>) {
    let cache_key = HeightPartitionedKey::from((height, key));
    let cache_value = CacheValue::from(value);
    
    let mut cache_guard = HEIGHT_PARTITIONED_CACHE.write().unwrap();
    if let Some(cache) = cache_guard.as_mut() {
        let old_len = cache.len();
        let _ = cache.insert(cache_key, cache_value);
        
        // Update statistics
        {
            let mut stats = CACHE_STATS.write().unwrap();
            stats.items = cache.len();
            stats.memory_usage = cache.mem_size();
            
            // If cache size decreased, items were evicted
            if cache.len() < old_len {
                stats.evictions += (old_len - cache.len()) as u64;
            }
        }
    }
}

/// Flush CACHE contents to LRU_CACHE
///
/// This function moves all entries from the immediate CACHE to the persistent
/// LRU_CACHE and then clears the CACHE. This is called by the main indexer
/// function before flush() to ensure that cached values persist across blocks.
///
/// This function should NOT be called during view functions.
pub fn flush_to_lru() {
    // This function will be implemented in metashrew-core since it needs access to CACHE
    // We'll add a callback mechanism or implement it there
}

/// Set the cache allocation mode
///
/// This function sets how memory should be allocated across the different caches.
/// - Indexer mode: All memory goes to main LRU cache
/// - View mode: Memory split between height-partitioned and API caches
///
/// # Arguments
///
/// * `mode` - The cache allocation mode to use
pub fn set_cache_allocation_mode(mode: CacheAllocationMode) {
    let mut allocation_mode = CACHE_ALLOCATION_MODE.write().unwrap();
    *allocation_mode = mode;
}

/// Get the current cache allocation mode
pub fn get_cache_allocation_mode() -> CacheAllocationMode {
    *CACHE_ALLOCATION_MODE.read().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache_basic_operations() {
        initialize_lru_cache();
        
        let key = Arc::new(b"test_key".to_vec());
        let value = Arc::new(b"test_value".to_vec());
        
        // Test set and get
        set_lru_cache(key.clone(), value.clone());
        let retrieved = get_lru_cache(&key);
        assert_eq!(retrieved, Some(value));
        
        // Test cache miss
        let missing_key = Arc::new(b"missing_key".to_vec());
        let missing = get_lru_cache(&missing_key);
        assert_eq!(missing, None);
    }
    
    #[test]
    fn test_api_cache_operations() {
        // Set to View mode to ensure API cache gets proper allocation
        set_cache_allocation_mode(CacheAllocationMode::View);
        initialize_lru_cache();
        
        let key = "test_api_key".to_string();
        let value = Arc::new(b"test_api_value".to_vec());
        
        // Test set and get
        api_cache_set(key.clone(), value.clone());
        let retrieved = api_cache_get(&key);
        assert_eq!(retrieved, Some(value));
        
        // Test cache miss
        let missing = api_cache_get("missing_api_key");
        assert_eq!(missing, None);
        
        // Test remove
        let removed = api_cache_remove(&key);
        assert_eq!(removed, Some(Arc::new(b"test_api_value".to_vec())));
        
        // Verify removal
        let after_remove = api_cache_get(&key);
        assert_eq!(after_remove, None);
        
        // Reset to default mode
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
    }
    
    #[test]
    fn test_cache_stats() {
        // Ensure we're in indexer mode for this test
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        initialize_lru_cache();
        clear_lru_cache(); // Reset stats
        
        // Use a unique key to avoid conflicts with other tests
        let unique_suffix = std::thread::current().id();
        let key = Arc::new(format!("stats_test_key_{:?}", unique_suffix).into_bytes());
        let value = Arc::new(format!("stats_test_value_{:?}", unique_suffix).into_bytes());
        
        // Clear cache again to ensure clean state
        clear_lru_cache();
        
        // Get initial stats - should be zero after clear
        let initial_stats = get_cache_stats();
        
        // Cache miss should increment misses
        let miss_result = get_lru_cache(&key);
        assert!(miss_result.is_none()); // Should be a miss
        let after_miss = get_cache_stats();
        assert!(after_miss.misses > initial_stats.misses);
        
        // Set value and hit should increment hits
        set_lru_cache(key.clone(), value);
        let hit_result = get_lru_cache(&key);
        assert!(hit_result.is_some()); // Should be a hit
        let after_hit = get_cache_stats();
        assert!(after_hit.hits > initial_stats.hits);
        assert!(after_hit.items >= 1); // At least our item should be there
    }
    
    #[test]
    fn test_memory_size_calculation() {
        let small_vec = Arc::new(vec![1, 2, 3]);
        let large_vec = Arc::new(vec![0u8; 1000]);
        
        let small_cache_value = CacheValue::from(small_vec);
        let large_cache_value = CacheValue::from(large_vec);
        
        let small_size = small_cache_value.mem_size();
        let large_size = large_cache_value.mem_size();
        
        // Large vector should use more memory
        assert!(large_size > small_size);
        
        // Size should include overhead plus data
        assert!(small_size >= 3 + std::mem::size_of::<Arc<Vec<u8>>>() + std::mem::size_of::<Vec<u8>>());
    }
}
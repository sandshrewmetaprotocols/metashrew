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

use lru_mem::{HeapSize, LruCache};
use std::sync::{Arc, RwLock};
use std::collections::{HashMap, HashSet};

/// Memory limit for the LRU cache (1GB)
const LRU_CACHE_MEMORY_LIMIT: usize = 1024 * 1024 * 1024; // 1GB

/// Preallocated memory region for LRU cache to ensure consistent memory layout
/// This is allocated at startup to guarantee the same memory addresses regardless
/// of whether the cache is actually used or not.
static PREALLOCATED_CACHE_MEMORY: std::sync::LazyLock<Vec<u8>> =
    std::sync::LazyLock::new(|| {
        // Preallocate exactly 1GB of memory at startup
        // This ensures that this memory region is always occupied at the same
        // virtual address, providing consistent memory layout for WASM execution
        let mut memory = Vec::with_capacity(LRU_CACHE_MEMORY_LIMIT);
        
        // Actually allocate the memory by filling it with zeros
        // This forces the OS to commit the memory pages immediately
        memory.resize(LRU_CACHE_MEMORY_LIMIT, 0);
        
        log::info!(
            "Preallocated 1GB LRU cache memory region at address: {:p}, size: {} bytes",
            memory.as_ptr(),
            memory.len()
        );
        
        memory
    });

/// Ensure the preallocated memory is initialized (only in indexer mode)
/// This function forces the lazy static to initialize, ensuring the memory
/// is allocated before any other operations that might affect memory layout.
///
/// **IMPORTANT**: Memory preallocation only happens in indexer mode.
/// View mode does not need deterministic memory layout.
pub fn ensure_preallocated_memory() {
    // Only preallocate memory in indexer mode
    // View mode doesn't need deterministic memory layout
    let allocation_mode = *CACHE_ALLOCATION_MODE.read().unwrap();
    
    match allocation_mode {
        CacheAllocationMode::Indexer => {
            // Access the lazy static to force initialization
            let memory_ptr = PREALLOCATED_CACHE_MEMORY.as_ptr();
            let memory_size = PREALLOCATED_CACHE_MEMORY.len();
            
            log::debug!(
                "LRU cache preallocated memory confirmed (indexer mode): ptr={:p}, size={} bytes",
                memory_ptr,
                memory_size
            );
            
            // Verify the memory is actually allocated by touching the first and last pages
            unsafe {
                // Touch first page
                std::ptr::read_volatile(memory_ptr);
                // Touch last page
                std::ptr::read_volatile(memory_ptr.add(memory_size - 1));
            }
            
            log::info!("LRU cache memory preallocation verified and committed (indexer mode)");
        }
        CacheAllocationMode::View => {
            log::debug!("Skipping LRU cache memory preallocation (view mode - deterministic memory layout not required)");
        }
    }
}

/// Key parser for intelligent formatting of cache keys
///
/// This module provides functionality to parse cache keys that contain
/// mixed UTF-8 and binary data, formatting them in a human-readable way.
/// Keys are expected to follow patterns like "/path/segments/binary_data"
/// where path segments are UTF-8 strings separated by '/' and binary data
/// is displayed as hexadecimal.
pub mod key_parser {
    /// Configuration for key parsing behavior
    #[derive(Debug, Clone)]
    pub struct KeyParseConfig {
        /// Maximum length of UTF-8 segments to display (default: 32)
        pub max_utf8_segment_length: usize,
        /// Maximum length of binary segments to display in hex (default: 16)
        pub max_binary_segment_length: usize,
        /// Whether to show full hex for short binary segments (default: true)
        pub show_full_short_binary: bool,
        /// Minimum length to consider a segment as potentially UTF-8 (default: 2)
        pub min_utf8_segment_length: usize,
    }
    
    impl Default for KeyParseConfig {
        fn default() -> Self {
            Self {
                max_utf8_segment_length: 32,
                max_binary_segment_length: 16,
                show_full_short_binary: true,
                min_utf8_segment_length: 2,
            }
        }
    }
    
    /// Represents a parsed segment of a key
    #[derive(Debug, Clone)]
    pub enum KeySegment {
        /// UTF-8 text segment
        Text(String),
        /// Binary data segment
        Binary(Vec<u8>),
        /// Separator (typically '/')
        Separator,
    }
    
    /// Parse a key into human-readable segments
    ///
    /// This function intelligently parses a key by:
    /// 1. Splitting on '/' separators when they appear to be path delimiters
    /// 2. Detecting UTF-8 segments vs binary data
    /// 3. Formatting binary data as hexadecimal
    ///
    /// # Arguments
    ///
    /// * `key` - The raw key bytes to parse
    /// * `config` - Configuration for parsing behavior
    ///
    /// # Returns
    ///
    /// A formatted string representation of the key
    ///
    /// # Example
    ///
    /// ```rust
    /// use metashrew_support::lru_cache::key_parser::{parse_key_readable, KeyParseConfig};
    ///
    /// let key = b"/blockhash/byheight/\x01\x00\x00\x00";
    /// let config = KeyParseConfig::default();
    /// let formatted = parse_key_readable(key, &config);
    /// // Result: "/blockhash/byheight/01000000"
    /// ```
    pub fn parse_key_readable(key: &[u8], config: &KeyParseConfig) -> String {
        let segments = parse_key_segments(key, config);
        format_segments(&segments, config)
    }
    
    /// Parse key into segments
    fn parse_key_segments(key: &[u8], config: &KeyParseConfig) -> Vec<KeySegment> {
        let mut segments = Vec::new();
        let mut current_pos = 0;
        
        while current_pos < key.len() {
            // Check for separator
            if key[current_pos] == b'/' {
                segments.push(KeySegment::Separator);
                current_pos += 1;
                continue;
            }
            
            // Find the next separator or end of key
            let segment_end = key[current_pos..]
                .iter()
                .position(|&b| b == b'/')
                .map(|pos| current_pos + pos)
                .unwrap_or(key.len());
            
            let segment_bytes = &key[current_pos..segment_end];
            
            // Try to parse as UTF-8
            if segment_bytes.len() >= config.min_utf8_segment_length {
                if let Ok(utf8_str) = std::str::from_utf8(segment_bytes) {
                    // Check if it looks like a reasonable UTF-8 string
                    if is_reasonable_utf8(utf8_str) {
                        segments.push(KeySegment::Text(utf8_str.to_string()));
                        current_pos = segment_end;
                        continue;
                    }
                }
            }
            
            // Treat as binary data
            segments.push(KeySegment::Binary(segment_bytes.to_vec()));
            current_pos = segment_end;
        }
        
        segments
    }
    
    /// Check if a UTF-8 string looks reasonable (printable, not too many control chars)
    fn is_reasonable_utf8(s: &str) -> bool {
        // Must be mostly printable ASCII or common UTF-8
        let printable_count = s.chars()
            .filter(|c| c.is_ascii_graphic() || c.is_ascii_whitespace() || *c as u32 > 127)
            .count();
        
        // At least 70% should be reasonable characters
        printable_count as f64 / s.len() as f64 >= 0.7
    }
    
    /// Format parsed segments into a readable string
    fn format_segments(segments: &[KeySegment], config: &KeyParseConfig) -> String {
        let mut result = String::new();
        
        for segment in segments {
            match segment {
                KeySegment::Text(text) => {
                    if text.len() > config.max_utf8_segment_length {
                        result.push_str(&text[..config.max_utf8_segment_length]);
                        result.push_str("...");
                    } else {
                        result.push_str(text);
                    }
                }
                KeySegment::Binary(data) => {
                    if data.is_empty() {
                        continue;
                    }
                    
                    let hex_str = if config.show_full_short_binary && data.len() <= 8 {
                        // Show full hex for short binary data
                        hex::encode(data)
                    } else if data.len() > config.max_binary_segment_length {
                        // Truncate long binary data
                        let truncated = &data[..config.max_binary_segment_length];
                        format!("{}...", hex::encode(truncated))
                    } else {
                        hex::encode(data)
                    };
                    
                    result.push_str(&hex_str);
                }
                KeySegment::Separator => {
                    result.push('/');
                }
            }
        }
        
        result
    }
    
    /// Parse a key with default configuration
    ///
    /// Convenience function that uses default parsing configuration.
    ///
    /// # Arguments
    ///
    /// * `key` - The raw key bytes to parse
    ///
    /// # Returns
    ///
    /// A formatted string representation of the key
    pub fn parse_key_default(key: &[u8]) -> String {
        parse_key_readable(key, &KeyParseConfig::default())
    }
    
    /// Advanced key parsing with heuristics for common patterns
    ///
    /// This function applies additional heuristics to detect common patterns:
    /// - Little-endian integers (4 bytes, 8 bytes)
    /// - Hash-like data (20, 32 bytes)
    /// - Timestamps
    ///
    /// # Arguments
    ///
    /// * `key` - The raw key bytes to parse
    /// * `config` - Configuration for parsing behavior
    ///
    /// # Returns
    ///
    /// A formatted string with enhanced pattern recognition
    pub fn parse_key_enhanced(key: &[u8], config: &KeyParseConfig) -> String {
        let segments = parse_key_segments_enhanced(key, config);
        format_segments_enhanced(&segments, config)
    }
    
    /// Enhanced segment types with pattern recognition
    #[derive(Debug, Clone)]
    pub enum EnhancedKeySegment {
        /// UTF-8 text segment
        Text(String),
        /// Binary data segment
        Binary(Vec<u8>),
        /// Little-endian 32-bit integer
        U32(u32),
        /// Little-endian 64-bit integer
        U64(u64),
        /// Hash-like data (20 or 32 bytes)
        Hash(Vec<u8>),
        /// Separator (typically '/')
        Separator,
    }
    
    /// Parse key into enhanced segments with pattern recognition
    fn parse_key_segments_enhanced(key: &[u8], config: &KeyParseConfig) -> Vec<EnhancedKeySegment> {
        let basic_segments = parse_key_segments(key, config);
        let mut enhanced_segments = Vec::new();
        
        for segment in basic_segments {
            match segment {
                KeySegment::Text(text) => {
                    enhanced_segments.push(EnhancedKeySegment::Text(text));
                }
                KeySegment::Separator => {
                    enhanced_segments.push(EnhancedKeySegment::Separator);
                }
                KeySegment::Binary(data) => {
                    // Apply pattern recognition
                    match data.len() {
                        4 => {
                            // Could be a 32-bit integer
                            let value = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                            enhanced_segments.push(EnhancedKeySegment::U32(value));
                        }
                        8 => {
                            // Could be a 64-bit integer
                            let value = u64::from_le_bytes([
                                data[0], data[1], data[2], data[3],
                                data[4], data[5], data[6], data[7],
                            ]);
                            enhanced_segments.push(EnhancedKeySegment::U64(value));
                        }
                        20 | 32 => {
                            // Likely a hash
                            enhanced_segments.push(EnhancedKeySegment::Hash(data));
                        }
                        _ => {
                            // Regular binary data
                            enhanced_segments.push(EnhancedKeySegment::Binary(data));
                        }
                    }
                }
            }
        }
        
        enhanced_segments
    }
    
    /// Format enhanced segments into a readable string
    fn format_segments_enhanced(segments: &[EnhancedKeySegment], config: &KeyParseConfig) -> String {
        let mut result = String::new();
        
        for segment in segments {
            match segment {
                EnhancedKeySegment::Text(text) => {
                    if text.len() > config.max_utf8_segment_length {
                        result.push_str(&text[..config.max_utf8_segment_length]);
                        result.push_str("...");
                    } else {
                        result.push_str(text);
                    }
                }
                EnhancedKeySegment::Binary(data) => {
                    if data.is_empty() {
                        continue;
                    }
                    
                    let hex_str = if config.show_full_short_binary && data.len() <= 8 {
                        hex::encode(data)
                    } else if data.len() > config.max_binary_segment_length {
                        let truncated = &data[..config.max_binary_segment_length];
                        format!("{}...", hex::encode(truncated))
                    } else {
                        hex::encode(data)
                    };
                    
                    result.push_str(&hex_str);
                }
                EnhancedKeySegment::U32(value) => {
                    result.push_str(&format!("{:08x}", value));
                }
                EnhancedKeySegment::U64(value) => {
                    result.push_str(&format!("{:016x}", value));
                }
                EnhancedKeySegment::Hash(data) => {
                    // Show first 8 bytes of hash
                    let preview_len = 8.min(data.len());
                    result.push_str(&hex::encode(&data[..preview_len]));
                    if data.len() > preview_len {
                        result.push_str("...");
                    }
                }
                EnhancedKeySegment::Separator => {
                    result.push('/');
                }
            }
        }
        
        result
    }
}

/// Cache allocation mode
#[derive(Debug, Clone, Copy)]
pub enum CacheAllocationMode {
    /// Indexer mode: allocate all memory to main LRU cache
    Indexer,
    /// View mode: allocate memory to height-partitioned and API caches
    View,
}

/// Current cache allocation mode
static CACHE_ALLOCATION_MODE: RwLock<CacheAllocationMode> =
    RwLock::new(CacheAllocationMode::Indexer);

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
static HEIGHT_PARTITIONED_CACHE: RwLock<Option<LruCache<HeightPartitionedKey, CacheValue>>> =
    RwLock::new(None);

/// Current view height for height-partitioned caching
///
/// When set, get() operations will use height-partitioned caching instead of
/// the main LRU cache. This is used by view functions to ensure cache isolation.
static CURRENT_VIEW_HEIGHT: RwLock<Option<u32>> = RwLock::new(None);

/// LRU cache debugging mode flag
static LRU_DEBUG_MODE: RwLock<bool> = RwLock::new(false);

/// Key prefix hit tracking for debugging
/// Maps prefix -> (hits, misses, unique_keys_set)
static PREFIX_HIT_STATS: std::sync::LazyLock<RwLock<HashMap<Vec<u8>, (u64, u64, HashSet<Vec<u8>>)>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

/// Configuration for prefix analysis
#[derive(Debug, Clone)]
pub struct PrefixAnalysisConfig {
    /// Minimum prefix length to analyze (default: 4)
    pub min_prefix_length: usize,
    /// Maximum prefix length to analyze (default: 16)
    pub max_prefix_length: usize,
    /// Minimum number of keys required for a prefix to be included (default: 2)
    pub min_keys_per_prefix: usize,
}

impl Default for PrefixAnalysisConfig {
    fn default() -> Self {
        Self {
            min_prefix_length: 8,
            max_prefix_length: 64,
            min_keys_per_prefix: 3,
        }
    }
}

/// Global prefix analysis configuration
static PREFIX_ANALYSIS_CONFIG: RwLock<PrefixAnalysisConfig> = RwLock::new(PrefixAnalysisConfig {
    min_prefix_length: 8,
    max_prefix_length: 64,
    min_keys_per_prefix: 3,
});

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

/// Key prefix statistics for debugging
#[derive(Debug, Clone)]
pub struct KeyPrefixStats {
    /// The prefix (as hex string for technical reference)
    pub prefix: String,
    /// The prefix parsed into human-readable format
    pub prefix_readable: String,
    /// Number of cache hits for this prefix
    pub hits: u64,
    /// Number of cache misses for this prefix
    pub misses: u64,
    /// Number of unique keys with this prefix
    pub unique_keys: usize,
    /// Percentage of total hits
    pub hit_percentage: f64,
}

/// LRU cache debugging statistics
#[derive(Debug, Clone, Default)]
pub struct LruDebugStats {
    /// Overall cache statistics
    pub cache_stats: CacheStats,
    /// Key prefix statistics (only prefixes with >1 key)
    pub prefix_stats: Vec<KeyPrefixStats>,
    /// Total number of prefixes analyzed
    pub total_prefixes: usize,
    /// Minimum prefix length used for analysis
    pub min_prefix_length: usize,
    /// Maximum prefix length used for analysis
    pub max_prefix_length: usize,
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
        // More accurate memory calculation:
        // - Arc overhead (reference counting, etc.)
        // - Vec overhead (capacity, length, pointer)
        // - Actual data size
        // - Additional overhead for heap allocation alignment
        let arc_overhead = std::mem::size_of::<Arc<Vec<u8>>>() * 2; // Conservative estimate
        let vec_overhead = std::mem::size_of::<Vec<u8>>() + std::mem::size_of::<usize>(); // capacity overhead
        let data_size = self.0.len();
        let alignment_overhead = (data_size + 7) & !7; // 8-byte alignment padding
        
        arc_overhead + vec_overhead + alignment_overhead
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
        // More accurate memory calculation for keys
        let arc_overhead = std::mem::size_of::<Arc<Vec<u8>>>() * 2; // Conservative estimate
        let vec_overhead = std::mem::size_of::<Vec<u8>>() + std::mem::size_of::<usize>(); // capacity overhead
        let data_size = self.0.len();
        let alignment_overhead = (data_size + 7) & !7; // 8-byte alignment padding
        
        arc_overhead + vec_overhead + alignment_overhead
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
        std::mem::size_of::<u32>()
            + std::mem::size_of::<Arc<Vec<u8>>>()
            + std::mem::size_of::<Vec<u8>>()
            + self.key.len()
    }
}

/// Initialize the LRU cache system
///
/// This function sets up the main LRU cache for key-value storage, the API cache
/// for user-defined caching, and the height-partitioned cache for view functions.
/// Memory allocation depends on the current cache allocation mode.
///
/// **IMPORTANT**: This function ensures that 1GB of memory is preallocated at startup
/// to guarantee consistent memory layout, but only in indexer mode.
///
/// # Thread Safety
///
/// This function is thread-safe and can be called multiple times. Subsequent
/// calls will be no-ops if the cache is already initialized.
pub fn initialize_lru_cache() {
    let allocation_mode = *CACHE_ALLOCATION_MODE.read().unwrap();
    
    // CRITICAL: Ensure preallocated memory is initialized FIRST (only in indexer mode)
    // This must happen before any other memory allocations to guarantee
    // consistent memory layout for WASM execution in indexer mode
    ensure_preallocated_memory();

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
        }
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
                    *api_cache = Some(LruCache::new(LRU_CACHE_MEMORY_LIMIT / 2));
                    // Half for API cache
                }
            }

            {
                let mut height_cache = HEIGHT_PARTITIONED_CACHE.write().unwrap();
                if height_cache.is_none() {
                    *height_cache = Some(LruCache::new(LRU_CACHE_MEMORY_LIMIT / 2));
                    // Half for height-partitioned cache
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

                    // Track prefix statistics for debugging
                    track_prefix_stats(key.as_ref(), result.is_some());

                    // Update statistics
                    {
                        let mut stats = CACHE_STATS.write().unwrap();
                        if result.is_some() {
                            stats.hits += 1;
                        } else {
                            stats.misses += 1;
                        }
                        stats.items = cache.len();
                        stats.memory_usage = cache.current_size();
                    }

                    return result;
                }
            }
        }
    }

    // Track prefix statistics for debugging (miss case)
    track_prefix_stats(key.as_ref(), false);

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
            stats.memory_usage = cache.current_size();

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
        let old_len = cache.len();
        let _ = cache.insert(cache_key, cache_value);
        
        // Update statistics
        {
            let mut stats = CACHE_STATS.write().unwrap();
            stats.items = cache.len();
            stats.memory_usage = cache.current_size();
            
            // If cache size decreased, items were evicted
            if cache.len() < old_len {
                stats.evictions += (old_len - cache.len()) as u64;
            }
        }
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
                        stats.memory_usage = cache.current_size();
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

/// Get the current memory usage of all caches combined
///
/// This function returns the total memory usage in bytes of the main
/// LRU cache, API cache, and height-partitioned cache.
pub fn get_total_memory_usage() -> usize {
    let mut total = 0;

    {
        let cache_guard = LRU_CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref() {
            total += cache.current_size();
        }
    }

    {
        let cache_guard = API_CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref() {
            total += cache.current_size();
        }
    }

    {
        let cache_guard = HEIGHT_PARTITIONED_CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref() {
            total += cache.current_size();
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
                        stats.memory_usage = cache.current_size();
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
            stats.memory_usage = cache.current_size();

            // If cache size decreased, items were evicted
            if cache.len() < old_len {
                stats.evictions += (old_len - cache.len()) as u64;
            }
        }
    }
}

/// Force eviction if memory usage exceeds the limit
///
/// This function checks if the total memory usage exceeds the 1GB limit and
/// forces eviction if necessary. This should be called at the end of indexer
/// runs to ensure memory stays within bounds.
pub fn force_evict_to_target() {
    let allocation_mode = *CACHE_ALLOCATION_MODE.read().unwrap();
    
    match allocation_mode {
        CacheAllocationMode::Indexer => {
            // In indexer mode, only the main LRU cache should be large
            let mut cache_guard = LRU_CACHE.write().unwrap();
            if let Some(cache) = cache_guard.as_mut() {
                let current_size = cache.current_size();
                let current_items = cache.len();
                
                // Only evict if significantly over limit to avoid thrashing
                let eviction_threshold = LRU_CACHE_MEMORY_LIMIT + (LRU_CACHE_MEMORY_LIMIT / 10); // 10% buffer
                
                if current_size > eviction_threshold {
                    println!("LRU Cache eviction triggered: {} bytes, {} items, limit: {} bytes",
                             current_size, current_items, LRU_CACHE_MEMORY_LIMIT);
                    
                    // Evict down to 90% of limit to provide breathing room
                    let target_size = LRU_CACHE_MEMORY_LIMIT - (LRU_CACHE_MEMORY_LIMIT / 10);
                    let mut evicted_count = 0;
                    
                    while cache.current_size() > target_size && !cache.is_empty() {
                        cache.remove_lru();
                        evicted_count += 1;
                        
                        // Safety check to prevent infinite loop
                        if evicted_count > current_items / 2 {
                            println!("LRU Cache eviction safety limit reached, stopping");
                            break;
                        }
                    }
                    
                    println!("LRU Cache eviction completed: evicted {} items, {} bytes remaining, {} items remaining",
                             evicted_count, cache.current_size(), cache.len());
                    
                    // Update eviction statistics
                    {
                        let mut stats = CACHE_STATS.write().unwrap();
                        stats.evictions += evicted_count as u64;
                        stats.items = cache.len();
                        stats.memory_usage = cache.current_size();
                    }
                }
            }
        },
        CacheAllocationMode::View => {
            // In view mode, check both height-partitioned and API caches
            {
                let mut cache_guard = HEIGHT_PARTITIONED_CACHE.write().unwrap();
                if let Some(cache) = cache_guard.as_mut() {
                    let target_size = LRU_CACHE_MEMORY_LIMIT / 2;
                    while cache.current_size() > target_size {
                        if cache.is_empty() {
                            break;
                        }
                        cache.remove_lru();
                        
                        {
                            let mut stats = CACHE_STATS.write().unwrap();
                            stats.evictions += 1;
                        }
                    }
                }
            }
            
            {
                let mut cache_guard = API_CACHE.write().unwrap();
                if let Some(cache) = cache_guard.as_mut() {
                    let target_size = LRU_CACHE_MEMORY_LIMIT / 2;
                    while cache.current_size() > target_size {
                        if cache.is_empty() {
                            break;
                        }
                        cache.remove_lru();
                        
                        {
                            let mut stats = CACHE_STATS.write().unwrap();
                            stats.evictions += 1;
                        }
                    }
                }
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

/// Enable LRU cache debugging mode
///
/// When enabled, the cache will track key prefix statistics for analysis.
/// This adds some overhead but provides valuable insights into cache usage patterns.
pub fn enable_lru_debug_mode() {
    let mut debug_mode = LRU_DEBUG_MODE.write().unwrap();
    *debug_mode = true;
}

/// Disable LRU cache debugging mode
pub fn disable_lru_debug_mode() {
    let mut debug_mode = LRU_DEBUG_MODE.write().unwrap();
    *debug_mode = false;
}

/// Check if LRU cache debugging mode is enabled
pub fn is_lru_debug_mode_enabled() -> bool {
    *LRU_DEBUG_MODE.read().unwrap()
}

/// Set the prefix analysis configuration
pub fn set_prefix_analysis_config(config: PrefixAnalysisConfig) {
    let mut analysis_config = PREFIX_ANALYSIS_CONFIG.write().unwrap();
    *analysis_config = config;
}

/// Get the current prefix analysis configuration
pub fn get_prefix_analysis_config() -> PrefixAnalysisConfig {
    PREFIX_ANALYSIS_CONFIG.read().unwrap().clone()
}

/// Clear all prefix hit statistics
pub fn clear_prefix_hit_stats() {
    let mut stats = PREFIX_HIT_STATS.write().unwrap();
    stats.clear();
}

/// Helper function to track prefix statistics for a key access
fn track_prefix_stats(key: &[u8], is_hit: bool) {
    if !is_lru_debug_mode_enabled() {
        return;
    }

    let config = get_prefix_analysis_config();
    let mut stats = PREFIX_HIT_STATS.write().unwrap();

    // Find the longest meaningful UTF-8 prefix for this key
    if let Some(meaningful_prefix) = find_longest_meaningful_prefix(key, &config) {
        let entry = stats.entry(meaningful_prefix).or_insert((0, 0, HashSet::new()));
        
        // Update hit/miss counts
        if is_hit {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
        
        // Track unique keys for this prefix
        entry.2.insert(key.to_vec());
    }
}

/// Find the longest meaningful UTF-8 prefix for a key
///
/// This function analyzes a key to find the longest prefix that:
/// 1. Contains only word characters and slashes [0-9a-zA-Z/]
/// 2. Must end with a '/' character
/// 3. Is within the configured length limits
/// 4. Stops at the last valid '/' before any invalid characters
fn find_longest_meaningful_prefix(key: &[u8], config: &PrefixAnalysisConfig) -> Option<Vec<u8>> {
    if key.len() < config.min_prefix_length {
        return None;
    }

    // Convert to string to work with characters
    let key_str = match std::str::from_utf8(key) {
        Ok(s) => s,
        Err(_) => {
            // If the key contains binary data, try to find the longest valid UTF-8 prefix
            let mut valid_len = 0;
            for i in 1..=key.len() {
                if std::str::from_utf8(&key[..i]).is_ok() {
                    valid_len = i;
                } else {
                    break;
                }
            }
            if valid_len == 0 {
                return None;
            }
            std::str::from_utf8(&key[..valid_len]).unwrap()
        }
    };

    // Find the longest valid prefix that ends with '/'
    let mut last_slash_pos = 0;
    let max_len = config.max_prefix_length.min(key_str.len());
    
    // Scan through the string to find valid characters and track the last '/' position
    for (i, c) in key_str.char_indices() {
        if i >= max_len {
            break;
        }
        
        // Check if character is valid [0-9a-zA-Z/]
        if c.is_ascii_alphanumeric() || c == '/' {
            // Track the position after each '/' character
            if c == '/' {
                last_slash_pos = i + 1; // Position after the '/'
            }
        } else {
            // Invalid character found - stop here and use the last valid '/' position
            break;
        }
    }
    
    // Only return a prefix if we found a '/' and the prefix is long enough
    if last_slash_pos >= config.min_prefix_length && last_slash_pos <= max_len {
        Some(key_str[..last_slash_pos].as_bytes().to_vec())
    } else {
        None
    }
}

/// Generate comprehensive LRU debug statistics
pub fn get_lru_debug_stats() -> LruDebugStats {
    let cache_stats = get_cache_stats();
    let config = get_prefix_analysis_config();
    
    let mut debug_stats = LruDebugStats {
        cache_stats,
        prefix_stats: Vec::new(),
        total_prefixes: 0,
        min_prefix_length: config.min_prefix_length,
        max_prefix_length: config.max_prefix_length,
    };

    if !is_lru_debug_mode_enabled() {
        return debug_stats;
    }

    let stats = PREFIX_HIT_STATS.read().unwrap();
    let total_hits = debug_stats.cache_stats.hits as f64;
    
    // Collect all qualifying prefixes
    let mut candidate_stats = Vec::new();
    for (prefix, (hits, misses, unique_keys)) in stats.iter() {
        if unique_keys.len() >= config.min_keys_per_prefix {
            let hit_percentage = if total_hits > 0.0 {
                (*hits as f64 / total_hits) * 100.0
            } else {
                0.0
            };

            // Parse the prefix into human-readable format
            let prefix_readable = key_parser::parse_key_enhanced(prefix, &key_parser::KeyParseConfig::default());

            candidate_stats.push(KeyPrefixStats {
                prefix: hex::encode(prefix),
                prefix_readable: prefix_readable.clone(),
                hits: *hits,
                misses: *misses,
                unique_keys: unique_keys.len(),
                hit_percentage,
            });
        }
    }

    // Remove redundant prefixes - keep only the most specific ones
    let filtered_stats = filter_redundant_prefixes(candidate_stats);
    
    // Sort by total accesses (hits + misses) descending to show most active prefixes first
    debug_stats.prefix_stats = filtered_stats;
    debug_stats.prefix_stats.sort_by(|a, b| {
        let total_a = a.hits + a.misses;
        let total_b = b.hits + b.misses;
        total_b.cmp(&total_a)
    });
    
    debug_stats.total_prefixes = debug_stats.prefix_stats.len();

    debug_stats
}

/// Filter out redundant prefixes, keeping only the most specific meaningful ones
///
/// This function removes shorter prefixes when longer, more specific prefixes exist
/// that cover the same key space with similar access patterns.
fn filter_redundant_prefixes(mut stats: Vec<KeyPrefixStats>) -> Vec<KeyPrefixStats> {
    // Sort by prefix length (longest first) to prioritize more specific prefixes
    stats.sort_by(|a, b| {
        let len_a = a.prefix_readable.len();
        let len_b = b.prefix_readable.len();
        len_b.cmp(&len_a)
    });
    
    let mut filtered = Vec::new();
    
    for candidate in stats {
        let mut is_redundant = false;
        
        // Check if this candidate is redundant with any already accepted prefix
        for accepted in &filtered {
            if is_prefix_redundant(&candidate, accepted) {
                is_redundant = true;
                break;
            }
        }
        
        if !is_redundant {
            filtered.push(candidate);
        }
    }
    
    filtered
}

/// Check if one prefix is redundant compared to another
///
/// A prefix is considered redundant if:
/// 1. It's a substring of a longer prefix
/// 2. The longer prefix has similar or higher access counts
/// 3. The access patterns are similar (hit rates within reasonable range)
fn is_prefix_redundant(candidate: &KeyPrefixStats, existing: &KeyPrefixStats) -> bool {
    // If candidate is longer or same length, it's not redundant
    if candidate.prefix_readable.len() >= existing.prefix_readable.len() {
        return false;
    }
    
    // Check if candidate is a prefix of existing
    if !existing.prefix_readable.starts_with(&candidate.prefix_readable) {
        return false;
    }
    
    // If the existing prefix has significantly more accesses, candidate is redundant
    let candidate_total = candidate.hits + candidate.misses;
    let existing_total = existing.hits + existing.misses;
    
    // If existing has at least 80% of candidate's accesses, consider candidate redundant
    if existing_total as f64 >= (candidate_total as f64 * 0.8) {
        return true;
    }
    
    false
}

/// Generate a formatted debug report
pub fn generate_lru_debug_report() -> String {
    let stats = get_lru_debug_stats();
    
    if !is_lru_debug_mode_enabled() {
        return "LRU Debug mode is disabled. Enable with enable_lru_debug_mode().".to_string();
    }

    let mut report = String::new();
    report.push_str("🔍 LRU CACHE DEBUG REPORT\n");
    report.push_str("═══════════════════════════════════════════════════════════════\n\n");
    
    // Overall cache stats
    report.push_str(&format!("📊 OVERALL CACHE STATISTICS\n"));
    report.push_str(&format!("├── Total Hits: {}\n", stats.cache_stats.hits));
    report.push_str(&format!("├── Total Misses: {}\n", stats.cache_stats.misses));
    report.push_str(&format!("├── Hit Rate: {:.1}%\n",
        if stats.cache_stats.hits + stats.cache_stats.misses > 0 {
            (stats.cache_stats.hits as f64 / (stats.cache_stats.hits + stats.cache_stats.misses) as f64) * 100.0
        } else { 0.0 }));
    report.push_str(&format!("├── Current Items: {}\n", stats.cache_stats.items));
    report.push_str(&format!("├── Memory Usage: {} bytes\n", stats.cache_stats.memory_usage));
    report.push_str(&format!("└── Evictions: {}\n\n", stats.cache_stats.evictions));

    // Prefix analysis
    report.push_str(&format!("🔑 KEY PREFIX ANALYSIS\n"));
    report.push_str(&format!("├── Analyzed Prefix Lengths: {}-{} bytes\n",
        stats.min_prefix_length, stats.max_prefix_length));
    report.push_str(&format!("├── Total Qualifying Prefixes: {}\n", stats.total_prefixes));
    report.push_str(&format!("└── Minimum Keys per Prefix: {}\n\n",
        get_prefix_analysis_config().min_keys_per_prefix));

    if stats.prefix_stats.is_empty() {
        report.push_str("No qualifying prefixes found (need at least 2 keys per prefix).\n");
        return report;
    }

    // Top prefixes by hit count
    report.push_str("🏆 TOP KEY PREFIXES BY CACHE HITS\n");
    report.push_str("┌─────────────────────────────────────────────────────────────────────────────────┐\n");
    report.push_str("│ Prefix (readable)                    │ Hits    │ Misses  │ Keys │ Hit %    │\n");
    report.push_str("├─────────────────────────────────────────────────────────────────────────────────┤\n");

    for (_i, prefix_stat) in stats.prefix_stats.iter().take(20).enumerate() {
        // Use readable format, truncate if too long
        let readable_prefix = if prefix_stat.prefix_readable.len() > 36 {
            format!("{}...", &prefix_stat.prefix_readable[..33])
        } else {
            prefix_stat.prefix_readable.clone()
        };
        
        report.push_str(&format!(
            "│ {:36} │ {:7} │ {:7} │ {:4} │ {:6.1}% │\n",
            readable_prefix,
            prefix_stat.hits,
            prefix_stat.misses,
            prefix_stat.unique_keys,
            prefix_stat.hit_percentage
        ));
    }
    
    report.push_str("└─────────────────────────────────────────────────────────────────────────────────┘\n\n");

    // Summary insights
    if !stats.prefix_stats.is_empty() {
        let top_5_percentage: f64 = stats.prefix_stats.iter().take(5).map(|s| s.hit_percentage).sum();
        report.push_str("💡 INSIGHTS\n");
        report.push_str(&format!("├── Top 5 prefixes account for {:.1}% of all cache hits\n", top_5_percentage));
        
        let high_hit_prefixes = stats.prefix_stats.iter().filter(|s| s.hit_percentage > 5.0).count();
        report.push_str(&format!("├── {} prefixes have >5% hit rate\n", high_hit_prefixes));
        
        let avg_keys_per_prefix: f64 = stats.prefix_stats.iter().map(|s| s.unique_keys as f64).sum::<f64>() / stats.prefix_stats.len() as f64;
        report.push_str(&format!("└── Average keys per prefix: {:.1}\n", avg_keys_per_prefix));
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use lru_mem::MemSize;

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
        // Clear all caches first to ensure clean state
        clear_lru_cache();
        
        // Set to View mode to ensure API cache gets proper allocation
        set_cache_allocation_mode(CacheAllocationMode::View);
        
        // Force reinitialization by clearing and reinitializing
        {
            let mut api_cache = API_CACHE.write().unwrap();
            *api_cache = None;
        }
        {
            let mut main_cache = LRU_CACHE.write().unwrap();
            *main_cache = None;
        }
        {
            let mut height_cache = HEIGHT_PARTITIONED_CACHE.write().unwrap();
            *height_cache = None;
        }
        initialize_lru_cache();

        let key = "test_api_key".to_string();
        let value = Arc::new(b"test_api_value".to_vec());

        // Test set and get
        api_cache_set(key.clone(), value.clone());
        let retrieved = api_cache_get(&key);
        assert_eq!(retrieved, Some(value.clone()), "API cache should return the stored value");

        // Test cache miss
        let missing = api_cache_get("missing_api_key");
        assert_eq!(missing, None);

        // Test remove
        let removed = api_cache_remove(&key);
        assert_eq!(removed, Some(value), "Remove should return the stored value");

        // Verify removal
        let after_remove = api_cache_get(&key);
        assert_eq!(after_remove, None);
        
        // Reset to default mode and clear caches
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        clear_lru_cache();
    }

    #[test]
    fn test_cache_stats() {
        // Ensure we're in indexer mode for this test
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        
        // Force reinitialization to ensure proper allocation
        {
            let mut cache = LRU_CACHE.write().unwrap();
            *cache = None;
        }
        {
            let mut api_cache = API_CACHE.write().unwrap();
            *api_cache = None;
        }
        {
            let mut height_cache = HEIGHT_PARTITIONED_CACHE.write().unwrap();
            *height_cache = None;
        }
        
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
        assert!(hit_result.is_some(), "Should be a hit after setting value"); // Should be a hit
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
        assert!(
            small_size >= 3 + std::mem::size_of::<Arc<Vec<u8>>>() + std::mem::size_of::<Vec<u8>>()
        );
    }

    #[test]
    fn test_total_memory_usage_reporting() {
        // Test that get_total_memory_usage() correctly reports memory usage
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        
        // Force reinitialization to ensure proper allocation
        {
            let mut cache = LRU_CACHE.write().unwrap();
            *cache = None;
        }
        {
            let mut api_cache = API_CACHE.write().unwrap();
            *api_cache = None;
        }
        {
            let mut height_cache = HEIGHT_PARTITIONED_CACHE.write().unwrap();
            *height_cache = None;
        }
        
        initialize_lru_cache();
        clear_lru_cache();

        // Get initial memory usage - should be minimal
        let initial_memory = get_total_memory_usage();
        println!("Initial memory usage: {} bytes", initial_memory);

        // Add some data to the main LRU cache
        let test_data = vec![
            (b"key1".to_vec(), vec![0u8; 1000]), // 1KB value
            (b"key2".to_vec(), vec![1u8; 2000]), // 2KB value
            (b"key3".to_vec(), vec![2u8; 3000]), // 3KB value
        ];

        for (key, value) in &test_data {
            set_lru_cache(Arc::new(key.clone()), Arc::new(value.clone()));
        }

        // Get memory usage after adding data
        let after_data_memory = get_total_memory_usage();
        println!("After adding data: {} bytes", after_data_memory);

        // Memory usage should have increased significantly
        assert!(after_data_memory > initial_memory + 3000,
                "Memory usage should have increased significantly. Initial: {}, After: {}",
                initial_memory, after_data_memory);

        // Memory usage should be reasonable (not just 4 or 8 bytes)
        assert!(after_data_memory > 1000,
                "Memory usage should be substantial, got: {}", after_data_memory);

        println!("✅ Total memory usage reporting test passed!");
    }

    #[test]
    fn test_cache_stats_memory_consistency() {
        // Test that cached stats and get_total_memory_usage() are consistent
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        
        // Force reinitialization to ensure proper allocation
        {
            let mut cache = LRU_CACHE.write().unwrap();
            *cache = None;
        }
        {
            let mut api_cache = API_CACHE.write().unwrap();
            *api_cache = None;
        }
        {
            let mut height_cache = HEIGHT_PARTITIONED_CACHE.write().unwrap();
            *height_cache = None;
        }
        
        initialize_lru_cache();
        clear_lru_cache();

        // Add some test data
        let key = Arc::new(b"test_key_for_consistency".to_vec());
        let value = Arc::new(vec![42u8; 5000]); // 5KB value
        
        set_lru_cache(key.clone(), value.clone());

        // Get stats and direct memory usage
        let stats = get_cache_stats();
        let direct_memory = get_total_memory_usage();

        println!("Stats memory usage: {} bytes", stats.memory_usage);
        println!("Direct memory usage: {} bytes", direct_memory);

        // They should be reasonably close (within some margin for overhead differences)
        let difference = if direct_memory > stats.memory_usage {
            direct_memory - stats.memory_usage
        } else {
            stats.memory_usage - direct_memory
        };

        // Allow for some difference due to different calculation methods
        assert!(difference < 1000,
                "Memory usage reporting should be consistent. Stats: {}, Direct: {}, Difference: {}",
                stats.memory_usage, direct_memory, difference);

        // Both should be substantial (not just a few bytes)
        assert!(stats.memory_usage > 1000,
                "Stats memory usage should be substantial, got: {}", stats.memory_usage);
        assert!(direct_memory > 1000,
                "Direct memory usage should be substantial, got: {}", direct_memory);

        println!("✅ Cache stats memory consistency test passed!");
    }

    #[test]
    fn test_heap_size_implementation() {
        // Test that our HeapSize implementations are working correctly
        let small_data = Arc::new(vec![1, 2, 3]);
        let large_data = Arc::new(vec![0u8; 1000]);

        let small_cache_value = CacheValue::from(small_data.clone());
        let large_cache_value = CacheValue::from(large_data.clone());

        let small_heap_size = small_cache_value.heap_size();
        let large_heap_size = large_cache_value.heap_size();

        println!("Small data (3 bytes): heap_size = {}", small_heap_size);
        println!("Large data (1000 bytes): heap_size = {}", large_heap_size);

        // Large should be bigger than small
        assert!(large_heap_size > small_heap_size,
                "Large heap size ({}) should be greater than small heap size ({})",
                large_heap_size, small_heap_size);

        // Both should be reasonable (not zero)
        assert!(small_heap_size > 0, "Small heap size should be greater than 0");
        assert!(large_heap_size > 0, "Large heap size should be greater than 0");

        // Test with cache
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        
        // Force reinitialization to ensure proper allocation
        {
            let mut cache = LRU_CACHE.write().unwrap();
            *cache = None;
        }
        {
            let mut api_cache = API_CACHE.write().unwrap();
            *api_cache = None;
        }
        {
            let mut height_cache = HEIGHT_PARTITIONED_CACHE.write().unwrap();
            *height_cache = None;
        }
        
        initialize_lru_cache();
        clear_lru_cache();

        let key = Arc::new(b"test_key".to_vec());
        set_lru_cache(key, large_data);

        let cache_guard = LRU_CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref() {
            let cache_mem_size = cache.current_size();
            println!("Cache mem_size after adding large data: {}", cache_mem_size);
            assert!(cache_mem_size > 0, "Cache mem_size should be greater than 0");
        }

        println!("✅ HeapSize implementation test passed!");
    }

    #[test]
    fn test_lru_cache_methods() {
        // Test what methods are available on LruCache
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        initialize_lru_cache();
        clear_lru_cache();

        let key = Arc::new(b"test_key".to_vec());
        let value = Arc::new(vec![0u8; 1000]);
        set_lru_cache(key, value);

        let cache_guard = LRU_CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref() {
            println!("Cache len: {}", cache.len());
            
            // Try different method names that might exist
            // Let's see what methods are available by trying to call them
            
            // This should work if the method exists
            let current_size = cache.current_size();
            println!("Cache current_size: {}", current_size);
            
            let max_size = cache.max_size();
            println!("Cache max_size: {}", max_size);
        }

        println!("✅ LRU cache methods test passed!");
    }

    #[test]
    fn test_lru_debug_functionality() {
        // Clear any existing state
        clear_lru_cache();
        disable_lru_debug_mode();
        clear_prefix_hit_stats();
        
        // Set cache allocation mode to indexer for this test
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        initialize_lru_cache();
        
        // Enable debug mode
        enable_lru_debug_mode();
        assert!(is_lru_debug_mode_enabled());
        
        // Configure prefix analysis
        let config = PrefixAnalysisConfig {
            min_prefix_length: 2,
            max_prefix_length: 4,
            min_keys_per_prefix: 1, // Lower threshold for testing
        };
        set_prefix_analysis_config(config);
        
        // Create test keys with common prefixes
        let keys_and_values = vec![
            (b"aa_key1".to_vec(), b"value1".to_vec()),
            (b"aa_key2".to_vec(), b"value2".to_vec()),
            (b"bb_key1".to_vec(), b"value3".to_vec()),
            (b"bb_key2".to_vec(), b"value4".to_vec()),
            (b"cc_unique".to_vec(), b"value5".to_vec()),
        ];
        
        // Insert values into cache
        for (key, value) in &keys_and_values {
            set_lru_cache(Arc::new(key.clone()), Arc::new(value.clone()));
        }
        
        // Access some keys to generate hits
        for (key, _) in &keys_and_values[0..3] {
            let result = get_lru_cache(&Arc::new(key.clone()));
            assert!(result.is_some());
        }
        
        // Test cache miss
        let missing_key = Arc::new(b"missing_key".to_vec());
        let miss_result = get_lru_cache(&missing_key);
        assert!(miss_result.is_none());
        
        // Get debug stats
        let debug_stats = get_lru_debug_stats();
        assert!(!debug_stats.prefix_stats.is_empty(), "Should have prefix statistics");
        
        // Verify we have stats for "aa" and "bb" prefixes
        let has_aa_prefix = debug_stats.prefix_stats.iter()
            .any(|stat| stat.prefix_readable.starts_with("aa")); // "aa" in readable format
        let has_bb_prefix = debug_stats.prefix_stats.iter()
            .any(|stat| stat.prefix_readable.starts_with("bb")); // "bb" in readable format
            
        assert!(has_aa_prefix, "Should have statistics for 'aa' prefix");
        assert!(has_bb_prefix, "Should have statistics for 'bb' prefix");
        
        // Generate and verify debug report
        let report = generate_lru_debug_report();
        assert!(report.contains("LRU CACHE DEBUG REPORT"), "Report should contain header");
        assert!(report.contains("KEY PREFIX ANALYSIS"), "Report should contain prefix analysis");
        assert!(report.len() > 100, "Report should be substantial");
        
        // Test disabling debug mode
        disable_lru_debug_mode();
        assert!(!is_lru_debug_mode_enabled());
        
        // Clear stats
        clear_prefix_hit_stats();
        let cleared_stats = get_lru_debug_stats();
        assert!(cleared_stats.prefix_stats.is_empty(), "Stats should be cleared");
        
        println!("✅ LRU debug functionality test passed!");
        
        // Reset to default state
        set_cache_allocation_mode(CacheAllocationMode::Indexer);
        clear_lru_cache();
    }

    #[test]
    fn test_key_parser_functionality() {
        use crate::lru_cache::key_parser::{parse_key_default, parse_key_enhanced, KeyParseConfig, parse_key_readable};
        
        // Test basic path parsing
        let key1 = b"/blockhash/byheight/\x01\x00\x00\x00";
        let parsed1 = parse_key_default(key1);
        println!("Basic parsing: {:?} -> {}", std::str::from_utf8(key1).unwrap_or("binary"), parsed1);
        assert!(parsed1.contains("/blockhash/byheight/"));
        assert!(parsed1.contains("01000000"));
        
        // Test enhanced parsing with pattern recognition
        let parsed1_enhanced = parse_key_enhanced(key1, &KeyParseConfig::default());
        println!("Enhanced parsing: {:?} -> {}", std::str::from_utf8(key1).unwrap_or("binary"), parsed1_enhanced);
        assert!(parsed1_enhanced.contains("/blockhash/byheight/"));
        assert!(parsed1_enhanced.contains("00000001")); // Should recognize as little-endian u32
        
        // Test with different key patterns
        let key2 = b"/user/profile/\x12\x34\x56\x78\x9a\xbc\xde\xf0";
        let parsed2 = parse_key_default(key2);
        println!("User key: {:?} -> {}", std::str::from_utf8(&key2[..13]).unwrap_or("binary"), parsed2);
        assert!(parsed2.contains("/user/profile/"));
        
        // Test with hash-like data (32 bytes)
        let hash_data = [0u8; 32];
        let mut key3 = b"/tx/hash/".to_vec();
        key3.extend_from_slice(&hash_data);
        let parsed3 = parse_key_enhanced(&key3, &KeyParseConfig::default());
        println!("Hash key: -> {}", parsed3);
        assert!(parsed3.contains("/tx/hash/"));
        
        // Test with custom configuration
        let config = KeyParseConfig {
            max_utf8_segment_length: 10,
            max_binary_segment_length: 4,
            show_full_short_binary: true,
            min_utf8_segment_length: 2,
        };
        let key4 = b"/very/long/path/segment/\x01\x02\x03\x04\x05\x06\x07\x08";
        let parsed4 = parse_key_readable(key4, &config);
        println!("Custom config: -> {}", parsed4);
        assert!(parsed4.contains("/very/long/"));
        
        // Test pure binary data
        let key5 = b"\x01\x02\x03\x04\x05\x06\x07\x08";
        let parsed5 = parse_key_default(key5);
        println!("Pure binary: -> {}", parsed5);
        assert_eq!(parsed5, "0102030405060708");
        
        // Test mixed UTF-8 and binary
        let key6 = b"/index/\xff\xfe\xfd/data/\x01\x00";
        let parsed6 = parse_key_default(key6);
        println!("Mixed data: -> {}", parsed6);
        assert!(parsed6.contains("/index/"));
        assert!(parsed6.contains("/data/"));
        
        println!("✅ Key parser functionality test passed!");
    }
}

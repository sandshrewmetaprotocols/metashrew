# LRU Cache Debugging Implementation

## Overview

This document describes the comprehensive LRU cache debugging functionality that has been implemented in the Metashrew project. The debugging system provides detailed insights into cache usage patterns, key prefix analysis, and performance metrics to help optimize cache behavior and understand data access patterns.

**NEW**: The system now includes intelligent key parsing that converts raw binary cache keys into human-readable formats, making cache analysis much more intuitive.

## Features

### 1. Memory Usage Bug Fix

**Problem**: The LRU cache was incorrectly reporting memory usage as only 4-8 bytes despite storing large amounts of data.

**Solution**: Fixed the memory calculation by changing from `cache.mem_size()` to `cache.current_size()` throughout the codebase and updated the memory tracking logic.

**Impact**: Now correctly reports actual memory usage (e.g., "Moving 70170 items (7095226 bytes of data)" with accurate byte counts).

### 2. Intelligent Key Parsing

**Purpose**: Convert raw binary cache keys into human-readable formats for intuitive analysis.

**Features**:
- **Smart UTF-8 Detection**: Automatically detects UTF-8 text segments vs binary data
- **Path Structure Recognition**: Preserves `/path/like/structures` in keys
- **Binary Data Formatting**: Displays binary segments as hexadecimal
- **Pattern Recognition**: Detects common patterns like little-endian integers, hashes, timestamps
- **Configurable Display**: Adjustable segment lengths and formatting options

**Examples**:
```
Raw: 2f626c6f636b686173682f62796865696768742f01000000
Basic: /blockhash/byheight/01000000
Enhanced: /blockhash/byheight/00000001 (recognizes as little-endian u32)
```

### 3. Key Prefix Analysis

**Purpose**: Analyze which parts of the key space are being accessed most frequently to identify cache usage patterns.

**Features**:
- Configurable prefix lengths (default: 4-16 bytes)
- Tracks hits, misses, and unique keys per prefix
- Calculates hit percentages for each prefix
- Filters prefixes with minimum key thresholds
- Sorts results by hit count for easy analysis
- **NEW**: Displays prefixes in human-readable format alongside hex

### 3. Comprehensive Statistics

**Cache Statistics**:
- Total hits and misses
- Hit rate percentage
- Current number of items
- Accurate memory usage reporting
- Eviction counts

**Prefix Statistics**:
- Hex-encoded prefix display
- Hit/miss counts per prefix
- Number of unique keys per prefix
- Hit percentage relative to total hits

### 4. Debug Reporting

**Formatted Reports**: Human-readable reports with:
- Overall cache statistics
- Top prefixes by cache hits
- Insights and analysis
- Configurable display options

**Example Report Output**:
```
🔍 LRU CACHE DEBUG REPORT
═══════════════════════════════════════════════════════════════

📊 OVERALL CACHE STATISTICS
├── Total Hits: 25
├── Total Misses: 8
├── Hit Rate: 75.8%
├── Current Items: 53
├── Memory Usage: 8432 bytes
└── Evictions: 0

🔑 KEY PREFIX ANALYSIS
├── Analyzed Prefix Lengths: 2-8 bytes
├── Total Qualifying Prefixes: 12
└── Minimum Keys per Prefix: 2

🏆 TOP KEY PREFIXES BY CACHE HITS
┌─────────────────────────────────────────────────────────────────────────────────┐
│ Prefix (readable)                    │ Hits    │ Misses  │ Keys │ Hit %    │
├─────────────────────────────────────────────────────────────────────────────────┤
│ /blockhash/byheight/                 │      16 │       0 │    8 │   64.0% │
│ /balance/address/                    │       6 │       0 │   10 │   24.0% │
│ /tx/index/                           │       3 │       0 │   15 │   12.0% │
└─────────────────────────────────────────────────────────────────────────────────┘

💡 INSIGHTS
├── Top 5 prefixes account for 100.0% of all cache hits
├── 3 prefixes have >5% hit rate
└── Average keys per prefix: 11.0
```

## API Reference

### Core Functions

#### `enable_lru_debug_mode()`
Enables LRU cache debugging mode. When enabled, the cache tracks key prefix statistics for analysis.

#### `disable_lru_debug_mode()`
Disables LRU cache debugging mode to reduce overhead.

#### `is_lru_debug_mode_enabled() -> bool`
Returns whether debug mode is currently enabled.

### Configuration

#### `set_prefix_analysis_config(config: PrefixAnalysisConfig)`
Configures prefix analysis parameters:
```rust
let config = PrefixAnalysisConfig {
    min_prefix_length: 4,    // Minimum prefix length to analyze
    max_prefix_length: 16,   // Maximum prefix length to analyze
    min_keys_per_prefix: 2,  // Minimum keys required for inclusion
};
```

#### `get_prefix_analysis_config() -> PrefixAnalysisConfig`
Returns the current prefix analysis configuration.

### Statistics and Reporting

#### `get_lru_debug_stats() -> LruDebugStats`
Returns comprehensive debug statistics including:
- Overall cache statistics
- Key prefix statistics
- Configuration parameters

#### `generate_lru_debug_report() -> String`
Generates a formatted, human-readable debug report.

#### `clear_prefix_hit_stats()`
Clears all collected prefix statistics for fresh analysis.

### Key Parsing Functions

#### `parse_cache_key(key: &[u8]) -> String`
Parse a cache key into human-readable format using default configuration.

#### `parse_cache_key_enhanced(key: &[u8]) -> String`
Parse a cache key with enhanced pattern recognition for integers, hashes, etc.

#### `parse_cache_key_with_config(key: &[u8], config: &KeyParseConfig) -> String`
Parse a cache key with custom configuration for full control over formatting.

## Data Structures

### `PrefixAnalysisConfig`
```rust
pub struct PrefixAnalysisConfig {
    pub min_prefix_length: usize,    // Default: 4
    pub max_prefix_length: usize,    // Default: 16
    pub min_keys_per_prefix: usize,  // Default: 2
}
```

### `KeyPrefixStats`
```rust
pub struct KeyPrefixStats {
    pub prefix: String,          // Hex-encoded prefix
    pub prefix_readable: String, // Human-readable prefix format
    pub hits: u64,              // Cache hits for this prefix
    pub misses: u64,            // Cache misses for this prefix
    pub unique_keys: usize,     // Number of unique keys
    pub hit_percentage: f64,    // Percentage of total hits
}
```

### `KeyParseConfig`
```rust
pub struct KeyParseConfig {
    pub max_utf8_segment_length: usize,    // Default: 32
    pub max_binary_segment_length: usize,  // Default: 16
    pub show_full_short_binary: bool,      // Default: true
    pub min_utf8_segment_length: usize,    // Default: 2
}
```

### `LruDebugStats`
```rust
pub struct LruDebugStats {
    pub cache_stats: CacheStats,           // Overall cache statistics
    pub prefix_stats: Vec<KeyPrefixStats>, // Prefix statistics
    pub total_prefixes: usize,             // Total qualifying prefixes
    pub min_prefix_length: usize,          // Analysis configuration
    pub max_prefix_length: usize,          // Analysis configuration
}
```

## Usage Examples

### Basic Usage
```rust
use metashrew_core::{
    initialize, enable_lru_debug_mode, generate_lru_debug_report
};

// Initialize and enable debugging
initialize();
enable_lru_debug_mode();

// Perform cache operations...
// (your normal cache usage here)

// Generate debug report
let report = generate_lru_debug_report();
println!("{}", report);
```

### Advanced Configuration
```rust
use metashrew_core::{
    enable_lru_debug_mode, set_prefix_analysis_config,
    get_lru_debug_stats, PrefixAnalysisConfig
};

// Enable debugging with custom configuration
enable_lru_debug_mode();

let config = PrefixAnalysisConfig {
    min_prefix_length: 8,
    max_prefix_length: 20,
    min_keys_per_prefix: 5,
};
set_prefix_analysis_config(config);

// Perform operations...

// Get detailed statistics
let stats = get_lru_debug_stats();
for prefix_stat in &stats.prefix_stats {
    println!("Prefix {}: {} hits, {} keys",
        prefix_stat.prefix_readable, // Now shows human-readable format!
        prefix_stat.hits,
        prefix_stat.unique_keys
    );
}
```

### Key Parsing Examples
```rust
use metashrew_core::{
    parse_cache_key, parse_cache_key_enhanced,
    parse_cache_key_with_config, key_parser::KeyParseConfig
};

// Basic key parsing
let key = b"/blockhash/byheight/\x01\x00\x00\x00";
let readable = parse_cache_key(key);
// Result: "/blockhash/byheight/01000000"

// Enhanced parsing with pattern recognition
let enhanced = parse_cache_key_enhanced(key);
// Result: "/blockhash/byheight/00000001" (recognizes little-endian u32)

// Custom configuration
let config = KeyParseConfig {
    max_utf8_segment_length: 20,
    max_binary_segment_length: 8,
    show_full_short_binary: true,
    min_utf8_segment_length: 3,
};
let custom = parse_cache_key_with_config(key, &config);
```

## Implementation Details

### Memory Tracking
- Uses `current_size()` method for accurate memory reporting
- Tracks memory usage across all cache types (main, API, height-partitioned)
- Updates statistics on every cache operation

### Prefix Analysis
- Analyzes prefixes of configurable lengths during cache operations
- Uses HashMap for efficient prefix tracking
- Stores hit/miss counts and unique key sets per prefix
- Only includes prefixes with sufficient key diversity

### Thread Safety
- All debugging state is protected by RwLock
- Safe for concurrent access from multiple threads
- Minimal performance impact when debugging is disabled

### Performance Considerations
- Debugging adds minimal overhead when disabled
- When enabled, adds small overhead for prefix tracking
- Uses efficient data structures (HashMap, HashSet)
- Configurable to balance detail vs. performance

## Integration

The debugging functionality is fully integrated into the Metashrew ecosystem:

1. **metashrew-support**: Core implementation of debugging logic
2. **metashrew-core**: Public API exports for WASM programs
3. **Comprehensive Testing**: Full test coverage with e2e validation
4. **Documentation**: Complete API documentation and examples

## Future Enhancements

The debugging system is designed to be extensible for future enhancements such as:

1. **alkanes-rs Integration**: Adding "lru-debug" feature to block processing reports
2. **Time-based Analysis**: Tracking cache patterns over time
3. **Export Formats**: JSON/CSV export for external analysis tools
4. **Real-time Monitoring**: Live dashboard for cache performance
5. **Automatic Optimization**: Suggestions based on usage patterns

## Conclusion

The LRU cache debugging implementation provides comprehensive insights into cache usage patterns, enabling developers to:

- Identify frequently accessed key prefixes
- Optimize cache allocation strategies
- Debug performance issues
- Understand data access patterns
- Monitor cache effectiveness

The system is production-ready with full test coverage and minimal performance impact when disabled.
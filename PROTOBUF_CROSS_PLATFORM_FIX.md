# Cross-Platform Protobuf Compilation Fix

## Problem

The Metashrew project was using `protoc-bin-vendored` for protobuf compilation, which failed on Android aarch64 with the error:

```
thread 'main' panicked at crates/metashrew-support/build.rs:4:62:
called `Result::unwrap()` on an `Err` value: Error { os: "android", arch: "aarch64" }
```

This occurred because `protoc-bin-vendored` doesn't support all target platforms, particularly Android aarch64.

## Solution

Replaced the vendored protoc approach with the pure Rust protobuf parser (`protobuf-parse`) which works on any platform that Rust supports.

## Changes Made

### 1. Updated Build Scripts

Modified the build.rs files in three crates to use `.pure()` instead of `.protoc()`:

- `crates/metashrew-support/build.rs`
- `crates/metashrew-core/build.rs` 
- `crates/metashrew-runtime/build.rs`

**Before:**
```rust
protobuf_codegen::Codegen::new()
    .protoc()
    .protoc_path(&protoc_bin_vendored::protoc_bin_path().unwrap())
    .out_dir("src/proto")
    .inputs(["proto/metashrew.proto"])
    .include("proto")
    .run()
    .expect("running protoc failed");
```

**After:**
```rust
protobuf_codegen::Codegen::new()
    // Use pure Rust parser instead of protoc for cross-platform compatibility
    .pure()
    .out_dir("src/proto")
    .inputs(["proto/metashrew.proto"])
    .include("proto")
    .run()
    .expect("running protobuf codegen failed");
```

### 2. Updated Dependencies

Modified the Cargo.toml files to remove `protoc-bin-vendored` and `protoc-rust` dependencies and add `protobuf-parse`:

**Removed dependencies:**
- `protoc-bin-vendored = "3.0.0"`
- `protoc-rust = { version = "2.28.0" }`

**Added dependency:**
- `protobuf-parse = "3.4.0"`

## Benefits

1. **Cross-Platform Compatibility**: Works on all platforms that Rust supports, including Android aarch64
2. **No External Dependencies**: Doesn't require protoc to be installed on the build system
3. **Pure Rust**: Uses a pure Rust implementation for parsing .proto files
4. **Simplified Build**: Removes the complexity of managing protoc binaries for different platforms

## Verification

The fix has been tested and verified to work correctly:

```bash
# Test individual crates
cargo check -p metashrew-support
cargo check -p metashrew-core  
cargo check -p metashrew-runtime

# Test release build
cargo build --release -p metashrew-support -p metashrew-core -p metashrew-runtime
```

All builds complete successfully without the Android aarch64 error.

## Compatibility

This change maintains full compatibility with existing protobuf definitions and generated code. The pure Rust parser produces the same output as the protoc-based approach, ensuring no breaking changes to the API.

## Future Considerations

- The pure Rust parser is well-tested but may not support some rarely used protobuf features
- For most use cases (including Metashrew), the pure Rust parser is sufficient and recommended
- If advanced protobuf features are needed in the future, the build scripts can be easily modified to use protoc conditionally based on the target platform
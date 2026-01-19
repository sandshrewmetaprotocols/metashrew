FROM rust:1.86

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    clang \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /metashrew

# Copy only the necessary files for the memory test
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/metashrew-core ./crates/metashrew-core
COPY crates/metashrew-support ./crates/metashrew-support
COPY crates/metashrew-runtime ./crates/metashrew-runtime
COPY crates/memshrew-runtime ./crates/memshrew-runtime
COPY crates/metashrew-memory-stress ./crates/metashrew-memory-stress
COPY src ./src

# Add wasm32 target for the project's toolchain version
RUN rustup target add wasm32-unknown-unknown --toolchain 1.86.0

# Build the WASM memory stress module
RUN cargo build --target wasm32-unknown-unknown --release -p metashrew-memory-stress

# Build the test binary (without rockshrew dependencies)
RUN cargo build --release --lib --no-default-features

# Default command: run the memory stress test
CMD ["cargo", "test", "--lib", "memory_stress_normal_environment", "--", "--ignored", "--nocapture"]

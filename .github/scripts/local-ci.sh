#!/bin/bash

# Local CI script to run the same checks as GitHub Actions
# This allows developers to test locally before pushing

set -e

echo "🚀 Running local CI checks for Metashrew..."
echo

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print status
print_status() {
    if [ $1 -eq 0 ]; then
        echo -e "${GREEN}✅ $2${NC}"
    else
        echo -e "${RED}❌ $2${NC}"
        exit 1
    fi
}

print_step() {
    echo -e "${YELLOW}🔄 $1${NC}"
}

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Cargo is not installed. Please install Rust and Cargo.${NC}"
    exit 1
fi

# Check formatting
print_step "Checking code formatting..."
cargo fmt --all -- --check
print_status $? "Code formatting check"

# Run clippy (allow warnings for now)
print_step "Running clippy lints..."
cargo clippy --all-targets --all-features
print_status $? "Clippy lints"

# Build project
print_step "Building project..."
cargo build --verbose --all-features
print_status $? "Project build"

# Run tests
print_step "Running tests..."
cargo test --verbose --all-features
print_status $? "Test suite"

# Run doc tests
print_step "Running doc tests..."
cargo test --doc --verbose --all-features
print_status $? "Doc tests"

# Check for security vulnerabilities (if cargo-audit is installed)
if command -v cargo-audit &> /dev/null; then
    print_step "Running security audit..."
    cargo audit
    print_status $? "Security audit"
else
    echo -e "${YELLOW}⚠️  cargo-audit not installed. Run 'cargo install cargo-audit' to enable security checks.${NC}"
fi

# Build release (optional)
if [ "$1" = "--release" ]; then
    print_step "Building release..."
    cargo build --release --all-features
    print_status $? "Release build"
fi

echo
echo -e "${GREEN}🎉 All local CI checks passed!${NC}"
echo -e "${GREEN}Your code is ready to be pushed.${NC}"
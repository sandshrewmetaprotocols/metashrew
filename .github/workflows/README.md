# GitHub Workflows

This directory contains GitHub Actions workflows for the Metashrew project.

## Workflows

### 1. CI Workflow (`ci.yml`)
**Triggers:** Push to main/master/develop branches, Pull requests to main/master/develop

A comprehensive CI pipeline that includes:

- **Multi-Rust Version Testing**: Tests against stable, beta, and nightly Rust versions
- **Code Quality Checks**:
  - Formatting check with `cargo fmt`
  - Linting with `cargo clippy`
  - Security audit with `cargo audit`
- **Testing**: Full test suite including doc tests
- **Code Coverage**: Generates coverage reports and uploads to Codecov
- **Release Build**: Builds release artifacts on main/master branch pushes
- **Docker Build Test**: Validates Docker builds for both indexer and view components

### 2. Test Workflow (`test.yml`)
**Triggers:** Push to any branch, Pull requests to main/master/develop

A lightweight, fast-feedback workflow focused on:

- **Quick Testing**: Runs the full test suite with minimal overhead
- **Warning Detection**: Ensures no compilation warnings are present
- **Fast Feedback**: Optimized for quick developer feedback during development

## System Dependencies

Both workflows install the following system dependencies required for building Metashrew:

- `build-essential`: Essential build tools
- `pkg-config`: Package configuration tool
- `libssl-dev`: SSL development libraries
- `libclang-dev`: Clang development libraries
- `clang`: Clang compiler

## Caching

Both workflows use GitHub Actions caching to speed up builds by caching:

- Cargo registry
- Cargo git dependencies
- Build artifacts in the `target` directory

## Artifacts

The CI workflow uploads release artifacts when building from the main/master branch:

- `rockshrew-mono`: Combined indexer and view binary
- `rockshrew`: Indexer binary
- `rockshrew-view`: View layer binary

Artifacts are retained for 30 days and can be downloaded from the Actions tab.

## Usage

These workflows will automatically run when:

1. **Any push to any branch** triggers the test workflow for quick feedback
2. **Push to main/master/develop** triggers the full CI pipeline
3. **Pull requests to main/master/develop** trigger both workflows

## Status Badges

You can add status badges to your README.md:

```markdown
[![CI](https://github.com/your-org/metashrew/workflows/CI/badge.svg)](https://github.com/your-org/metashrew/actions/workflows/ci.yml)
[![Tests](https://github.com/your-org/metashrew/workflows/Tests/badge.svg)](https://github.com/your-org/metashrew/actions/workflows/test.yml)
```

## Local Testing

To run the same checks locally before pushing:

```bash
# Format check
cargo fmt --all -- --check

# Linting
cargo clippy --all-targets --all-features -- -D warnings

# Tests
cargo test --verbose --all-features

# Security audit
cargo audit

# Build
cargo build --release --all-features
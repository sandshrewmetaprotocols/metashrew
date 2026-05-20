# Contributing to Metashrew

## Development Setup

1. Install prerequisites:
   - Rust toolchain (stable)
   - `wasm32-unknown-unknown` target: `rustup target add wasm32-unknown-unknown`
   - Bitcoin Core (for testing)

2. Clone and build:
   ```sh
   git clone https://github.com/kungfuflex/metashrew
   cd metashrew
   cargo build --release -p rockshrew-mono
   ```

## Project Structure

All crates live under `crates/`:

- `crates/metashrew-core/`: WASM-side host imports + helpers used by indexer modules
- `crates/metashrew-support/`: Shared types and traits used by both runtime and core
- `crates/metashrew-runtime/`: Native WASM runtime (wasmtime/wasmi), KV abstractions, view engine
- `crates/metashrew-sync/`: Block sync framework + traits (`StorageAdapter`, `NodeAdapter`)
- `crates/rockshrew-runtime/`: RocksDB-backed `StorageAdapter` implementation
- `crates/rockshrew-mono/`: Combined indexer + JSON-RPC view server binary
- `crates/memshrew-runtime/`: In-memory `StorageAdapter` (tests, mempool-only previews)

## Development Workflow

1. Create a feature branch:
   ```sh
   git checkout -b feature-name
   ```

2. Make your changes, ensuring:
   - Tests are added for new functionality
   - Documentation is updated if needed
   - Code follows existing style (run `cargo fmt`)
   - No new clippy warnings: `cargo clippy`

3. Test your changes:
   ```sh
   cargo test --all
   ```

4. Commit with clear messages:
   ```
   component: brief change description

   Longer explanation if needed
   ```

## Pull Request Process

1. Ensure all tests pass and code is formatted
2. Update relevant documentation
3. Create PR with clear description of changes
4. Address review feedback
5. Maintain clean commit history (rebase if needed)

## Code Style

- Follow rustfmt defaults 
- Use meaningful variable names
- Add comments for complex logic
- Keep functions focused and reasonably sized
- Prefer standard library solutions where possible

## Testing

- Unit tests go alongside the code in the same file
- Integration tests go in `tests/` directories
- Test both success and error cases
- Mock external services where appropriate

## Questions?

Feel free to:
- Open an issue for discussion
- Ask in existing relevant issues
- Contact maintainers directly

## License

By contributing, you agree that your contributions will be licensed under the project's license.

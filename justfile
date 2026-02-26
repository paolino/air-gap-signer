default:
    @just --list

# Build all workspace crates
build:
    cargo build --workspace --exclude echo-hex

# Build WASM interpreters
build-wasm:
    cargo build -p echo-hex --target wasm32-unknown-unknown --release

# Run all tests
test: build-wasm
    cargo test --workspace --exclude echo-hex

# Format code
format:
    cargo fmt --all

# Check formatting
format-check:
    cargo fmt --all -- --check

# Lint
lint:
    cargo clippy --workspace --exclude echo-hex -- -D warnings

# Full CI pipeline
ci: format-check lint build build-wasm test

# Clean build artifacts
clean:
    cargo clean

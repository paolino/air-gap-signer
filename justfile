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

# Build documentation
build-docs:
    mkdocs build

# Serve documentation locally
serve-docs:
    mkdocs serve

# Deploy documentation to GitHub Pages
deploy-docs:
    mkdocs gh-deploy --force

# Generate SE050 breakout board Gerber files
gerbers:
    python3 generate_gerbers.py

# Clean build artifacts
clean:
    cargo clean

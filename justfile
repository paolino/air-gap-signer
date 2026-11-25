default:
    @just --list

# Build all workspace crates
build:
    cargo build --workspace --exclude echo-hex

# Build WASM interpreters
build-wasm:
    cargo build -p echo-hex --target wasm32-unknown-unknown --release

# Cross-compile the Raspberry Pi demo (static aarch64 binary)
build-pi:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "$(uname -s)" != Linux ]; then echo "build-pi: skipped, needs a Linux host"; exit 0; fi
    cargo build -p signer-pi --release --target aarch64-unknown-linux-musl
    bin=target/aarch64-unknown-linux-musl/release/signer-pi-demo
    file "$bin"
    file "$bin" | grep -q "ARM aarch64.*statically linked"

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
ci: format-check lint build build-wasm build-pi test gerbers schematic

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
    cd hardware/SE050_breakout && python3 generate_gerbers.py

# Generate SE050 breakout board schematic SVG
schematic:
    cd hardware/SE050_breakout && python3 generate_schematic.py

# Run desktop simulator
sim usb_dir="./test-usb" keystore="./keys.json":
    cargo run -p signer-sim -- --usb-dir {{usb_dir}} --keystore {{keystore}}

# Clean build artifacts
clean:
    cargo clean

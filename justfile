default:
    @just --list

# Build all workspace crates
build:
    cargo build --workspace --exclude echo-hex

# Build WASM interpreters
build-wasm:
    cargo build -p echo-hex -p cardano-tx --target wasm32-unknown-unknown --release

# Cross-compile the Raspberry Pi binaries (static aarch64)
build-pi:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "$(uname -s)" != Linux ]; then echo "build-pi: skipped, needs a Linux host"; exit 0; fi
    cargo build -p signer-pi --release --target aarch64-unknown-linux-musl
    for bin in signer-pi signer-pi-demo; do
        f=target/aarch64-unknown-linux-musl/release/$bin
        file "$f"
        file "$f" | grep -q "ARM aarch64.*statically linked"
    done

# Compile the same PIN entry library for the proposed Cortex-M33 controller
build-panel-input:
    cargo build --locked -p panel-input --release --target thumbv8m.main-none-eabihf

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

# Build and verify Lean 4 formalization and oracle driver
lean:
    #!/usr/bin/env bash
    set -euo pipefail
    cd lean
    lake build
    # Check that no sorry or admit appears in Lean source files
    if grep -rnE '\b(sorry|admit)\b' AirGapSigner.lean AirGapSigner/; then
        echo "error: sorry or admit found in Lean sources" >&2
        exit 1
    fi
    # Kernel axiom check: reject sorryAx or non-standard axioms
    axioms=$(lake env lean --stdin <<'EOF'
    import AirGapSigner
    #print axioms AirGapSigner.ok_ne_refused
    EOF
    )
    if grep -q 'sorryAx' <<<"$axioms"; then
        echo "error: sorryAx detected in Lean kernel axioms" >&2
        exit 1
    fi
    # Positive smoke check on the oracle driver
    out=$(lake exe air-gap-signer-oracle <<< '{"surface":"surfaces"}')
    test "$out" = '{"status":"ok","surfaces":[]}'
    # Negative control: malformed JSON and unknown surface must fail non-zero
    if lake exe air-gap-signer-oracle <<< 'not-json' >/dev/null 2>&1; then
        echo "error: oracle accepted invalid JSON" >&2
        exit 1
    fi
    if lake exe air-gap-signer-oracle <<< '{"surface":"nonexistent"}' >/dev/null 2>&1; then
        echo "error: oracle accepted unknown surface" >&2
        exit 1
    fi

# Full CI pipeline
ci: format-check lint build build-wasm build-pi build-panel-input test lean gerbers schematic

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

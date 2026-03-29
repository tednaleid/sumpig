# Install required toolchain components
setup:
    rustup component add clippy rustfmt

# Run all checks (test + lint)
check: test lint

# Run all tests
test:
    cargo test

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Build release binary
build:
    cargo build --release

# Format code
fmt:
    cargo fmt

# Check formatting without modifying
fmt-check:
    cargo fmt -- --check

# Run benchmarks (available after Phase 3)
bench:
    cargo bench

# Run a specific test by name
test-one NAME:
    cargo test {{NAME}}

# Install release binary to ~/.cargo/bin (or CARGO_INSTALL_ROOT)
install:
    cargo install --path .

# Build and run with arbitrary arguments
run *ARGS:
    cargo run -- {{ARGS}}

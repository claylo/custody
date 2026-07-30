# Default: fast feedback loop.
default: check

# Compile, lint, and format-check without running tests.
check:
    cargo fmt --check
    cargo clippy --all-targets --locked -- -D warnings
    cargo build --locked

# Full test suite.
test:
    cargo test --locked

# Everything CI would run.
ci: check test

# Apply formatting.
fmt:
    cargo fmt

# Probe configuration and external PDF tools.
doctor:
    cargo run --locked -- doctor

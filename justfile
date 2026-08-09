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

# Check for outdated dependencies (root only, no transitive noise)
outdated:
    cargo outdated --root-deps-only

# Safe update: respects semver constraints, only touches Cargo.lock
#
# NOTE: no `--workspace` here. In `cargo update`, `--workspace` is shorthand
# for `-p <each workspace member>` — it re-resolves only the workspace's own
# packages, which is a no-op for a single-crate workspace. Bare `cargo update`
# is what actually walks the dependency tree.
update:
    cargo update --verbose

# Upgrade Cargo.toml to latest compatible versions
upgrade:
    cargo upgrade
    cargo update

# The nuclear option: upgrade to latest incompatible versions (breaking changes)
upgrade-breaking:
    cargo upgrade --incompatible
    cargo update

# See what WOULD update without doing it
check-updates:
    cargo update --dry-run


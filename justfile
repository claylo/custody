set shell := ["bash", "-c"]
toolchain := `taplo get -f rust-toolchain.toml toolchain.channel | tr -d '"'`
msrv := "1.89.0"

default:
  @just --list

clippy:
  cargo +{{toolchain}} clippy --all-targets --all-features --message-format=short -- -D warnings

msrv-check:
  cargo +{{msrv}} check --all-targets --all-features

fix:
  echo "Using toolchain {{toolchain}}"
  cargo +{{toolchain}} clippy --fix --allow-dirty --allow-staged -- -W clippy::all

# Check dependencies for security advisories and license compliance
deny:
  cargo deny --all-features --config .config/deny.toml check

test:
  cargo nextest run

# Apply formatting.
fmt:
    cargo fmt --all -- --config-path .config/rustfmt.toml

# Verify formatting without modifying the worktree.
fmt-check:
    cargo fmt --all --check -- --config-path .config/rustfmt.toml

# Probe configuration and external PDF tools.
doctor:
    cargo run --locked -- doctor

test-ci:
  cargo nextest run --profile ci

doc-test:
  cargo test --doc --all-features
  
doc:
  cargo doc --all-features --no-deps

check: fmt-check msrv-check clippy deny test doc-test doc

ci: fmt-check msrv-check clippy deny test-ci doc-test doc

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

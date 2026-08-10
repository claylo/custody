# Handoff: Phase 4, Schema, and README

**Date:** 2026-08-10
**Branch:** main
**State:** Green

> Green = tests pass, safe to continue. Yellow = tests pass but known issues exist. Red = broken state, read Landmines first.

## Where things stand

`receipts` is at 51 commits on `main`, 164 tests. The justfile was updated
mid-session to the fleet baseline (nextest, toolchain pinning via
`rust-toolchain.toml`, `cargo deny`, `.config/` for rustfmt/nextest/deny
configs). `just test` and `just check` are the canonical verification commands.

Since the Phase 3 handoff (2026-08-09), ten commits landed covering three
areas: Phase 4 implementation, librebar schema integration, and a full README
rewrite.

### Phase 4: Review tier (6 commits)

Records semantic verdicts (`supported`, `partial`, `unsupported`, `unclear`)
bound to both claim text and evidence set via dual SHA-256. Six new error
codes: `stale_review_claim`, `stale_review_evidence`, `unknown_review_claim`,
`duplicate_review_claim`, `missing_review`, `unsupported_verdict`.
`--require-review` on `check` and `audit` gates on missing reviews and
non-supported verdicts. Full details in
`.handoffs/2026-08-10-phase-4-complete.md`.

### Schema and completions (1 commit)

Switched from `Cli::parse()` to `librebar::cli::parse_with()` with full
`SchemaMetadata`. `receipts schema` now emits CLI Spec v0.2 JSON declaring:

- Every subcommand with `mutating: false`, `stability: stable`
- Typed output fields per command (e.g., `check` → `valid`, `skipped`,
  `invalid`, `summaries`)
- All 18 error codes with exit codes, retryability, and descriptions
- Self-contained examples for `locate`, `check`, `audit`, `propose`
- Shell completions via `receipts completions SHELL`

Also enabled librebar `crash` and `diagnostics` features. Crash handler
installed in `main.rs` — panics now produce structured dumps in
`~/Library/Caches/receipts/crashes/`.

### README rewrite (1 commit)

Full rewrite covering all Phases 0-4: named sources, section paths, token
coverage, weak sections, propose, review tier, `--require-review`, schema,
completions. Fixed the stale `cargo install receipts` instruction (not
published). Added documentation for every config block and subcommand.

### Justfile update (1 commit, by Clay)

Justfile updated to fleet baseline: `cargo nextest run` instead of
`cargo test`, toolchain pinning via `rust-toolchain.toml` (1.97.1), `cargo
deny` for advisory/license checks, `.config/rustfmt.toml` for formatting,
`.config/nextest.toml` for test profiles.

## New files (this session)

| File | Purpose |
|------|---------|
| `src/review.rs` | Review data types, verdict enum, canonical evidence hashing, structural validation |
| `tests/review.rs` | 18 tests covering evidence hashing and review validation |
| `rust-toolchain.toml` | Pins toolchain to 1.97.1 |
| `.config/deny.toml` | cargo-deny configuration |
| `.config/nextest.toml` | nextest profiles |
| `.config/rustfmt.toml` | rustfmt configuration |
| `.config/scrat.toml` | scrat release configuration |
| `.config/bito.yaml` | bito configuration |

## Modified files (this session)

| File | Changes |
|------|---------|
| `src/lib.rs` | Register `review` module |
| `src/evidence.rs` | Add `review: Option<Review>` to `SummaryDocument` |
| `src/terms.rs` | Extend `canonicalize`/`localize` for review vocabulary |
| `src/validate.rs` | Call `validate_review` during validation, add `require_review` parameter |
| `src/cli.rs` | `--require-review` flag, `schema_metadata()`, switch to `librebar::cli::parse_with()` |
| `src/main.rs` | Install crash handler |
| `Cargo.toml` | Add `crash` and `diagnostics` librebar features |
| `README.md` | Full rewrite for Phases 0-4 |
| `justfile` | Fleet baseline (nextest, toolchain, deny) |

## Decisions made

All decisions from previous handoffs remain in force. New decisions:

- **`SchemaMetadata` declares all error codes centrally.** The
  `schema_metadata()` function in `cli.rs` is the single source of truth for
  what `receipts schema` emits. When new error codes are added, they must be
  added there too.

- **All commands are `mutating: false`, `stability: stable`.** Every receipts
  subcommand is read-only (`locate` emits output but doesn't write to the
  evidence contract). Stability is declared stable because the CLI contract is
  the agent interface.

- **Crash handler is unconditional.** Installed before any other work in
  `main()`. Panic dumps go to `~/Library/Caches/receipts/crashes/` with
  owner-only permissions, 10-dump retention.

- **`just test` uses `cargo nextest run`.** The justfile was updated to the
  fleet baseline. All verification should use `just` commands, not raw cargo.

## What's next

1. **Code audit** — Clay is running one before release.

2. **Phase 5: Arbitrary OCR backends (deferrable).** `PdfBackend` becomes a
   validated name rather than a closed enum.

3. **Release** — no git remote yet; publishing is a deliberate pending decision.

Standard checks:

```sh
just check    # fmt, clippy, deny, test, doc-test, doc
just test     # just the test suite
just doctor   # probe tools
```

## Landmines

- **All previous landmines remain in effect.** See the Phase 4 handoff.

- **Justfile changed significantly.** `just check` now runs fmt → clippy →
  deny → test → doc-test → doc (was fmt → clippy → build). `just test` uses
  nextest (was `cargo test`). Clippy uses the pinned toolchain from
  `rust-toolchain.toml`.

- **`schema_metadata()` must stay in sync with error codes.** Adding a new
  error code in `evidence.rs`, `validate.rs`, or `review.rs` without adding
  a corresponding `.error(ErrorMetadata::new(...))` in `schema_metadata()`
  means agents won't know about it.

- **`librebar::cli::parse_with()` injects `schema` and `completions`
  subcommands.** These names are reserved — adding a receipts subcommand
  named `schema` or `completions` will panic at startup.

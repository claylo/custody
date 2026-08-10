# Handoff: Phase 2 Complete

**Date:** 2026-08-09
**Branch:** main
**State:** Green

> Green = tests pass, safe to continue. Yellow = tests pass but known issues exist. Red = broken state, read Landmines first.

## Where things stand

`receipts` is at 34 commits on `main`, 123 tests, and `cargo fmt --check`,
`cargo clippy --all-targets --locked -- -D warnings`, `cargo build --locked`,
`cargo test --locked`, and `cargo run --locked -- doctor` all pass against
`mutool` 1.28.2 and `tesseract` 5.5.3.

Since the previous handoff (Phase 0+1, 2026-08-09), nine commits landed
implementing Phase 2: Material Tokens and Section Paths.

**Severity system.** `EvidenceIssue` now carries a `Severity` field
(`Error` | `Warning`). `ValidationReport::is_valid()` ignores warnings.
CLI output prefixes warnings with `warning:`. All pre-existing issues are
`Error`; new issues use both levels.

**Section paths.** `MarkdownUnit` tracks a `section: Vec<String>` — the
ancestor heading texts computed during `parse_units` via a heading stack.
`MarkdownLocator` gained an optional `section` field (`serde(default)`,
backward compatible). `validate_document` verifies recorded section paths
match the computed path; a mismatch is `stale_section`. `locate` now records
the computed section path in its output.

**Token coverage.** `src/tokens.rs` extracts required tokens (numeric
literals, number words, quoted phrases) and advisory tokens (capitalized
multiword terms) from claim text. `validate_document` checks that every
required token is covered by at least one locator's `exact` text, using
word-boundary matching that prevents `3` matching inside `13`. Number words
are matched case-insensitively. Severity is configurable: `coverage.tokens`
= `error` (default) | `warn` | `off`.

**Weak section warning.** `sections.weak` lists heading substrings
(case-insensitive). When every locator for a claim sits under a weak
section, `weak_section_only` fires as a permanent warning.

**Config YAML fix.** `TokenSeverity` has a hand-written `Deserialize` impl
that accepts YAML's boolean `false` as `Off`, because YAML 1.1 resolves
the bare scalar `off` to `false` before serde sees it.

Design specs live in `record/superpowers/specs/`, implementation plans in
`record/superpowers/plans/`. `docs/` is empty and reserved for end-users.

There is no git remote. Publishing is a deliberate pending decision.

## Decisions made

All decisions from previous handoffs remain in force. New decisions:

- **`Severity` on `EvidenceIssue`, not on error codes.** Severity is a
  property of each issue instance, not of the code string. `uncovered_token`
  can be `Error` or `Warning` depending on config; `weak_section_only` is
  always `Warning`. Encoding severity in the code table would force a lookup
  step for every consumer.

- **`is_valid()` ignores warnings.** Warnings appear in output and in the
  JSON payload but do not affect exit status. This means `check` and `audit`
  can succeed while still reporting weak-section and warn-mode token issues.

- **Section paths are equality-checked, not fuzzy.** A recorded path that
  differs from the computed path is `stale_section`, period. The spec
  considered substring matching for weak sections (where converter noise is
  expected), but the recorded path is what `receipts` itself wrote — it
  should be exactly reproducible.

- **Token boundary matching uses ASCII-alphanumeric, not Unicode `\b`.**
  The spec says "non-ASCII-alphanumeric character or a string edge."
  Simpler, deterministic, no Unicode word-break dependency.

- **Apostrophe handling in quoted-phrase extraction.** `opens_quote` and
  `closes_quote` check whether a single-quote delimiter is adjacent to an
  alphanumeric character. This prevents `didn't` from opening a quoted
  phrase while still allowing `'gradient descent'` to be extracted.

- **Hand-written `Deserialize` for `TokenSeverity`.** YAML 1.1 resolves
  `off` to `false`. The derived deserializer would reject `false` as an
  invalid string variant. The hand-written impl accepts both `"off"` and
  `false` → `Off`.

## New files

| File | Purpose |
|------|---------|
| `src/tokens.rs` | Token extraction from claim text, coverage checking |
| `src/sections.rs` | Section path comparison, weak-section detection |
| `tests/tokens.rs` | 24 token extraction and boundary-matching tests |
| `tests/sections.rs` | 12 section path and weak-section tests |

## New error codes

| Code | Severity | Meaning |
|------|----------|---------|
| `uncovered_token` | per `coverage.tokens` | a required claim token appears in no locator |
| `stale_section` | error | recorded section path disagrees with the source |
| `weak_section_only` | warning | every locator for a claim is under a weak section |

## New config blocks

```yaml
coverage:
  tokens: error    # error | warn | off (default: error)

sections:
  weak: ["Limitations", "Future Work", "Related Work"]
```

## What's next

The spec defines three remaining phases:

1. **Phase 3: `propose`.** Deterministic candidate locator ranking. Depends
   on Phases 1 and 2 (shares the material-token extractor). No model, no
   randomness.

2. **Phase 4: Review tier.** Records a semantic verdict and binds it to
   both the claim text and the evidence set via dual SHA-256.

3. **Phase 5: Arbitrary OCR backends (deferrable).** `PdfBackend` becomes
   a validated name rather than a closed enum.

Standard checks before each commit:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo run --locked -- --format text doctor
```

## Landmines

- **All previous landmines remain in effect.** See the Phase 0+1 handoff.

- **`coverage.tokens: off` in YAML must be quoted or the hand-written
  deserializer handles it.** Without the custom `Deserialize`, bare `off`
  becomes boolean `false` and fails. The custom impl handles this, but
  downstream YAML generators that emit `false` instead of `"off"` will
  work correctly only because of this shim.

- **Section paths are empty for documents without headings.** A locator
  with `section: []` in a headed document produces no `stale_section` —
  the check only fires when the recorded section is non-empty. This is
  intentional: existing evidence records predate section tracking and
  must remain valid.

- **Token extraction runs per-claim, not per-locator.** A required token
  is covered if ANY locator for that claim contains it. Removing a locator
  that was the only source of a token will surface `uncovered_token` on the
  next `check`.

- **Advisory tokens are never enforced.** They appear in `--json` output
  and will appear in `propose` output (Phase 3), but they never produce
  issues and never affect exit status. `coverage.tokens: off` disables
  required-token enforcement but leaves advisory extraction intact.

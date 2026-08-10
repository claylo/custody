# Handoff: Phase 4 Complete

**Date:** 2026-08-10
**Branch:** main
**State:** Green

> Green = tests pass, safe to continue. Yellow = tests pass but known issues exist. Red = broken state, read Landmines first.

## Where things stand

`receipts` is at 44 commits on `main`, 164 tests, and `cargo fmt --check`,
`cargo clippy --all-targets --locked -- -D warnings`, `cargo build --locked`,
`cargo test --locked`, and `cargo run --locked -- doctor` all pass against
`mutool` 1.28.2 and `tesseract` 5.5.3.

Since the previous handoff (Phase 3, 2026-08-09), six commits landed
implementing Phase 4: the review tier.

**Review tier.** A summary may now include a `review` section recording
semantic verdicts for each claim:

```yaml
review:
  claims:
    - claim: 0
      claim_sha256: "..."
      evidence_sha256: "..."
      verdict: supported
      reviewer: "claude-opus-5"
      note: "Locator [a] covers the mechanism."
      at: "2026-08-10"
```

**Dual SHA-256 binding.** Each review entry is bound to both the claim
text (`claim_sha256`) and the evidence set (`evidence_sha256`). The
evidence hash is the SHA-256 of the canonical JSON serialization of
that claim's `locators` array, with object keys sorted lexicographically
at every level and `exact` values normalized. Array order is significant:
reordering locators invalidates the review.

**Four verdicts.** `supported`, `partial`, `unsupported`, `unclear`. The
tool records verdicts — it never produces them and never treats them as
truth.

**Staleness detection.** Edit the claim text and the review becomes
`stale_review_claim`. Change a locator and it becomes
`stale_review_evidence`. Both are errors that fire during `check` and
`audit` without any flags.

**`--require-review` flag.** `check --require-review` and
`audit --require-review` gate on missing reviews (`missing_review`)
and non-`supported` verdicts (`unsupported_verdict`). Without the flag,
verdicts are recorded but not enforced.

**Vocabulary support.** The `review.claims[]` entries participate in
the configurable claim vocabulary system. A corpus using `proposition`
instead of `claim` gets the same canonicalization/localization treatment
in the review section.

Design specs live in `record/superpowers/specs/`, implementation plans in
`record/superpowers/plans/`. `docs/` is empty and reserved for end-users.

There is no git remote. Publishing is a deliberate pending decision.

## New files

| File | Purpose |
|------|---------|
| `src/review.rs` | Review data types, verdict enum, canonical evidence hashing, structural validation |
| `tests/review.rs` | 18 tests covering evidence hashing and review validation |

## Modified files

| File | Changes |
|------|---------|
| `src/lib.rs` | Register `review` module |
| `src/evidence.rs` | Add `review: Option<Review>` to `SummaryDocument` |
| `src/terms.rs` | Extend `canonicalize`/`localize` for review vocabulary |
| `src/validate.rs` | Call `validate_review` during validation, add `require_review` parameter |
| `src/cli.rs` | Add `--require-review` flag to `check` and `audit` |
| `tests/validate.rs` | 3 new review integration tests, updated all callers for new parameter |
| `tests/cli.rs` | 4 new CLI integration tests for `--require-review` |

## Decisions made

All decisions from previous handoffs remain in force. New decisions:

- **`evidence_sha256` uses canonical JSON with sorted keys.** `serde_json`
  does not guarantee field order across compiler versions. Sorting keys at
  every nesting level makes the hash stable regardless of serialization
  order. `exact` values are normalized before hashing so whitespace
  differences don't produce different hashes.

- **`validate_document` gained a `require_review` parameter.** This is a
  CLI flag, not a config setting, because review gating is per-invocation.
  A corpus might want `check` to pass without reviews during development
  but require them in CI.

- **Review validation runs after evidence validation.** The review block
  sits after the evidence processing loop because `evidence_sha256` needs
  resolved locators. When `evidence` is `None`, `validate_document` returns
  early with `missing_evidence` — review checks don't run in that path,
  which is correct since there's nothing to verify the review against.

- **`unsupported_verdict` skips out-of-range claims.** If a review entry
  references a nonexistent claim, only `unknown_review_claim` fires. No
  verdict check for a claim that doesn't exist.

- **`#[allow(clippy::fn_params_excessive_bools)]` on `audit`.** Adding
  `require_review` pushed `audit` to four bool parameters. Scoped allow
  is better than restructuring the function signature across all callers
  for a private function called from one place.

## New error codes

| Code | Severity | Meaning |
|------|----------|---------|
| `stale_review_claim` | error | claim text changed since review |
| `stale_review_evidence` | error | locator set changed since review |
| `unknown_review_claim` | error | review entry indexes a nonexistent claim |
| `duplicate_review_claim` | error | two review entries for one claim |
| `missing_review` | error under `--require-review` | claim has no review entry |
| `unsupported_verdict` | error under `--require-review` | verdict is not `supported` |

## What's next

The spec defines one remaining phase:

1. **Phase 5: Arbitrary OCR backends (deferrable).** `PdfBackend` becomes
   a validated name rather than a closed enum.

Standard checks before each commit:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo run --locked -- --format text doctor
```

## Landmines

- **All previous landmines remain in effect.** See the Phase 3 handoff.

- **`validate_document` now takes a fourth parameter.** Any new caller
  must pass `require_review: bool`. All existing callers were updated.

- **`SummaryDocument` gained a `review` field.** Test fixtures that
  construct `SummaryDocument` via struct literal need `review: None`.
  All existing fixtures were updated.

- **The `issue()` helper in `review.rs` takes 4 args (no locator).**
  The `issue()` helpers in `validate.rs` and `evidence.rs` take 5 args.
  They are separate private functions with the same name — not a shared
  helper. If you add review-related issues that need a locator index,
  construct `EvidenceIssue` directly.

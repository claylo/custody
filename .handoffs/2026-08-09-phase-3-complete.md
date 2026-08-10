# Handoff: Phase 3 Complete

**Date:** 2026-08-09
**Branch:** main
**State:** Green

> Green = tests pass, safe to continue. Yellow = tests pass but known issues exist. Red = broken state, read Landmines first.

## Where things stand

`receipts` is at 38 commits on `main`, 139 tests, and `cargo fmt --check`,
`cargo clippy --all-targets --locked -- -D warnings`, `cargo build --locked`,
`cargo test --locked`, and `cargo run --locked -- doctor` all pass against
`mutool` 1.28.2 and `tesseract` 5.5.3.

Since the previous handoff (Phase 2, 2026-08-09), four commits landed
implementing Phase 3: `propose`.

**`propose` subcommand.** `receipts propose [ID...] [--all] [--candidates N]`
emits ranked candidate locators for claims that lack evidence, or for every
claim with `--all`. The algorithm is deterministic end to end: no model,
no randomness, stable ordering. Identical input yields byte-identical output.

**Sentence splitting.** Candidate spans are built from each semantic
Markdown unit: every sentence (split on terminal punctuation followed by
whitespace) is a candidate, plus the whole unit text as a fallback when the
unit contains multiple sentences. The split is deliberately naive — `et al.`
and `Fig. 3` will mis-split — acceptable because every candidate is verified
against both sources before being offered.

**Token scoring.** Candidates are scored by the count of distinct required
tokens they cover (using the Phase 2 `tokens::extract` extractor). A
candidate covering zero required tokens is discarded.

**Uniqueness filtering.** A candidate that occurs more than once within its
own unit is discarded — ambiguity is an error, same as in `locate` and
`validate`.

**PDF verification.** One native extraction pass per PDF per source. A
candidate is offered only when it appears on exactly one page. Zero matches
and multiple matches are both discarded. `propose` does not run OCR.

**Ranking.** Token count descending, then span length ascending (shorter is
tighter evidence), then source name, line, column. Fully ordered for
reproducible output.

**Output.** `--format json` emits structured JSON. Text mode emits
commented YAML safe to paste and edit, with an `accept [a]:` suggestion
showing the `receipts locate` command to commit the top candidate.

**`propose` never writes.** Asserted by test: corpus files are hashed
before and after, and the hashes must match.

Design specs live in `record/superpowers/specs/`, implementation plans in
`record/superpowers/plans/`. `docs/` is empty and reserved for end-users.

There is no git remote. Publishing is a deliberate pending decision.

## New files

| File | Purpose |
|------|---------|
| `src/propose.rs` | Core algorithm: sentence splitting, candidate generation, scoring, PDF verification, ranking |
| `tests/propose.rs` | 15 tests covering sentence splitting and propose_document |

## Decisions made

All decisions from previous handoffs remain in force. New decisions:

- **Candidates without PDF matches are discarded, not offered.** The spec
  says "offer it only when it occurs on exactly one page." A candidate that
  can't be verified natively isn't useful — it would require OCR, and
  `propose` deliberately avoids OCR.

- **`UnitKind::as_str()` added.** Human output uses lowercase unit names
  (`paragraph`, `list_item`) matching the `serde(rename_all)` form, not
  `Debug` output (`Paragraph`, `ListItem`).

- **`covers()` helper factors out number-word case-insensitivity.** Both
  `validate.rs` and `propose.rs` need the same "is this token covered in
  this text" logic with case-insensitive handling for number words. The
  propose module has its own private `covers()` that encapsulates this.
  If a third consumer appears, consider lifting it to `tokens.rs`.

- **Uncovered tokens computed after truncation.** `uncovered_tokens` in the
  output reflects which required tokens aren't covered by the *offered*
  candidates (after ranking and truncation), not by all possible candidates.
  This is more useful: it tells the reviewer what the top candidates miss.

## What's next

The spec defines two remaining phases:

1. **Phase 4: Review tier.** Records a semantic verdict and binds it to
   both the claim text and the evidence set via dual SHA-256. Never
   produces a review, never treats verdict as truth.

2. **Phase 5: Arbitrary OCR backends (deferrable).** `PdfBackend` becomes
   a validated name rather than a closed enum.

Standard checks before each commit:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo run --locked -- --format text doctor
```

## Landmines

- **All previous landmines remain in effect.** See the Phase 2 handoff.

- **`propose` does not run OCR.** If a candidate's text exists in the
  Markdown but only appears on a PDF page through OCR (e.g., a rotated
  table), `propose` will not find it. The human output suggests
  `locate --page N` for manual OCR verification.

- **Sentence splitting is deliberately naive.** `et al.`, `Fig. 3`, and
  decimal-adjacent abbreviations will produce bad splits. Each bad split
  yields a candidate that fails the `exact_count == 1` or PDF verification
  check, so it costs a candidate slot but never produces a bad locator.

- **`TEXT_PDF` in `tests/cli.rs` is a hand-crafted minimal PDF.** It
  embeds a single line of text via a Type1/Helvetica font and raw PDF
  content stream. It works with `mutool` 1.28.2 but could break with a
  PDF parser that validates cross-references more strictly.

- **The `covers()` helper is duplicated between `validate.rs` and
  `propose.rs`.** Both inline the same pattern: `is_number_word` →
  `is_covered_case_insensitive`, else `is_covered`. If a third use appears,
  lift it to `tokens::covers()`.

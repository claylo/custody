# Handoff: Full-Repo Audit Remediation

**Date:** 2026-08-10
**Branch:** main
**Commit:** b738899
**State:** Green

> Green = tests pass, safe to continue. Yellow = tests pass but known issues exist. Red = broken state, read Landmines first.

## What happened

Validated and remediated all 20 findings from the
`record/audits/2026-08-10-16-full-repo/` CASED audit (codex-gpt-5 auditor).
Three commits across two passes, plus a ledger commit. 164/164 tests pass;
full `just check` gate (fmt, clippy, deny, test, doc-test, doc) green
throughout.

### Pass 1: 11 fixes (b5e6dca)

**Correctness (5 findings)**

- `unchecked-empty-markdown-candidates-panic` — replaced `unwrap_or(&vec[0])`
  indexing panic with `first()` + structured `empty_markdown_candidates` error
  in `validate.rs`.
- `propose-multi-id-invalid-json` — multi-ID `propose --format json` now wraps
  output in `{"summaries": [...]}`. Single-ID output unchanged.
- `doctor-json-shape-contradicts-schema` — doctor JSON emits named fields
  (`corpus`, `config`, `mutool`, `tesseract`, `native_profile`, `ocr_profile`,
  `cache`) matching CLI Spec instead of `[label, valid, detail]` tuples.
- `vocabulary-leaks-in-validation-messages` — `validate_review` now takes
  `&Terms` and uses configured vocabulary in all human-readable messages. Error
  codes remain fixed.
- `ocr-temporary-file-cleanup-errors-discarded` — cleanup failures are
  reported to stderr (non-NotFound only) instead of silently discarded.

**Performance (3 findings)**

- `markdown-coordinate-resolution-rescans-prefixes` — O(N) line-start index
  built once + O(log N) `partition_point` lookup per unit, replacing O(U×N)
  prefix rescans. New functions: `build_line_starts`, `line_column_from_index`.
- `case-insensitive-token-check-allocates-per-comparison` — zero-allocation
  byte-slice scan via `eq_ignore_ascii_case` instead of two `to_lowercase()`
  allocations per comparison.
- `tesseract-tsv-allocates-three-vectors-per-row` — geometry column indexes
  resolved to `Option<[usize; 4]>` once before the row loop.

**Dependencies (2 findings)**

- `unscoped-librebar-default-features` — `default-features = false` on
  librebar in Cargo.toml.
- `pulldown-cmark-unused-default-features` — `default-features = false` on
  pulldown-cmark.

**Build (1 finding)**

- `missing-just-ci-entry-point` — added `ci: check` recipe to justfile;
  aligned README Development section comments.

### Pass 2: 5 fixes + 4 accepts (bfcb40b)

**Performance (2 findings)**

- `ocr-cache-hit-spawns-version-probes` — `OcrProfile` resolved once per
  `PdfTools` via `OnceCell` and reused across all page lookups. New function
  `tesseract::resolve_profile` extracted. Eliminates 2N subprocess spawns for
  N cached pages.
- `proposal-ranking-verifies-unbounded-candidate-set` — raw candidates ranked
  before PDF verification; loop stops after `max_candidates` pass. New function
  `rank_raw_candidates` sorts on the same total comparison keys as before.

**Schema/contract (3 findings)**

- `cli-schema-omits-runtime-errors` — CLI Spec now declares all 37 static
  error codes + 5 dynamic code patterns (up from 18).
- `advisory-tokens-never-reported` — `ClaimProposal` gains `advisory_tokens`
  field populated from `ExtractedTokens::advisory`.
- `custom-summary-template-inventory` — `summary_ids` now walks the full
  template pattern recursively. New function `walk_summaries` traverses below
  the static prefix, matches prefix/suffix, and extracts `{id}`. Supports
  nested templates like `records/{id}/summary.yaml`.

**Accepted (4 findings, no code change)**

- `cache-manifest-invariants-are-bypassable` — no library consumers; CLI
  path validates rigorously.
- `unvalidated-discovered-state` — no library consumers; symptom (panic)
  already fixed.
- `propose-silently-suppresses-source-failures` — intentional resilience;
  `propose` is advisory by design.
- `public-apis-erase-error-types` — advisory; no library consumers to benefit
  from typed errors at internal boundaries.

## Files changed

| File | What changed |
|------|-------------|
| `Cargo.toml` | `default-features = false` on librebar and pulldown-cmark |
| `Cargo.lock` | Regenerated (fewer transitive deps) |
| `README.md` | Development section comments aligned with justfile |
| `justfile` | Added `ci: check` recipe |
| `src/cli.rs` | Doctor JSON, propose multi-ID, schema errors, template discovery |
| `src/corpus.rs` | Added `summary_template()` accessor |
| `src/markdown.rs` | Line-start index + `partition_point` coordinate resolution |
| `src/pdf/mod.rs` | `OnceCell`-cached `OcrProfile` in `PdfTools` |
| `src/pdf/tesseract.rs` | Extracted `resolve_profile`, `ocr_page_with_profile`, cleanup warnings, TSV parsing |
| `src/propose.rs` | `advisory_tokens` in `ClaimProposal`, rank-before-verify |
| `src/review.rs` | `validate_review` takes `&Terms` |
| `src/tokens.rs` | Zero-alloc `is_covered_case_insensitive` |
| `src/validate.rs` | Empty-candidates guard, vocabulary in review call |
| `tests/review.rs` | Updated `validate_review` calls with `&Terms::default()` |
| `record/audits/.../actions-taken.md` | Full remediation ledger |

## Audit ledger

The canonical record is at
`record/audits/2026-08-10-16-full-repo/actions-taken.md`.

Final status: **16 fixed, 4 accepted, 0 open.**

## What to watch

- **1300-paper corpus performance.** The OCR profile hoist and bounded
  proposal verify are designed for this scale but haven't been profiled
  against a real corpus yet. If `propose` is still slow on large corpora,
  the next lever is to also short-circuit `generate_candidates` to stop
  producing spans once enough high-scoring ones are in hand.
- **Template discovery.** The recursive `walk_summaries` rejects IDs that
  contain `/`, so a template like `records/{id}/summary.yaml` works but
  `records/{id}/sub/{id}.yaml` would not. This matches the current
  `validate_id` constraints.
- **Schema completeness.** The dynamic error codes
  (`{label}_hash_mismatch`, `{label}_source_mismatch`, etc.) are documented
  as patterns in the schema, not as individual entries per source name. An
  agent consuming the schema should treat `*_hash_mismatch` as a family.

## Landmines

None. Green state, no known issues.

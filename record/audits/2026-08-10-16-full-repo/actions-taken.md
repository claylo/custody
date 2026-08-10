---
audit: 2026-08-10-16-full-repo
last_updated: 2026-08-10
status:
  fixed: 11
  mitigated: 0
  accepted: 0
  disputed: 0
  deferred: 0
  open: 9
---

# Actions Taken: Full repository audit — Rust source, tests, dependencies, configuration, and documented behavior

Summary of remediation status for the [2026-08-10 Full repository audit — Rust source, tests, dependencies, configuration, and documented behavior audit](README.md).

---

## 2026-08-10 — Fix panic, invalid JSON, vocabulary leaks, performance, and dependency scope

**Disposition:** fixed
**Addresses:** [missing-just-ci-entry-point](README.md#missing-just-ci-entry-point), [pulldown-cmark-unused-default-features](README.md#pulldown-cmark-unused-default-features), [unscoped-librebar-default-features](README.md#unscoped-librebar-default-features), [unchecked-empty-markdown-candidates-panic](README.md#unchecked-empty-markdown-candidates-panic), [case-insensitive-token-check-allocates-per-comparison](README.md#case-insensitive-token-check-allocates-per-comparison), [tesseract-tsv-allocates-three-vectors-per-row](README.md#tesseract-tsv-allocates-three-vectors-per-row), [ocr-temporary-file-cleanup-errors-discarded](README.md#ocr-temporary-file-cleanup-errors-discarded), [markdown-coordinate-resolution-rescans-prefixes](README.md#markdown-coordinate-resolution-rescans-prefixes), [propose-multi-id-invalid-json](README.md#propose-multi-id-invalid-json), [doctor-json-shape-contradicts-schema](README.md#doctor-json-shape-contradicts-schema), [vocabulary-leaks-in-validation-messages](README.md#vocabulary-leaks-in-validation-messages)
**Commit:** b5e6dca
**Author:** clay

Eleven findings addressed in a single commit across five audit surfaces. All 164 tests pass; clippy, deny, and doc gates clean.

**Correctness (5 findings).** The empty-markdown-candidates panic (`validate.rs:84`) is replaced with `first()` plus a structured `empty_markdown_candidates` error — the CLI path already rejected empty template lists, so this only affects the public API. Multi-ID `propose --format json` now emits a single `{"summaries": [...]}` wrapper instead of adjacent JSON documents; single-ID output is unchanged for backward compatibility. Doctor JSON is restructured from `[label, valid, detail]` tuples to named fields (`corpus`, `config`, `mutool`, `tesseract`, `native_profile`, `ocr_profile`, `cache`) matching CLI Spec declarations. Human-readable doctor output retains its original display labels. Review validation now receives `&Terms` and uses configured vocabulary in all messages. OCR temp-file cleanup reports non-NotFound errors to stderr instead of discarding them.

**Performance (3 findings).** Markdown coordinate resolution replaces O(U×N) prefix rescans with a one-pass line-start index and O(log N) `partition_point` lookups. Case-insensitive token coverage uses `eq_ignore_ascii_case` on byte slices instead of allocating two lowercase strings per comparison. TSV geometry parsing resolves `Option<[usize; 4]>` column indexes once before the row loop instead of collecting three temporary vectors per row.

**Dependencies (2 findings).** `default-features = false` added to both `librebar` and `pulldown-cmark` in Cargo.toml, dropping the unused librebar cache subsystem and pulldown-cmark's `getopts`/`html` features.

**Build (1 finding).** Added `ci: check` recipe to justfile; aligned README Development section comments with actual recipe behavior.

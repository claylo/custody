---
audit: 2026-08-10-16-full-repo
last_updated: 2026-08-10
status:
  fixed: 16
  mitigated: 0
  accepted: 4
  disputed: 0
  deferred: 0
  open: 0
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

---

## 2026-08-10 — Hoist OCR profile, bound proposal verification, complete schema, accept 4 findings

**Disposition:** fixed
**Addresses:** [ocr-cache-hit-spawns-version-probes](README.md#ocr-cache-hit-spawns-version-probes), [proposal-ranking-verifies-unbounded-candidate-set](README.md#proposal-ranking-verifies-unbounded-candidate-set), [cli-schema-omits-runtime-errors](README.md#cli-schema-omits-runtime-errors), [advisory-tokens-never-reported](README.md#advisory-tokens-never-reported), [custom-summary-template-inventory](README.md#custom-summary-template-inventory)
**Commit:** bfcb40b
**Author:** clay

**Performance (2 findings).** OCR profile (tool versions, render/recognition commands) is now resolved once per `PdfTools` via `OnceCell` and reused for all page-level cache lookups. On a corpus with N cached OCR pages, this eliminates 2N subprocess spawns. The `resolve_profile` function is extracted so the low-level `ocr_page` function retains its current signature for test doubles. Proposal ranking now sorts `RawCandidate`s before PDF verification and stops after `max_candidates` pass, so a claim with 200 raw candidates verifies ~3-10 against the PDF instead of all 200. The ranking uses the same total comparison keys (`matched`, `exact.len()`, `source`, `line`, `column`), so output is identical.

**Schema/contract (3 findings).** CLI Spec now declares all 37 static error codes and 5 dynamic code patterns emitted at runtime, up from 18. `ClaimProposal` gains an `advisory_tokens` field populated from `ExtractedTokens::advisory`, surfacing advisory token data in structured output independently of enforcement mode. Summary discovery now derives from the full template pattern: `walk_summaries` recursively traverses below the template's static prefix, matches against prefix/suffix, and extracts the `{id}` component. This supports nested templates like `records/{id}/summary.yaml` that the previous flat `read_dir` missed.

---

## 2026-08-10 — Accept 4 findings with no code change

**Disposition:** accepted
**Addresses:** [cache-manifest-invariants-are-bypassable](README.md#cache-manifest-invariants-are-bypassable), [unvalidated-discovered-state](README.md#unvalidated-discovered-state), [propose-silently-suppresses-source-failures](README.md#propose-silently-suppresses-source-failures), [public-apis-erase-error-types](README.md#public-apis-erase-error-types)
**Commit:** bfcb40b
**Author:** clay

The first three findings in the Configuration and Cache Integrity surface (`cache-manifest-invariants-are-bypassable`, `unvalidated-discovered-state`, and the upstream root cause of the now-fixed `unchecked-empty-markdown-candidates-panic`) describe validation-bypass paths through the public API. There are no library consumers — the `pub` surface exists for internal/test convenience only — and the CLI path validates rigorously. The symptom (indexing panic) is already fixed; the structural encapsulation work is not warranted without library consumers.

`propose-silently-suppresses-source-failures` describes intentional resilience: `propose` is advisory and non-authoritative by design. Missing or unreadable sources produce fewer candidates rather than a hard failure, matching the tool's role as a suggestion engine rather than an evidence authority.

`public-apis-erase-error-types` is an advisory finding about `anyhow::Result` at module boundaries. Without library consumers, typed errors at internal boundaries add maintenance cost without a consumer to benefit.

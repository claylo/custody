---
audit: 2026-08-10-20-full-repo
last_updated: 2026-08-10
status:
  fixed: 33
  mitigated: 0
  accepted: 0
  disputed: 0
  deferred: 0
  open: 15
---

# Actions Taken: Full repository — Rust source (src/, tests/), dependencies, configuration (.config/, justfile, Cargo.toml, deny.toml), and documented behavior (README.md, receipts.yaml)

Summary of remediation status for the [2026-08-10 Full repository — Rust source (src/, tests/), dependencies, configuration (.config/, justfile, Cargo.toml, deny.toml), and documented behavior (README.md, receipts.yaml) audit](README.md).

---

## 2026-08-10 — Ignore corpus-local OCR cache entries by default

**Disposition:** fixed
**Addresses:** [ocr-cache-entries-are-unauthenticated-evidence](README.md#ocr-cache-entries-are-unauthenticated-evidence)
**Commit:** f4f541eb34e48437f2987f0adf022b532fbab685
**Author:** Codex

Added an explicit cache read policy and made corpus-local OCR caches write-only by default. Existing entries under a repository-controlled cache root are now treated as misses, while the platform cache remains reusable. Operators can restore reads for a trusted checkout only with the global `--trust-cache` CLI flag; project configuration cannot opt itself in.

The CLI now constructs one policy-bound `PdfTools` instance per invocation, and the cache documentation no longer describes every hit as a determinism guarantee. Regression coverage proves write-only caches retain artifacts without returning their TSV payloads and pins the local, external, and explicit-opt-in policy branches.

---

## 2026-08-10 — Contain configured cache roots

**Disposition:** fixed
**Addresses:** [cache-root-escapes-corpus-containment](README.md#cache-root-escapes-corpus-containment)
**Commit:** f0eac702620bc2beb509137da127e475fc380d16
**Author:** Codex

Configured cache roots must now be relative paths without parent components. Resolution canonicalizes the nearest existing ancestor, rejects existing symlinks that leave the corpus, and reconstructs only validated missing components before any cache directory is created. The default platform cache remains outside this project-configured path contract.

Regression tests cover absolute paths, `..` traversal, and symlink escape attempts. The README and `CacheConfig` documentation now state the same containment boundary enforced by `Corpus::from_discovered`.

---

## 2026-08-10 — Contain and bound corpus text reads

**Disposition:** fixed
**Addresses:** [summary-and-markdown-reads-skip-containment-guard](README.md#summary-and-markdown-reads-skip-containment-guard)
**Commit:** 4d76a9d18f34a8eea20dcfa2b527d0fefcfefe4f
**Author:** Codex

Moved path canonicalization, corpus containment, regular-file validation, and bounded UTF-8 reading into a shared corpus boundary. Summary and Markdown reads in `locate`, `check`, `audit`, validation, and proposal generation now use that boundary instead of opening joined paths directly.

Each text input is capped at 64 MiB using both file metadata and a limited reader, covering files that grow after the initial check. CLI regressions prove that an outside-corpus summary symlink is rejected and that an oversized sparse summary fails before its content is loaded.

---

## 2026-08-10 — Bound summary discovery

**Disposition:** fixed
**Addresses:** [summary-walk-recurses-without-a-depth-bound](README.md#summary-walk-recurses-without-a-depth-bound)
**Commit:** 8a8fd27a8119cfd8b0e36f035c2db7722f384bca
**Author:** Codex

Summary discovery now inspects directory-entry metadata without following symlinks and skips symlinked entries entirely. Its recursion ceiling is derived from the configured summary template, so valid nested layouts remain discoverable while directories deeper than any possible match fail with a contextual error.

Regression coverage exercises a broken summary symlink, an over-depth real directory, and the supported `records/{id}/summary.yaml` layout. The README now documents the symlink and depth boundaries.

---

## 2026-08-10 — Bound OCR render resolution

**Disposition:** fixed
**Addresses:** [ocr-dpi-has-no-upper-bound](README.md#ocr-dpi-has-no-upper-bound)
**Commit:** 0e1e7367389114ed9d95b76ffd41f53eda81e2be
**Author:** Codex

OCR DPI is now validated against an explicit `1..=1200` range during corpus configuration loading. `PdfTools` repeats the same validation and returns an error during construction, replacing the previous `u32`-to-`u16` saturation that could turn an invalid value into a 65,535 DPI render request.

Boundary tests pin acceptance at 1200, rejection at 1201, and rejection when a caller bypasses corpus validation and constructs `PdfTools` with `u32::MAX`. The public configuration docs and README state the enforced range.

---

## 2026-08-10 — Pin external tool executables

**Disposition:** fixed
**Addresses:** [external-tool-paths-resolved-from-ambient-path](README.md#external-tool-paths-resolved-from-ambient-path)
**Commit:** 1d0daa74eac52707719c85dc88f3df3f0cd5c64b
**Author:** Codex

Added optional absolute `pdf.tools.mutool` and `pdf.tools.tesseract` paths while retaining bare-name lookup as the default. Both forms are resolved and canonicalized once when `PdfTools` is constructed, and every subprocess invocation uses that stored absolute path. `doctor` now reports each resolved path alongside its version.

The canonical paths are part of `OcrProfile` and therefore the cache key, preventing same-version binaries at different locations from sharing entries. The OCR profile directory is bumped to `v2`; regression coverage proves configured binaries work with an empty ambient `PATH`, relative configured paths are rejected, and cache identity changes with executable identity.

---

## 2026-08-10 — Reject drifted MuPDF structured text

**Disposition:** fixed
**Addresses:** [stext-schema-drift-yields-silent-empty-extraction](README.md#stext-schema-drift-yields-silent-empty-extraction)
**Commit:** b1d00425e8932f4201fb900a56de01df68eb11ff
**Author:** Codex

MuPDF structured-text parsing now requires the documented page and block containers and requires `lines` on blocks declared as `type: text`. Non-text blocks remain valid and are ignored, preserving image-only pages for OCR fallback instead of conflating them with schema drift.

Missing bounding boxes remain representable, but `locate` now emits `no geometry available for this backend` when a match has no coordinates. Regression tests cover missing structural fields, non-text blocks, optional geometry, and the explicit diagnostic.

---

## 2026-08-10 — Enforce external tool version ranges

**Disposition:** fixed
**Addresses:** [external-tool-versions-probed-never-validated](README.md#external-tool-versions-probed-never-validated)
**Commit:** 217673f6d1c7405733523481b855ef68dc8d255d
**Author:** Codex

Defined and documented supported runtime ranges of MuPDF `>=1.28.0, <1.29.0` and Tesseract `>=5.5.0, <5.6.0`. Numeric probe output is parsed and checked against adapter-local constants instead of treating every zero-exit probe as compatible.

`doctor` now distinguishes `missing`, `unsupported`, and `ok`, exposes the tool states in JSON, and fails preflight for an unsupported version. Boundary tests cover both supported minor lines, both exclusion boundaries, malformed output, and an end-to-end unsupported MuPDF probe.

---

## 2026-08-10 — Prevent multibyte inline HTML panics

**Disposition:** fixed
**Addresses:** [inline-html-byte-slice-panics-on-multibyte-markdown](README.md#inline-html-byte-slice-panics-on-multibyte-markdown)
**Commit:** dafa20b550015c199f292d9fa96f690d136a7c96
**Author:** Codex

Replaced the `<br` probe's direct UTF-8 string slice with a bounds-checked byte-prefix comparison. The comparison remains ASCII-case-insensitive while no longer requiring byte three to be a character boundary.

A regression test feeds `parse_units` the previously crashing `para <?é?> tail` input and confirms that the processing instruction is ignored without a panic.

---

## 2026-08-10 — Preflight the PDF toolchain before evidence commands

**Disposition:** fixed
**Addresses:** [tool-failure-indistinguishable-from-invalid-evidence](README.md#tool-failure-indistinguishable-from-invalid-evidence)
**Commit:** 6e68092b13fd06344c7890259f76fe9735845f45
**Author:** Codex

Added a shared PDF-toolchain preflight that validates both configured executables and their supported versions before `locate`, `check`, `audit`, or `propose` reads and judges summary evidence. Missing or unsupported infrastructure now aborts outside evidence counters instead of being reported as invalid corpus content.

CLI regression coverage runs aggregate commands with an empty `PATH` and proves they fail with a toolchain-preflight error before emitting invalid-evidence totals. The README now distinguishes infrastructure failures from evidence findings.

---

## 2026-08-10 — Handle closed stdout without a crash report

**Disposition:** fixed
**Addresses:** [broken-pipe-panic-writes-crash-dump](README.md#broken-pipe-panic-writes-crash-dump)
**Commit:** 30676612de967fbfbf2d232074c635b37984a7d7
**Author:** Codex

Routed report output through fallible writes to a locked stdout and preserved I/O errors through the `anyhow` chain. The binary now recognizes `BrokenPipe` as a clean termination, so short-lived pipeline consumers cannot trigger a panic or Librebar crash report.

Unit coverage proves broken-pipe errors remain detectable after context is attached, and a Unix CLI regression closes the stdout reader before running `doctor` and verifies a successful, panic-free exit. All report-producing stdout call sites use the shared fallible path.

---

## 2026-08-10 — Preserve partial proposal batches

**Disposition:** fixed
**Addresses:** [propose-aborts-the-batch-on-one-unreadable-summary](README.md#propose-aborts-the-batch-on-one-unreadable-summary)
**Commit:** 1c0b187e44919deed56a868f37b5b244f013aafd
**Author:** Codex

`propose` now records malformed summaries and filename/ID mismatches as per-summary failures and continues processing the remaining corpus. Aggregate JSON retains successful proposals beside structured `summary_parse_failed` and `id_mismatch` entries; human mode keeps proposals on stdout and names failures on stderr before returning a non-zero status.

A mixed-corpus CLI regression covers a valid proposal, malformed YAML, and an ID mismatch in both output modes. The README documents the partial-result and final-exit behavior.

---

## 2026-08-10 — Encode vocabulary-rename fallibility

**Disposition:** fixed
**Addresses:** [vocabulary-rename-results-discarded-without-rationale](README.md#vocabulary-rename-results-discarded-without-rationale)
**Commit:** 550dbbd968bde60bf6c94c603a4e34521a70d16a
**Author:** Codex

Split vocabulary renaming into a fallible strict input path and an infallible localization path, so output code no longer discards `Result` values whose safety depended on a distant boolean branch. Canonicalization retains conflict detection while localization exposes no fallible result to ignore.

SHA-256 hex encoding now writes high and low nibbles directly into the output string instead of discarding the formally infallible result of `write!`. Existing vocabulary and hashing regressions pass unchanged, confirming the refactor preserves behavior.

---

## 2026-08-10 — Unify runtime issue codes and CLI schema metadata

**Disposition:** fixed
**Addresses:** [schema-declares-source-error-codes-the-code-never-emits](README.md#schema-declares-source-error-codes-the-code-never-emits), [runtime-error-codes-absent-from-cli-spec](README.md#runtime-error-codes-absent-from-cli-spec), [error-code-registry-is-hand-maintained](README.md#error-code-registry-is-hand-maintained)
**Commit:** 62513107404616cab769ae7c4eb86b314acb49d8
**Author:** Codex

Moved every runtime issue kind, exit classification, and description into one typed registry used by evidence, validation, review, batch-error construction, and CLI schema generation. New issue construction paths must select a registry value, eliminating unrelated string literals on the emit and declaration sides. `summary_parse_failed` and `id_mismatch` are now declared automatically, and warning-only `weak_section_only` is omitted from error metadata instead of making `receipts schema` fail with an invalid zero exit code.

Source-integrity issues now emit the declared stable `source_*` codes and carry the configured source name in the structured `source` field. Regression coverage proves source hash and containment failures retain their source identity, the live schema command succeeds, the previously divergent codes are declared, and every declared error has a valid non-zero exit code.

---

## 2026-08-10 — Stabilize proposal JSON output

**Disposition:** fixed
**Addresses:** [propose-json-shape-varies-by-summary-count](README.md#propose-json-shape-varies-by-summary-count)
**Commit:** fa85d1947fbabdaa185216ed4814253b57b3819b
**Author:** Codex

`propose --format json` now always emits a `{"summaries": [...]}` envelope, removing the count-dependent single-summary object. The CLI Spec declares that same top-level field, and the README documents the unconditional contract.

Integration coverage pins the exact top-level key set for empty, one-summary, and two-summary corpora while retaining the mixed-success batch behavior added in the preceding remediation.

---

## 2026-08-10 — Preserve authoring output defaults

**Disposition:** fixed
**Addresses:** [default-output-becomes-json-when-redirected](README.md#default-output-becomes-json-when-redirected)
**Commit:** 437e841dc4550e6dfd82ef21747853eab1ef4ad1
**Author:** Codex

`locate` and `propose` now default to human-readable output regardless of whether stdout is attached to a terminal. Machine consumers must request JSON explicitly with `--format json`, while status-oriented commands retain their terminal-aware output selection.

An integration regression captures stdout for both authoring commands without a format flag and verifies that each preserves the documented human output contract.

---

## 2026-08-10 — Restrict configuration to repository-scoped sources

**Disposition:** fixed
**Addresses:** [undocumented-user-config-and-environment-layers](README.md#undocumented-user-config-and-environment-layers)
**Commit:** 1d5a9973f79c52772cea1a14b7f4c69d51e4a4c3
**Author:** Codex

Disabled Librebar's implicit user configuration and process-environment overlays. Receipts configuration now comes only from built-in defaults, project discovery, and an explicit `--config` path, matching the repository-scoped evidence contract documented in the README.

A child-process regression sets a conflicting `RECEIPTS_CORPUS__SUMMARIES` value and proves it cannot override the project configuration.

---

## 2026-08-10 — Expose the summary ID grammar

**Disposition:** fixed
**Addresses:** [summary-id-constraints-undocumented](README.md#summary-id-constraints-undocumented)
**Commit:** 69cfb9983218474ef91e0a078900b2091181b71a
**Author:** Codex

Documented the accepted summary ID grammar in the README and repeated it in runtime rejection messages and CLI schema metadata. IDs must contain at least three lowercase ASCII letters, digits, or interior hyphens, with no leading or trailing hyphen.

The corpus regression now covers traversal, uppercase, undersized, and leading-hyphen inputs and requires the actionable grammar explanation for every rejection.

---

## 2026-08-10 — Encode a non-publishable package posture

**Disposition:** fixed
**Addresses:** [entire-crate-is-a-published-public-api](README.md#entire-crate-is-a-published-public-api)
**Commit:** ac261483db640e5564fb661c8b70ea57e2519422
**Author:** Codex

Set `publish = false` in the package manifest, encoding Receipts as a binary product whose library target exists for integration testing rather than as a crates.io library contract. Cargo metadata now reports an empty publication allowlist, so the internal module surface cannot be published accidentally.

The existing path-install workflow remains unchanged, and the complete repository gate passes with the non-publishable package posture.

---

## 2026-08-10 — Type coordinate domains and publish character columns

**Disposition:** fixed
**Addresses:** [coordinate-primitives-are-interchangeable-usize](README.md#coordinate-primitives-are-interchangeable-usize), [byte-offset-published-as-a-markdown-column](README.md#byte-offset-published-as-a-markdown-column)
**Commit:** 0d55ae5a5186144d182422e6a273bf45801a4719
**Author:** Codex

Introduced transparent `Line`, `Column`, `Page`, `ClaimIndex`, and `LocatorIndex` types throughout evidence, review, proposal, CLI, validation, PDF extraction, and OCR cache boundaries. Spatial constructors and deserializers reject zero; claim and locator indices retain their documented zero-based contract. Transparent serialization preserves the existing numeric YAML and JSON shapes while preventing cross-domain swaps at compile time.

Markdown columns now count Unicode scalar values from the start of the line instead of UTF-8 bytes. The README states those semantics, and regressions pin non-ASCII table coordinates, zero-coordinate rejection during decoding, and the unchanged evidence workflow.

---

## 2026-08-10 — Reuse evidence locators in proposals

**Disposition:** fixed
**Addresses:** [propose-duplicates-locator-types](README.md#propose-duplicates-locator-types)
**Commit:** 1097b108902cde525c204d8dbcb60f87d8e97731
**Author:** Codex

Removed the field-for-field `MarkdownMatch` and `PdfMatch` copies. Proposal candidates now store the canonical `MarkdownLocator` and `PdfLocator` types directly, keeping proposal output structurally tied to the evidence schema.

The proposal regression constructs an accepted evidence `Locator` from a candidate without field conversion, and the existing CLI contract tests confirm the serialized proposal shape remains compatible.

---

## 2026-08-10 — Render domain enums with schema spellings

**Disposition:** fixed
**Addresses:** [debug-formatting-leaks-into-user-facing-messages](README.md#debug-formatting-leaks-into-user-facing-messages)
**Commit:** 460ea77ba37a6fec30625a3821ad8e6884193366
**Author:** Codex

Implemented `Display` for `UnitKind`, `PdfBackend`, and `Verdict`, with each implementation delegating to the same stable spelling used by serialization. Validation, unit-resolution, and locator diagnostics now name accepted schema values such as `paragraph` and `unsupported` instead of Rust variant names.

Focused regressions assert the actionable schema spellings in Markdown and review diagnostics, and Clippy passes with every user-facing enum site using the stable display contract.

---

## 2026-08-10 — Require verified PDF matches in proposals

**Disposition:** fixed
**Addresses:** [candidate-pdf-option-is-never-none](README.md#candidate-pdf-option-is-never-none)
**Commit:** c93d4566205b6391391e13b6c8b3f5ec636dfd01
**Author:** Codex

Changed `Candidate.pdf` from `Option<PdfLocator>` to `PdfLocator`, matching the proposal generator's invariant that a candidate is emitted only after a unique native PDF page is verified. The serialized `pdf` object remains unchanged.

Removed the unreachable human-output fallback and optional acceptance-command guard. Proposal and CLI regressions now access the PDF locator directly.

---

## 2026-08-10 — Support typed report decoding

**Disposition:** fixed
**Addresses:** [report-types-are-write-only](README.md#report-types-are-write-only)
**Commit:** 5198fe99856eff95c52bb9b042e18ba9becd2f34
**Author:** Codex

Added `Deserialize` and comparison support across proposal reports, validation reports, evidence issues, severity values, locators, and PDF bounding boxes. Report producers and consumers can now share the same public types instead of falling back to untyped JSON values.

The CLI proposal regression decodes the command envelope into `ProposalReport`, asserts through typed fields, and verifies a serialize-deserialize round trip with `PartialEq`. The complete repository gate passes 199 tests.

---

## 2026-08-10 — Batch multi-page native PDF extraction

**Disposition:** fixed
**Addresses:** [validate-spawns-one-mutool-per-cited-page](README.md#validate-spawns-one-mutool-per-cited-page)
**Commit:** 0fc1bd311dcd4e6b56486b192f79c84de9b452df
**Author:** Codex

Validation now collects distinct native page citations per source before checking locators. A source citing multiple pages is extracted once with `native_pages(..., None)` and indexed into the existing page memo; a source citing exactly one page retains the lower-memory single-page extraction path.

A counting-provider regression proves two cited pages produce one whole-document call instead of two subprocess-shaped calls. Missing pages and extraction failures remain memoized as page-specific validation errors, and the complete gate passes 200 tests.

---

## 2026-08-10 — Prepare proposal spans once per source

**Disposition:** fixed
**Addresses:** [propose-regenerates-claim-independent-spans-per-claim](README.md#propose-regenerates-claim-independent-spans-per-claim)
**Commit:** 063f469e73e6a415d2f25860c939258e241ac6af
**Author:** Codex

Split proposal generation into claim-independent span preparation and per-claim token scoring. Sentence splitting, exact-text uniqueness checks, source allocation, and section preparation now occur once per Markdown source rather than once per claim.

Scored candidates borrow prepared spans, share section metadata through `Rc<[String]>`, and clone locator data only after PDF verification accepts a candidate. Existing proposal ranking, determinism, truncation, and output tests pass unchanged.

---

## 2026-08-10 — Index Markdown locator coordinates

**Disposition:** fixed
**Addresses:** [resolve-unit-linear-scan-called-twice-per-locator](README.md#resolve-unit-linear-scan-called-twice-per-locator)
**Commit:** 3f53e4ab4d5d9d09450c98f0c58a2c3b0fad323b
**Author:** Codex

Added a `UnitIndex` keyed by unit kind, line, and column, with explicit unique and ambiguous states. Validation builds the index once beside each parsed Markdown document and resolves locator coordinates with constant-time lookups while preserving the existing missing and multiple-unit diagnostics.

Each resolved unit is retained through the claim pass and reused by weak-section evaluation, eliminating the second lookup. Regression coverage pins successful, missing-kind, and duplicate-coordinate behavior; the complete gate passes 200 tests.

---

## 2026-08-10 — Bound and memoize proposal PDF verification

**Disposition:** fixed
**Addresses:** [pdf-verification-unbounded-when-candidates-fail](README.md#pdf-verification-unbounded-when-candidates-fail)
**Commit:** 09cbfd941ed34562b62c467b3b2a565be503708d
**Author:** Codex

Proposal generation now limits previously unseen PDF span checks for each claim to eight times the requested candidate count. Failed and successful verification results are memoized by source and exact text across claims, so repeated spans do not consume the per-claim budget or trigger another PDF scan.

A regression places a valid span after nine higher-ranked PDF misses and confirms a one-candidate request stops after eight attempts. The README documents the deterministic verification budget.

---

## 2026-08-10 — Consume owned JSON during canonical sorting

**Disposition:** fixed
**Addresses:** [owned-json-value-borrowed-then-deep-cloned](README.md#owned-json-value-borrowed-then-deep-cloned)
**Commit:** 5e3f829c933fc14498623911bf8ce17d51e1fa55
**Author:** Codex

Changed canonical JSON sorting to consume each owned `serde_json::Value`, moving object keys, array elements, and leaf values into the result instead of deep-cloning the locator tree. Review hash semantics remain unchanged, as confirmed by the complete deterministic hash test suite.

---

## 2026-08-10 — Borrow configured corpus source names

**Disposition:** fixed
**Addresses:** [source-names-allocates-a-vec-to-answer-a-lookup](README.md#source-names-allocates-a-vec-to-answer-a-lookup)
**Commit:** 205d613d2f47000596c7efbe2ed4c30318a5a2ef
**Author:** Codex

`Corpus::source_names` now returns a borrowed iterator, and `Corpus::declares_source` delegates membership checks directly to the configured source map. Proposal generation iterates borrowed names, while validation replaces allocation plus linear scan with the map lookup.

Foundation regressions cover iteration order and positive and negative membership. The complete repository gate passes 201 tests.

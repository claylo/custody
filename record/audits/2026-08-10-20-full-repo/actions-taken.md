---
audit: 2026-08-10-20-full-repo
last_updated: 2026-08-10
status:
  fixed: 9
  mitigated: 0
  accepted: 0
  disputed: 0
  deferred: 0
  open: 39
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

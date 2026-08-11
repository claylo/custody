---
audit: 2026-08-10-20-full-repo
last_updated: 2026-08-10
status:
  fixed: 1
  mitigated: 0
  accepted: 0
  disputed: 0
  deferred: 0
  open: 47
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

---
audit: 2026-08-10-20-full-repo
last_updated: 2026-08-10
status:
  fixed: 3
  mitigated: 0
  accepted: 0
  disputed: 0
  deferred: 0
  open: 45
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

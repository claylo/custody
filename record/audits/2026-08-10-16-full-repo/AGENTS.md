# Agent Briefing — Full repository audit — Rust source, tests, dependencies, configuration, and documented behavior

You are in a `cased` audit output directory. This file exists to help you pick
up remediation work without thrashing. Read it once, then act.

**Audit:** `2026-08-10-16-full-repo`
**Date:** 2026-08-10
**Findings:** 20 total

## Files in this directory

- `README.md`        — authored narrative report (markdown, GitHub-rendered companion to report.html). Read-only for remediation work.
- `report.html`      — interactive rendered report (primary deliverable). Read-only.
- `findings.yaml`    — structured findings (source for the build). Read-only.
- `recon.yaml`       — structural model. Read-only.
- `assets/`          — generated sparkline SVGs. Don't edit.
- `actions-taken.md` — append-only remediation ledger. May not exist yet;
  create it the first time you log an action.
- `AGENTS.md`        — this file.

## The loop

For each finding you address:

1. Find it in `README.md` or `report.html` by its slug. Anchors match the slug
   exactly; every finding is pre-listed in the index below so you don't need
   to grep.
2. Read the concern, location, and remediation text.
3. Make the code change in the target repository.
4. Append one entry to `actions-taken.md`. **One entry per action**, even
   when a single action resolves multiple findings — put every slug it
   addresses in the `Addresses` field.

## `actions-taken.md` format

YAML front matter plus chronological markdown entries. Front matter is
mandatory; update `last_updated` and the `status` counts every time you
add an entry. The `open` count is `20 - (fixed + mitigated +
accepted + disputed + deferred)`.

```markdown
---
audit: 2026-08-10-16-full-repo
last_updated: YYYY-MM-DD
status:
  fixed: 0
  mitigated: 0
  accepted: 0
  disputed: 0
  deferred: 0
  open: 20
---

# Actions Taken: Full repository audit — Rust source, tests, dependencies, configuration, and documented behavior

Summary of remediation status for the [2026-08-10 Full repository audit — Rust source, tests, dependencies, configuration, and documented behavior audit](README.md).

---

## YYYY-MM-DD — brief description of the action

**Disposition:** fixed
**Addresses:** [finding-slug](README.md#finding-slug)
**Commit:** {SHA or PR link}
**Author:** {who did the work}

One to three paragraphs describing what changed, in which files, and why
this approach. If the disposition is `accepted` or `disputed`, the rationale
must be here. If `deferred`, include the target date or milestone.
```

## Dispositions

- `fixed` — code change deployed; commit SHA or PR link required
- `mitigated` — compensating control in place; root cause remains; explain
  the residual risk
- `accepted` — risk acknowledged; rationale mandatory (who decided, why).
  This is not a euphemism for "ignored"
- `disputed` — finding contested with evidence; not a dismissal. The
  original finding stays in `README.md`; this entry records the counterargument
- `deferred` — scheduled for later; target date or milestone reference
  required. A deferred finding without a target is an accepted finding in
  disguise

## What you must not do

- Do not edit `README.md`, `report.html`, `findings.yaml`, `recon.yaml`, or
  anything in `assets/`. They are the audit artifact and must stay immutable.
- Do not edit past `actions-taken.md` entries. The file is append-only. If
  a previous action is superseded, add a new entry referencing the old one.
- Do not invent finding slugs. Use the ones in the index below, verbatim.
- Do not create an empty `actions-taken.md` until you have at least one
  action to log.

## Finding index

Every finding in this audit. Use these exact slugs in the `Addresses` field
of your `actions-taken.md` entries.

### The Configuration and Cache Integrity Surface

- `cache-manifest-invariants-are-bypassable` (significant) — `src/pdf/cache.rs:27-35`
- `unvalidated-discovered-state` (moderate) — `src/corpus.rs:29-56`
- `unchecked-empty-markdown-candidates-panic` (significant) — `src/validate.rs:84-87`

### The Error Architecture Surface

- `propose-silently-suppresses-source-failures` (significant) — `src/propose.rs:135-155`
- `ocr-temporary-file-cleanup-errors-discarded` (moderate) — `src/pdf/tesseract.rs:246-250`
- `public-apis-erase-error-types` (advisory) — `src/output.rs:1-6`

### The Dependency Reachability Surface

- `unscoped-librebar-default-features` (moderate) — `Cargo.toml:16`
- `pulldown-cmark-unused-default-features` (advisory) — `Cargo.toml:17`

### The Repeated-Work Performance Surface

- `ocr-cache-hit-spawns-version-probes` (significant) — `src/pdf/tesseract.rs:206-223`
- `proposal-ranking-verifies-unbounded-candidate-set` (significant) — `src/propose.rs:157-194`
- `markdown-coordinate-resolution-rescans-prefixes` (moderate) — `src/markdown.rs:236-245`
- `case-insensitive-token-check-allocates-per-comparison` (advisory) — `src/tokens.rs:57-59`
- `tesseract-tsv-allocates-three-vectors-per-row` (advisory) — `src/pdf/tesseract.rs:273-301`

### The Documented Agent Contract Surface

- `cli-schema-omits-runtime-errors` (significant) — `src/evidence.rs:206-224`
- `custom-summary-template-inventory` (significant) — `src/cli.rs:969-984`
- `propose-multi-id-invalid-json` (significant) — `src/cli.rs:834-860`
- `advisory-tokens-never-reported` (moderate) — `.handoffs/2026-08-09-phase-2-complete.md:157-160`
- `doctor-json-shape-contradicts-schema` (moderate) — `src/cli.rs:445-472`
- `vocabulary-leaks-in-validation-messages` (moderate) — `src/review.rs:104-137`
- `missing-just-ci-entry-point` (moderate) — `README.md:337-344`

## If you have the `cased` skill loaded

Invoke it. The skill's Phase 5 covers remediation tracking with the full
schema reference and worked examples. This briefing exists for the case
where you land in the directory without the skill available.

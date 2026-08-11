# Agent Briefing — Full repository — Rust source (src/, tests/), dependencies, configuration (.config/, justfile, Cargo.toml, deny.toml), and documented behavior (README.md, receipts.yaml)

You are in a `cased` audit output directory. This file exists to help you pick
up remediation work without thrashing. Read it once, then act.

**Audit:** `.`
**Date:** 2026-08-10
**Findings:** 48 total

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
add an entry. The `open` count is `48 - (fixed + mitigated +
accepted + disputed + deferred)`.

```markdown
---
audit: .
last_updated: YYYY-MM-DD
status:
  fixed: 0
  mitigated: 0
  accepted: 0
  disputed: 0
  deferred: 0
  open: 48
---

# Actions Taken: Full repository — Rust source (src/, tests/), dependencies, configuration (.config/, justfile, Cargo.toml, deny.toml), and documented behavior (README.md, receipts.yaml)

Summary of remediation status for the [2026-08-10 Full repository — Rust source (src/, tests/), dependencies, configuration (.config/, justfile, Cargo.toml, deny.toml), and documented behavior (README.md, receipts.yaml) audit](README.md).

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

### The Evidence Trust Surface

- `ocr-cache-entries-are-unauthenticated-evidence` (critical) — `src/pdf/cache.rs:115-133`
- `cache-root-escapes-corpus-containment` (significant) — `src/corpus.rs:36-45`
- `summary-and-markdown-reads-skip-containment-guard` (moderate) — `src/cli.rs:1119-1125`
- `summary-walk-recurses-without-a-depth-bound` (moderate) — `src/cli.rs:1157-1161`
- `ocr-dpi-has-no-upper-bound` (moderate) — `src/config.rs:274-282`

### The External Tool Surface

- `external-tool-paths-resolved-from-ambient-path` (significant) — `src/pdf/mutool.rs:14-26`
- `stext-schema-drift-yields-silent-empty-extraction` (significant) — `src/pdf/mutool.rs:144-167`
- `external-tool-versions-probed-never-validated` (moderate) — `src/cli.rs:594-601`

### The Failure Mode Surface

- `inline-html-byte-slice-panics-on-multibyte-markdown` (significant) — `src/markdown.rs:135-143`
- `tool-failure-indistinguishable-from-invalid-evidence` (moderate) — `src/validate.rs:275-281`
- `broken-pipe-panic-writes-crash-dump` (moderate) — `src/output.rs:1-8`
- `propose-aborts-the-batch-on-one-unreadable-summary` (advisory) — `src/cli.rs:993-1002`
- `vocabulary-rename-results-discarded-without-rationale` (note) — `src/terms.rs:113-119`

### The Published Contract Surface

- `schema-declares-source-error-codes-the-code-never-emits` (significant) — `src/cli.rs:553-582`
- `runtime-error-codes-absent-from-cli-spec` (significant) — `src/cli.rs:844-862`
- `propose-json-shape-varies-by-summary-count` (significant) — `src/cli.rs:1010-1016`
- `error-code-registry-is-hand-maintained` (moderate) — `src/cli.rs:539-552`
- `default-output-becomes-json-when-redirected` (moderate) — `src/cli.rs:183-185`
- `undocumented-user-config-and-environment-layers` (moderate) — `src/config.rs:185-199`
- `summary-id-constraints-undocumented` (advisory) — `src/corpus.rs:168-179`

### The Type and API Surface

- `entire-crate-is-a-published-public-api` (advisory) — `src/lib.rs:1-17`
- `coordinate-primitives-are-interchangeable-usize` (significant) — `src/markdown.rs:163-174`
- `propose-duplicates-locator-types` (significant) — `src/propose.rs:55-69`
- `byte-offset-published-as-a-markdown-column` (moderate) — `src/markdown.rs:247-254`
- `debug-formatting-leaks-into-user-facing-messages` (moderate) — `src/validate.rs:189-198`
- `candidate-pdf-option-is-never-none` (advisory) — `src/propose.rs:176-197`
- `report-types-are-write-only` (advisory) — `src/propose.rs:21-36`

### The Performance Surface

- `validate-spawns-one-mutool-per-cited-page` (significant) — `src/validate.rs:233-248`
- `propose-regenerates-claim-independent-spans-per-claim` (significant) — `src/propose.rs:159-169`
- `resolve-unit-linear-scan-called-twice-per-locator` (moderate) — `src/markdown.rs:172-181`
- `pdf-verification-unbounded-when-candidates-fail` (moderate) — `src/propose.rs:171-179`
- `owned-json-value-borrowed-then-deep-cloned` (moderate) — `src/review.rs:46-56`
- `source-names-allocates-a-vec-to-answer-a-lookup` (advisory) — `src/corpus.rs:127-131`
- `weak-section-list-lowercased-per-comparison` (advisory) — `src/sections.rs:14-24`
- `release-profile-left-at-cargo-defaults` (note) — `Cargo.toml:22-33`

### The Verification Gate Surface

- `structured-output-contracts-untested` (significant) — `src/cli.rs:632-642`
- `just-check-is-not-a-complete-gate` (moderate) — `justfile:1-10`
- `no-automated-ci-for-the-documented-gate` (moderate) — `justfile:40-42`
- `justfile-loads-repo-supplied-dotenv` (moderate) — `justfile:1-4`
- `license-allowlist-inherited-from-absent-dependency-tree` (moderate) — `.config/deny.toml:28-42`
- `summary-discovery-recursion-untested` (moderate) — `src/cli.rs:1157-1176`
- `markdown-coordinates-only-pinned-at-line-one` (moderate) — `tests/markdown.rs:20-28`
- `transitive-syn-duplicate-is-not-actionable` (note) — `.config/deny.toml:61-79`

### The Structure Surface

- `validate-document-carries-seven-concerns` (moderate) — `src/validate.rs:161-176`
- `check-and-audit-duplicate-the-summary-loop` (moderate) — `src/cli.rs:909-926`
- `duplicated-issue-constructors` (moderate) — `src/validate.rs:486-500`
- `duplicated-subprocess-runner` (advisory) — `src/pdf/tesseract.rs:361-372`
- `dead-public-vocabulary-and-report-helpers` (advisory) — `src/terms.rs:88-99`

## If you have the `cased` skill loaded

Invoke it. The skill's Phase 5 covers remediation tracking with the full
schema reference and worked examples. This briefing exists for the case
where you land in the directory without the skill available.

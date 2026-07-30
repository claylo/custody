# Receipts Evidence Validation Design

**Date:** 2026-07-28
**Status:** Implemented

## Purpose

A corpus of academic PDFs summarized with LLM assistance accumulates claims
faster than anyone can verify them by hand. `receipts` makes each claim's cited
evidence mechanically checkable against both:

1. the converted source Markdown used during summarization; and
2. the canonical source PDF from which that Markdown was produced.

A locator is valid only when the same exact literal, after deterministic
whitespace normalization, occurs unambiguously in both sources. This catches
stale evidence, conversion artifacts, missing claim coverage, changed
claims, and locators that cannot be reproduced from the canonical PDF.

`receipts` does not decide whether a selected passage logically supports a claim.
That remains the responsibility of adversarial summary verification. The tool
proves that the claim has not changed since evidence selection and that its
claimed source text is still present in both immutable source versions.

## Scope

The first release provides:

- `receipts doctor`;
- `receipts locate`;
- `receipts check`;
- `receipts audit`;
- corpus layout declared in `receipts.yaml`, discovered by walking upward;
- semantic Markdown parsing with GFM table-cell boundaries;
- native PDF text validation through MuPDF;
- page-level OCR fallback through Tesseract;
- persistent, content-addressed extraction caching; and
- an optional evidence property, so a corpus can adopt evidence incrementally.

The first release does not:

- rewrite summary YAML files;
- use fuzzy, case-insensitive, punctuation-insensitive, or semantic matching;
- accept an intermediate or staging directory as a canonical source location;
- OCR an entire PDF merely because one page needs OCR;
- use an LLM to normalize or validate locator text; or
- make an optional third PDF parser part of pass/fail status.

## Project Layout

The crate is self-contained:

```text
receipts/
├── Cargo.toml
├── README.md
├── receipts.yaml
├── docs/specs/
├── src/
└── tests/
```

The corpus is a separate directory tree, located by discovering
`receipts.yaml`. The crate never assumes that the tool and the corpus live
together, so one installed binary serves any number of unrelated corpora.

A corpus declares only its layout:

```yaml
corpus:
  summaries: "summaries/{id}.yaml"
  markdown:
    - "md/{id}.md"
    - "md/{id}/{id}.md"
  pdf: "pdfs/{id}.pdf"
```

Every template is relative to the corpus root and must contain `{id}`.
Absolute templates and `..` segments are rejected, so a configuration file
cannot direct reads outside the corpus.

Runtime extraction data defaults to the platform cache directory
(`~/Library/Caches/receipts/pdf-text` on macOS) and moves with `cache.root`, so
a corpus that wants reproducible, co-located artifacts can keep them in-tree.
The working directory is resolved after applying Librebar's `-C`/`--chdir`
option.

`receipts` is a standalone Cargo binary:

```sh
cargo install receipts --locked
receipts doctor
```

## Configuration

Discovery is delegated to Librebar rather than reimplemented, so `receipts`
behaves like other tools that locate a project by walking upward.

From the working directory, each ancestor is checked for `.config/receipts.EXT`,
then `.receipts.EXT`, then `receipts.EXT`, stopping at a `.git` boundary. `EXT`
is tried in the order `toml`, `yaml`, `yml`, `json`; YAML is the documented
format, but a corpus that prefers TOML or JSON needs no code change. A stray
`receipts.toml` beside a `receipts.yaml` therefore wins, which is worth knowing
when a corpus has both.

Sources merge lowest to highest precedence:

1. struct defaults;
2. user config under the platform config directory;
3. the discovered project file; and
4. any file named with `--config`.

The corpus root is resolved in this order:

1. the directory holding the discovered project file, or its parent when that
   file sits in `.config/`;
2. the nearest `.git` boundary; or
3. the working directory.

Defaults are real defaults. A corpus laid out as `summaries/`, `md/`, and
`pdfs/` inside a Git repository needs no configuration file at all, and
`receipts doctor` reports which file was used — or `defaults` when none was
found, so a misplaced config is visible rather than silent.

Deriving the root from the config file, rather than probing for marker files,
keeps the tool free of assumptions about what else a corpus contains.

## Build-vs-Adopt Decision

The design extends established tools instead of implementing PDF parsing or
OCR:

- `librebar = 0.3.0` supplies common CLI, working-directory, configuration,
  and YAML parsing infrastructure;
- `pulldown-cmark` supplies CommonMark/GFM events and source byte ranges;
- MuPDF's `mutool draw -F stext.json` supplies mature native PDF text and
  geometry extraction; and
- Tesseract supplies mature OCR recognition and TSV coordinates.

The Rust code provides the corpus-agnostic schema, source resolution,
normalization, exact matching, evidence coverage, caching, diagnostics, and
command orchestration that those tools do not provide.

## Rust Dependencies

The implementation is built on `librebar = 0.3.0`, with the features needed
for CLI and configuration support.

Direct dependencies are limited to what the corpus-specific layer needs:

- `pulldown-cmark`, with GFM tables enabled;
- `clap` derive support for application-specific subcommands;
- `serde` and `serde_json` for typed records, cache manifests, MuPDF JSON,
  Tesseract-derived records, and `--json` output; and
- a SHA-256 implementation for source and claim fingerprints.

An additional error-reporting dependency is used only if Librebar's error
types cannot represent validation and operational failures clearly.

The first release does not depend on a Rust PDF or OCR crate. The extraction
boundary remains internal so another implementation can be added without
changing the evidence YAML.

## Required Runtime Tools

### MuPDF

`mutool` is required for:

- native structured text extraction; and
- deterministic page rendering for Tesseract.

The accepted native backend identifier is:

```text
mutool-native
```

### Tesseract

Tesseract is required only for locators whose exact literal cannot be
reproduced through native PDF extraction.

The accepted OCR backend identifier is:

```text
tesseract-ocr
```

The initial OCR profile is versioned independently from the installed binary:

```text
tesseract-eng-300dpi-v1
```

The profile specifies English recognition, 300-DPI MuPDF rendering, automatic
page segmentation, orientation detection and correction in 90-degree
increments, and TSV output. Its exact commands and settings are recorded in
the cache manifest.

The canonical PDF is never modified. OCR operates only on deterministic
renderings in the runtime cache.

## CLI

Librebar's common flags are global:

```text
-q, --quiet
-v, --verbose
--json
--color <auto|always|never>
-C, --chdir <directory>
--version-only
```

### `receipts doctor`

```text
receipts doctor
```

`doctor`:

- resolves the corpus root and required corpus directories;
- checks that `mutool` and `tesseract` are available;
- reports their versions;
- reports the active native and OCR extraction profiles;
- checks that the runtime cache directory can be created or written; and
- reports errors without changing corpus sources.

### `receipts locate`

```text
receipts locate <ID> --claim <INDEX> --exact <TEXT>
  [--page <PDF_PAGE>]
  [--line <MARKDOWN_LINE>]
  [--column <MARKDOWN_COLUMN>]
```

`locate` resolves the summary YAML, Markdown, and canonical PDF for `<ID>`.
It validates the claim index and prints a YAML-ready locator with all source
and claim fingerprints.

Markdown behavior:

- `--exact` is normalized with the same function used for source text.
- A line and optional column restrict candidate semantic units.
- Without coordinates, the exact literal must resolve to one semantic unit.
- Ambiguous selections fail and report candidate coordinates and text.

PDF behavior:

1. With `--page`, native extraction searches that physical PDF page.
2. Without `--page`, native extraction may infer the page only when exactly
   one page contains an unambiguous match.
3. If native extraction cannot reproduce the literal, OCR fallback is allowed
   only when `--page` was supplied.
4. `locate` never silently OCRs every page in a PDF.
5. The successful backend is recorded in the locator.

The command never edits summary YAML. It emits reviewable YAML to stdout or a
stable object through `--json`.

### `receipts check`

```text
receipts check [ID...]
```

With IDs, every requested summary is subject to the complete evidence
contract. Missing summary YAML, missing evidence, incomplete claim coverage,
stale hashes, environment failures, and invalid locators are failures.

Without IDs, `check` validates every summary that already has an `evidence`
section and reports how many legacy summaries were skipped. Evidence becomes
mandatory for a record once it has been added.

The command exits nonzero when any selected evidence record is invalid.

### `receipts audit`

```text
receipts audit [--strict] [ID...]
```

`audit` classifies selected summaries as:

- `valid`: complete evidence, current hashes, and valid dual-source locators;
- `missing`: no evidence section; or
- `invalid`: malformed, incomplete, stale, ambiguous, unmatched, or
  operationally unverifiable evidence.

Missing evidence is reported but does not make the default audit fail, so a
corpus can adopt evidence incrementally. Invalid evidence always fails.
`--strict` also fails when any selected summary is missing evidence.

Human output is concise and grouped by status. `--json` emits stable totals
and per-record diagnostics suitable for scripts.

## YAML Evidence Contract

A summary document carries an optional top-level `evidence` property:

```yaml
evidence:
  markdown:
    source: md/smith-2019/smith-2019.md
    sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

  pdf:
    source: pdfs/smith-2019.pdf
    sha256: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"

  claims:
    - claim: 0
      claim_sha256: "123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0"
      locators:
        - exact: "S1: 445"
          markdown:
            line: 42
            column: 17
            unit: table_cell
          pdf:
            page: 2
            backend: mutool-native
```

Rules:

1. `claim` is the zero-based index into the top-level `claims` array.
2. Every claim index appears exactly once.
3. Every claim evidence entry contains at least one locator.
4. `claim_sha256` is the lowercase SHA-256 digest of the decoded YAML string
   value at `claims[claim]`.
5. Any claim text change invalidates its prior evidence, even if its index
   and locator still exist.
6. `markdown.source` and `pdf.source` are corpus-relative paths.
7. Each source must resolve to a path produced by its configured template in
   `corpus.markdown` or `corpus.pdf`, and must stay inside the corpus root.
8. Each source SHA-256 is the lowercase digest of the raw file bytes.
9. Each locator contains one non-empty normalized exact literal.
10. Markdown lines and columns are one-based.
11. PDF pages are one-based physical PDF pages, not printed page labels.
12. Initial backend values are `mutool-native` and `tesseract-ocr`.
13. Unknown evidence fields are rejected.

The property is optional so a corpus can adopt evidence incrementally. When
present, every field above is required. Targeted `receipts check <id>` always
requires evidence, so a document without it is never a validated document.

Any other top-level field is ignored. `deny_unknown_fields` applies to the
`evidence` block and below, not to the document, so a summary may carry
`authors`, `doi`, `notes`, or anything else alongside the block that
`receipts` owns.

## Exact-Match Semantics

The same normalized `exact` value must pass both sides of a locator:

1. It occurs exactly once inside the selected Markdown semantic unit.
2. It occurs exactly once on the selected physical PDF page through the
   locator's recorded backend.

Duplicate matches fail with an instruction to choose a longer literal. The
first release does not use occurrence numbers, guessed coordinates, or fuzzy
disambiguation. If the rule proves too strict in corpus use, it may be relaxed
only through an explicit schema and design change.

Composite claims use multiple locators. Every claim must have at least one
locator, but the verifier remains responsible for selecting enough locators to
support every material part of a compound claim.

## Markdown Evidence Units

`pulldown-cmark::Parser::into_offset_iter()` supplies events with raw source
byte ranges. `receipts` folds inline events into bounded semantic units:

- headings;
- paragraphs;
- paragraphs inside list items;
- paragraphs inside block quotes;
- individual GFM table cells; and
- fenced or indented code blocks.

Text may cross emphasis, strong text, visible link text, inline code, math, and
soft line breaks inside one unit. It may never cross a paragraph, heading,
list item, block quote, table cell, or code-block boundary.

A Markdown table row is not flattened into one searchable string. A compound
claim supported by several cells uses several cell-local locators. This
prevents the validator from manufacturing evidence by joining unrelated cells
or rows.

The stored line and column identify the beginning of the semantic unit, not
the beginning of the exact substring. Coordinates are derived from the raw
source byte range.

Inline HTML formatting tags contribute no literal text. `<br>` contributes
whitespace. Visible text inside formatting tags remains available. Image and
link destinations are excluded. Raw HTML tables are not interpreted as GFM
tables and cannot satisfy table-cell locators.

## Native PDF Extraction

For `mutool-native`, `receipts` invokes MuPDF structured-text JSON for the requested
page. It retains page, block, line, and geometry information needed to
construct a deterministic searchable page stream.

The normalized literal must occur exactly once on that page. Geometry is
reported by `locate` and in JSON diagnostics for human inspection, but is not
persisted in the YAML or compared during v1. MuPDF coordinates may change
slightly across versions even when the extracted text remains valid.

MuPDF may expose a PDF's fragmented glyph encoding as separate line strings
such as `on-o` plus `ff`, or `67` plus `.` plus `5`. `receipts` does not silently
repair those strings because doing so would exceed whitespace normalization.
When an otherwise valid literal cannot match native text for this reason,
`locate --page` proceeds to the Tesseract backend.

## OCR PDF Extraction

For `tesseract-ocr`, `receipts`:

1. renders only requested physical pages through MuPDF at 300 DPI;
2. detects and corrects orientation in 90-degree increments without modifying
   the PDF;
3. runs Tesseract with the versioned English OCR profile;
4. stores raw TSV plus a manifest in the runtime cache;
5. reconstructs searchable text in TSV page/block/paragraph/line/word order;
   and
6. requires the exact normalized literal to occur exactly once on the page.

Tesseract confidence never determines validity. Only exact recognized text
does. Low confidence is retained for diagnostics.

## Normalization

Markdown units, native PDF page text, OCR page text, and locator literals use
one deterministic normalization function:

1. replace every non-empty run of Unicode whitespace with one ASCII space;
2. treat Markdown soft breaks, hard breaks, and `<br>` as whitespace;
3. trim leading and trailing whitespace; and
4. preserve case, punctuation, numbers, symbols, and all non-whitespace
   Unicode characters exactly.

There is no case folding, Unicode punctuation rewriting, smart-quote
conversion, dehyphenation, ligature expansion, stemming, regular-expression
interpretation, or fuzzy matching.

The YAML stores exact visible-text literals after normalization. It never
stores executable `awk`, `mawk`, `sed`, or regular-expression recipes.

## Persistent Extraction Cache

Extraction artifacts are stored under the resolved cache root: the platform
cache directory by default, or `cache.root` when a corpus prefers to keep them
in-tree.

```text
~/Library/Caches/receipts/pdf-text/     # macOS default
$XDG_CACHE_HOME/receipts/pdf-text/      # Linux default
<corpus-root>/<cache.root>/             # configured override
```

The cache is content-addressed by:

- raw PDF SHA-256;
- backend identifier;
- extraction-profile version;
- relevant `mutool` and `tesseract` versions;
- physical page number;
- language; and
- rendering and recognition settings.

Representative layout:

```text
<cache-root>/
└── <pdf-sha256>/
    └── tesseract-eng-300dpi-v1/
        └── <toolchain-fingerprint>/
            ├── manifest.json
            ├── page-0002.tsv
            └── page-0007.tsv
```

Only pages referenced by OCR-backed locators are rendered and recognized.
When several claims reference the same source and page, `check` and `audit`
group the work and OCR the page once.

A cache entry is reused only when its manifest matches every key component.
A changed source, tool version, or profile creates a new entry rather than
reusing stale output. Missing entries are regenerated atomically. Concurrent
checks cannot expose partial cache files.

Native MuPDF output may use the same cache mechanism, although it is cheap to
regenerate.

The cache is not evidence authority; it can always be regenerated from the
hashed canonical PDF. Because every key component is verified before reuse, a
hit is a determinism guarantee rather than merely a saved subprocess.

## Validation Algorithm

For each selected summary, `receipts`:

1. parses YAML through Librebar;
2. verifies that the filename and `id` agree;
3. verifies evidence shape and complete `claims[]` index coverage;
4. hashes every decoded claim string and compares `claim_sha256`;
5. rejects absolute paths, traversal, locations no configured template can
   produce, and sources that resolve outside the corpus root;
6. hashes raw Markdown and PDF bytes and compares both source digests;
7. parses Markdown once into normalized semantic units;
8. resolves every locator's unit by kind, line, and column;
9. requires one exact normalized match inside that unit;
10. groups PDF work by source, backend, and page;
11. loads or generates extraction artifacts;
12. requires one exact normalized match on the selected page; and
13. reports every failure instead of stopping after the first locator.

`receipts` validates the evidence contract, not the entire summary schema. Any
external schema validation a corpus applies remains authoritative for every
other field.

## Error Behavior

Diagnostics identify:

- summary ID and YAML path;
- claim and locator indexes;
- Markdown and PDF source paths;
- expected Markdown coordinates and unit kind;
- PDF page and backend;
- the precise failure reason; and
- nearby or competing candidates when matching is ambiguous.

Operational failures—such as a missing executable, unreadable source, invalid
cache manifest, or subprocess failure—are distinct from evidence mismatches.
Neither is silently converted into missing evidence or a weaker validation
status.

Normal human or JSON results go to stdout. Diagnostics go to stderr.
`--quiet` suppresses non-error summaries.

No command mutates Markdown, PDFs, or summary YAML. Commands may write only
content-addressed data beneath the resolved cache root.

## Incremental Adoption

An existing corpus is unlikely to have evidence for every summary at once, so
the commands are built to make partial coverage workable without weakening the
contract:

1. Targeted `receipts check <id>` always requires evidence. A document without
   it is never a validated document.
2. Bare `receipts check` validates every evidence-bearing summary and skips
   those without, so the gate can be adopted before coverage is complete.
3. `receipts audit` inventories valid, missing, and invalid records. Missing
   evidence is reported without failing; invalid evidence always fails.
4. `receipts audit --strict` promotes missing evidence to fatal. Once coverage
   is complete, this becomes the corpus-wide gate.

Any record that has evidence is permanently subject to strict checks. Partial
coverage relaxes *which* records are checked, never *how* they are checked.

## Tests and Verification

Unit tests cover:

- Unicode whitespace-run normalization;
- preservation of case, punctuation, numbers, and non-whitespace Unicode;
- prose, headings, lists, block quotes, and code blocks;
- text split across emphasis, links, and line breaks;
- GFM table-cell matches;
- refusal to manufacture matches across cells or rows;
- `<br>` and inline formatting inside table cells;
- claim SHA-256 calculation;
- source SHA-256 calculation;
- missing, duplicate, and out-of-range claim indexes;
- empty locator lists and exact literals;
- wrong unit kinds or coordinates;
- zero and multiple matches;
- path traversal and sources outside canonical roots;
- one-based page and coordinate validation;
- cache-key and manifest validation;
- atomic cache replacement; and
- stable human and JSON diagnostics.

CLI and fixture-backed tests cover:

- `doctor` success and missing-tool failures;
- native PDF success, absence, and ambiguity;
- OCR fallback requiring `--page`;
- a rotated table whose numeric phrase is fragmented in native extraction but
  matches exactly after orientation-corrected Tesseract OCR;
- successful Tesseract cache creation and reuse;
- changed tool/profile versions invalidating cache reuse;
- targeted `check` requiring evidence;
- bare `check` skipping legacy records;
- phased `audit` behavior;
- strict-audit failure; and
- nonzero exit status for every validation or operational failure.

Tests use checked-in MuPDF JSON and Tesseract TSV fixtures for deterministic
behavior. A lightweight local smoke test exercises the installed runtime
tools against a tiny PDF fixture when they are available.

Implementation follows red-green-refactor cycles. Verification is:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
receipts check <id>
receipts audit
git diff --check
```

`receipts` validates the evidence contract only. Any external schema
validation a corpus applies to its other summary fields remains authoritative
for those fields.

## Alternatives and Deferred Integrations

### `open-redact-pdf-text`

This is the strongest pure-Rust candidate examined. It exposes page text,
glyphs, bounding boxes, quads, and search results and may later provide an
independent native-text corroboration backend.

It does not determine pass/fail in v1. Adding it would introduce a substantial
PDF dependency before corpus behavior has been benchmarked. The evidence
schema does not need to change if it is added later.

### `pdf-text-extract`

Rejected as an authoritative backend. Corpus probes showed PDF-version
limitations, missing pages, lost spaces, control characters, poor font
handling, and fragmented table output.

### `ocr` crate

Rejected as an accepted evidence backend in version 0.1.2. Its default pattern
engine produced unusable output on a clean corpus table while reporting high
confidence. Its learned recognizer initializes random weights rather than
loading a trained model, and its own accuracy targets remain unfulfilled.

### MuPDF OCR output

Deferred as an accepted OCR backend. Native MuPDF structured text performed
well, but its Tesseract-backed OCR output was poor on the rotated table used
for evaluation. Direct page rendering plus explicit Tesseract settings gives
`receipts` clearer control and provenance.

### OCRmyPDF

Retained for corpus preparation through `bin/pdf-preflight`, but not used by
`receipts` for page-level validation. Creating a temporary OCR PDF for every
locator is slower and less direct than rendering and recognizing only the
referenced pages.

### `sed`, `mawk`, or regular-expression validation

Rejected because they cannot preserve CommonMark block and GFM table-cell
boundaries safely. The YAML remains easy for scripts to inspect, but `receipts`
owns normalization and structural matching.

### Rumdl AST export

Rumdl uses `pulldown-cmark` internally but does not expose a general AST export
contract suitable for source locators.

### Comrak or `markdown-rs`

Both can represent tables, but `pulldown-cmark` is already trusted by the user
and its offset event stream fits this read-only validator directly.

### Automatic YAML rewriting

Rejected for v1 because it risks broad formatting churn and hides evidence
selection inside a mechanical rewrite. `locate` emits exact, reviewable YAML
fragments instead.

## Decision

**EXTEND.** Build the corpus-specific validator around Librebar,
pulldown-cmark, MuPDF, and Tesseract; do not reimplement Markdown parsing, PDF
text extraction, or OCR.

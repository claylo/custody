# receipts

Deterministic evidence validation for LLM summaries of PDF sources.

`receipts` binds each claim in a summary document to literal text in both the
converted Markdown and the canonical PDF. It is deliberately strict: after
[normalization](#normalization), every locator must occur exactly once inside
one Markdown semantic unit and exactly once on one physical PDF page.
Ambiguity is an error, not an occurrence to choose from.

It does not judge whether a passage logically supports a claim. It proves the
claim has not changed since evidence was selected, and that the cited text is
still present in both sources. The [review tier](#review) records semantic
verdicts separately, bound to both the claim text and the evidence set — the
tool never produces a verdict and never treats one as truth.

## Install

```bash
cargo install --path .
```

## Runtime dependencies

Both external tools are required, not optional:

```bash
brew install mupdf tesseract
receipts doctor
```

Supported runtime versions are MuPDF `>=1.28.0, <1.29.0` and Tesseract
`>=5.5.0, <5.6.0`. `doctor` reports each tool as `missing`, `unsupported`, or
`ok` and fails unless both versions are supported.

Every evidence command runs the same toolchain preflight before reading or
judging a summary. An unavailable or unsupported backend aborts the command as
an infrastructure failure; it is never counted as invalid evidence.

`doctor` reports the resolved corpus, which config file was used, both tool
versions, the extraction profiles, and the cache root. Run it first.

## Configuration

Layout lives in `receipts.yaml`. The directory holding that file is the corpus
root, so the tool makes no assumptions about what else the corpus contains.
Summary discovery skips symlinks and rejects directories deeper than the
configured summary template can match.
OCR render resolution must be between 1 and 1200 DPI.

```yaml
corpus:
  summaries: "summaries/{id}.yaml"
  sources:
    default:
      markdown:
        - "md/{id}.md"
        - "md/{id}/{id}.md"
      pdf: "pdfs/{id}.pdf"
    supplement:
      markdown:
        - "md/{id}-supp.md"
      pdf: "pdfs/{id}-supp.pdf"

cache:
  root: null

pdf:
  tools:
    mutool: null
    tesseract: null
  ocr:
    enabled: true
    dpi: 300
    lang: eng
    page_segmentation_mode: 3

coverage:
  tokens: error    # error | warn | off

sections:
  weak: ["Limitations", "Future Work", "Related Work"]

normalize:
  dehyphenate: true
  quotes: true
```

Set `pdf.tools.mutool` or `pdf.tools.tesseract` to an absolute executable path
to pin a tool explicitly. A null path is resolved from `PATH` once at startup;
`doctor` reports the resulting canonical path alongside the version.

The bare `corpus.markdown` / `corpus.pdf` shorthand is still accepted and
desugars into a single source named `default`.

Every template is relative to the corpus root and must contain `{id}`. Absolute
paths and `..` segments are rejected, so a config file cannot direct reads
outside the corpus.

`cache.root` must be relative, may not contain `..`, and must resolve within the
corpus root; existing symlinks are checked before the directory is created. The
resulting corpus-local cache is write-only by default because corpus content is
not trusted evidence. Pass `--trust-cache` to reuse entries from that directory
only when you trust the checkout; the opt-in is deliberately a CLI flag, not
project configuration.

Summary and converted-Markdown inputs must resolve to regular files inside the
corpus and may be at most 64 MiB each. The same boundary applies to `locate`,
`check`, `audit`, and `propose`.

Discovery walks up from the working directory, checking `.config/receipts.yaml`,
`.receipts.yaml`, then `receipts.yaml` in each ancestor, stopping at a `.git`
boundary. TOML and JSON are also accepted. If nothing is found, the corpus root
falls back to the nearest `.git` boundary, then to the working directory, and
`doctor` reports `config: ok (defaults)`.

These are real defaults: a corpus laid out as `summaries/`, `md/`, and `pdfs/`
inside a Git repository needs no config file at all. Use `--config FILE` to name
one explicitly.

Configuration is repository-scoped and deterministic: user-level config files
and `RECEIPTS_*` environment variables are intentionally ignored. Only built-in
defaults, project discovery, and an explicit `--config` file participate; there
are no hidden machine-wide layers.

### Coverage

`coverage.tokens` controls whether material-token checking is enforced:

- `error` (default) — required tokens (numbers, number words, quoted phrases)
  from each claim must appear in at least one locator.
- `warn` — uncovered tokens produce warnings, not errors.
- `off` — disables required-token enforcement.

### Weak sections

`sections.weak` lists heading substrings that mark evidence as weak. If *every*
locator for a claim sits under a weak section (e.g., "Limitations"), a
`weak_section_only` warning fires. This is permanently a warning — heading
hierarchy from converters is too unreliable to gate on.

### Normalization

Every literal, Markdown unit, and PDF page passes through the same
normalization before comparison, so a locator only has to match what both
sides become. Whitespace collapsing is always on; the other two rules are
configured under `normalize:` and default to on.

- **whitespace** — every run of Unicode whitespace becomes one ASCII space.
- **dehyphenate** — a hyphen at the end of a line followed by a line starting
  with a lowercase letter is a typesetter's break inside a word; the hyphen and
  the break are removed (`attach-` / `ment` → `attachment`). Only whitespace
  runs that contain a line break qualify, so `self- report` on one line is
  untouched, and `Main-` / `Hesse` keeps its hyphen because the next line
  starts with a capital.
- **quotes** — `‘ ’ ‚ ‛` fold to `'` and `“ ” „ ‟` fold to `"`. PDF text
  layers carry the typographic forms and most converters emit the straight
  ones; without folding, any literal containing an apostrophe fails on one
  side.

Nothing else changes: ligatures, dashes, minus signs, superscripts, and OCR
errors are compared as they are. Normalization rules are a property of the
corpus and are read once at startup; `doctor` reports the active set.

## Run

```bash
receipts doctor
receipts locate ID --claim 0 --exact "literal present in both sources" --page 3
receipts check [ID...]
receipts check --require-review [ID...]
receipts audit [--strict] [ID...]
receipts propose [ID...] [--all] [--candidates N]
receipts extract ID [--source NAME] [--write [--force]]
receipts schema
receipts completions SHELL
```

Report writes are fallible. If a downstream reader closes stdout early (for
example, `receipts audit --format json | head -1`), receipts exits cleanly
without creating a crash report.

`locate` and `propose` default to their human, YAML-ready text even when stdout
is redirected; use `--format json` explicitly for structured output. Status
commands retain the normal terminal-aware `auto` behavior.

### doctor

Probes corpus paths, external tools, extraction profiles, and the cache root.
Non-zero exit if any requirement is unavailable.

### locate

Chooses a pulldown-cmark semantic unit and validates the exact literal against
MuPDF native structured text. If native text has no match and `--page` was
supplied, it renders that physical page at the configured DPI, detects
orientation, and tries Tesseract OCR. Native ambiguity is an error and never
triggers OCR fallback.

`locate --format json` also reports the bounding box of the matched native-text
lines or OCR words, and mean OCR confidence when the backend provides them. The
default YAML-ready output keeps those diagnostics in a comment so they are not
persisted in the strict evidence contract. If a backend returns matching text
without coordinates, the diagnostic says `no geometry available for this
backend` instead of silently omitting the reason.

MuPDF structured-text output must contain the documented page and block
containers, and every text block must contain its line list. Image-only blocks
remain valid so native misses can proceed to OCR fallback.

`--source NAME` selects which named source pair to resolve against. Defaults to
`default`.

### check

Validates evidence structure, source hashes, Markdown unit resolution, PDF page
matching, token coverage, section paths, and review staleness. Non-zero exit on
any error.

`--require-review` gates on missing review entries and non-`supported` verdicts.
Without the flag, reviews are checked for staleness but verdicts are not
enforced.

### audit

Inventories every summary as valid, missing, or invalid. Non-zero exit on
invalid evidence; `--strict` makes missing evidence fatal too.
`--require-review` adds review gating to the validation pass.

### propose

Emits ranked candidate locators for claims that lack evidence, or for every
claim with `--all`. The algorithm is deterministic: no model, no randomness,
stable ordering. `propose` never writes to any file.

`--candidates N` (default 3) limits output per claim. Human-readable output is
commented YAML safe to paste and edit, with a suggested `receipts locate`
command to commit the top candidate. PDF verification examines at most `8 × N`
previously unseen spans per claim and reuses the result when another claim
scores the same source text.

JSON output always uses `{"summaries": [...]}`, including targeted runs and
empty corpora, so consumers do not need a count-dependent parser.

Multi-summary runs continue past malformed summaries and filename/ID
mismatches. Aggregate JSON keeps the successful proposals alongside per-ID
issues; human output writes those issues to stderr. Either mode exits non-zero
after the batch if any summary failed.

### extract

Emits Markdown built from the PDF's own native text layer, through the same
MuPDF structured-text pass and the same normalization that `locate` and
`check` use. A literal copied from this Markdown therefore agrees with the PDF
by construction: no converter re-reading the page, no quote flattening, no
dropped signs.

The output is YAML frontmatter (`id`, `source_format: pdf-native`, the PDF's
corpus-relative path and SHA-256, the extractor and profile, the active
normalization rules), then one `## Page N` heading per physical page and one
paragraph per MuPDF text block. Every page gets a heading, even an empty one,
so a locator's section path always names the physical page. There are no
tables and no other headings; keep a converter's Markdown as a separate source
for table cells.

Without `--write` the Markdown goes to stdout regardless of `--format`. With
`--write` it is written to the first Markdown template of `--source`, creating
parent directories, and the report (path, pages, paragraphs, SHA-256) follows
the usual text/JSON rules. An existing file is never replaced without
`--force`. The usual pattern is a dedicated source:

```yaml
corpus:
  sources:
    default:
      markdown: ["md/{id}/{id}.md"]     # converter output, for tables
      pdf: "pdfs/{id}.pdf"
    native:
      markdown: ["native-md/{id}.md"]   # receipts extract --source native --write
      pdf: "pdfs/{id}.pdf"
```

Only born-digital pages produce useful text. A scanned page yields whatever
OCR layer the PDF already carries, or nothing.

### schema

Prints a machine-readable CLI Spec v0.2 JSON document describing every
subcommand, flag, type, default, output field, and error code. This is the
primary interface for agents integrating with `receipts` programmatically.

`receipts schema propose` narrows to one command.

### completions

Generates shell completions: `receipts completions zsh > _receipts`.

## Evidence contract

Summary IDs must be at least three characters and contain only lowercase ASCII
letters, digits, and interior hyphens. Hyphens cannot be the first or last
character.

```yaml
id: smith-2019
claims:
  - "Transport remained laminar across all three test regimes."
evidence:
  sources:
    default:
      markdown: {source: "md/smith-2019/smith-2019.md", sha256: "..."}
      pdf:      {source: "pdfs/smith-2019.pdf", sha256: "..."}
    supplement:
      markdown: {source: "md/smith-2019-supp.md", sha256: "..."}
      pdf:      {source: "pdfs/smith-2019-supp.pdf", sha256: "..."}
  claims:
    - claim: 0
      claim_sha256: "..."
      locators:
        - exact: "no measurable turbulent mixing was observed"
          markdown:
            line: 12
            column: 1
            unit: paragraph
            section: ["Results", "Onset"]
          pdf:
            page: 3
            backend: mutool-native
        - source: supplement
          exact: "42.8% within tolerance"
          markdown:
            line: 4
            column: 1
            unit: table_cell
            section: ["Appendix B", "Table B2"]
          pdf:
            page: 2
            backend: tesseract-ocr
```

The bare `evidence.markdown` / `evidence.pdf` shorthand (no `sources:` map) is
still accepted and desugars into a single source named `default`. `source` on a
locator defaults to `default` and may be omitted.

Claim indexes are zero-based; Markdown lines, character columns (counted as
Unicode scalar values), and physical PDF pages are one-based. Every claim must
have one entry and at least one locator. Use several locators when a single
literal does not support every material assertion in the claim.

`section` records the heading path the locator sits under. It is verified
against the live document — a heading change produces a `stale_section` error.

Unknown fields are rejected inside `evidence` and below, but not at the document
level, so a summary may carry its own metadata — `authors`, `doi`, `notes` —
alongside the block `receipts` owns.

The `evidence` property is optional, so a corpus can adopt evidence
incrementally. Targeted `receipts check ID` always requires it, so a document
without evidence is never a validated document. Bare `receipts check` validates
every evidence-bearing summary and skips the rest. `audit` reports missing
evidence without failing; `audit --strict` makes it fatal.

Only `receipts` should produce normalized literals, SHA-256 values, coordinates,
pages, and backend names. Normalization is the fixed set of rules described
under [Normalization](#normalization); the corpus configuration decides which
optional rules are on, and a record validates only under the rules it was made
with.

## Review

Semantic judgment, recorded separately from proof and never confused with it:

```yaml
review:
  claims:
    - claim: 0
      claim_sha256: "..."
      evidence_sha256: "..."
      verdict: supported
      reviewer: "claude-opus-5"
      note: "Locator [a] covers the mechanism; [b] covers the regime count."
      at: "2026-08-10"
```

Four verdicts: `supported`, `partial`, `unsupported`, `unclear`.

Each review entry is bound to both the claim text (`claim_sha256`) and the
evidence set (`evidence_sha256`). `evidence_sha256` is the SHA-256 of the
canonical JSON serialization of that claim's `locators` array, with object keys
sorted lexicographically and `exact` values normalized. Array order is
significant: reordering locators invalidates the review.

Edit the claim text → `stale_review_claim`. Change a locator → `stale_review_evidence`. Both are errors that fire during `check` and `audit` without any flags.

`--require-review` makes missing reviews (`missing_review`) and non-`supported`
verdicts (`unsupported_verdict`) fatal.

The tool never produces a review and never treats `verdict` as truth.

## Vocabulary

`claim` is a deliberate default, not an assumption: a claim is definitionally an
assertion that requires support, which is exactly what this tool checks. If your
field words it differently, say so:

```yaml
terms:
  claim: proposition
  claims: propositions
```

Documents then use your vocabulary throughout — the document key, the nested
`evidence` key, the `review` key, the entry index, and the `_sha256` suffix:

```yaml
id: smith-2019
propositions:
  - "Transport remained laminar across all three test regimes."
evidence:
  propositions:
    - proposition: 0
      proposition_sha256: "..."
      locators: [...]
review:
  propositions:
    - proposition: 0
      proposition_sha256: "..."
      evidence_sha256: "..."
      verdict: supported
      reviewer: "..."
```

Both forms are explicit because English pluralization is unreliable — `thesis`
and `theses` would defeat any rule worth writing. Human-readable messages follow
your vocabulary; `locate` emits it too.

Two things stay fixed regardless. **Error codes** are vocabulary-free
(`missing_evidence_entry`, `stale_hash`, `entry_out_of_range`,
`duplicate_entry`), so a script consuming `--format json` is portable across
corpora. Source-validation issues also keep stable `source_*` codes and carry
the configured source name in a separate `source` field. `receipts schema`
lists every declared error code from the same registry used to construct
runtime issues. And **`--claim N` keeps its name**, because it takes an index
rather than the word: the command is identical whichever vocabulary a document
uses, and a configurable flag would fragment every example and shell script.

## Cache

Tesseract TSV is reusable runtime data, content-addressed by the PDF digest,
backend, fixed OCR settings and command templates, canonical MuPDF and
Tesseract executable paths and versions, and physical page. The manifest also
records the applied render rotation.

```text
<cache-root>/PDF_SHA256/tesseract-eng-300dpi-v2/TOOLCHAIN_SHA256/page-NNNN/
```

The root defaults to the platform cache directory
(`~/Library/Caches/receipts/pdf-text` on macOS). Set `cache.root` to keep
artifacts beside the corpus instead; a relative path resolves against the corpus
root. Corpus-local entries are written but never read unless the operator passes
`--trust-cache`.

A trusted manifest that disagrees with any key component is a miss rather than a
stale hit. The cache is only a subprocess optimization: repository-controlled
entries are inert by default, and every entry can be regenerated from the hashed
PDF.

## Development

```bash
just check   # fmt, clippy, deny, test, doc-test, doc
just test    # test suite
just ci      # alias for check
just doctor  # probe tools
```

## License

Apache-2.0 OR MIT.

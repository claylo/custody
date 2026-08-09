# Multi-Source Evidence and Support Checking Design

**Date:** 2026-07-30
**Status:** Approved, not yet implemented
**Extends:** `2026-07-28-evidence-validation-design.md`

## Purpose

`receipts` proves that a claim's cited literal is present, unambiguously, in both
a converted Markdown file and a canonical PDF page. Three gaps limit how far that
guarantee reaches.

**One source per summary.** `Evidence` carries exactly one `markdown` and one
`pdf` record, so a claim resting on a paper plus its supplement, or a chapter
split across files, cannot be expressed at all.

**Every locator is hand-found.** A human or agent must choose each literal and
know which physical page carries it. At corpus scale that is the dominant cost,
and nothing helps: the tool validates a decision it never assists in making.

**Presence is not support.** A claim can cite a literal that genuinely appears on
the page and still misrepresent it. Quote-mining passes every current check.
Taking a number out of a Limitations paragraph and presenting it as a finding
passes every current check.

This design closes the first two and narrows the third, without putting a model
in the verification path. That constraint is not incidental — mechanical
verifiability is the only property that distinguishes `receipts` from
LLM-judge groundedness scoring, and no feature here is worth trading it for.

## Non-Goals

- Semantic entailment checking. `receipts` will not decide whether a passage
  logically supports a claim. Phase 4 makes that judgment *recordable and
  staleness-tracked*, not automated.
- Rewriting summary YAML. `propose` emits candidates; only `locate` writes, and
  only for one claim at a time.
- Fuzzy, case-insensitive, or punctuation-insensitive matching, anywhere.
- Inferring source relationships. A summary declares its sources; the tool never
  guesses that a file is a supplement.

## Sequencing

Phase order is dictated by dependencies, not by importance:

| Phase | Adds | Depends on |
|---|---|---|
| 0 | Honor the existing `pdf.ocr` config (bug fix) | — |
| 1 | Named Markdown+PDF source pairs | — |
| 2 | Material token coverage, section paths | 1 |
| 3 | `propose` | 1, 2 (shares the token extractor) |
| 4 | Review tier | 1 |
| 5 | Arbitrary OCR backends (deferrable) | 0 |

Phase 3 must follow Phase 2 because both consume one material-token extractor.
Defining it twice would let the proposer and the validator disagree about what a
claim asserts, which is the worst possible place for a discrepancy.

Phase 0 is a bug fix and is not deferrable. Phase 5 may be cut or deferred
without affecting anything else.

---

## Phase 0: Honor the Existing `pdf.ocr` Config

`config.rs` defines and *validates* `pdf.ocr.enabled`, `pdf.ocr.dpi`, and
`pdf.ocr.lang` — rejecting a zero `dpi` and an empty `lang`. Nothing reads any of
them. `src/pdf/tesseract.rs` hardcodes:

```rust
pub const PROFILE_NAME: &str = "tesseract-eng-300dpi-v1";
const OCR_DPI: u16 = 300;
const OCR_PSM: u8 = 3;
```

So `dpi: 600` in `receipts.yaml` silently does nothing, and `enabled: false` does
not prevent OCR fallback. A configuration file that states something the program
ignores is worse than one that omits the setting, and it is a particularly bad
failure in a tool whose premise is that recorded facts are verified rather than
trusted.

### Changes

- Thread `OcrConfig` from `Corpus` into `PdfTools`, so `build_profile` uses the
  configured `dpi` and `lang` instead of constants.
- Derive the profile name from its settings — `tesseract-{lang}-{dpi}dpi-v1` —
  rather than hardcoding a string that can disagree with them.
- Honor `enabled: false` by refusing OCR fallback outright: a locator that would
  require OCR fails with `ocr_disabled` rather than silently succeeding through a
  path the corpus turned off.
- Expose `psm` as `pdf.ocr.page_segmentation_mode` (default 3), since it is
  already a field of `OcrProfile` and already part of the cache key.

### Cache correctness

Changing `dpi` must not reuse cached output produced at another resolution. It
already cannot: `OcrProfile` carries `dpi` and `language`, `cache_key` hashes the
whole profile into `toolchain_sha256`, and `entry_dir` includes that hash as a
path component. A differing setting is therefore already a cache miss.

Deriving the profile *name* from the settings is nevertheless required, because
otherwise a 600-DPI entry sits in a directory literally named `…-300dpi-v1`. The
cache would be correct and the filesystem would be lying.

### New error code

| Code | Severity | Meaning |
|---|---|---|
| `ocr_disabled` | error | a locator names an OCR backend but `pdf.ocr.enabled` is false |

---

## Phase 1: Named Source Pairs

### Schema

`evidence.sources` is a map from a source name to a Markdown+PDF pair:

```yaml
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
          markdown: {line: 31, column: 1, unit: paragraph, section: ["Results"]}
          pdf:      {page: 3, backend: mutool-native}
        - source: supplement
          exact: "42.8% within tolerance"
          markdown: {line: 4, column: 1, unit: table_cell,
                     section: ["Appendix B", "Table B2"]}
          pdf:      {page: 2, backend: tesseract-ocr}
```

The pair is the atomic unit. A locator cites one *pair*, never a Markdown from
one source and a PDF from another, because the dual-source guarantee depends on
the Markdown having been produced from that specific PDF. Decoupling them would
allow a locator to satisfy both halves against unrelated files.

`source` is optional on a locator and defaults to `default`.

Source names are validated as cache-safe path components: non-empty, ASCII
lowercase alphanumeric plus `-` and `_`. They appear in error messages and in
cache paths.

### Desugaring

A bare `markdown`/`pdf` pair remains valid and means a single source named
`default`:

```yaml
evidence:
  markdown: {source: "...", sha256: "..."}
  pdf:      {source: "...", sha256: "..."}
```

Normalization happens at the `serde_json::Value` boundary in `parse_summary`,
immediately after `terms.canonicalize` — the same seam, for the same reason:
reshape the author's dialect into the canonical form before any typed struct
sees it, so no downstream code knows more than one shape exists.

Order is fixed: vocabulary canonicalization first, then source desugaring. They
are independent (source names are not vocabulary-configurable), but a fixed
order keeps error messages predictable.

Declaring both forms is `conflicting_source_form`, consistent with how a document
using two vocabularies is already rejected rather than silently resolved.

### Corpus resolution

Config declares each source role explicitly. There is no wildcard: a corpus has
a small known set of roles, and requiring them to be named keeps the
template-match guarantee intact and the config free of magic.

```yaml
corpus:
  summaries: "summaries/{id}.yaml"
  sources:
    default:
      markdown: ["md/{id}.md", "md/{id}/{id}.md"]
      pdf: "pdfs/{id}.pdf"
    supplement:
      markdown: ["md/{id}-supp.md"]
      pdf: "pdfs/{id}-supp.pdf"
```

Bare `corpus.markdown`/`corpus.pdf` desugars to `corpus.sources.default`, so an
existing config keeps working unchanged.

A summary citing a source name with no configured templates is
`unknown_source_template`. That is a useful error: the document claims a role
this corpus does not define.

Every template keeps the existing constraints — relative, contains `{id}`, no
`..` segments, no absolute paths — because they are what stop a config file from
directing reads outside the corpus root.

### Validation

`validate_document` gains a source dimension. Work is done once per source, not
once per locator:

1. Resolve each declared source's expected paths from its configured templates.
2. Verify each recorded `source` string matches a template-produced path, stays
   inside the corpus root (including through symlinks), and hashes correctly.
3. Parse each Markdown source into units exactly once, keyed by source name.
4. Group PDF work by `(source, backend, page)` rather than `(backend, page)`.
5. Resolve each locator against its own source's units and PDF.

An `unused_source` — declared but cited by no locator — is an error. It is the
same class of inconsistency as a duplicate claim entry: a record asserting
something the evidence does not use.

### New error codes

| Code | Meaning |
|---|---|
| `empty_sources` | `sources` is present but empty |
| `unknown_source` | a locator names a source not declared in `sources` |
| `unused_source` | a declared source is cited by no locator |
| `conflicting_source_form` | both bare `markdown`/`pdf` and `sources` present |
| `unknown_source_template` | no configured templates for a declared source name |
| `invalid_source_name` | source name is not a safe path component |

---

## Phase 2: Material Tokens and Section Paths

### Material token coverage

A claim's locators must collectively account for what the claim asserts. Tokens
are extracted from the claim text and split into two tiers, which is what makes
the check strict where it matters without drowning in proper-noun noise.

**Required tokens** must each appear in at least one of that claim's locators:

- **Numeric literals.** A maximal run beginning with an ASCII digit and
  containing digits, `.`, or `,`; trailing `.` and `,` trimmed; an immediately
  following `%` included. So `67.5%`, `1,228`, `0.05`, `3`.
- **Number words.** Cardinals `zero` through `twenty`, the tens `thirty` through
  `ninety`, and the scale words `hundred`, `thousand`, `million`, `billion`.
  Matched case-insensitively. "three cohorts" versus "two cohorts" is exactly
  the distortion this exists to catch, and digits alone would miss it.
- **Quoted phrases.** Text inside paired `"` or `'` in the claim, which must
  appear verbatim after normalization.

**Advisory tokens** are reported but never enforced:

- **Capitalized multiword terms.** Two or more consecutive capitalized words not
  at the start of a sentence. Useful signal, too noisy to gate on.

Advisory tokens appear in `propose` output and in the `check --json` payload as
an `advisory_tokens` field. They never produce an issue and never affect exit
status. `coverage.tokens: off` disables required-token enforcement but leaves
advisory reporting intact, since reporting was never a gate.

### Coverage matching

A required token is covered when it appears in the normalized `exact` of at
least one locator belonging to that claim, delimited on both sides by a
non-ASCII-alphanumeric character or a string edge.

The boundary rule is load-bearing: plain substring matching would let the
locator `"13 regimes"` satisfy the claim token `3`.

Number words match case-insensitively. Numeric literals and quoted phrases match
exactly.

An uncovered required token is `uncovered_token`, one issue per token, naming the
token and the claim.

```yaml
coverage:
  tokens: error    # error | warn | off
```

Default `error`. This is a new check over existing records and will fail
documents that pass today; `warn` exists as a migration setting, not as the
recommended posture.

### Section paths

`MarkdownUnit` gains `section: Vec<String>` — its ancestor headings' texts,
outermost first, each passed through the same `normalize()` used for locator
literals so a heading wrapped across lines yields a stable single-spaced string.

`pulldown-cmark` emits a flat event stream with no ancestor tracking, so the path
is derived state maintained during the existing single pass, alongside
`item_depth`, `blockquote_depth`, and `html_block_depth`. Two implementation
notes:

- `Tag::Heading { level, .. }` currently discards `level`; it is needed.
- A heading's text arrives as `Text` events between Start and End, which is why
  headings are already units. The stack updates when a `Heading` unit completes:
  pop entries at level ≥ this heading's, then push `(level, text)`. A heading's
  own path is therefore its ancestors, excluding itself.

There is no PDF equivalent. `mutool`'s stext output carries geometry, not
document structure, so `section` belongs to the Markdown half of a locator only.

`MarkdownLocator` gains `section`, recorded and verified:

```yaml
markdown: {line: 31, column: 1, unit: paragraph, section: ["Results", "Onset"]}
```

Recording and then re-verifying matches how the tool already treats `unit`,
`line`, `column`, and every digest: nothing is trusted because it was written
down; it is written down so that disagreement becomes an error. A recomputed path
that differs from the record is `stale_section`.

Storing it also makes provenance visible in a code-review diff, and improves
diagnostics — `no match in Results > Onset at md:31:1` beats
`no match at md:31:1`.

### Section rules

```yaml
sections:
  weak: ["Limitations", "Future Work", "Related Work"]
```

Matched case-insensitively as a substring against any element of a locator's
section path, because real converter output is inconsistent — `5. Limitations`,
`Limitations and Future Work`, or a Methods section nested under nothing.

`weak_section_only` fires when *every* locator for a claim sits under a weak
section: a finding whose sole support is the limitations paragraph.

Section rules are **warnings**, permanently. The recorded path is strictly
verified, but heading hierarchy from Marker or Datalab is too unreliable to gate
on. Conflating "this path is wrong" with "this path looks suspicious" would
teach users to ignore both.

### New error codes

| Code | Severity | Meaning |
|---|---|---|
| `uncovered_token` | per `coverage.tokens` | a required claim token appears in no locator |
| `stale_section` | error | recorded section path disagrees with the source |
| `weak_section_only` | warning | every locator for a claim is under a weak section |

---

## Phase 3: `propose`

```
receipts propose [ID...] [--all] [--candidates N] [--json]
```

Emits ranked candidate locators for claims that lack evidence, or for every
claim with `--all`. Turns N authoring decisions into N reviews.

### Algorithm

Deterministic end to end. No model, no randomness, stable ordering.

1. Load the summary, resolve every source, parse each Markdown once.
2. Extract required tokens from the claim, using the Phase 2 extractor.
3. Build candidate spans: each sentence within each unit, plus the whole unit
   text as a fallback. Sentences split on terminal punctuation followed by
   whitespace. This split is deliberately naive and will mis-handle `et al.`,
   `Fig. 3`, and decimal-adjacent abbreviations — acceptable because every
   candidate is verified against both sources before being offered, and reviewed
   by a human or agent before being committed. A bad split yields a candidate
   that fails verification, not a bad locator.
4. Score each candidate by the count of distinct required tokens it contains.
5. Discard candidates that do not occur exactly once within their own unit.
6. Run one native extraction pass per PDF and locate the candidate. Offer it
   only when it occurs on exactly one page. Zero matches and multiple matches are
   both reported and never proposed — ambiguity is an error here for the same
   reason it is an error in `locate`.
7. Rank by token count descending, then span length ascending (a shorter span is
   tighter evidence), then source name, line, column. Fully ordered, so output
   is reproducible.
8. Emit the top `--candidates N` (default 3).

### OCR

`propose` does not run OCR. A candidate with no native match is reported as such,
with a suggestion to use `locate --page N`.

Page-scoped OCR is deliberate, and speculatively OCRing pages to improve
proposals would fight that. It also keeps `propose` useful to anyone whose OCR is
better than Tesseract — see Phase 5.

### Output

Human-readable form is commented YAML, safe to paste and edit:

```
# claim 0  "Transport remained laminar across all three test regimes."
#   required tokens: three
#   [a] 1/1  default  md:31:1 paragraph [Results > Onset]  p.3 mutool-native
#       "no measurable turbulent mixing was observed"
#   [b] 1/1  default  md:12:1 paragraph [Introduction]  p.1 mutool-native
#       "across all three regimes tested"
#
#   accept [a]:
#     receipts locate smith-2019 --claim 0 \
#       --exact "no measurable turbulent mixing was observed" --page 3
```

`--json` emits the same data structurally, and is the mode that matters: agents
drive this loop, humans review its output.

```json
{"id": "smith-2019",
 "claims": [{"claim": 0,
             "required_tokens": ["three"],
             "uncovered_tokens": [],
             "candidates": [{"source": "default",
                             "exact": "no measurable turbulent mixing was observed",
                             "coverage": {"matched": 1, "required": 1},
                             "markdown": {"line": 31, "column": 1,
                                          "unit": "paragraph",
                                          "section": ["Results", "Onset"]},
                             "pdf": {"page": 3, "backend": "mutool-native"}}]}]}
```

### `propose` never writes

It emits candidates. `locate` commits one, for one claim, after a human or agent
accepts it. Automating acceptance would automate away the reviewed decision that
makes the evidence worth trusting — the tool would then be asserting its own
conclusions.

---

## Phase 4: Review Tier

Semantic judgment, recorded separately from proof and never confused with it.

```yaml
review:
  claims:
    - claim: 0
      claim_sha256: "..."
      evidence_sha256: "..."
      verdict: supported        # supported | partial | unsupported | unclear
      reviewer: "claude-opus-5"
      note: "Locator [a] covers the mechanism; [b] covers the regime count."
      at: "2026-07-30"
```

### Two bindings

`claim_sha256` binds the review to the claim text, exactly as evidence does. Edit
the claim and the review is `stale_review_claim`.

`evidence_sha256` binds the review to *what was reviewed*: the SHA-256 of the
canonical JSON serialization of that claim's `locators` array, including every
field of every locator — `source`, `exact`, and the full Markdown and PDF
coordinates — with object keys sorted lexicographically and `exact` values
normalized. Array order is significant, so reordering locators invalidates the
review; two orderings are two different sets of evidence to have read, and
treating them as equivalent would require deciding that order carries no meaning.

Swap a locator for a weaker one and the review silently stops describing the
evidence it examined — that becomes `stale_review_evidence`.

The second binding is the point of this phase. A review that survives a change
to its own evidence is worse than no review, because it looks like diligence.

### What `receipts` does and does not do

It records, hashes, and staleness-checks. It never produces a review, and never
treats `verdict` as truth — `verdict: supported` is an assertion by whoever is
named in `reviewer`, and the tool's only contribution is proving that assertion
still applies to the current claim and the current evidence.

`check` does not fail on `unsupported`. `--require-review` makes missing, stale,
or non-`supported` verdicts fatal, for corpora that want review as a gate.
`audit` gains review columns alongside valid/missing/invalid.

### New error codes

| Code | Severity | Meaning |
|---|---|---|
| `stale_review_claim` | error | claim text changed since review |
| `stale_review_evidence` | error | locator set changed since review |
| `unknown_review_claim` | error | review entry indexes a nonexistent claim |
| `duplicate_review_claim` | error | two review entries for one claim |
| `missing_review` | only under `--require-review` | claim has no review entry |
| `unsupported_verdict` | only under `--require-review` | verdict is not `supported` |

---

## Phase 5: Arbitrary OCR Backends (deferrable)

Builds on Phase 0, independent of Phases 1–4, and safe to cut.

`OcrEngine` is already a trait and `OcrProfile` already fingerprints command
templates and tool versions, so the extraction seam exists. What does not exist
is a way to *record* a different backend: `PdfBackend` is a closed enum of
`mutool-native` and `tesseract-ocr`, so a corpus using better OCR cannot write
down what it used.

### Changes

`PdfBackend` becomes a validated name rather than an enum, using the charset
already required of cache path components — it is used as one today. This is
backward compatible at the YAML level: `mutool-native` and `tesseract-ocr` are
already serialized as those strings, so existing evidence records parse
unchanged. Only the Rust type changes.

The OCR tool is declared in config:

```yaml
pdf:
  ocr:
    enabled: true
    backend: surya-ocr
    dpi: 300
    lang: eng
    render_command: "mutool draw -r {dpi} -o {out} {pdf} {page}"
    version_command: "surya_ocr --version"
    recognize_command: "surya_ocr {image} --output-format tsv"
```

Recognition output must be Tesseract-compatible TSV: a header row naming at
least `text` and `conf`, one row per word. A backend that cannot produce it needs
an adapter, which keeps the parsing contract single.

Determinism is unaffected. A different tool yields a different profile
fingerprint and therefore a different cache namespace, which is already how
invalidation works — the profile hash covers the command templates and the
version output, so changing either is a cache miss rather than a stale hit.

### Trust boundary

Config-supplied command templates execute programs. This is the same trust level
as a `justfile` or a Cargo build script: anyone who can write the config can
already run code in that context. It is worth stating plainly rather than
leaving implicit, and it argues against ever reading `receipts.yaml` from an
untrusted corpus.

---

## Complete Error Code Table

Existing codes are unchanged. Vocabulary-free naming continues, so `--json`
consumers stay portable across corpora.

| Code | Phase | Severity |
|---|---|---|
| `ocr_disabled` | 0 | error |
| `empty_sources` | 1 | error |
| `unknown_source` | 1 | error |
| `unused_source` | 1 | error |
| `conflicting_source_form` | 1 | error |
| `unknown_source_template` | 1 | error |
| `invalid_source_name` | 1 | error |
| `uncovered_token` | 2 | configurable, default error |
| `stale_section` | 2 | error |
| `weak_section_only` | 2 | warning |
| `stale_review_claim` | 4 | error |
| `stale_review_evidence` | 4 | error |
| `unknown_review_claim` | 4 | error |
| `duplicate_review_claim` | 4 | error |
| `missing_review` | 4 | error under `--require-review` |
| `unsupported_verdict` | 4 | error under `--require-review` |

Warnings are reported and do not affect exit status. `--quiet` suppresses
successful human-readable output without suppressing warnings or errors.

## Tests and Verification

Beyond the existing suite:

- configured `dpi` and `lang` reaching the OCR profile, and the profile name
  deriving from them rather than staying `tesseract-eng-300dpi-v1`
- a changed `dpi` producing a cache miss rather than reusing another resolution
- `enabled: false` refusing OCR fallback with `ocr_disabled` rather than
  succeeding
- desugaring a bare pair into `sources.default`, and rejecting both forms at once
- a locator citing a named non-default source, end to end through `check`
- `unknown_source`, `unused_source`, `unknown_source_template`
- Markdown parsed once per source rather than once per locator
- PDF work grouped by `(source, backend, page)`
- token extraction: decimals, thousands separators, percentages, number words,
  quoted phrases, and the boundary rule that stops `3` matching `13`
- coverage failing when a claim's number appears in no locator
- `coverage.tokens` set to `warn` and `off`
- section paths for nested headings, sibling headings, a heading's own path
  excluding itself, and documents with no headings at all
- `stale_section` when the Markdown is edited to move a unit
- `weak_section_only` firing only when every locator is weak, not merely one
- `propose` ranking stability: identical input yields byte-identical output
- `propose` declining candidates with zero or multiple PDF page matches
- `propose` never modifying any file, asserted by hashing the corpus before and
  after
- `evidence_sha256` changing when a locator is swapped, added, or reordered
- `--require-review` gating on missing, stale, and non-`supported` verdicts

Verification remains:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
receipts check <id>
receipts audit
```

## Decision

Fix Phase 0 first: it is small, and leaving configuration that the program
ignores contradicts the premise the rest of this design rests on. Then implement
Phases 1 through 4 in dependency order, as separate plans. Phase 5 is recorded
here and deferred.

Version stays `0.1.0`. Nothing is published yet, so the schema changes land
without a migration story beyond the desugaring that keeps existing single-source
documents valid.

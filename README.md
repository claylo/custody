# receipts

Deterministic evidence validation for LLM summaries of PDF sources.

`receipts` binds each claim in a summary document to literal text in both the
converted Markdown and the canonical PDF. It is deliberately strict: after
Unicode whitespace-run normalization, every locator must occur exactly once
inside one Markdown semantic unit and exactly once on one physical PDF page.
Ambiguity is an error, not an occurrence to choose from.

It does not judge whether a passage logically supports a claim. It proves the
claim has not changed since evidence was selected, and that the cited text is
still present in both sources.

## Install

```bash
cargo install receipts
```

## Runtime dependencies

Both external tools are required, not optional:

```bash
brew install mupdf tesseract
receipts doctor
```

`doctor` reports the resolved corpus, which config file was used, both tool
versions, the extraction profiles, and the cache root. Run it first.

## Configuration

Layout lives in `receipts.yaml`. The directory holding that file is the corpus
root, so the tool makes no assumptions about what else the corpus contains.

```yaml
corpus:
  summaries: "summaries/{id}.yaml"
  markdown:
    - "md/{id}.md"
    - "md/{id}/{id}.md"
  pdf: "pdfs/{id}.pdf"

cache:
  root: null

pdf:
  ocr:
    enabled: true
    dpi: 300
    lang: eng
```

Every template is relative to the corpus root and must contain `{id}`. Absolute
paths and `..` segments are rejected, so a config file cannot direct reads
outside the corpus.

Discovery walks up from the working directory, checking `.config/receipts.yaml`,
`.receipts.yaml`, then `receipts.yaml` in each ancestor, stopping at a `.git`
boundary. TOML and JSON are also accepted. If nothing is found, the corpus root
falls back to the nearest `.git` boundary, then to the working directory, and
`doctor` reports `config: ok (defaults)`.

These are real defaults: a corpus laid out as `summaries/`, `md/`, and `pdfs/`
inside a Git repository needs no config file at all. Use `--config FILE` to name
one explicitly.

## Run

```bash
receipts doctor
receipts locate ID --claim 0 --exact "literal present in both sources" --page 3
receipts check ID
receipts audit
receipts audit ID...
```

`locate` chooses a pulldown-cmark semantic unit and validates the exact literal
against MuPDF native structured text. If native text has no match and `--page`
was supplied, it renders that physical page at 300 DPI, detects orientation, and
tries Tesseract OCR. Native ambiguity is an error and never triggers OCR
fallback.

`locate --json` also reports the bounding box of the matched native-text lines or
OCR words, and mean OCR confidence when the backend provides them. The default
YAML-ready output keeps those diagnostics in a comment so they are not persisted
in the strict evidence contract.

Pass IDs to `audit` to inspect only part of a corpus. `--quiet` suppresses
successful human-readable totals while retaining errors; explicit JSON output is
never suppressed.

Only `receipts` should produce normalized literals, SHA-256 values, coordinates,
pages, and backend names. Normalization replaces each Unicode whitespace run
with one ASCII space and changes nothing else.

## Evidence contract

```yaml
id: smith-2019
claims:
  - "Transport remained laminar across all three test regimes."
evidence:
  markdown:
    source: "md/smith-2019/smith-2019.md"
    sha256: "..."
  pdf:
    source: "pdfs/smith-2019.pdf"
    sha256: "..."
  claims:
    - claim: 0
      claim_sha256: "..."
      locators:
        - exact: "no measurable turbulent mixing was observed"
          markdown:
            line: 12
            column: 1
            unit: paragraph
          pdf:
            page: 3
            backend: mutool-native
```

Claim indexes are zero-based; Markdown coordinates and physical PDF pages are
one-based. Every claim must have one entry and at least one locator. Use several
locators when a single literal does not support every material assertion in the
claim.

Unknown fields are rejected inside `evidence` and below, but not at the document
level, so a summary may carry its own metadata — `authors`, `doi`, `notes` —
alongside the block `receipts` owns.

The `evidence` property is optional, so a corpus can adopt evidence
incrementally. Targeted `receipts check ID` always requires it, so a document
without evidence is never a validated document. Bare `receipts check` validates
every evidence-bearing summary and skips the rest. `audit` reports missing
evidence without failing; `audit --strict` makes it fatal.

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
`evidence` key, the entry index, and the `_sha256` suffix:

```yaml
id: smith-2019
propositions:
  - "Transport remained laminar across all three test regimes."
evidence:
  propositions:
    - proposition: 0
      proposition_sha256: "..."
      locators: [...]
```

Both forms are explicit because English pluralization is unreliable — `thesis`
and `theses` would defeat any rule worth writing. Human-readable messages follow
your vocabulary; `locate` emits it too.

Two things stay fixed regardless. **Error codes** are vocabulary-free
(`missing_evidence_entry`, `stale_hash`, `entry_out_of_range`,
`duplicate_entry`), so a script consuming `--json` is portable across corpora.
And **`--claim N` keeps its name**, because it takes an index rather than the
word: the command is identical whichever vocabulary a document uses, and a
configurable flag would fragment every example and shell script.

## Cache

Tesseract TSV is reusable runtime data, content-addressed by the PDF digest,
backend, fixed OCR settings and command templates, MuPDF and Tesseract versions,
and physical page. The manifest also records the applied render rotation.

```text
<cache-root>/PDF_SHA256/tesseract-eng-300dpi-v1/TOOLCHAIN_SHA256/page-NNNN/
```

The root defaults to the platform cache directory
(`~/Library/Caches/receipts/pdf-text` on macOS). Set `cache.root` to keep
artifacts beside the corpus instead; a relative path resolves against the corpus
root.

A manifest that disagrees with any key component is a miss rather than a stale
hit, so entries never expire and a hit is a determinism guarantee rather than
only a saved subprocess. The cache is never evidence authority — it can always
be regenerated from the hashed PDF.

## Development

```bash
just check   # fmt, clippy, build
just test    # full suite
just ci      # both
```

## License

Apache-2.0 OR MIT.

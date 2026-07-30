# Handoff: Multi-Source and Support Checking Spec Ready

**Date:** 2026-07-30
**Branch:** main
**State:** Yellow

> Green = tests pass, safe to continue. Yellow = tests pass but known issues exist. Red = broken state, read Landmines first.

## Where things stand

`receipts` 0.1.0 is implemented and clean: 9 commits on `main`, 56 tests, and
`cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`,
`cargo build --locked`, and `cargo test --locked` all pass. `receipts doctor`
reports green against `mutool` 1.28.0 and `tesseract` 5.5.3.

`docs/specs/2026-07-30-multi-source-and-support-design.md` is written and its
design was approved section by section, but it has had no line-by-line read and
no implementation plan. Nothing in it is built.

The spec closes three gaps in the current tool: only one source per summary, every
locator found by hand, and presence not implying support. It also records one bug
in shipped code — see Landmines.

There is no git remote. Publishing is a deliberate pending decision, not an
oversight.

## Decisions made

Full detail is in the spec; these are the choices that constrain implementation.

- **Sources are named Markdown+PDF pairs.** `evidence.sources` maps a name to a
  pair, and a locator cites one pair. The pair is atomic because the dual-source
  guarantee depends on the Markdown having come from that specific PDF.
- **Single-source documents keep working.** A bare `markdown`/`pdf` pair desugars
  to `sources.default`, and a locator with no `source` means `default`.
  Normalization happens at the `serde_json::Value` boundary in `parse_summary`,
  right after `terms.canonicalize`.
- **No wildcard in corpus source templates.** Every source role is declared
  explicitly; an undeclared name is `unknown_source_template`. A wildcard was
  designed and rejected as unnecessary magic — a corpus has a small known set of
  roles, not generated names.
- **Section paths are recorded and verified**, matching how `unit`, `line`,
  `column`, and every digest are already treated. Section *rules*
  (`sections.weak`) stay warnings permanently, because converter heading
  hierarchy is unreliable.
- **`coverage.tokens` defaults to `error`.** It is a new check over existing
  records and will fail documents that pass today. `warn` exists as a migration
  setting, not a recommended posture.
- **`propose` never writes evidence and never runs OCR.** It emits ranked
  candidates; `locate` commits one after review. No native match is reported, not
  worked around.
- **The review tier is recorded, never produced.** `receipts` hashes and
  staleness-checks a semantic verdict and never treats it as proof. `check` only
  gates on it under `--require-review`.
- **Version stays 0.1.0.** No migration story beyond the desugaring above.
- **Phase order is 0 → 1 → 2 → 3 → 4, with 5 deferred.** Phase 3 must follow
  Phase 2: `propose` and coverage checking share one material-token extractor, and
  defining it twice would let the proposer and the validator disagree about what a
  claim asserts.

## What's next

1. Read `docs/specs/2026-07-30-multi-source-and-support-design.md` end to end and
   flag anything wrong before planning starts.
2. Write implementation plans — Phase 0 standalone (it is small), then one plan
   per phase. Do not combine phases into one plan.
3. Implement Phase 0 first. It is a bug fix, it is not deferrable, and it is a
   contradiction of the premise the rest of the spec rests on.
4. Separately decide whether to publish, and to where.

Standard checks before each commit:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo run --locked -- doctor
```

## Landmines

- **`pdf.ocr` config is currently decorative.** `config.rs` defines *and
  validates* `pdf.ocr.enabled`, `dpi`, and `lang` — it rejects a zero `dpi` and an
  empty `lang` — but nothing reads any of them. `src/pdf/tesseract.rs` hardcodes
  `OCR_DPI = 300`, `-l eng`, `--psm 3`, and `PROFILE_NAME =
  "tesseract-eng-300dpi-v1"`. Setting `dpi: 600` silently does nothing and
  `enabled: false` does not prevent OCR fallback. This is Phase 0.
- **The OCR cache is already safe against that**, so do not "fix" it twice.
  `OcrProfile` carries `dpi` and `language`, `cache_key` hashes the whole profile
  into `toolchain_sha256`, and `entry_dir` uses that hash as a path component — a
  changed setting is already a miss. What Phase 0 must still fix is deriving the
  profile *name* from its settings, so a 600-DPI entry does not sit in a directory
  named `…-300dpi-v1`.
- **`PdfBackend` is a closed enum**, so a corpus using OCR better than Tesseract
  cannot record what it used. That is Phase 5, and it is why `propose` reports
  rather than OCRs.
- **Do not let `propose` write.** Automating acceptance removes the reviewed
  decision that makes the evidence worth trusting, and the tool would then be
  asserting its own conclusions.
- **`Corpus::cache_root()` returns `&Path` but `PdfTools::new` takes `PathBuf`.**
  Four call sites in `cli.rs` use `.to_path_buf()`. Threading `OcrConfig` through
  in Phase 0 touches the same constructor.
- **The corpus root must stay canonicalized.** `config::resolve_root`
  canonicalizes both branches deliberately: source containment compares a
  canonicalized source path against the root, and on macOS a non-canonical root
  (`/var` versus `/private/var`) fails every comparison. A test covers this.
- **`tests/cli.rs` and `tests/validate.rs` must pin `cache.root`** inside their
  temp dirs. Without it the real binary writes to the platform cache directory and
  the suite stops being hermetic.
- **Test fixtures encode specific behavior, not sample data.**
  `mutool-rotated-table.json` must stay fragmented so native extraction cannot
  produce `42.8% within tolerance`; that absence is what forces the OCR path. The
  TSV geometry produces an asserted bounding-box width of 274. Changing either
  breaks a test for the wrong reason.

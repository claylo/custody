# Needs before the first release

> **Superseded 2026-11-16 (Clay):** 0.1.0 ships as it stood on 2026-09-28, on
> librebar 0.6. All three items below move to 0.2, with the core/CLI split
> first. The "before 0.1.0" framing in the rest of this document no longer
> applies; the analysis and the suggested order still do. The README now says
> the library surface is unstable until `custody-core` lands.

**Written 2026-09-30**, from a separate session that tried to bring a
publisher ePub into the summary pipeline. There is no PDF, and a PDF made from the ePub would only carry a renderer's page numbers.

Three changes are needed before 0.1.0. The first two come from that session.
Clay named the third, the library/CLI split, as a pre-release requirement; he
has a few other items in mind that aren't recorded here yet. The release
blockers in `.handoffs/2026-09-28-1336-release-readiness.md` (GPG secret name,
README install line, history rewrite, `scrat ship --dry-run`) still apply.

## Why the PDF can't stay mandatory

- The corpus's canonical source for a publisher ePub is the ePub. Its chapter
  Markdown comes from `fte split`, keyed by the chapter's XHTML path inside the
  ePub.
- APA 7 says not to use page numbers for reflowable ebooks. Cite the chapter,
  the section heading, or the paragraph number instead: *(Cain, 2012,
  "Introduction" section, para. 3)*. For an ePub, the thing to pin a claim to
  is (chapter file, section path, paragraph), not a PDF page.
- custody is going to pin claims in more kinds of source than academic PDFs,
  so dropping the PDF requirement is the next step in any case.
- Today's behaviour, from the Sep 9 `receipts` build that failing-to-die
  still runs:
  - A source with no `pdf:` fails to load with `missing field pdf`.
  - A source with `pdf:` set but no file on disk fails with
    `canonical PDF is missing` (`src/cli.rs:807`).

## 1. Sources without a PDF

Commit 8ecb226 made `markdown` optional to support PDF-only sources. This is
the same change in the other direction. Neither design spec considered a
source with Markdown and no PDF. Both treat the PDF as the anchor: the 07-28
spec says a locator is valid "only when the same exact literal… occurs
unambiguously in both sources" (lines 15–16), and the 07-30 spec calls the
pair atomic "because the dual-source guarantee depends on the Markdown having
been produced from that specific PDF" (lines 142–145). For an ePub that
guarantee moves: the Markdown is produced from the ePub.

Where a PDF is required today:

| Area | Site | What it assumes |
|---|---|---|
| Config | `src/config.rs:146`, `:389`, `:415` | `SourceTemplates.pdf: String` has no default, desugaring inserts one, and `validate()` always checks it |
| Evidence | `src/evidence.rs:48-52`, `:71-79`, `:345` | `SourcePair.pdf` and `Locator.pdf` are required; the structure check always validates `{name}/pdf` |
| CLI startup | `src/cli.rs:277-311`, `:415` | `PdfTools` is always built; every command except `doctor` runs `preflight_toolchain` (mutool and tesseract must validate) |
| Doctor | `src/cli.rs:656-659` | fails unless both PDF tools are OK |
| Locate | `src/cli.rs:804-816`, `:214-221` | the PDF half is unconditional; `LocateResult.pdf` isn't optional |
| Propose | `src/propose.rs:43-49`, `:180-191` | every candidate must pass `verify_pdf`; a Markdown-only source yields none |
| Validate | `src/validate.rs:48-52`, `:321-333`, `:399-418`, `:595-641` | `ResolvedSource.pdf_path` is required; the cited page is always extracted and must hold the literal exactly once |
| Wording | `src/cli.rs:38-47`, `Cargo.toml:7`, `.github/coda.yml:4`, `.github/formula.rb.tmpl:5` | "exactly once on one physical PDF page", "summaries of PDF sources" |

What's needed:

- **Config.** `pdf` becomes optional. A source declares at least one leg:
  a non-empty `markdown` list, a `pdf` template, or both. A source with
  neither is a config error.
- **Evidence.** `SourcePair.pdf` and `Locator.pdf` become `Option`, like
  `markdown`. The pair and locator must agree on whether each half is
  present, which needs a PDF counterpart to `MARKDOWN_LOCATOR_MISMATCH`.
- **Binding.** A locator must bind in every leg its source declares. For a
  Markdown-only source, the recorded Markdown half is the whole gate: the
  literal occurs exactly once in the unit at (line, column), and the section
  path matches.
- **Legs and corroboration.** Today `pdf_confirmed` is the only leg a
  recorded locator produces (`src/validate.rs:89-142`). The recorded Markdown
  half is a pass/fail gate but never a leg; Markdown only counts through
  whole-document corroboration. **Decided (Clay, 2026-09-30):** a
  Markdown-only locator's recorded half counts as a `markdown` leg, so
  `single_leg` policy applies the same way to every source.
- **Tooling.** Require mutool and tesseract only when some source in play
  declares a PDF. `doctor` should report them as optional when no source
  does.
- **Propose.** Markdown-only sources need their own candidates. The Phase 3
  handoff decided to discard candidates without a PDF match
  (`.handoffs/2026-08-09-phase-3-complete.md:70-73`). The audit anticipated
  reversing that (`record/audits/2026-08-10-20-full-repo/README.md:1488-1490`).
- **Existing records.** failing-to-die holds about 900 v1 summaries, all with
  PDF legs. With `skip_serializing_if = "Option::is_none"`, a record whose PDF
  is present should serialize byte-for-byte as before, so `claim_sha256`
  (`src/review.rs:59-66`) is unchanged. Pin that with a test against a real
  record.

## 2. Paragraph number within the section

APA's reflowable-ebook form needs "para. N". The Markdown locator records
`line`, `column`, `unit` and `section` (`src/evidence.rs:87-93`), with no
ordinal. `MarkdownUnit` has no ordinal either (`src/markdown.rs:47-54`).

- Add an optional `paragraph: N` to `MarkdownLocator`: the 1-based count of
  paragraph units since the nearest heading. `check` validates it when present,
  as it does `section`. Records without it still validate.
- **What counts is still open, but the test is settled.** Clay: it's about how
  the text looks to someone trying to find the reference in the source. N
  should be the count a reader arrives at scanning down from the heading in
  the rendered book, not a count of Markdown unit kinds. Blockquotes and list
  items look like separate blocks, so they probably advance N; table cells and
  captions probably don't. Those two guesses are mine, not decided. Settle the
  rule against real rendered pages before coding it. The AAP case chapters
  (6–9) are a good test, since their story transcripts are set off as
  separate blocks.
- This depends on real section paths. `section` comes from pulldown-cmark
  heading events (`src/markdown.rs:130-134`, `:278-287`), and publisher ePubs
  often style section heads as paragraphs. fte's `feat/epub-toc-headings`
  branch turns those into headings, using the TOC plus a per-publisher class
  map. For the Guilford book, section paths then read
  `Chapter 3 › THE AAP VALIDATION STUDY › Measures`.
- Adding the field changes `claim_sha256` only for records that carry it.

## 3. Split into `custody` (CLI) and `custody-core` (library)

This is Clay's requirement. What the code looks like now:

- **Packaging.** One package plus the `xtask` member, with no
  `[workspace.dependencies]` (`Cargo.toml:15-20`). The canon cli chunk expects
  dependencies inherited through `[workspace.dependencies]`, and the release
  handoff notes custody hasn't adopted that. librebar is pinned `=0.6.0` in
  both the root and xtask.
- **Public surface.** `src/lib.rs:3-18` makes all 16 modules public,
  including `cli` and `output`. `src/main.rs` installs the crash handler and
  calls `custody::cli::run()`.
- **Logic living in the CLI.** `src/cli.rs` is the only clap and
  `librebar::cli` user, but the `locate`, `extract` and `doctor` logic lives
  there too and would move to core.
- **librebar in core modules.** `src/config.rs:330` uses `ConfigLoader` and
  `src/evidence.rs:251`, `:558` use `librebar::config::parse_yaml`. So core
  either keeps librebar's `config` feature or takes already-parsed input.
- **PDF abstraction.** The `PdfTextProvider` trait (`src/pdf/mod.rs:224-228`)
  already isolates PDF text for `validate` and `propose`. `cli.rs` bypasses it
  and uses `PdfTools.mutool` directly (`:332-333`, and in doctor at
  `:622-634`).
- **Consumers of internals.** xtask imports `custody::cli::Cli`
  (`xtask/src/commands/man.rs:19`, `completions.rs:25`). The integration
  tests import library modules; `tests/cli.rs`, `corroborate.rs` and
  `extract.rs` drive `CARGO_BIN_EXE_custody`.
- **Publishing.**
  - Only `custody` is claimed on crates.io (the 0.0.1 stub). Claim
    `custody-core` too.
  - `CARGO_PUBLISH_PACKAGES: custody` (`.github/coda.yml:34-35`) needs both,
    with core published first. `cd.yml` reads the version from
    `cargo pkgid --package custody`.
  - Each crate needs its own `include` list.
  - The Homebrew formula installs `bin/custody`, which doesn't change, but its
    description says PDF.
- **librebar 0.7** will break `schema_metadata()` (`src/cli.rs:429`). After the
  split that break stays in the CLI crate.

## Suggested order

1. The core/CLI split. It moves the code that item 1 has to change (locate's
   unconditional PDF half is in `cli.rs`), so doing the split second would
   mean changing that code twice.
2. Sources without a PDF, in core.
3. The paragraph number.
4. Release.


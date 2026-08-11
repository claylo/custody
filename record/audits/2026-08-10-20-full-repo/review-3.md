# Review 3 — `performance`, `verification-gate`, `structure` (21 findings)

Verified against working tree at `b78d869585b48532c90155d6f70b9486687aef54` (clean except the untracked audit dir).

---

## READ THIS FIRST — systemic defect: `evidence_markers` line numbering

Ten of the 21 findings in my scope express `evidence_markers.lines` as **absolute file line numbers**. That is wrong and the markers silently render as nothing.

Authority, `references/findings-schema.yaml.md:913-916`:

> **`evidence`.** The exact text that appears in the rendered code block. Line numbers for `evidence_markers` are **relative to this string**, not to the source file — they start at 1 at the first line of the `evidence` block.

Confirmed in the renderer. `scripts/build-report.js:40053` passes `startLineNumber: locations[0].start_line` to expressive-code, which only changes the **gutter label**. The text-marker plugin resolves ranges with `const lineIndex = lineNumber - 1; codeBlock.getLine(lineIndex)` (`build-report.js:31432-31433`) — an index into the block. An absolute number like `634` on an 11-line block returns `undefined` and the annotation is dropped. So these markers are not merely mispositioned, they are **invisible**.

Batch fix (all verified line-by-line against the evidence blocks):

| slug | current `lines` | correct `lines` |
|---|---|---|
| `structured-output-contracts-untested` | `634-640` | `3-9` |
| `summary-discovery-recursion-untested` | `1159-1160` / `1167-1172` | `3-4` / `11-16` |
| `markdown-coordinates-only-pinned-at-line-one` | `27` | `8` |
| `license-allowlist-inherited-from-absent-dependency-tree` | `32` / `35` / `42` | `5` / `8` / `15` |
| `transitive-syn-duplicate-is-not-actionable` | `77-79` | `17-19` |
| `validate-document-carries-seven-concerns` | `161-176` | `1-16` |
| `check-and-audit-duplicate-the-summary-loop` | `909-926` | `1-18` |
| `duplicated-issue-constructors` | `486-500` | `1-15` |
| `duplicated-subprocess-runner` | `361-372` | `1-12` |
| `dead-public-vocabulary-and-report-helpers` | `89` | `2` |

The other eleven findings in my scope already use relative markers correctly. Worth sweeping the four narratives I do not cover for the same defect — the split is roughly per-agent, so whole narratives are likely affected.

---

## The five prior performance fixes: ALL FIVE HELD

I read each one independently. No regressions. Details:

**(a) Markdown coordinate resolution — HELD.** `src/markdown.rs:61` calls `build_line_starts(source)` exactly once inside `parse_units`, before the event loop; the `&[usize]` is threaded into `finish_unit` by reference (`markdown.rs:107`, `:110`, `:158`). `line_column_from_index` (`markdown.rs:247-254`) is a `partition_point` binary search. No per-call index construction anywhere; `build_line_starts` has exactly one call site.

**(b) OCR profile `OnceCell` — HELD and genuinely shared.** `PdfTools` owns `ocr_profile: std::cell::OnceCell<cache::OcrProfile>` (`src/pdf/mod.rs:96`), initialized empty in `new` (`:108`), resolved at most once in `ocr_profile()` (`:112-118`), consumed by `ocr_page` via `&self` (`:130`). The cell is not recreated: `PdfTools::new` is called once per command (`cli.rs:836` check, `:896` audit, `:991` propose) and `&tools` is passed into every `validate_document` call, so `resolve_profile` runs at most once per process, not once per summary.

**(c) Token coverage byte comparison — HELD.** `tokens::is_covered_case_insensitive` (`src/tokens.rs:59-80`) operates on `text.as_bytes()` / `token.as_bytes()` and compares with `eq_ignore_ascii_case` on the slice. Zero allocations in the function body.

**(d) TSV column indexes — HELD.** `text_index`, `confidence_index`, and `geometry_indexes` are resolved at `src/pdf/tesseract.rs:286-297`, collapsed into `geo_indexes` at `:302-305`, and the row loop starts at `:306`. No `columns.iter().position(...)` inside the loop.

**(e) Proposal ranking order — HELD.** `rank_raw_candidates(&mut raw_candidates)` at `src/propose.rs:169` precedes the verification loop at `:172`, and `:173-175` breaks once `candidates.len() >= max_candidates`. Sort-then-verify is intact. (The residual gap is the subject of `pdf-verification-unbounded-when-candidates-fail`, which is correctly filed as a residual, not a regression — see below.)

---

# Narrative: `performance`

## `validate-spawns-one-mutool-per-cited-page`

**CONFIRMED**

Evidence at `src/validate.rs:233-248` is verbatim and the range is exact (16 lines, `let Some(pdf_path)` through `});`). Second location `src/validate.rs:375-386` is exactly the `PdfBackend::MutoolNative` arm of `extract_pdf_page`. Markers `4-5` are correctly relative and land on the cache-key and memo-entry lines.

Mechanism traced and correct:
- `pdf_pages` is declared at `validate.rs:154`, **inside** `validate_document`, above the claim loop at `:161` — so it dedupes across claims within one summary and dies with the call. Per-summary memo confirmed.
- Key is `(source_name, locator.pdf.backend, locator.pdf.page)` (`:236`), so distinct pages are distinct misses. Confirmed.
- The native path is `provider.native_pages(pdf_path, Some(page))` (`:378`) → `Mutool::native_pages` (`src/pdf/mutool.rs:30-55`), which builds `mutool draw -q -F stext.json -o -` and calls `run` at `:41`. One fork+exec, one full document open, one `parse_stext_json` per call. **No disk cache on this path** — confirmed: `src/pdf/cache.rs` `CacheManifest::new(pdf_sha256, page, profile)` requires an `OcrProfile`, and its only construction site is `tesseract.rs:241` inside `ocr_page_with_profile`.
- `propose_document` does demonstrate the batch alternative on the same trait: `native_pages(&pdf_path, None)` at `propose.rs:146`, indexed by `page.page` at `:152`. `parse_stext_json` assigns `page: index + 1` (`mutool.rs:116`), so the returned `Vec` is directly indexable by page number — the remediation is mechanically sound and needs no trait change, no test-double change, and touches nothing `unsafe`/MSRV-sensitive.

Concern `significant` is defensible: N×D fork+exec on the dominant cost center.

Optional prose nit (not required): "the same evidence set costs **N** spawns instead of N×D" holds for a single-source corpus. Multi-source corpora are supported, so strictly it is N×(sources). If you want it exact, replace that clause with `so the same evidence set costs one spawn per source per summary instead of N×D`.

## `propose-regenerates-claim-independent-spans-per-claim`

**CONFIRMED**

Evidence at `src/propose.rs:159-169` is verbatim including the blank line at 163 (11 lines). Second location `:244-258` is the `for unit in units { for span in candidate_spans(...)` nest inside `generate_candidates`. Marker `9` is relative and lands on the `generate_candidates` call. Correct.

Loop nesting verified — the asymptotic claim holds and is not overstated:
- `generate_candidates` is called at `:167`, inside the per-claim loop opened at `:159`, once per source.
- Inside it, `candidate_spans(&unit.text)` (`:245`) → `split_sentences` (`:276`) → `push_normalized` → `normalize(segment)` allocating a fresh `String` per sentence (`:101-106`), plus `text.to_owned()` for the whole-unit span when `spans.len() > 1` (`:277-279`).
- `exact_count(&unit.text, &span)` at `:248` is `haystack.match_indices(needle).count()` (`markdown.rs:190`) — a full sweep of the unit text, per span.
- The only claim-dependent value is `matched` (`:251-255`). Everything above it is invariant across claims. Confirmed O(K × U × S × T) where O(U × S × T) once would do.
- Per-surviving-candidate `source.to_owned()` (`:260`) and `section: unit.section.clone()` (`:266`) confirmed.

Remediation is sound and edition-2024/MSRV-clean. One thing to protect during the refactor, worth adding to `effort_notes` if you want it explicit: `rank_raw_candidates` uses `sort_by` (stable), so byte-identical output requires the hoisted span list to preserve the current generation order (source outer via `BTreeMap` iteration, then unit, then span). If it does, ties break identically and the existing propose tests remain a valid gate.

## `resolve-unit-linear-scan-called-twice-per-locator`

**ADJUSTED** — the mechanism overstates the "twice"; the remediation will not compile as written.

Evidence at `src/markdown.rs:172-181` is verbatim and exact. Second location `src/validate.rs:311-325` is exactly the weak-section block. Markers `1-3` relative and correct.

Three corrections:

**1. The second `resolve_unit` is gated on configured weak sections, which are empty by default.** `validate.rs:312` reads `if !weak_sections.is_empty() && !entry.locators.is_empty()`, and `SectionsConfig::weak` is `#[derive(Default)] Vec<String>` (`src/config.rs:165-170`) — the default corpus never enters the block. The doubling only occurs when a corpus configures `sections.weak`, and even then `.all()` short-circuits on the first non-weak locator. "twice for every locator" is unconditional and wrong for the default configuration.

Replace this sentence in `mechanism`:

```
`validate_document` then performs that lookup twice for every
locator: once in the Markdown-verification block and again in the weak-section
block below, which re-resolves the exact same `(unit, line, column)` triple it
has already resolved. Across a corpus that is N × K × L × 2 × U unit comparisons
where a hash lookup would make it N × K × L × 2.
```

with:

```
`validate_document` then performs that lookup a second time for any corpus that
configures `sections.weak`: once in the Markdown-verification block and again in
the weak-section block below, which re-resolves the exact same
`(unit, line, column)` triple it has already resolved. That block is skipped
entirely when the weak list is empty, which is the default
(`src/config.rs:165-170`), and `.all()` short-circuits on the first non-weak
locator, so the doubling is a configured-corpus cost rather than a universal
one. Across such a corpus that is up to N × K × L × 2 × U unit comparisons where
a hash lookup would make it N × K × L × 2.
```

Then change the trailing `and half of it is a straight repeat.` to `and on a weak-section corpus half of it is a straight repeat.`

**2. "always consumes the entire iterator" is true only when at most one unit matches.** `matches.next()` at `markdown.rs:178` short-circuits the moment a second match is found. That is the ambiguous-coordinate error path, so the statement holds for every successful lookup — but "always" is literally false. Suggested minimal edit: `because it must prove no second unit shares the coordinates, consumes the entire iterator on every successful lookup`.

**3. The remediation does not compile as written.** `UnitKind` derives `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize` (`src/markdown.rs:8`) — **no `Hash`, no `Ord`**. A `HashMap<(UnitKind, usize, usize), …>` needs `Hash`; the `BTreeMap` alternative the remediation offers needs `Ord + PartialOrd`. Append to `remediation`:

```
`UnitKind` (src/markdown.rs:8) derives neither `Hash` nor `Ord`, so add `Hash`
to its derive list for the `HashMap` form (or `PartialOrd, Ord` for the
`BTreeMap` form) — both are safe on a fieldless enum and neither changes the
serialized representation.
```

Also note for `effort_notes`: `resolve_unit` is `pub`, so "keeps its signature by taking the index alongside the slice" is itself a breaking public-API change — related to `entire-crate-is-a-published-public-api`.

Concern `moderate` stands.

## `pdf-verification-unbounded-when-candidates-fail`

**CONFIRMED** — and the adjudication against the prior audit is correct as written.

Evidence at `src/propose.rs:171-179` is verbatim and exact (9 lines). Second location `:293-300` is exactly `verify_pdf`. Markers `3-5` and `7-8` are relative and land precisely on the break condition and the failure `continue`.

The loop reads exactly as the finding claims. `candidates.len() >= max_candidates` (`:173`) tests the **accepted** vector; the `else { continue }` at `:177-179` on a failed `verify_pdf` never touches `candidates`, so the bound cannot fire on a claim whose spans all fail verification. The prior audit's fix (`proposal-ranking-verifies-unbounded-candidate-set`) is intact and is correctly credited here as landed — the finding is filed as the residual, not a regression, and that framing is accurate. No contradiction to adjudicate: both statements are true of the same loop.

Cost per rejected candidate confirmed: `verify_pdf` (`:293-300`) builds a lazy filter over the page map and calls `matched.next()` twice. Zero matches ⇒ the full page sweep. (Worth knowing: a *successful* verification also sweeps all P pages, because proving uniqueness requires exhausting the iterator — but those attempts are bounded, so the finding's framing is right.)

Remediation is sound; the memoization variant is the better half of it, since the same span text recurs across claims and the memo also fixes the accepted-candidate re-verification.

Concern `moderate` stands.

## `owned-json-value-borrowed-then-deep-cloned`

**ADJUSTED** — the finding understates its own case; the mechanism should say the clone is *entirely* wasted.

Evidence at `src/review.rs:46-56` is verbatim and exact (11 lines). Second location `:68-84` is exactly `sort_keys`. Markers `5` and `7` are relative and land on the `to_value` and `sort_keys(&locator)` lines. All correct.

I resolved the open question the remediation raises. `Cargo.lock` shows `serde_json` 1.0.151 with dependencies `itoa, memchr, serde, serde_core, zmij` — **no `indexmap`**, so `preserve_order` is off in this build and `serde_json::Map` is `BTreeMap`-backed. Consequence: the map is *already* key-ordered, `to_string` already emits sorted keys, and `sort_keys` therefore performs **no reordering at all** — its only effect is to deep-clone the tree it was handed. This is stronger than "the borrow buys nothing"; the whole function is a no-op with a copy attached.

Append to `mechanism`:

```
The open question about map ordering resolves against the function: `Cargo.lock`
carries `serde_json` 1.0.151 with no `indexmap` dependency, so `preserve_order`
is off and `serde_json::Map` is `BTreeMap`-backed. The keys are already sorted
before `sort_keys` runs, which means the function performs no reordering
whatever — its entire observable effect is the deep clone.
```

And tighten `remediation`'s last two sentences to:

```
The map type is already key-ordered in this build (no `indexmap` in
`Cargo.lock`), so the object arm exists only to survive a future
`preserve_order` feature unification. Keep it — as a by-value move it costs
nothing — rather than deleting the function and depending on a feature flag
another crate could flip.
```

Concern `moderate` is defensible on the strength of "100% wasted work" and I am not asking you to move it, though `advisory` would also be fair given the absolute cost is a handful of small values per reviewed claim.

## `source-names-allocates-a-vec-to-answer-a-lookup`

**ADJUSTED** — call-site count is off by one test.

Evidence at `src/corpus.rs:127-131` is verbatim and exact (5 lines, doc comment through closing brace). Second location `src/validate.rs:55-61` is correct. Markers `3` and `4` are relative and correct.

Mechanism verified: `self.layout.sources` is `BTreeMap<String, SourceTemplates>` (`src/config.rs:33`), so `contains_key` is the O(log n) answer the API throws away. `validate.rs:61` is the `configured.iter().any(|name| name == source_name)` scan; `propose.rs:136` iterates and reborrows via `&source_name` at `:137`/`:144`. Neither stores an owned `String`. Confirmed.

Correction: there are **two** src callers and **two** test call sites, not three callers total. Change `All three callers want a question answered` to `Both callers want a question answered`, and change `effort_notes` from:

```
Two small methods on `Corpus`, two call-site edits, and one test that needs
an explicit `collect`.
```

to:

```
Two small methods on `Corpus`, two call-site edits, and two tests that need
an explicit `collect` (tests/foundation.rs:176 and tests/foundation.rs:193).
```

Same fix in `remediation`: `The `tests/foundation.rs` assertions comparing against `vec!["default"]` collect explicitly instead.` → `The two `tests/foundation.rs` assertions (:176 and :193) collect explicitly instead.` (`:193` does `let mut sources = corpus.source_names(); sources.sort();` and needs the same treatment.)

Concern `advisory` stands.

## `weak-section-list-lowercased-per-comparison`

**CONFIRMED**

Evidence at `src/sections.rs:14-24` is verbatim and exact (11 lines). Markers `9` and `6` are relative and land on `weak.to_lowercase()` and `heading.to_lowercase()` respectively. Correct.

Mechanism holds: `weak.to_lowercase()` allocates in the innermost closure, once per (heading, weak-entry) pair, for data fixed at config load. The N·K·L·D·W figure is a worst case (`.any()` short-circuits on both levels), which is appropriate for an advisory. The remediation's reasoning about keeping `to_lowercase` on the heading (non-ASCII headings) rather than reaching for `eq_ignore_ascii_case` is correct and shows the author checked the Unicode case.

Concern `advisory` stands. The cross-reference to `resolve-unit-linear-scan-called-twice-per-locator` ("sits inside the same per-locator nest") is accurate — both live under the `weak_sections` guard at `validate.rs:312`.

## `release-profile-left-at-cargo-defaults`

**CONFIRMED**

Evidence at `Cargo.toml:22-33` is verbatim and exact (12 lines, `[dev-dependencies]` through `too_many_lines = "allow"`). Marker `12` is relative and lands on the last line of the block — correct for a "nothing follows this" annotation.

The manifest genuinely has no `[profile.release]` (file is 34 lines, ends at `too_many_lines`). Default profile characterization is right. The `librebar` `crash` feature caveat against `panic = "abort"` is real (`Cargo.toml:16` carries `features = ["cli", "config", "crash", "diagnostics"]`). Filing this at `note` with an explicit "measure before keeping it" is the correct call for a subprocess-dominated tool.

---

# Narrative: `verification-gate`

## `structured-output-contracts-untested`

**ADJUSTED** — markers (see batch table: `634-640` → `3-9`). Everything else confirmed, and here is the complete coverage list you asked for.

Evidence at `src/cli.rs:632-642` is verbatim and exact (11 lines).

Every sub-claim in the mechanism verified true:
- `receipts_json` (`tests/cli.rs:33`) has exactly **one** call site in the entire suite: `tests/cli.rs:381`, in `propose_emits_candidates_for_claims_without_evidence`. Every other invocation goes through `receipts` / `receipts_with_path`, both of which hard-code `--format text` (`:16`, `:26`).
- The doctor assertions at `tests/cli.rs:109-113` read the human labels; `:112` is `"native profile: ok (mutool-native)"` and `:113` is `"OCR profile: ok (tesseract-eng-300dpi-v1)"`. The JSON keys are `native_profile` / `ocr_profile`. Renaming a JSON key breaks nothing. Confirmed.
- Multi-ID wrapper at `src/cli.rs:1010-1016` is exactly the `if reports.len() == 1 { … } else { print_json(&serde_json::json!({ "summaries": reports })) }` block. No test passes two IDs to `propose`. Confirmed.
- `advisory_tokens` (`src/propose.rs:34`): grep across `tests/` returns zero hits. Its only appearances are `src/propose.rs:34`, `:215`, and a help string at `src/cli.rs:322`. Confirmed.

**Accurate JSON-shape coverage list** (every `print_json` call site in `src/cli.rs`):

| shape | site | asserted? |
|---|---|---|
| `doctor` (7 named fields) | `cli.rs:642` | **no** |
| `locate` payload | `cli.rs:763` | **no** |
| `audit` report (`AuditReport`) | `cli.rs:969` | **no** |
| `propose`, single ID (`ProposalReport`) | `cli.rs:1012` | **yes** — `tests/cli.rs:385-409` |
| `propose`, multi ID (`{"summaries": […]}`) | `cli.rs:1014` | **no** |
| `check` report (`CheckReport`) | `cli.rs:1197` | **no** |

So the title holds exactly as written: one of six shapes is covered. Even the covered one is partial — `tests/cli.rs:385-409` asserts `id`, `claims[].claim`, `required_tokens`, `uncovered_tokens`, `candidates[]`, `coverage`, `markdown`, `pdf`, but never `advisory_tokens`. Worth adding `check` and `audit` to the mechanism's list of untested shapes; the current text names only the three that changed today, which reads narrower than the (correct) title. Suggested insertion after the `(3)` clause:

```
Beyond the three contracts changed today, the `check`, `audit`, and `locate`
JSON shapes are equally unasserted; of the six `print_json` shapes the binary
emits, exactly one is read by a test.
```

Remediation is sound — `receipts_json` exists and the three proposed assertions are mechanical.

Concern `significant` stands.

## `just-check-is-not-a-complete-gate`

**CONFIRMED** — both merged halves hold, and so does the third claim.

Evidence at `justfile:1-10` is verbatim and exact (10 lines, `set shell` through the `clippy` recipe body). Markers `4` and `3` are relative (start_line is 1, so they coincide with file lines) and land on `msrv := "1.89.0"` and the `toolchain :=` backtick line. Correct.

**Half one — fmt writes instead of verifying: TRUE.** `justfile:40` is `check: fmt clippy deny test doc-test doc`, and `fmt` (`justfile:24-25`) is `cargo fmt --all -- --config-path .config/rustfmt.toml` with no `--check`. It mutates. `ci: check` (`justfile:42`) inherits the defect, so `just ci` can never fail on formatting.

**Half two — MSRV declared but unenforced: TRUE.** `msrv` appears exactly once in the file, at `:4`. No recipe interpolates `{{msrv}}`. Every recipe that builds pins `+{{toolchain}}`, sourced from `rust-toolchain.toml`. `Cargo.toml:5` independently declares `rust-version = "1.89"`, so the value is duplicated in two places and read by nothing.

**Third claim — dead nextest CI profile: TRUE.** `test-ci` at `justfile:31-32` is the only `--profile ci` user, and no recipe depends on it (`check` uses `test` at `:20-21`).

Remediation is sound and MSRV-compatible: `cargo +1.89.0` overrides `rust-toolchain.toml` (explicit `+toolchain` wins), and edition 2024 needs only 1.85+, so a 1.89 check build is viable. `cargo fmt --all --check -- --config-path .config/rustfmt.toml` is valid argument order.

Concern `moderate` stands.

## `no-automated-ci-for-the-documented-gate`

**CONFIRMED**

Evidence at `justfile:40-42` is verbatim and exact (3 lines including the blank at 41). Marker `3` is relative and lands on `ci: check`. Second location `README.md:337-344` is exactly the `## Development` heading through the closing fence of the `just` block. Correct.

Every absence claim verified: no `.github` directory; `.gitignore` contains exactly `/target`, `/.cache`, `/dist`, `commit.txt`, `scratch/`, `.DS_Store`, `.crustoleum` and does not exclude `.github`; no other pipeline config in the tree (`./receipts.yaml` and `./.config/bito.yaml` are app/tool config, not CI); `.git/hooks` contains no non-sample hooks. The remediation's warning that CI must not call `just ci` directly (because `fmt` mutates) is correct and consistent with the sibling finding.

Concern `moderate` stands.

Low-priority note: `temporal.commit_count: 57` / `monthly_commits: [.., 10, 47]` are repo-wide totals, while the schema defines temporal as `git log` on the finding's primary file (`justfile` = 4 commits, `[0,0,0,0,0,0,0,0,0,0,1,3]`). Defensible as an authorial choice for an absence-of-file finding — your call, but flagging it since the sparkline will read very differently from its siblings.

## `justfile-loads-repo-supplied-dotenv`

**CONFIRMED**

Evidence at `justfile:1-4` is verbatim and exact. Marker `2` is relative and lands on `set dotenv-load := true`. Correct.

Mechanism verified end to end. `.env` is genuinely absent from `.gitignore` (full contents listed above). No recipe in the 71-line justfile references an environment variable — I read all of it; the only interpolations are `{{toolchain}}`, a `just` variable from a backtick command. So the setting is pure exposure with no offsetting benefit, exactly as claimed. The cargo escalation vector is real: `RUSTC`, `CARGO_BUILD_RUSTC_WRAPPER`, and `CARGO_TARGET_<TRIPLE>_RUNNER` are all read from the process environment and all name an executable cargo then runs. Provenance confirmed: the justfile's most recent commit is `b5e6dca`, but `git log --format='%ad %s' -- .config/deny.toml` shows the fleet-baseline commit `chore: update justfile and config to match fleet baseline` on 2026-08-10, matching the finding's attribution.

Two notes, neither blocking:

1. **Calibration.** `moderate` is defensible for a single-maintainer repo with no external PR flow. If the security narrative frames this as reviewer-machine RCE, `significant` ("meaningful risk under realistic conditions") is the better fit, since the trigger is a maintainer running the sanctioned first command on a contributed branch. Keep them consistent with whatever the `sec` reviewer concluded — do not have two findings describe the same line at two levels.
2. **Temporal inconsistency with its sibling.** This finding says `introduced: "2026-08-10"`; `just-check-is-not-a-complete-gate` says `introduced: "2026-07-29"` for the same file with the same `commit_count: 4`. The justfile's first commit is 2026-07-29 (verified). The 08-10 date is truer to the *line* but contradicts the schema's file-based definition and its own sibling. Pick one.

## `license-allowlist-inherited-from-absent-dependency-tree`

**ADJUSTED** — markers (`32`→`5`, `35`→`8`, `42`→`15`) plus a `monthly_commits` data error. Substance fully confirmed.

Evidence at `.config/deny.toml:28-42` is verbatim and exact — including the three-space indent anomaly on line 28 (`   # Unicode/text processing`), which the evidence block faithfully preserves. 15 lines. Good transcription.

Every factual claim verified against `Cargo.lock`:
- 122 packages exactly (`grep -c '^\[\[package\]\]'` = 122). ✓
- `reqwest`, `aws-lc-sys`, `webpki-roots`, `openssl`, `rustls`, `hyper` — **zero** occurrences each. ✓
- `unused-allowed-license = "allow"` at `:42` suppresses precisely the warning that would surface this. ✓
- Header at `:1` still reads `Enterprise security policy configuration for bito`. ✓

Fix `temporal.monthly_commits`. `git log -- .config/deny.toml` returns exactly one commit, and `commit_count: 1` agrees, but the array `[0,0,0,0,0,0,0,0,0,0,10,47]` is the repo-wide sparkline (57 total) and contradicts it. Replace with:

```yaml
          monthly_commits: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
```

Concern `moderate` stands.

## `summary-discovery-recursion-untested`

**ADJUSTED** — markers only (`1159-1160`→`3-4`, `1167-1172`→`11-16`). Mechanism and remediation fully confirmed.

Evidence at `src/cli.rs:1157-1176` is verbatim and exact (20 lines, `for entry in entries {` through the closing `}` of the loop).

Gap verified by reading the tests rather than assuming: the recursion at `:1159-1160` is reachable only when the scan directory contains a subdirectory. `summary_ids` (`:1127-1139`) splits the template on `{id}` and scans `corpus.summaries_dir()`; every CLI test uses the default `summaries/{id}.yaml` and writes flat files into `summaries/`, so `path.is_dir()` is never true. `tests/foundation.rs:85` (`honors_custom_layout_templates`) does declare `corpus.summaries: "records/{id}/summary.yaml"` at `:90` and asserts only `summary_path`, `markdown_candidates`, and `summaries_dir` — it never lists IDs. So the nested-template behavior and both guards are unexercised. Confirmed.

The remediation would work: with prefix `records/` and suffix `/summary.yaml`, `records/doc-001/summary.yaml` yields id `doc-001`, the decoy `records/doc-001/notes.md` fails `strip_suffix`, and a two-levels-down file yields `a/b` which `!id.contains('/')` rejects. Both proposed tests hit exactly the guards they claim to pin.

Concern `moderate` stands.

## `markdown-coordinates-only-pinned-at-line-one`

**ADJUSTED** — marker (`27`→`8`), and the remediation's column example is factually wrong.

Evidence at `tests/markdown.rs:20-28` is verbatim and exact (9 lines, `#[test]` through `}`).

Headline claim is TRUE — I swept the suite. Confirmed and refined:
- `tests/markdown.rs:27` is the only assertion of a literal `(line, column)` **pair** anywhere in `tests/`. ✓
- Two further literal *line* assertions exist that the mechanism does not mention, both at 1: `tests/propose.rs:99` (`assert_eq!(best.markdown.line, 1)`) and `tests/cli.rs:406` (`assert_eq!(candidate["markdown"]["line"], 1)`). Neither weakens the finding.
- `tests/validate.rs:606-607` supplies literal lines 3 and 7 via `weak_locator(...)` — the one place non-(1,1) coordinates appear. It is **not** a regression gate: the test asserts only the *absence* of `weak_section_only`, and a coordinate off-by-one would make `resolve_unit` fail, `is_ok_and` return false, `all_weak` become false, and the assertion still pass (with unrelated `markdown_unit_missing` errors going unchecked). Worth one sentence in the mechanism — it is the strongest available illustration that the suite tolerates coordinate drift.
- `resolves_units_by_exact_coordinates_and_kind` (`tests/markdown.rs:64-76`) is self-referential exactly as described. ✓
- The `Fixture` round-trip is real, but the citation is imprecise: `tests/validate.rs:809` is the `summary` method; the actual `line: unit.line, column: unit.column` feedback is at `tests/validate.rs:839-840`. Recommend re-citing to `:839-840`.

**The remediation's second example does not produce what it claims.** `parse_units("# H\n\n- an item\n")` gives the list item column **1**, not a column above 1: `Tag::Item`'s `range.start` is the `-` marker, which sits at byte offset 5, and the line-3 start is also offset 5, so `line_column_from_index` returns `(3, 1)`. Top-level list markers always begin at the line start. (The first example is correct — I verified it by hand: `parse_units("first\n\nsecond\n")` has `line_starts = [0, 6, 7]`, the second paragraph starts at offset 7, `partition_point` gives index 3 → `(3, 1)`.)

Replace `remediation` with:

```
Add one test asserting literal coordinates for a document with several
units at known offsets — e.g. `parse_units("first\n\nsecond\n")` must
report the second paragraph at `(3, 1)` — and one asserting a column above
1. A top-level list item will not do it: its `-` marker sits at the line
start, so `"# H\n\n- an item\n"` yields `(3, 1)` as well. Use a unit whose
start offset is genuinely past its line start — a nested list item, a
paragraph inside a blockquote, or a GFM table cell — and pin the literal
pair the parser reports for it. Both are single `assert_eq!` lines against
values a reader can count off the fixture.
```

Concern `moderate` stands. Temporal is correct (`tests/markdown.rs`: 1 commit, 2026-07-29, sparkline `[…,1,0]`) — one of the few that got the sparkline exactly right.

## `transitive-syn-duplicate-is-not-actionable`

**ADJUSTED** — markers (`77-79`→`17-19`) plus the same `monthly_commits` error as its sibling.

Evidence at `.config/deny.toml:61-79` is verbatim and exact (19 lines, `[bans]` through the closing `]`).

Every claim verified against `Cargo.lock`, and the dependency attribution is exactly right:
- `syn` 2.0.119 (`Cargo.lock:795-797`) and `syn` 3.0.3 (`:806-808`) both present. ✓
- `"syn 2.0.119"` appears in exactly one dependency list — `tracing-attributes` (`:969`). ✓
- `"syn 3.0.3"` appears in three — `clap_derive` (`:163`), `serde_derive` (`:701`), `thiserror-impl` (`:856`). ✓ Exactly the three the finding names, in that order.
- `owo-colors` 4.3.0 (`:536-543`) lists both `supports-color 2.1.0` and `supports-color 3.0.2`. The skip entry is not stale. ✓
- `librebar` 0.6.0 (`:432`) does pull `owo-colors`. ✓

The "no action" recommendation is right: a `syn` skip would suppress a signal the project wants, and cargo-deny would then warn on the unused skip after upstream converges.

Fix `temporal.monthly_commits` the same way as `license-allowlist-inherited-from-absent-dependency-tree`:

```yaml
          monthly_commits: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
```

Concern `note` stands.

---

# Narrative: `structure`

## `validate-document-carries-seven-concerns`

**ADJUSTED** — marker (`161-176`→`1-16`) plus a count that contradicts itself.

Evidence at `src/validate.rs:161-176` is verbatim and exact (16 lines, `for entry in &evidence.claims {` through the `{` that opens the section-mismatch branch).

Everything structural checks out. `validate_document` spans `:41`–`:366` = 326 lines exactly, matching the title. The six levels in the excerpt are real: `for` (161) → `for` (162) → `if let` (165) → `match` (166) → `match` (172) → `if` (174). The three parallel `BTreeMap` accumulators are at `:56`, `:57`, `:58`, populated in the first loop and read ~100 lines later. The cross-loop invariant is exactly as described: `continue` at `:69`, `:81`, `:96`, `:108`, `:127` decides which keys exist, and `:233-235` silently skips the missing ones. `too_many_lines = "allow"` is at `Cargo.toml:33`. All confirmed.

One correction: the title and mechanism say **seven** separable jobs, then the mechanism enumerates **nine** (source-template resolution, recorded-path comparison, containment checking, file hashing, Markdown unit resolution, PDF page extraction with memoization, required-token coverage, weak-section warning, review dispatch). Pick one. Cheapest fix, keeping the slug stable, is to change the mechanism's opening clause from:

```
performs seven
separable jobs against three interleaved `BTreeMap` accumulators keyed by
source name:
```

to:

```
performs nine
separable jobs against three interleaved `BTreeMap` accumulators keyed by
source name:
```

and retitle to `validate_document is a 326-line function nested six levels deep` — which is already the title, so only the mechanism and the slug's implied "seven" diverge. If you would rather preserve "seven", drop `recorded-path comparison` and `containment checking` into the source-resolution item and merge `file hashing` with it.

Remediation is sound and behavior-preserving; the one-struct-per-source change is the right seam.

Concern `moderate` stands.

## `check-and-audit-duplicate-the-summary-loop`

**ADJUSTED** — marker only (`909-926`→`1-18`).

Evidence at `src/cli.rs:909-926` is verbatim and exact (18 lines). Every cross-reference in the mechanism verified:
- `check` spans `:823`–`:883`. ✓
- `audit` spans `:888`–`:983` (the `#[allow(clippy::fn_params_excessive_bools)]` sits at `:887`). ✓
- The parallel parse-failure branch in `check` is `:844-853`, matching the marker label. ✓
- Identical message text `format!("summary ID {:?} does not match filename", summary.id)` appears at `:859` and `:935`. ✓ Byte-identical.
- `print_issues` at `:1209-1219` and `print_audit_issues` at `:1221-1231` differ only in `report.id` vs `summary.id`. ✓
- The four shared decision points appear in the same order in both. ✓

Remediation is sound; the counters are the only risk and `effort_notes` already says so.

Concern `moderate` stands.

## `duplicated-issue-constructors`

**ADJUSTED** — marker only (`486-500`→`1-15`).

Evidence at `src/validate.rs:486-500` is verbatim and exact (15 lines). Every claimed duplicate verified at its exact cited range:
- `src/evidence.rs:412-426` — byte-identical to the evidence block. ✓ (Marker label is accurate.)
- `src/review.rs:192-205` — the four-parameter variant hardcoding `locator: None`. ✓
- Inline literals at `src/cli.rs:916-922` and `:932-938`. ✓
- `error_report` at `src/cli.rs:1233-1248`. ✓

Six construction sites, three of them copies. Remediation is sound and behavior-neutral.

Concern `moderate` stands.

## `duplicated-subprocess-runner`

**ADJUSTED** — marker only (`361-372`→`1-12`).

Evidence at `src/pdf/tesseract.rs:361-372` is verbatim and exact (12 lines). I diffed the two ranges directly: `diff <(sed -n '131,142p' src/pdf/mutool.rs) <(sed -n '361,372p' src/pdf/tesseract.rs)` returns empty. **Byte-identical confirmed**, and the cited range `src/pdf/mutool.rs:131-142` is exact.

The choke-point argument holds — `run` is the only path to `.output()` in either backend (`mutool.rs:41`, `:76`, `:82`; `tesseract.rs` likewise). Remediation is sound: both files already import `std::process::{Command, Output}`, both call sites pass a descriptive label, and moving it to `src/pdf/mod.rs` as `pub(crate)` needs no signature change.

Concern `advisory` stands.

## `dead-public-vocabulary-and-report-helpers`

**ADJUSTED** — marker only (`89`→`2`).

Evidence at `src/terms.rs:88-99` is verbatim and exact (12 lines, doc comment through the `drop(rename(...))` line).

All four claims verified:
- `Terms::localize` — one caller in the entire tree, `tests/terms.rs:70`. Zero in `src/`. ✓
- Production reaches vocabulary localization only through `localize_locate` (`src/cli.rs:761`), which calls `localize_entry` internally (`src/terms.rs:130`). ✓ (Strictly, `localize_entry` has no direct production caller — it is reached transitively. The finding's phrasing is fine.)
- Module doc at `src/terms.rs:4` does say documents are "localized back on output," and `check --format json` (`cli.rs:1197`) prints the report with no localization pass. The reader-misleading claim holds. ✓
- `ValidationReport::has_warnings` (`src/validate.rs:33-36`) — zero callers in `src/` or `tests/`; the only other occurrence in the repo is the plan file. `record/superpowers/plans/2026-08-09-phase2-material-tokens-and-section-paths.md:114` is exactly `pub fn has_warnings(&self) -> bool {`. ✓ Citation is precise.

Remediation is sound — the fork (wire it up vs. delete it and narrow the doc) is the right framing.

Concern `advisory` stands.

---

## Summary

| Finding | Verdict | Correction |
|---|---|---|
| `validate-spawns-one-mutool-per-cited-page` | **CONFIRMED** | optional: "N spawns" → per source per summary |
| `propose-regenerates-claim-independent-spans-per-claim` | **CONFIRMED** | — |
| `resolve-unit-linear-scan-called-twice-per-locator` | **ADJUSTED** | weak-section gate qualifier; "always"; `UnitKind` needs `Hash`/`Ord` |
| `pdf-verification-unbounded-when-candidates-fail` | **CONFIRMED** | — |
| `owned-json-value-borrowed-then-deep-cloned` | **ADJUSTED** | `preserve_order` off ⇒ `sort_keys` reorders nothing; strengthen mechanism |
| `source-names-allocates-a-vec-to-answer-a-lookup` | **ADJUSTED** | two callers, two test sites |
| `weak-section-list-lowercased-per-comparison` | **CONFIRMED** | — |
| `release-profile-left-at-cargo-defaults` | **CONFIRMED** | — |
| `structured-output-contracts-untested` | **ADJUSTED** | markers `3-9`; add `check`/`audit`/`locate` to the untested list |
| `just-check-is-not-a-complete-gate` | **CONFIRMED** | both halves + dead `test-ci` all verified |
| `no-automated-ci-for-the-documented-gate` | **CONFIRMED** | optional: temporal is repo-wide, not per-file |
| `justfile-loads-repo-supplied-dotenv` | **CONFIRMED** | calibration + `introduced` date conflicts with its sibling |
| `license-allowlist-inherited-from-absent-dependency-tree` | **ADJUSTED** | markers `5`/`8`/`15`; `monthly_commits` → `[…,0,1]` |
| `summary-discovery-recursion-untested` | **ADJUSTED** | markers `3-4` / `11-16` |
| `markdown-coordinates-only-pinned-at-line-one` | **ADJUSTED** | marker `8`; list-item column example is wrong |
| `transitive-syn-duplicate-is-not-actionable` | **ADJUSTED** | markers `17-19`; `monthly_commits` → `[…,0,1]` |
| `validate-document-carries-seven-concerns` | **ADJUSTED** | marker `1-16`; "seven" vs nine enumerated |
| `check-and-audit-duplicate-the-summary-loop` | **ADJUSTED** | marker `1-18` |
| `duplicated-issue-constructors` | **ADJUSTED** | marker `1-15` |
| `duplicated-subprocess-runner` | **ADJUSTED** | marker `1-12` |
| `dead-public-vocabulary-and-report-helpers` | **ADJUSTED** | marker `2` |

Zero disputed. Every evidence block in all 21 findings is verbatim and every `locations[].start_line`–`end_line` range is exact — transcription quality is high. The defects are concentrated in marker line numbering (10 findings), two `monthly_commits` arrays, and four substantive mechanism/remediation corrections (`resolve-unit`, `owned-json-value`, `source-names`, `markdown-coordinates`).

## Observations (not findings — no action requested)

1. `MarkdownUnit.column` is computed in **bytes**, not characters (`src/markdown.rs:252`: `offset.saturating_sub(line_start) + 1`). A unit preceded by non-ASCII text on the same line reports a column a human counting characters cannot reproduce. Not in my scope, and it may be intentional (locators round-trip through the same function), but no finding in the three narratives I reviewed mentions it, and `markdown-coordinates-only-pinned-at-line-one` is the finding that would have caught it if the suite tested a column above 1.
2. The nine-item list in `validate-document-carries-seven-concerns` and the seven-field doctor JSON in `structured-output-contracts-untested` both point at `validate_document`/`doctor` as the two places where a contract and its description have already drifted apart once. Not worth a finding; worth a sentence in the verdict if you want one.

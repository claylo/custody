# Review 2 — `published-contract` + `type-and-api` (14 findings)

Reviewed against working tree at `b78d869585b48532c90155d6f70b9486687aef54` (clean except the untracked audit dir). No tests run, no binary executed — source + README reading only.

**Summary:** 8 CONFIRMED, 6 ADJUSTED, 0 DISPUTED. Every evidence block in scope is verbatim-accurate and every `start_line`–`end_line` range is exact. The corrections below are: two broken `evidence_markers` sets, one wrong error-code count, two wrong mechanism sentences, two line-number nits, and two concern-level recalibrations.

---

## Cross-cutting: `evidence_markers` are RELATIVE, not absolute

This affects two findings in my scope and I verified it in the renderer rather than guessing.

`build-report.js:40053` passes `startLineNumber: locations[0].start_line` to expressive-code, so the **gutter** shows absolute file lines. But the marker resolution at `build-report.js:31430-31433` is:

```js
const lineNumbers = parse_numeric_range(range);
lineNumbers.forEach((lineNumber, idx) => {
  const lineIndex = lineNumber - 1;
  codeBlock.getLine(lineIndex)?.addAnnotation(...)
```

`getLine(lineIndex)` indexes into the code block's own lines and ignores `startLineNumber` entirely. The `?.` swallows out-of-range indices, so an absolute marker number renders **no highlight and no label, silently**. The reference examples in `references/findings-schema.yaml.md` confirm relative numbering.

Twelve of the fourteen findings in my scope already use relative markers correctly. The two that don't are corrected below.

---

## `schema-declares-source-error-codes-the-code-never-emits`

**CONFIRMED**

- Evidence at `src/cli.rs:553-582` is verbatim and the range is exact (30 lines, `.error(` at 553 through `)` at 582).
- Markers `2 / 8 / 14 / 20 / 26` resolve to lines 554, 560, 566, 572, 578 — the five `ErrorMetadata::new(...)` names. Correct.
- README.md:309-315 is the Vocabulary paragraph and does say "**Error codes** are vocabulary-free … so a script consuming `--format json` is portable across corpora." README.md:166-168 does say the CLI Spec describes "every … error code." Both quotes check out.
- Mechanism verified independently. `src/validate.rs:112-113` builds `format!("{source_name}/markdown")` / `{source_name}/pdf`, and the five interpolating sites are `src/validate.rs:408` (`{label}_source_mismatch`), `:427` (`{label}_hash_mismatch`), `:440` (`{label}_read_failed`), `:461` (`{label}_source_outside_repo`), `:475` (`{label}_source_unresolvable`). I grepped every `ErrorMetadata::new(` (42 total) and every emitted code literal: **none of the five `source_*` names is emitted anywhere.** Count of 5 is correct.
- The naming-drift observation is exactly right: declared `source_hash_mismatch` vs. runtime suffix `_hash_mismatch`, declared `source_outside_repo` vs. runtime `_source_outside_repo`.
- `significant` is right — this is the documented "primary interface for agents," and the failure is silent fall-through.

**Optional strengthener (not a correction).** The prior audit's `actions-taken.md:47` states: *"CLI Spec now declares all 37 static error codes and 5 dynamic code patterns emitted at runtime, up from 18."* Those five `source_*` entries **are** the attempted declaration of the dynamic patterns. Adding one sentence to the mechanism would make this finding much harder to wave off:

> The prior remediation recorded these five entries as declarations of "5 dynamic code patterns emitted at runtime" (`record/audits/2026-08-10-16-full-repo/actions-taken.md:47`), but `ErrorMetadata.kind` is a literal string with no pattern facility, so the declaration and the emission never meet.

---

## `runtime-error-codes-absent-from-cli-spec`

**CONFIRMED**

- Evidence at `src/cli.rs:844-862` is verbatim; range exact (19 lines).
- Markers `7` → line 850 (`summary_parse_failed`) and `15` → line 858 (`id_mismatch`). Correct.
- "the 42 `ErrorMetadata` declarations" — I counted exactly 42. Correct.
- The duplicate emission sites at `src/cli.rs:917` and `:933` are real (`AuditSummary` construction).
- README.md:164-168 is the `### schema` section with both quoted phrases. Correct.
- **Exhaustiveness verified.** I enumerated every statically emitted code across `src/validate.rs`, `src/evidence.rs`, `src/review.rs`, `src/cli.rs`: 39 distinct codes. 37 are declared; exactly two — `summary_parse_failed` and `id_mismatch` — are not. The finding's claim of two, and only two, is correct and exhaustive.
- Remediation is trivially sound (two more `.error(...)` links in the existing builder chain).

### The three error-code findings do not overlap

Verified as three genuinely distinct claims with three distinct code-level facts:

| Finding | Claim | Verified count |
|---|---|---|
| `schema-declares-source-error-codes…` | declared, never emitted | **5** (`source_hash_mismatch`, `source_mismatch`, `source_read_failed`, `source_outside_repo`, `source_unresolvable`) |
| `runtime-error-codes-absent-from-cli-spec` | emitted, never declared | **2** (`summary_parse_failed`, `id_mismatch`) |
| `error-code-registry-is-hand-maintained` | no mechanism keeps the sets in sync | 42 declarations vs. 39 static + 5 dynamic emission sites, joined by nothing |

The arithmetic is self-consistent: 42 declared = 37 declared-and-emitted + 5 declared-only; 39 emitted = 37 + 2 emitted-only. They share a remediation (the declaration-vs-emission test) and cross-reference each other correctly. Keep all three.

---

## `propose-json-shape-varies-by-summary-count`

**CONFIRMED**

- Evidence at `src/cli.rs:1010-1016` is verbatim; range exact. Marker `2-5` → lines 1011-1014. Correct.
- The two shapes are real and top-level-distinct: `reports.len() == 1` serializes a bare `ProposalReport` (`{id, claims}`); anything else — including **zero** — serializes `{"summaries": [...]}`. The `len() == 0` observation in the mechanism is correct.
- `src/cli.rs:315-330` declares `propose`'s output as exactly two fields, `id` (string) and `claims` (object[]). `summaries` is undeclared. Correct.
- README.md:154-162 (`### propose`) says nothing whatsoever about JSON output. Correct.
- **On the deliberate-decision question:** yes, this describes the prior audit's decision (`actions-taken.md:28`: *"single-ID output is unchanged for backward compatibility"*), and the finding says so in its first sentence. **It still stands**, and the reason is stronger than backward compatibility: the prior fix changed observable output without updating either the CLI Spec or the README, and this project's governing rule is that documentation wins. The undeclared `summaries` key is a contract gap regardless of which shape is chosen. There is also no external consumer the compatibility argument protects — the crate is unpublished and `propose` is advisory.
- `significant` is defensible: a schema-generated parser breaks on a data-dependent shape switch with no flag to key off.

---

## `error-code-registry-is-hand-maintained`

**ADJUSTED** — one wrong count, one wrong line range, one wrong file range, and broken markers. The finding itself holds.

**1. Count and declaration range.** Replace in `mechanism`:

```
          `schema_metadata` declares 37 error codes as string literals across
          src/cli.rs:331-583.
```

with:

```
          `schema_metadata` declares 42 error codes as string literals across
          src/cli.rs:331-582.
```

(42 `ErrorMetadata::new(` calls; the first `.error(` opens at 331 and the last closes at 582 — 583 is the function's closing brace.)

**2. evidence.rs range.** The `issue(` codes in `src/evidence.rs` run from line 197 to line 403, not `194-327` — the cited range stops short of seven codes (`empty_source`, `invalid_sha256`, `empty_exact`, `unnormalized_exact`, `invalid_markdown_line`, `invalid_markdown_column`, `invalid_pdf_page`). Replace:

```
          the twenty-odd codes in
          `validate_evidence_structure` (src/evidence.rs:194-327),
```

with:

```
          the seventeen codes in
          `src/evidence.rs:197-403`,
```

**3. Markers are absolute and render as nothing.** Replace the whole `evidence_markers` block with:

```yaml
        evidence_markers:
          - lines: "4"
            type: mark
            label: "declared here"
          - lines: "10-11"
            type: mark
            label: "exit_code 0 for a warning-severity code"
```

**4. effort_notes.** Replace `"37 declarations and ~40 emission sites must move to shared constants."` with `"42 declarations and ~44 emission sites must move to shared constants."`

Everything else verified correct: evidence at `src/cli.rs:539-552` is verbatim (14 lines, exact); `src/validate.rs:90` is `empty_markdown_candidates`; `src/validate.rs:328` is `weak_section_only`; `src/review.rs:106-187` encloses the six review codes; `src/cli.rs:850, 858, 917, 933` are the four inline sites; `src/validate.rs:427` is `format!("{label}_hash_mismatch")`. There is no shared constant, enum, or test joining the two sets — grepped and confirmed. The "18 → 37 by hand" history matches `actions-taken.md:47`. `moderate` is right for a root-cause/process finding whose symptoms are already filed separately.

---

## `default-output-becomes-json-when-redirected`

**CONFIRMED**

- Evidence at `src/cli.rs:183-185` is verbatim; range exact; marker `1` correct.
- Mechanism verified in the dependency, not inferred. `librebar-0.6.0/src/cli.rs:197` — `output_format()` calls `output_format_for(std::io::stdout().is_terminal())`; `:80-86` — `resolve_for` maps `Auto if stdout_is_terminal => Text`, `Auto | Json => Json`. So the default format is Text **only** on a TTY and JSON everywhere else. The documented "YAML-ready" / "commented YAML" default flips on every redirect and pipe. Confirmed.
- README.md:130-133 ("The default YAML-ready output keeps those diagnostics in a comment so they are not persisted in the strict evidence contract") and README.md:160-162 ("Human-readable output is commented YAML safe to paste and edit, with a suggested `receipts locate` command") are both quoted accurately at the cited lines.
- Remediation works: `output_format_for(bool)` is `pub const fn` at `librebar-0.6.0/src/cli.rs:205` and honors `--format json` / `--legacy_json` before the terminal branch, so passing `true` yields text for `auto` and JSON for explicit `--format json`. Compatible with edition 2024 / MSRV 1.89 / `forbid(unsafe_code)`.

Two non-blocking notes the lead may fold in:

- librebar documents `output_format_for` as "useful for deterministic rendering tests. Normal applications should call `output_format`." The fix is correct but is using a test-oriented accessor; if that reads badly, the equivalent is matching `cli.common.format` directly for the two commands.
- The suggested test ("assert the first line begins with `# PDF match diagnostic:`") only holds when a PDF match exists — `src/cli.rs:1261-1263` guards that line behind `payload.get("pdf_match")`. Worth qualifying as "with a fixture whose locator matches the PDF."

---

## `undocumented-user-config-and-environment-layers`

**CONFIRMED**

- Evidence at `src/config.rs:185-199` is verbatim; range exact (15 lines). Markers `3` → line 187 and `10-12` → lines 194-196. Correct.
- `src/config.rs:204-208` does populate `config_file` from `sources.project_file` alone. Correct.
- README.md:78-86 is the discovery paragraph and does read as exhaustive (walk up, `.config/receipts.yaml` → `.receipts.yaml` → `receipts.yaml`, `.git` boundary, defaults, `--config FILE`). It never mentions a user file or environment variables. Correct.
- Every dependency claim verified in `librebar-0.6.0/src/config.rs`: `include_user_config: true` and `environment_source: Some(Arc::new(ProcessEnvironment))` in the `Default` impl at `:494` / `:497`; merge order at `:598-650` is default → user file → project file → environment overlay → explicit `--config` file; `find_user_config` at `:751` uses `directories::ProjectDirs::from("", "", "receipts").config_dir()`, i.e. `~/Library/Application Support/receipts/config.{yaml,toml,json}` on macOS. The `RECEIPTS_*` prefix and the `__` path separator are confirmed at `src/config/environment.rs:243` (`suffix.split("__")`) and `:311`.
- `pdf.ocr.enabled` (`src/config.rs:93`) and `corpus.summaries` are real keys, so both example variables in the mechanism are valid.
- Remediation compiles as written: `with_user_config(bool)` is `pub const fn` at `librebar-0.6.0/src/config.rs:510` and `without_environment()` at `:546`. The alternative plumbing option is also viable — `ConfigSources` really does carry `user_file` and `environment_variables`.
- `moderate` is right.

---

## `summary-id-constraints-undocumented`

**CONFIRMED**

- Evidence at `src/corpus.rs:168-179` is verbatim; range exact (12 lines); marker `2-7` → lines 169-174. Correct.
- `validate_id` is called from all three path resolvers (`src/corpus.rs:123, 138, 152`), so "every command that resolves a path" holds.
- README.md:106-115 is the Run block using bare `ID` / `[ID...]`. I grepped the whole README: the rule appears nowhere, and `smith-2019` is the only example ID.
- `--help` carries nothing either — `ids: Vec<String>` in `CheckArgs` / `AuditArgs` / `ProposeArgs` (`src/cli.rs:87, 101, 106`) has no doc comment. The `invalid_id` description at `src/cli.rs:503` is exactly the quoted string.
- `advisory` is right.

---

## `entire-crate-is-a-published-public-api`

**ADJUSTED** — it is **not** a re-report, but the concern level and one remediation clause need correcting.

**Is it narrower than the accepted findings? Yes.** The prior acceptance (`actions-taken.md:54-62`) covers `cache-manifest-invariants-are-bypassable`, `unvalidated-discovered-state`, and `public-apis-erase-error-types`, all on the rationale "there are no library consumers — the `pub` surface exists for internal/test convenience only." This finding's load-bearing evidence is `Cargo.toml:1-10` — `license`, `repository`, `keywords`, `categories`, `description`, no `publish = false` — which none of the accepted findings considered and which the acceptance rationale does not answer. Keep it.

Evidence checks out: `src/lib.rs:1-17` is verbatim (17 lines, exact); markers `3`, `9-10`, `3-17` are correct (`start_line: 1` makes relative and absolute coincide here). `normalize` and `output` really are one-function modules (`src/normalize.rs:5`, `src/output.rs:5`). Public-item count: 263 lines matching `pub ` minus the 15 `pub mod` ≈ 248, so "~240" is fair.

**1. Concern: `significant` → `advisory`.**

```
        concern: advisory
```

By the skill's own ladder, `significant` is "meaningful risk under realistic conditions" and `advisory` is "not a vulnerability, but limits future safety." Nothing here is exploitable or lossy today; the entire cost is contingent on a `cargo publish` that has not happened on an unreleased 0.1.0. This is the textbook shape of `advisory`, and filing it there also stops it competing with the genuinely-now findings in the report's ordering. (If you keep `significant`, the mechanism should say what makes the publish imminent rather than possible — I could not find anything in the tree that does.)

**2. The remediation's binary-path clause is nearly a no-op as written.** Replace:

```
          If this is a binary with a test-only
          library target, add `publish = false` to `Cargo.toml` and demote the modules
          to `pub(crate)`, keeping only what integration tests import (integration
          tests link against the public API, so keep that set explicit and small).
```

with:

```
          If this is a binary with a test-only
          library target, add `publish = false` to `Cargo.toml` — that one line removes
          the compatibility obligation outright and costs nothing. Module-level demotion
          buys much less than it looks: `tests/` imports 13 of the 15 modules, several at
          depth (`pdf::cache::{CacheManifest, OcrCache, OcrProfile, cache_key}`,
          `pdf::tesseract::{OcrEngine, PageRenderer, ocr_page, parse_tsv, ...}`,
          `propose::{propose_document, split_sentences}`, `validate::validate_document`,
          `normalize::normalize`), leaving only `cli` and `output` demotable as whole
          modules. The reachable win is per-item: `#[doc(hidden)]` on the test-only
          surface plus a curated `pub use` façade for the three product entry points.
```

I verified that module list by grepping every `use receipts::` in `tests/` — only `cli` and `output` are absent.

---

## `coordinate-primitives-are-interchangeable-usize`

**ADJUSTED** — two line-number nits in the mechanism prose. Evidence and reasoning are otherwise exact.

- Evidence at `src/markdown.rs:163-174` is verbatim; range exact (12 lines). Markers `4-5` → 166-167, `7-9` → 169-171, `12` → 174. All correct.
- Both other locations check out: `src/validate.rs:166-171` is the `resolve_unit` call with `locator.markdown.line` at 169 and `.column` at 170 (adjacent, same type, transposable); `src/evidence.rs:412-418` is `fn issue(..., claim: Option<usize>, locator: Option<usize>)`, and `src/validate.rs:486-492` has the identical shape.
- Field inventory verified: `MarkdownLocator.line/.column` (`src/evidence.rs:71-72`), `PdfLocator.page` (`:81`), `ClaimEvidence.claim`, `CoverageScore.matched/.required` (`src/propose.rs:51-52`), `UnitBuilder.start` (`src/markdown.rs:49`), `build_line_starts` (`:237`) — all bare `usize`.

**Correction — two of the six one-based guard sites are off by one line.** In `mechanism`, replace:

```
          `markdown.rs:169`,
          `evidence.rs:383-409`, `mutool.rs:31`, `mutool.rs:67`, `tesseract.rs:238`, and
          `cache.rs:46`
```

with:

```
          `markdown.rs:169`,
          `evidence.rs:383-409`, `mutool.rs:31`, `mutool.rs:66`, `tesseract.rs:238`, and
          `cache.rs:47`
```

(`src/pdf/mutool.rs:66` is `if page == 0 {` — 67 is the `bail!`. `src/pdf/cache.rs:47` is `if page == 0 {` — 46 is the closing brace of the SHA-256 guard above it. The other four are exact, and there really are six.)

Remediation is sound: `#[repr(transparent)]` + `#[serde(transparent)]` newtypes over `usize` with fallible constructors need no unsafe, work on edition 2024 / MSRV 1.89, and preserve the on-disk schema. One implementation note worth carrying in `effort_notes` if you want it: the `--line` / `--column` / `--page` clap arguments will need `value_parser` or `FromStr` on each newtype. `significant` is well-calibrated — this is the domain's central failure mode.

---

## `propose-duplicates-locator-types`

**ADJUSTED** — markers are absolute (render as nothing); concern is one notch hot.

- Evidence at `src/propose.rs:55-69` is verbatim; range exact (15 lines).
- Duplication confirmed field-for-field: `MarkdownMatch{line, column, unit, section}` vs. `MarkdownLocator` (`src/evidence.rs:70-76`); `PdfMatch{page, backend}` vs. `PdfLocator` (`src/evidence.rs:80-83`). `print_propose_human` reads all six fields at `src/cli.rs:1050-1067` and prints the `receipts locate` command at `:1084-1091`. Mechanism holds.
- Remediation is accurate, including the subtle part: `MarkdownLocator.section` carries `#[serde(default, skip_serializing_if = "Vec::is_empty")]` while `MarkdownMatch.section` does not, so substituting the type omits `section` from propose JSON when empty. The finding already names this and offers `#[serde(flatten)]` as the shape-preserving alternative. `MarkdownLocator` additionally derives `Deserialize` + `deny_unknown_fields`, which is harmless in a `Serialize`-only `Candidate`.

**1. Markers.** Replace the whole `evidence_markers` block with:

```yaml
        evidence_markers:
          - lines: "3-8"
            type: mark
            label: "identical to evidence::MarkdownLocator"
          - lines: "12-15"
            type: mark
            label: "identical to evidence::PdfLocator"
```

**2. Concern: `significant` → `moderate`.**

```
        concern: moderate
```

No drift exists yet and nothing is currently wrong in the output; the risk is future silent divergence between two 4-field structs in the same crate, which is the definition of a robustness/maintainability gap rather than meaningful present risk. Filing it at `moderate` also keeps it from outranking `byte-offset-published-as-a-markdown-column` in the same narrative, which describes a value that is already wrong on disk. Flagging rather than insisting — if the lead reads "propose silently stops being able to describe the locator it is proposing" as a correctness risk, `significant` is arguable.

---

## `byte-offset-published-as-a-markdown-column`

**CONFIRMED**

- Evidence at `src/markdown.rs:247-254` is verbatim; range exact (8 lines). Markers `1`, `6`, `7` correct.
- `src/markdown.rs:34-43` (`MarkdownUnit`) and `src/evidence.rs:68-76` (`MarkdownLocator`) are the right supporting locations.
- Mechanism verified: `parse_units` builds the parser with `.into_offset_iter()` at `src/markdown.rs:60` and passes `range.start` into `UnitBuilder::new` at `:76, :90, :93, :98, :101`. Those are byte offsets. `line_column_from_index` subtracts the line-start byte offset and names the result `column`, which flows to `MarkdownUnit.column` → `MarkdownLocator.column` (serialized) → the `--column` CLI flag (`src/cli.rs:655, 658, 689`). No char-boundary conversion anywhere.
- The internal-consistency argument is right — the same function writes and re-derives the value, so `resolve_unit` never disagrees with itself.
- "`tests/markdown.rs` only ever asserts `column == 1`" is accurate: `tests/markdown.rs:27` is the only column assertion in the file, and every other `column` in `tests/` is a literal `1`.
- Remediation is sound both ways; `source[line_start..offset].chars().count() + 1` is safe here because `line_start` and `offset` are both pulldown-cmark boundaries.
- `moderate` is right — wrong-but-self-consistent data with an external-consumer cost.

---

## `debug-formatting-leaks-into-user-facing-messages`

**CONFIRMED**

- Evidence at `src/validate.rs:189-198` is verbatim; range exact (10 lines). Markers `5` → line 193 (the `{:?}` format string) and `6` → line 194. Correct.
- `src/markdown.rs:19-32` is `impl UnitKind` with the exact doc comment "The serialized name, so human output and JSON agree." and the `list_item` / `table_cell` / `code_block` mappings. `src/review.rs:174-186` is the `unsupported_verdict` arm.
- Call-site inventory verified exactly as claimed: `as_str()` used at `src/validate.rs:225, 258, 269` and `src/cli.rs:1057, 1067` (five sites), bypassed at `src/validate.rs:193`. `PdfBackend::as_str` exists at `src/evidence.rs:94`; `Verdict` has no `as_str` anywhere; none of the three enums implements `Display`. Confirmed by grep.
- The `{:?}` on the verdict is at `src/review.rs:180` (format string) with the value on 181 — the finding's "review.rs:180" is the right pointer.
- `Verdict` serializes `rename_all = "lowercase"` (`src/review.rs:11`), so `Unsupported` vs. the file's `unsupported` is real.
- `src/validate.rs:181` genuinely is a `Vec<String>` section path where `{:?}` is intended — the carve-out is correct.
- Remediation is sound and behavior-preserving. `moderate` is right.

---

## `candidate-pdf-option-is-never-none`

**ADJUSTED** — one mechanism sentence is not supported by the cited lines.

- Evidence at `src/propose.rs:176-197` is verbatim; range exact (22 lines). Markers `2-4` → 177-179 (the `let … else { continue; }` guard) and `18` → 193 (`pdf: Some(PdfMatch {`). Correct.
- The core claim holds: this is the only `Candidate` construction site in the crate, it sits after the guard, and it always writes `Some`. `None` is unconstructible.
- `src/cli.rs:1055-1058` is the `map_or_else` with the unreachable `"  (no native PDF match)"` branch; `src/cli.rs:1073` is the `first.pdf.as_ref()` check. Both cited correctly.

**Correction.** `src/cli.rs:315-330` declares only two output fields for `propose` — `id` and `claims` — and the `claims` description (`:321-323`) is the field-name list `{claim, required_tokens, uncovered_tokens, advisory_tokens, candidates}`. It says nothing at all about a candidate's sub-structure, so it cannot be telling consumers a candidate may lack a PDF match. Replace in `mechanism`:

```
          The `Option<PdfMatch>` field therefore models a state the
          code cannot produce, and it costs at both ends: the JSON schema declared at
          `cli.rs:321-323` tells consumers a candidate may lack a PDF match, and
          `cli.rs:1055-1058` carries a `"  (no native PDF match)"` branch that can
          never print.
```

with:

```
          The `Option<PdfMatch>` field therefore models a state the
          code cannot produce, and it costs at both ends: `cli.rs:1055-1058` carries a
          `"  (no native PDF match)"` branch that can never print, and `cli.rs:1072-1073`
          guards the pasteable `receipts locate` command behind an `Option` that is always
          `Some`. The declared schema (`cli.rs:315-330`) describes `propose` output only
          down to the `claims` field list, so a consumer reading the type gets the
          optionality from the serialized `pdf` key alone — which in practice never
          appears as `null`.
```

Everything else confirmed. The remediation (drop the `Option`, the `map_or_else`, and the `as_ref()` check) leaves the JSON shape unchanged because `Candidate.pdf` has no `skip_serializing_if`. `advisory` is right.

---

## `report-types-are-write-only`

**ADJUSTED** — the derive inventory is wrong for three of the ten types.

- Evidence at `src/propose.rs:21-36` is verbatim; range exact (16 lines). Markers `2` → line 22 and `9` → line 29. Correct.
- `tests/cli.rs:384-392` is the untyped-`Value` decode with string indexing, exactly as described (`:385` decodes, `:391` indexes `claim["required_tokens"]`). Correct.
- The core claim — **no report type derives `Deserialize`** — is true for all ten types, and the asymmetry against the input types is real: `Evidence` (`src/evidence.rs:25`), `ClaimEvidence` (`:46`), `Locator` (`:54`), `MarkdownLocator` (`:68`), `PdfLocator` (`:78`), `ReviewEntry` (`src/review.rs:19`) all derive both directions.

**Correction — `PartialEq` is not uniformly missing.** Three of the listed types already have it:

- `src/evidence.rs:103` — `Severity`: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]`
- `src/evidence.rs:111` — `EvidenceIssue`: `#[derive(Debug, Clone, PartialEq, Eq, Serialize)]`
- `src/pdf/mod.rs:29` — `PdfBbox`: `#[derive(Debug, Clone, Copy, PartialEq, Serialize)]`

Replace in `mechanism`:

```
          `ProposalReport`, `ClaimProposal`, `Candidate`, `CoverageScore`,
          `MarkdownMatch`, `PdfMatch`, `ValidationReport`, `EvidenceIssue`, `Severity`,
          and `PdfBbox` all derive `Serialize` and stop there.
```

with:

```
          `ProposalReport`, `ClaimProposal`, `Candidate`, `CoverageScore`,
          `MarkdownMatch`, `PdfMatch`, `ValidationReport`, `EvidenceIssue`, `Severity`,
          and `PdfBbox` all derive `Serialize` with no `Deserialize`. Seven of the ten —
          every type in the `propose` report family plus `ValidationReport` — also lack
          `PartialEq`; only `Severity` (src/evidence.rs:103), `EvidenceIssue`
          (src/evidence.rs:111), and `PdfBbox` (src/pdf/mod.rs:29) derive it.
```

and in the same paragraph replace:

```
          The missing `PartialEq` compounds it — reports cannot be compared with
          `assert_eq!`, so round-trip tests are not expressible either.
```

with:

```
          The missing `PartialEq` on the report family compounds it — a decoded
          `ProposalReport` or `ValidationReport` still cannot be compared with
          `assert_eq!`, so round-trip tests are not expressible either.
```

The remediation is otherwise sound and I checked that it compiles end to end: adding `Deserialize` to `EvidenceIssue` requires it on `Severity` (in the list), and to `Candidate` requires it on `UnitKind` (`src/markdown.rs:8` — already has it) and `PdfBackend` (`src/evidence.rs:85` — already has it). The `f64`-blocks-`Eq`-not-`PartialEq` note and the `PdfBbox`-derives-`Copy` note are both correct. `advisory` is right.

---

## Observations (outside my scope, no action taken)

1. **The absolute-marker bug is not confined to my two narratives.** Spot-checking the rest of `findings.yaml`, the same pattern appears in at least `external-tool-paths-resolved-from-ambient-path` (`"17"`, `"23"` against a block starting at 14 → should be `"4"`, `"10"`), `external-tool-versions-probed-never-validated` (`"594-597"`, `"598-601"` against a block starting at 594 → should be `"1-4"`, `"5-8"`), `stext-schema-drift-yields-silent-empty-extraction` (`"152-153"`, `"158-159"`, `"164-165"` against a block starting at 144 → should be `"9-10"`, `"15-16"`, `"21-22"`), and `structured-output-contracts-untested` (`"634-640"` against a block starting at 632 → should be `"3-9"`). Worth a single sweep across the file before rendering rather than per-narrative patches.

2. `weak_section_only` is declared with `exit_code(0)` (`src/cli.rs:549`) while every other declaration uses 1 — the `error-code-registry` marker labels this but no finding examines whether librebar's `ErrorMetadata.exit_code` is meaningful for a warning-severity code. Not a finding, just unexamined.

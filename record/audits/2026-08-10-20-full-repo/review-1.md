# Review 1 — evidence-trust, external-tool, failure-modes

Reviewer: `audit-reviewer`. Commit `b78d869`. 13 findings in scope.
Verification method: source reading only (no tests, no binary run). One filesystem
probe was run in the scratchpad to settle the walk-termination disagreement; it did
not touch the repo.

---

## Cross-cutting correction: `evidence_markers` line numbers are RELATIVE

This affects three findings in my scope and is worth checking across the other
narratives too.

`buildMetaString` (`scripts/build-report.js:40008`) passes marker ranges straight to
expressive-code's text-markers plugin, which does:

```js
const lineIndex = lineNumber - 1;
codeBlock.getLine(lineIndex)?.addAnnotation(...)
```

(`scripts/build-report.js:31432-31433`) — a **1-based index into the evidence block**,
not the file. `startLineNumber` is consumed only by the separate line-numbers plugin
(`scripts/build-report.js:32129`), so it never shifts markers. The schema examples in
`references/findings-schema.yaml.md:89-97` confirm this convention (`start_line: 393`,
marker `lines: "2"`).

Consequence: any marker whose number exceeds the evidence block's line count is
**silently dropped** by the `?.` optional chain — no marker, no label, no error.
Three findings in my scope used absolute file lines and currently render zero markers.
Corrections are given per finding below.

---

## evidence-trust

### `ocr-cache-entries-are-unauthenticated-evidence`

**CONFIRMED**

I tried to break this and could not. Every link holds.

- **Evidence.** `src/pdf/cache.rs:115-133` is verbatim correct, 19 lines, exact match
  including the dedent. Markers `8-12`, `13-15`, `16-18` are relative and land on the
  manifest read (122-126), the `matches_request` gate (127-129), and the TSV read
  (130-132). Correct as written.
- **`matches_request` compares only attacker-copyable fields.** `src/pdf/cache.rs:76-82`
  compares `pdf_sha256`, `page`, `profile`, `toolchain_sha256`. Two additional facts
  strengthen the finding rather than weaken it: (a) `toolchain_sha256` is *derived* —
  `CacheManifest::new` sets it to `cache_key(&profile)` (`src/pdf/cache.rs:58`, `:157-159`),
  so it is not an independent factor, only a restatement of `profile`; (b)
  `render_rotation_degrees` is range-checked (`matches!(self.render_rotation_degrees, 0|90|180|270)`)
  but **never compared against `expected`**, so a forged manifest can declare any legal
  rotation and still match. Nothing binds `ocr.tsv`. Confirmed.
- **Full call chain verified.** `check` (`src/cli.rs:867`) / `audit` (`src/cli.rs:951`)
  → `validate_document` → `extract_pdf_page` via `pdf_pages.entry(key).or_insert_with`
  (`src/validate.rs:237-248`) → `provider.ocr_page` for `PdfBackend::TesseractOcr`
  (`src/validate.rs:387`) → `PdfTools::ocr_page` (`src/pdf/mod.rs:126-140`) →
  `ocr_page_with_profile` (`src/pdf/tesseract.rs:229-244`) → `cache.load` at
  `src/pdf/tesseract.rs:242`, early-returning `extracted_page(page, &tsv)` at `:243` →
  `parse_tsv` (`src/pdf/tesseract.rs:284`) → `exact_count(text, &locator.exact) == 1`
  (`src/validate.rs:250-251`) → no issue pushed → locator passes. Every hop exists.
- **Corpus-relative `cache.root` is genuinely reachable.** `CacheConfig.root: Option<String>`
  (`src/config.rs:75-79`), consumed at `src/corpus.rs:36-46`, never passed through
  `validate_template`. `config::validate` (`src/config.rs:229-285`) never touches it.
  `README.md:328-330` documents it verbatim. Confirmed.
- **Preconditions the attacker also controls, all from the same `receipts.yaml`:**
  `pdf.ocr.enabled` defaults to `true` (`src/config.rs:102-111`), and `lang`/`dpi`/`psm`
  feed `OcrProfile` directly (`src/pdf/tesseract.rs:195-213`). `render_command`,
  `orientation_command`, `recognition_command`, and `name` are pure functions of those
  three (`src/pdf/tesseract.rs:21-31`). So the *only* unknowns are `mutool_version` and
  `tesseract_version` — exactly as the mechanism states, and both are emitted by
  `doctor` (`src/cli.rs:636-637` JSON, `:644-646` human).
- **Doc contradictions verified verbatim.** `src/pdf/cache.rs:88-89` ("a hit is a
  determinism guarantee") and `README.md:334` ("The cache is never evidence authority").
- **Remediation** is sound and compatible: an added `tsv_hmac: String` field on
  `CacheManifest` needs a `#[serde(default)]` because the struct carries
  `deny_unknown_fields` (`src/pdf/cache.rs:29`) and old entries lack the field —
  treating a defaulted/empty MAC as a miss is exactly the described behavior, so this
  works. No `unsafe`, no new deps required (`sha2 0.11` is already present; HMAC-SHA256
  is a few lines, or add `hmac`).
- **Concern.** `critical` is correct: the exploit path exists now, runs entirely through
  the validated CLI, and produces a false `valid` verdict — the tool's exact inversion.

No changes required.

### `cache-root-escapes-corpus-containment`

**CONFIRMED**

- **Evidence.** `src/corpus.rs:36-45` verbatim, 10 lines, exact. Markers `4-5` and `7`
  are relative and land on `if candidate.is_absolute() { candidate` and
  `root.join(candidate)`. Correct.
- **Mechanism.** All citations verified: `validate_template` at `src/config.rs:287-298`
  rejects absolute and `..` (`:291-296`); `README.md:74-76` states the guarantee
  verbatim; `config::load` at `src/config.rs:185-214` with the walk-up documented at
  `:183-184` and `README.md:78`; `OcrCache::store` writes at `src/pdf/cache.rs:135-153`;
  `doctor`'s unconditional `create_dir_all` at `src/cli.rs:605-610` and the
  `.doctor-<pid>` write/delete probe at `:611-619`, both before the `all_ok` gate at
  `:621`. Every claim holds.
- **Remediation** sound. Note for the implementer: `librebar`'s `ConfigLoader` already
  separates user from project config (`sources.project_file` at `src/config.rs:205-208`),
  so the "require absolute roots to come from user-level config" variant is implementable
  with what is there.
- **Concern.** `significant` is right — real writes to attacker-chosen paths, and it is
  the stated precondition for the `critical`.

No changes required.

### `summary-and-markdown-reads-skip-containment-guard`

**ADJUSTED** — evidence is correct; one clause of the mechanism overstates two of the
four call sites.

- **Evidence.** `src/cli.rs:1119-1125` verbatim, 7 lines, exact. Marker `2-4` relative,
  correct.
- **Verified correct:** `validate_resolved_source` at `src/validate.rs:451-481`
  canonicalizes and checks `starts_with(corpus.root())`; the comment at
  `src/validate.rs:125` reads exactly "Never read a source that resolves outside the
  corpus."; `validate_id` (`src/corpus.rs:168-179`) restricts `{id}` to
  `[a-z0-9-]` and cannot see symlinks; `Corpus::summary_path` (`src/corpus.rs:122-125`)
  does `validate_id` + `join` with no file-type or containment check; all four call
  sites exist as cited (`src/cli.rs:1119-1125`, `:663-666`, `:684-685`,
  `src/propose.rs:138-139`).
- **The problem.** The FIFO / `/dev/zero` sentence is applied to all four sites, but the
  two *Markdown* reads are already gated by an `is_file()` filter that follows the
  symlink and returns `false` for FIFOs and character devices:
  `resolve_markdown_for` → `.find(|path| path.is_file())` (`src/cli.rs:1182-1185`), and
  `propose_document` → `.find(|path| path.is_file())` (`src/propose.rs:138`). Those two
  can still read a symlink to an out-of-corpus **regular** file of unbounded size, but
  not a FIFO and not `/dev/zero`. The claim is fully correct for the two *summary*
  reads, which have no such filter.

**Replacement for the sentence beginning "Content disclosure is limited"** (copy-paste,
re-indent to the block's 10 spaces):

```
Content disclosure is limited — main.rs:4 prints the anyhow chain with `{error:#}` and
the YAML decode errors that surface generally carry positions rather than payload, while
`propose` and `locate` only emit spans that also occur in the PDF — so the practical
impact is unbounded reads of out-of-corpus files. The exposure differs by call site.
The two summary reads (`read_summary` at src/cli.rs:1119-1125 and `locate`'s at
src/cli.rs:661-666) go through `Corpus::summary_path` (src/corpus.rs:122-125), which
does `validate_id` and a `join` with no file-type check at all: a
`summaries/aaa.yaml -> /tmp/fifo` link makes `read_to_string` block forever, and a link
to `/dev/zero` exhausts memory, because NUL bytes are valid UTF-8 and the read never
reaches EOF. The two Markdown reads are narrower — `resolve_markdown_for`
(src/cli.rs:1182-1185) and `propose_document` (src/propose.rs:138) both select the
candidate with `.find(|path| path.is_file())`, and `is_file()` follows the symlink and
returns false for FIFOs and character devices — but a symlink to an out-of-corpus
regular file of arbitrary size is still read in full. The finding is the inconsistency
itself: an invariant the codebase states and enforces on one path is silently absent on
four others that open files first.
```

- **Remediation** sound. One implementation note: `validate_resolved_source` currently
  pushes into `&mut Vec<EvidenceIssue>` and returns `bool`, so lifting it to
  `Corpus::open_contained(path) -> Result<String>` is a signature change, not a move.
  The "confirm the result is a regular file" clause is the part that actually closes the
  FIFO/chardev hole and is not currently anywhere in `validate.rs`.
- **Concern.** `moderate` is right.

### `summary-walk-recurses-without-a-depth-bound`

**ADJUSTED** — the merged "target-dependent: ENAMETOOLONG or stack overflow" rewrite is
technically wrong. **Both** agents were wrong about the termination mode, and the real
impact is a different one.

- **Evidence.** `src/cli.rs:1157-1161` verbatim, 5 lines, exact. Markers `3` and `4`
  relative, correct. No change.
- **`is_dir()` follows symlinks** — correct.
- **There is no "constant path length" variant.** `walk_summaries` descends only via
  `entry?.path()`, which is `dir.join(file_name)`, so the accumulated path grows by at
  least one component on **every** level regardless of the link target. The
  stack-overflow branch of the rewrite describes a code path that does not exist.
- **Neither does ENAMETOOLONG fire.** The kernel's symlink-resolution cap is hit long
  first. Measured on this machine (macOS 26.4, APFS) with `summaries/loop -> ..`:
  resolution fails at the **33rd** `loop` component with `ELOOP` ("Too many levels of
  symbolic links") at a path length of **504 bytes**, against `getconf PATH_MAX / = 1024`.
  Linux caps at 40 links, ~600 bytes, against `PATH_MAX 4096`. Depth is therefore
  bounded at ~32-40 frames — nowhere near an 8 MB stack.
- **And it is not a command failure either, for the ancestor-link case.** At the depth
  that exceeds the cap, the failing call is `path.is_dir()` at `src/cli.rs:1159`.
  `Path::is_dir` swallows every metadata error and returns `false`, so `fs::read_dir` is
  never called on the over-limit path and the non-`NotFound` arm at `src/cli.rs:1151-1155`
  is **never reached** for this variant. Control falls to the `else` branch, `relative`
  is computed, and the `contains('/')` filter at `src/cli.rs:1171` discards it. The walk
  terminates *silently*. The current mechanism's central claim — that the error "converts
  that into a hard error that fails the whole command" — does not hold for
  `summaries/loop -> ..`.
- **The real impact is breadth, not depth.** Two loop links in one directory
  (`summaries/a -> .` and `summaries/b -> .`) branch the explored path set 2-ways per
  level up to the 32/40-level cap: on the order of 2^32 stat+opendir pairs. That is an
  effective hang, and it is what an unbounded walk actually costs here.
- **The `summaries/all -> /` variant is the one that does hard-fail**, and via a
  different errno: the walk leaves the corpus and the first `EACCES` from `fs::read_dir`
  is not `NotFound`, so it hits `src/cli.rs:1151-1155` and fails the whole command.
- **Still correct as written:** no bogus ID escalation (filter at `src/cli.rs:1162-1174`
  verified), every id-less invocation reaches it, and the recursion is new
  (introduced 2026-08-10).
- **Minor citation fix:** the mechanism says `summary_ids` is called at
  "src/cli.rs:834, 903, 987". The audit call is at **904** (line 903 is
  `let ids = if ids.is_empty() {`). 834 and 987 are correct.

**Replacement mechanism** (copy-paste, re-indent to the block's 10 spaces):

```
`walk_summaries` recurses once per subdirectory with no depth parameter and no
`symlink_metadata` check. `Path::is_dir` follows symlinks, so a symlink inside the
summaries tree that points at an ancestor (`summaries/loop -> ..`) sends the walk back
through the same directories, and every level allocates a `PathBuf`, a `ReadDir`, and a
stack frame. Depth alone is not the danger: the walk descends only via
`entry?.path()`, so the accumulated path grows by a component on every level, and the
kernel's symlink-resolution cap terminates the chain long before any resource limit —
measured on macOS 26.4, a `summaries/loop -> ..` chain fails at the 33rd `loop`
component with ELOOP at a path length of 504 bytes, against a PATH_MAX of 1024 (Linux
caps at 40 links). Worse, that failure is silent: the call that trips the cap is
`path.is_dir()` at src/cli.rs:1159, and `Path::is_dir` swallows the error and returns
false, so `fs::read_dir` is never reached and the non-`NotFound` arm at
src/cli.rs:1151-1155 never fires. The walk simply stops. The real cost is breadth. Two
loop links in one directory (`summaries/a -> .`, `summaries/b -> .`) branch the explored
path set two ways per level up to the 32-40 level cap — on the order of 2^32 stat and
opendir pairs — which is an effective hang with no output and no way to tell what the
command is doing. A `summaries/all -> /` link is the variant that does produce a hard
error: the walk leaves the corpus entirely and the first EACCES from `fs::read_dir` is
not `NotFound`, so it reaches src/cli.rs:1151-1155 and fails the whole command. None of
these escalate to a bogus ID being accepted — the prefix/suffix filter at
src/cli.rs:1162-1174 discards anything not directly under the template prefix — so the
impact is unbounded work or a denied command triggered by a file the corpus author
placed, not a forged verdict. Every id-less invocation reaches this code: `check`,
`audit`, and `propose` all call `summary_ids` (src/cli.rs:834, 904, 987) when no ids are
passed, which is the documented default usage. The corpus is user-controlled data that
the tool is expected to walk without supervision, and a stray symlink is a configuration
mistake rather than an attack. Note this recursion is new: it was introduced on
2026-08-10 in bfcb40b as the fix for the prior audit's `custom-summary-template-inventory`
finding, replacing a flat `read_dir`, so it has not previously been reviewed for
traversal safety.
```

- **Title.** "with no depth or cycle bound" still fits — the cycle bound is the missing
  piece and the kernel is the only thing supplying a depth bound. No change needed.
- **Remediation** stays sound and is actually *better* than the mechanism gave it credit
  for: swapping `path.is_dir()` for `entry.file_type()` (which does not follow links)
  eliminates the breadth explosion outright, not just the depth. Keep as written.
- **Concern.** `moderate` remains correct — unbounded work / denied command from corpus
  data, no forged verdict.

### `ocr-dpi-has-no-upper-bound`

**ADJUSTED** — one numeric overstatement, understated by two orders of magnitude.

- **Evidence.** `src/config.rs:274-282` verbatim, 9 lines, exact. Markers `1-3` and
  `7-9` relative, correct.
- **Mechanism verified:** `dpi: u32` (`src/config.rs:95`), narrowing at
  `src/pdf/mod.rs:102` is exactly `u16::try_from(ocr.dpi).unwrap_or(u16::MAX)`, and
  `src/pdf/mutool.rs:70` is `command.args(["draw", "-q", "-r", &dpi.to_string()]);`.
  All correct. The `page_segmentation_mode` contrast at `src/config.rs:280-282` is real.
- **The number is wrong.** "tens of gigabytes for a single page" understates badly. A US
  Letter page at 65535 dpi is 8.5 × 65535 ≈ 5.57e5 px by 11 × 65535 ≈ 7.21e5 px ≈ 4.0e11
  pixels ≈ **1.2 TB** at 24 bpp. The conclusion (OOM / thrash) is unchanged, but the
  figure should not be citable as wrong.

**Replacement for that sentence** (copy-paste):

```
The subprocess attempts an allocation on the order of a terabyte for a single page — a
US Letter page at 65535 dpi is roughly 5.6e5 by 7.2e5 pixels — and fails, is OOM-killed,
or thrashes the host; `run` then reports a failed subprocess with no hint that the
configured dpi caused it.
```

- **Remediation** sound. `1..=1200` is a reasonable ceiling and mirrors the neighbouring
  check exactly; turning `unwrap_or(u16::MAX)` into a hard error requires making
  `PdfTools::new` fallible or clamping at config load — the latter is simpler and worth
  saying, since `PdfTools::new` is `#[must_use]`-annotated infallible today
  (`src/pdf/mod.rs:100-110`) and four call sites depend on that.
- **Concern.** `advisory` is defensible but I would argue `moderate`: this is a live
  robustness gap reachable from untrusted config today, not a design choice limiting
  future safety. Your call — I am not treating this as a required change.

---

## external-tool

### `external-tool-paths-resolved-from-ambient-path`

**ADJUSTED** — markers are absolute and currently render nothing.

- **Evidence.** `src/pdf/mutool.rs:14-26` verbatim, 13 lines, exact. Line numbers correct.
- **Markers are broken.** `"17"` and `"23"` are absolute file lines; the block is 13
  lines, so `getLine(16)` and `getLine(22)` both return undefined and the markers plus
  their labels are silently dropped.

**Replacement `evidence_markers`** (copy-paste, re-indent to 8 spaces under the finding):

```yaml
        evidence_markers:
          - lines: "4"
            type: mark
            label: "private field, no constructor accepts an executable path"
          - lines: "10"
            type: mark
            label: "bare name — resolved against ambient PATH at every invocation"
```

- **Mechanism verified in full:** `Tesseract::default()` at `src/pdf/tesseract.rs:88` and
  `Tesseract::new` at `:100` both hardcode `"tesseract".to_owned()`; the `executable`
  field is private in both adapters with no setter; `PdfTools::new`
  (`src/pdf/mod.rs:101-110`) constructs both with `default()`/`new(lang, dpi, psm)` and
  has no path parameter; `config::validate` (`src/config.rs:274-282`) covers only the
  three OCR keys and `CorpusLayout`/`CacheConfig`/`OcrConfig` declare no tool-path field;
  `OcrProfile` carries `mutool_version`/`tesseract_version` and feeds `cache_key`
  (`src/pdf/cache.rs:157-159`), so the same-version-different-binary collision is real.
  Everything checks out.
- **Remediation** sound. The profile-version-bump caveat in `effort_notes` is correct and
  important: `OcrProfile` has `deny_unknown_fields` (`src/pdf/cache.rs:14`), so adding a
  field invalidates every stored manifest as an unparseable entry rather than a clean
  miss — `load` would surface that as an `Err` from the `serde_json::from_slice`
  `with_context` at `src/pdf/cache.rs:122-126`, not `Ok(None)`. Worth a sentence in the
  remediation so the implementer handles the parse failure as a miss.
- **Concern.** `significant` sits at the top of its band; `moderate` (robustness /
  defense-in-depth) is the better fit given that PATH hijack presupposes environment
  control. Defensible either way — flagging, not requiring.

### `stext-schema-drift-yields-silent-empty-extraction`

**ADJUSTED** — markers are absolute; the serde claim itself is fully confirmed.

- **Evidence.** `src/pdf/mutool.rs:144-167` verbatim, 24 lines, exact.
- **The serde defaults do exactly what the finding says.** Confirmed against the source:
  `StextPage.blocks` and `StextBlock.lines` carry `#[serde(default)]`, and **none of
  these four structs carries `deny_unknown_fields`** (unlike `OcrProfile` and
  `CacheManifest`, which do). So a renamed upstream field is ignored as unknown, the
  container defaults to empty, and deserialization succeeds. `parse_stext_json` then
  flat-maps to zero lines (`src/pdf/mutool.rs:105-114`), yielding
  `ExtractedPage { text: normalize("") , spans: [] }`. The only guard,
  `document.pages.is_empty()` at `src/pdf/mutool.rs:96-98`, sees a non-empty `pages`
  vector and does not fire. `text: String` at `:166` has no default, so a rename there
  is a hard `missing field` error. The finding's asymmetry claim is exactly right.
- **Markers are broken.** `"152-153"`, `"158-159"`, `"164-165"` are absolute; the block
  is 24 lines, so all three are dropped.

**Replacement `evidence_markers`** (copy-paste, re-indent to 8 spaces):

```yaml
        evidence_markers:
          - lines: "9-10"
            type: mark
            label: "renamed or restructured `blocks` deserializes to empty, not an error"
          - lines: "15-16"
            type: mark
            label: "same for `lines` — every page extracts as empty text"
          - lines: "21-22"
            type: mark
            label: "missing bbox silently drops locator coordinates"
```

- **Remediation** sound. Small nit worth folding in: `#[serde(default)]` on
  `bbox: Option<StextBbox>` is redundant — `Option` already defaults to `None` — so
  removing the attribute would change nothing there. The advice to keep the `Option` and
  surface "no geometry available" is the right call, but the finding should not imply
  the attribute is what causes the `bbox` behavior.
- **Concern.** `significant` correct: a silent corpus-wide extraction failure presented
  to the user as unsupported claims.

### `external-tool-versions-probed-never-validated`

**ADJUSTED** — markers are absolute.

- **Evidence.** `src/cli.rs:594-601` verbatim, 8 lines, exact.
- **Markers are broken.** `"594-597"` and `"598-601"` on an 8-line block are dropped.

**Replacement `evidence_markers`** (copy-paste, re-indent to 8 spaces):

```yaml
        evidence_markers:
          - lines: "1-4"
            type: mark
            label: "ok = the process exited 0; the version string is never compared to anything"
          - lines: "5-8"
            type: mark
            label: "same for tesseract"
```

- **Mechanism verified:** `run` bails on non-zero exit (`src/pdf/mutool.rs:135-140`,
  and the tesseract equivalent), so `mutool_ok`/`tesseract_ok` are precisely
  "exited 0". `all_ok` at `src/cli.rs:621` is the only gate and `bail!` at `:648-650`
  the only consequence. `README.md:33` reads "versions, the extraction profiles, and the
  cache root. Run it first." and `README.md:28` is `brew install mupdf tesseract` —
  both verbatim. `grep` confirms no version-range constant anywhere in `src/`.
- **Remediation** sound and cheap.
- **Concern.** `moderate` correct.

---

## failure-modes

### `inline-html-byte-slice-panics-on-multibyte-markdown`

**ADJUSTED** — the finding is correct; the *narrative verdict* overstates reachability.

- **Evidence.** `src/markdown.rs:135-143` verbatim, 9 lines, exact. Markers `4` and `3`
  relative, correct.
- **Mechanism verified end to end**, including the third-party claims:
  - `pulldown-cmark 0.13.4` in `Cargo.lock` ✓
  - `scan_inline_html_processing` at `.../pulldown-cmark-0.13.4/src/scanners.rs:1505` ✓,
    and it does accept arbitrary bytes: `while let Some(offset) = memchr(b'?', ...)`
    with no UTF-8 or ASCII constraint (`:1513-1518`).
  - `ItemBody::InlineHtml => return Event::InlineHtml(text[item.start..item.end].into())`
    at `.../pulldown-cmark-0.13.4/src/parse.rs:2262` ✓
  - For `<?é?>` the bytes are `<`, `?`, `0xC3`, `0xA9`, `?`, `>`; the scanner returns the
    full 6-byte span, `trimmed.len() == 6 >= 3`, and `trimmed[..3]` splits `é` at byte 3.
    Panic confirmed by construction.
  - The finding is right to use a **mid-paragraph** example (`text <?é?> more`): a
    line-leading `<?` opens a CommonMark HTML block type 3, and the arm's
    `html_block_depth == 0` guard would suppress it. Good catch by the original agent.
  - No `panic = "abort"` in `Cargo.toml`, so unwind → exit 101 ✓.
  - `Cargo.toml [lints.clippy]` is `all` + `pedantic` with `missing_panics_doc = "allow"`;
    `indexing_slicing` is a restriction lint and is not enabled ✓.
- **The overstatement is in the narrative, not the finding.** The `failure-modes`
  `verdict` says the panic is "reachable from ordinary non-English markdown rather than
  from an attack." That is not true: the trigger requires an inline `<?…?>` processing
  instruction whose *first* character after `<?` is non-ASCII. Ordinary accented prose,
  `<br>`, `<!-- comments -->`, and normal tags with accented attribute values all pass
  byte 3 safely. The finding's own mechanism is precise about this — only the narrative
  verdict generalizes.

**Replacement clause in the `failure-modes` narrative `verdict`** (copy-paste):

```
The inline-HTML byte slice panics on a `<?…?>` processing instruction whose first
character is multi-byte — narrow, but it is corpus data the tool is designed to accept
without supervision, and the result is a crash dump rather than a Markdown error.
```

- **Remediation** sound. `trimmed.as_bytes()` compared against `b"<br"` with
  `eq_ignore_ascii_case` is the cleanest form and preserves semantics exactly.
  `trimmed.get(..3).is_some_and(...)` also works. Both are MSRV-1.89 clean.
- **Concern.** `significant` is correct and I would not downgrade it: an unauthenticated
  corpus file aborts the process and writes a persistent crash artifact.

### `tool-failure-indistinguishable-from-invalid-evidence`

**CONFIRMED**

- **Evidence.** `src/validate.rs:275-281` verbatim, 7 lines, exact. Markers `1-3` and `4`
  relative, correct.
- **Mechanism verified:** `extract_pdf_page` (`src/validate.rs:368-392`) ends in
  `.map_err(|error| error.to_string())` at `:391`, flattening every backend failure —
  including `Command::output()` failing to spawn (`src/pdf/mutool.rs:132-134`) — into
  `Err(String)`. `is_valid()` false → `report.invalid += 1` (`src/cli.rs:871`) →
  `bail!` at `src/cli.rs:876-881` and `src/cli.rs:979-981`. `src/main.rs:3-6` maps every
  `Err` to `exit(1)`. `README.md:141` and `:150` say only "Non-zero exit on ..." — the
  "no documented contract is being violated" qualifier is accurate and honest.
  `EvidenceIssue.code` is serialized, so the JSON-consumer escape hatch is real.
- **Remediation** sound; option (b) is the cheaper one and the `doctor` probe logic is
  genuinely already factored (`Mutool::version`, `Tesseract::version`).
- **Concern.** `moderate` correct — no forged verdict, but a false accusation on the
  single most likely CI failure.

No changes required.

### `broken-pipe-panic-writes-crash-dump`

**ADJUSTED** — one count is wrong and the call-site list is incomplete.

- **Evidence.** `src/output.rs:1-8` is the **entire file**, verbatim, exact. Markers `6`
  and `5` happen to be both relative and absolute here (`start_line: 1`). Correct.
- **Mechanism verified, including the third-party claims:**
  - `librebar 0.6.0` has **zero** SIGPIPE handling — `grep -rn 'SIGPIPE|sigpipe'` over
    its entire `src/` returns nothing ✓. Rust's runtime sets SIG_IGN, so EPIPE is
    returned and `println!` panics.
  - `librebar::crash::install` at `src/main.rs:2` before dispatch ✓; the hook writes a
    serialized `CrashInfo` with message, location, and backtrace to a persistent dump
    under the cache dir (`librebar-0.6.0/src/crash.rs:147-172`, `try_write_crash_dump_to`
    at `:179-207`) and chains the previous hook ✓.
  - No `panic = "abort"` → exit 101 ✓.
- **Wrong count.** "the ~20 bare `println!` calls in src/cli.rs" — the actual count is
  **17**, at lines 645, 971, 1030, 1037, 1043, 1059, 1069, 1082, 1083, 1084, 1088, 1093,
  1200, 1216, 1228, 1262, 1264. The parenthetical omits the check-report printers at
  1200/1216/1228, which are on the `check` human-output path — arguably the most-used
  report path of all, so the omission weakens the finding rather than the reverse.

**Replacement for that sentence** (copy-paste):

```
The same applies to all 17 bare `println!` calls in src/cli.rs (doctor at 645, audit at
971, propose at 1030-1093, the check report at 1200-1228, locate at 1262-1264).
```

- **Remediation** sound. Note for the implementer: treating `BrokenPipe` as clean
  requires the error to *reach* `main`, which means every `println!` on a report path
  must be converted — a partial conversion leaves the panic in place.
- **Concern.** `moderate` correct.

### `propose-aborts-the-batch-on-one-unreadable-summary`

**CONFIRMED**

- **Evidence.** `src/cli.rs:993-1002` verbatim, 10 lines, exact. Markers `3` and `4-9`
  relative, correct.
- **Every cross-reference verified:** `summary_ids` call inside `src/cli.rs:986-990`;
  `check`'s tolerant handling at `src/cli.rs:844-862` (`summary_parse_failed` at `:850`,
  `id_mismatch` at `:858`); `audit`'s at `src/cli.rs:909-941`; the human print at
  `src/cli.rs:1005-1007` executing inside the loop before any abort; `print_json` after
  the loop at `src/cli.rs:1010-1016`. The "human mode truncates, JSON mode emits nothing"
  asymmetry is exactly right.
- The distinction drawn against the prior audit's accepted
  `propose-silently-suppresses-source-failures` is correct: `propose_document` does
  tolerate unreadable Markdown (`src/propose.rs:138-142`, an `if let` that simply skips)
  and unreadable PDFs, which is the opposite polarity from this hard abort. The argument
  holds.
- **Remediation** sound — `src/cli.rs:1014` already emits the `{"summaries": [...]}`
  envelope.
- **Concern.** `advisory` correct.

No changes required.

### `vocabulary-rename-results-discarded-without-rationale`

**CONFIRMED**

- **Evidence.** `src/terms.rs:113-119` verbatim, 7 lines, exact. Marker `5-6` relative,
  correct.
- **Counts verified exactly:** six `drop(rename(...))` at `src/terms.rs:99, 107, 109,
  117, 118, 132` — no more, no fewer. All six pass `false` as `strict`. `rename`'s only
  `Err` arm is `if strict && object.contains_key(to)` at `src/terms.rs:150-152`, with
  early `Ok(())` returns at `:141-149`. The soundness argument and the "proof lives in a
  different function" complaint are both accurate. `src/hash.rs:14` is
  `let _ = write!(s, "{b:02x}");` ✓.
- **Remediation** sound and behavior-preserving.
- **Concern.** `note` correct.

No changes required.

---

## Summary table

| Finding | Verdict | Change required |
|---|---|---|
| ocr-cache-entries-are-unauthenticated-evidence | **CONFIRMED** | none — critical stands, could not refute |
| cache-root-escapes-corpus-containment | **CONFIRMED** | none |
| summary-and-markdown-reads-skip-containment-guard | **ADJUSTED** | replace the "Content disclosure is limited" sentence |
| summary-walk-recurses-without-a-depth-bound | **ADJUSTED** | replace the whole mechanism; ELOOP + silent stop + breadth explosion, not ENAMETOOLONG/stack overflow |
| ocr-dpi-has-no-upper-bound | **ADJUSTED** | fix "tens of gigabytes" → terabyte scale |
| external-tool-paths-resolved-from-ambient-path | **ADJUSTED** | markers 17,23 → 4,10 |
| stext-schema-drift-yields-silent-empty-extraction | **ADJUSTED** | markers 152-153,158-159,164-165 → 9-10,15-16,21-22 |
| external-tool-versions-probed-never-validated | **ADJUSTED** | markers 594-597,598-601 → 1-4,5-8 |
| inline-html-byte-slice-panics-on-multibyte-markdown | **ADJUSTED** | narrative verdict overstates reachability |
| tool-failure-indistinguishable-from-invalid-evidence | **CONFIRMED** | none |
| broken-pipe-panic-writes-crash-dump | **ADJUSTED** | "~20 println!" → 17, add 1200-1228 |
| propose-aborts-the-batch-on-one-unreadable-summary | **CONFIRMED** | none |
| vocabulary-rename-results-discarded-without-rationale | **CONFIRMED** | none |

Concern-level recommendations (not required): `ocr-dpi-has-no-upper-bound`
advisory → moderate; `external-tool-paths-resolved-from-ambient-path`
significant → moderate. Both defensible as filed.

---

## Observations (not findings — outside my remit)

1. **Check every narrative for absolute `evidence_markers`.** Three of my thirteen used
   absolute file lines and render nothing. The failure is silent (`getLine(i)?.`), so it
   will not show up as a build error — only as a report with missing highlights and
   missing labels.
2. **`buildMetaString` emits labels as bare quoted strings** appended after the range
   groups (`scripts/build-report.js:40015, 40018`), rather than attaching each label to
   its own range. With multiple markers of the same type the label-to-range association
   is whatever expressive-code's meta parser infers. Worth a look if the rendered labels
   have ever seemed to land on the wrong lines.
3. **`Corpus::root()` is never canonicalized.** `validate_resolved_source`
   (`src/validate.rs:457-458`) compares `path.canonicalize()` against the raw
   `corpus.root()`. If the corpus root itself sits behind a symlink (`/tmp` → `/private/tmp`
   on macOS is the obvious case), `starts_with` false-negatives and legitimate sources
   are reported as `*_source_outside_repo`. Not in any finding I reviewed.

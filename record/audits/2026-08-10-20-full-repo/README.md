---
audit_date: 2026-08-10
project: receipts
commit: b78d869585b48532c90155d6f70b9486687aef54
scope: Full repository — Rust source (src/, tests/), dependencies, configuration (.config/, justfile, Cargo.toml, deny.toml), and documented behavior (README.md, receipts.yaml)
auditor: cased skill + crustoleum rubrics, Claude Opus 5 (7 analysis agents, 3 verification agents)
findings:
  critical: 1
  significant: 12
  moderate: 23
  advisory: 9
  note: 3
---

# Audit: receipts

`receipts` answers one question — is this sentence in an LLM-written summary
actually supported by the PDF it cites? — and that purpose sets the bar, because
an evidence authority that is wrong certifies a fabrication. **The Evidence Trust
Surface** carries the one critical finding: the OCR cache authenticates its
request key but nothing binds the cached `ocr.tsv` to the PDF it claims to
describe, and a `cache.root` that accepts absolute and parent-relative paths puts
that cache within reach of the corpus. **The External Tool Surface** delegates
ground truth to two binaries located by bare name on the ambient `PATH`, never
version-checked, parsed with serde defaults that turn upstream schema drift into
a silently empty page. **The Failure Mode Surface** cannot distinguish "the
evidence is bad" from "I could not check the evidence". **The Published Contract
Surface** has drifted from the README in both directions, **The Type and API
Surface** models line, column, page and claim index as four interchangeable
`usize`, **The Performance Surface** spawns one subprocess per cited page,
**The Verification Gate Surface** reformats instead of verifying and runs
nowhere automatically, and **The Structure Surface** is ordinary consolidation
work. The engineering hygiene underneath is genuinely good — unsafe is forbidden
and absent, clippy passes at `pedantic` with `-D warnings`, and `cargo audit`,
`deny`, `machete` and `udeps` are all clean — so fix the cache authentication and
the external-tool contract, and the rest is scheduled work rather than risk.

This audit was run against the tree left by the remediation pass earlier the
same day, which fixed 16 findings and accepted 4. All 16 fixes were re-checked
and every one held — including all five performance fixes. Two findings below
exist *because* of that pass: the recursive summary walk was introduced by it
and has never been reviewed for traversal safety, and the `propose` multi-ID
JSON wrapper changed observable output without a matching README update.

Coverage bound worth stating: an unsafe census of the 122 transitive
dependencies could not be collected (`cargo-geiger` failed on a missing rmeta
artifact), and binary-size contribution was assessed by inspection rather than
measurement. This crate forbids unsafe in its own code; what its dependency
tree contains is unmeasured, not clean.

---

## The Evidence Trust Surface

*Every verdict receipts issues rests on a chain from PDF bytes to matched text, and one link in that chain — the OCR cache — is authenticated by a key stored next to the payload it is supposed to authenticate.*

### A cache entry is trusted as extracted PDF text with nothing binding the TSV to the PDF {#ocr-cache-entries-are-unauthenticated-evidence}

**critical** · `src/pdf/cache.rs:115-133` · effort: medium · <img src="assets/sparkline-ocr-cache-entries-are-unauthenticated-evidence.svg" height="14" alt="commit activity" />

`OcrCache::load` authenticates the request key but not the payload.
`matches_request` compares the stored manifest's `pdf_sha256`, `page`,
`profile`, and `toolchain_sha256` against the expected manifest — all values an
attacker simply copies — and nothing in the manifest binds `ocr.tsv`. There is
no digest of the TSV, no MAC, and no machine-local secret, so any party who can
place files under the resolved cache root dictates what `receipts` believes is
on a PDF page. That root is corpus-relative whenever `receipts.yaml` says so
(README.md:328-330 documents this as supported, and `receipts.yaml` is itself
corpus content the tool treats as untrusted). The chain is reachable from the
CLI with no library consumer involved - `check`/`audit` -> `validate_document`
(src/validate.rs:237-248) -> `PdfTools::ocr_page` (src/pdf/mod.rs:126-140) ->
`ocr_page_with_profile` (src/pdf/tesseract.rs:242-244) -> `cache.load` ->
`parse_tsv` -> `exact_count(text, locator.exact) == 1` -> the locator passes. An
attacker who commits a PDF, a summary whose locators declare `backend:
tesseract-ocr`, a `receipts.yaml` with a corpus-relative `cache.root`, and a
hand-written `ocr.tsv` gets a `valid` verdict for text appearing nowhere in the
PDF, defeating the tool's purpose as an evidence gate. The one real obstacle is
that `profile` embeds the validating machine's `mutool -v` and `tesseract
--version` strings; but `receipts doctor` prints both (src/cli.rs:632-641), CI
logs publish them, container images pin them, and each guess costs one extra
directory, so this is enumeration cost rather than a barrier. Native
(`mutool-native`) locators are unaffected because that backend re-runs the
subprocess every time. This is distinct from the previously accepted
`cache-manifest-invariants-are-bypassable`, which concerned in-process
construction of `CacheManifest` through public fields by a hypothetical library
consumer; that acceptance rested on "there are no library consumers — the CLI
path validates rigorously." The rationale does not cover this: the attack is on
data at rest in the cache directory, runs entirely through the validated CLI
path with a well-formed manifest, and needs no API access. It also contradicts
both stated guarantees — "a hit is a determinism guarantee"
(src/pdf/cache.rs:88-89) and "The cache is never evidence authority"
(README.md:334).

```rust src/pdf/cache.rs:115-133
pub fn load(&self, expected: &CacheManifest) -> Result<Option<String>> {
    let directory = self.entry_dir(expected);
    let manifest_path = directory.join("manifest.json");
    let tsv_path = directory.join("ocr.tsv");
    if !manifest_path.is_file() || !tsv_path.is_file() {
        return Ok(None);
    }
    let stored: CacheManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    if !stored.matches_request(expected) {
        return Ok(None);
    }
    fs::read_to_string(&tsv_path)
        .with_context(|| format!("failed to read {}", tsv_path.display()))
        .map(Some)
}
```

> I don't need to touch the PDF. The tool already decided the PDF is authoritative —
> what it didn't decide is who gets to say what the PDF *says*. The manifest it
> checks against is sitting in the same directory as the answer it's checking, and
> I wrote both. The only thing I actually have to guess is which `mutool -v` string
> the validating machine prints, and `receipts doctor` prints it for me.

Enabled by [cache.root accepts absolute and .. paths, unlike every other config-supplied path](#cache-root-escapes-corpus-containment). Related [Three of five commands read corpus files without the containment check check enforces](#summary-and-markdown-reads-skip-containment-guard).

**Remediation:** Stop treating a foreign cache directory as an evidence source. Either change
closes the hole. (1) Authenticate the payload with a machine-local key: generate
a random key once at the cache root (mode 0600), store `tsv_hmac = HMAC(key,
canonical_manifest_bytes || tsv_bytes)` in the manifest, and treat a missing or
non-verifying MAC as a miss, so entries written by anyone without the local key
are inert rather than authoritative. (2) Refuse to read a cache root that
resolves inside the corpus root unless the operator opts in with an explicit
`--trust-cache` flag; writing there can stay permitted, since a repo-local cache
is a legitimate build artifact. Also drop or qualify the "determinism guarantee"
wording at src/pdf/cache.rs:88-89 and README.md:332-335 so the docs match what
the code proves.

*Effort (medium):* Adding an HMAC field plus key management touches `CacheManifest`, `load`, `store`, and the cache-hit tests; the read-side containment refusal is a dozen lines in `Corpus::from_discovered`.

<div>&hairsp;</div>

### cache.root accepts absolute and .. paths, unlike every other config-supplied path {#cache-root-escapes-corpus-containment}

**significant** · `src/corpus.rs:36-45` · effort: small · <img src="assets/sparkline-cache-root-escapes-corpus-containment.svg" height="14" alt="commit activity" />

`config::validate_template` (src/config.rs:287-298) rejects absolute paths and
`..` segments for every corpus template, and README.md:74-76 states the
resulting guarantee outright: "a config file cannot direct reads outside the
corpus." `cache.root` is the one config-supplied path that never reaches that
function. `Corpus::from_discovered` accepts an absolute path verbatim and joins
a relative one without inspecting it, so `cache: {root: "/etc/receipts"}` or
`cache: {root: "../../../../../../tmp/x"}` is honoured as written. Because
config discovery walks up from the working directory and reads `receipts.yaml`
out of the corpus itself (src/config.rs:185-214), the value is
attacker-controlled whenever the corpus is — the normal case for a tool that
validates LLM output on an incoming change. The reached sinks are real writes,
not just path construction: `fs::create_dir_all` plus `fs::write` of `ocr.tsv`
and `manifest.json` in `OcrCache::store` (src/pdf/cache.rs:135-153), and in
`doctor` an unconditional `create_dir_all` on the root plus a write-and-delete
probe named `.doctor-<pid>` (src/cli.rs:605-619) that runs before any other
check. An untrusted config file therefore yields attacker-chosen directory
creation anywhere the process can write, plus file writes at fixed names inside
those directories. The same freedom is what lets the cache root be pointed at
repo-controlled storage, the precondition for
`ocr-cache-entries-are-unauthenticated-evidence`.

```rust src/corpus.rs:36-45
let cache_root = match config.cache.root.as_deref() {
    Some(value) => {
        let candidate = PathBuf::from(value);
        if candidate.is_absolute() {
            candidate
        } else {
            root.join(candidate)
        }
    }
    None => config::platform_cache_root()?,
```

> Every other path in this config gets canonicalized and checked against the corpus
> root. This one gets `PathBuf::from`. I don't need an escape technique — I need
> one line of YAML pointing the cache somewhere I can write, and the tool walks
> into it on my behalf.

Enables [A cache entry is trusted as extracted PDF text with nothing binding the TSV to the PDF](#ocr-cache-entries-are-unauthenticated-evidence). Related [Three of five commands read corpus files without the containment check check enforces](#summary-and-markdown-reads-skip-containment-guard).

**Remediation:** Run `cache.root` through the same containment check as the templates: reject
`..` segments, and either reject absolute paths outright or require them to come
from user-level config (`~/.config/receipts.yaml`) or an environment variable
rather than the project file — the config loader already distinguishes the two
sources. Canonicalize the resolved root and confirm the result before creating
it. If absolute project-supplied roots must stay supported, gate them behind an
explicit CLI flag so the untrusted file cannot choose the destination on its
own, and correct README.md:74-76 to name the exception.

*Effort (small):* One validation call plus a canonicalize-and-compare; the template validator already exists.

<div>&hairsp;</div>

### Three of five commands read corpus files without the containment check check enforces {#summary-and-markdown-reads-skip-containment-guard}

**moderate** · `src/cli.rs:1119-1125` · effort: small · <img src="assets/sparkline-summary-and-markdown-reads-skip-containment-guard.svg" height="14" alt="commit activity" />

`validate_document` is careful: it canonicalizes each resolved Markdown and PDF
path, compares against the corpus root, and refuses to read on failure — the
comment at src/validate.rs:125 states the invariant as "Never read a source that
resolves outside the corpus." That guard exists in exactly one place.
`read_summary` (src/cli.rs:1119-1125, used by `check`, `audit`, and `propose`),
`locate`'s summary read (src/cli.rs:663-666), `locate`'s Markdown read
(src/cli.rs:684-685), and `propose`'s Markdown read (src/propose.rs:138-139) all
join the template result onto the root and open it directly. `validate_id`
blocks traversal through the `{id}` component, but it cannot see symlinks: a
corpus containing `summaries/x.yaml -> /home/runner/.ssh/id_rsa` or `md/x.md ->
/dev/zero` is opened without complaint, because `Path::join` and
`fs::read_to_string` both follow links. Content disclosure is limited —
main.rs:4 prints the anyhow chain with `{error:#}` and the YAML decode errors
that surface generally carry positions rather than payload, while `propose` and
`locate` only emit spans that also occur in the PDF — so the practical impact is
unbounded reads of out-of-corpus files. The exposure differs by call site. The
two summary reads (`read_summary` at src/cli.rs:1119-1125 and `locate`'s at
src/cli.rs:661-666) go through `Corpus::summary_path` (src/corpus.rs:122-125),
which does `validate_id` and a `join` with no file-type check at all: a
`summaries/aaa.yaml -> /tmp/fifo` link makes `read_to_string` block forever, and
a link to `/dev/zero` exhausts memory, because NUL bytes are valid UTF-8 and the
read never reaches EOF. The two Markdown reads are narrower —
`resolve_markdown_for` (src/cli.rs:1182-1185) and `propose_document`
(src/propose.rs:138) both select the candidate with `.find(|path|
path.is_file())`, and `is_file()` follows the symlink and returns false for
FIFOs and character devices — but a symlink to an out-of-corpus regular file of
arbitrary size is still read in full. The finding is the inconsistency itself:
an invariant the codebase states and enforces on one path is silently absent on
four others that open files first.

```rust src/cli.rs:1119-1125
fn read_summary(corpus: &Corpus, id: &str) -> Result<crate::evidence::SummaryDocument> {
    let path = corpus.summary_path(id)?;
    let source =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    parse_summary(&source, corpus.terms())
        .with_context(|| format!("failed to parse {}", path.display()))
}
```

> The comment says "Never read a source that resolves outside the corpus," and it
> means it — on exactly one code path. The other four call `join` and open the
> file. `validate_id` stops me putting `../` in the id, but it has nothing to say
> about a symlink that was already on disk when the walk started.

Related [cache.root accepts absolute and .. paths, unlike every other config-supplied path](#cache-root-escapes-corpus-containment). Related [summary-walk-follows-directory-symlinks](#summary-walk-follows-directory-symlinks).

**Remediation:** Lift `validate_resolved_source` out of src/validate.rs into a corpus-level
helper — something like `Corpus::open_contained(path)` that canonicalizes,
checks `starts_with(root)`, confirms the result is a regular file, and only then
reads — and route every corpus read through it: `read_summary`, `locate`'s
summary and Markdown reads, and `propose`'s Markdown read. Add a byte cap on the
read so an oversized or endless file fails with a clear error instead of
consuming the process.

*Effort (small):* One shared helper plus four call-site swaps; the containment logic is already written.

<div>&hairsp;</div>

### Summary discovery recurses into symlinked directories with no depth or cycle bound {#summary-walk-recurses-without-a-depth-bound}

**moderate** · `src/cli.rs:1157-1161` · effort: small · <img src="assets/sparkline-summary-walk-recurses-without-a-depth-bound.svg" height="14" alt="commit activity" />

`walk_summaries` recurses once per subdirectory with no depth parameter and no
`symlink_metadata` check. `Path::is_dir` follows symlinks, so a symlink inside
the summaries tree that points at an ancestor (`summaries/loop -> ..`) sends the
walk back through the same directories, and every level allocates a `PathBuf`, a
`ReadDir`, and a stack frame. Depth alone is not the danger: the walk descends
only via `entry?.path()`, so the accumulated path grows by a component on every
level, and the kernel's symlink-resolution cap terminates the chain long before
any resource limit — measured on macOS 26.4, a `summaries/loop -> ..` chain
fails at the 33rd `loop` component with ELOOP at a path length of 504 bytes,
against a PATH_MAX of 1024 (Linux caps at 40 links). Worse, that failure is
silent: the call that trips the cap is `path.is_dir()` at src/cli.rs:1159, and
`Path::is_dir` swallows the error and returns false, so `fs::read_dir` is never
reached and the non-`NotFound` arm at src/cli.rs:1151-1155 never fires. The walk
simply stops. The real cost is breadth. Two loop links in one directory
(`summaries/a -> .`, `summaries/b -> .`) branch the explored path set two ways
per level up to the 32-40 level cap — on the order of 2^32 stat and opendir
pairs — which is an effective hang with no output and no way to tell what the
command is doing. A `summaries/all -> /` link is the variant that does produce a
hard error: the walk leaves the corpus entirely and the first EACCES from
`fs::read_dir` is not `NotFound`, so it reaches src/cli.rs:1151-1155 and fails
the whole command. None of these escalate to a bogus ID being accepted — the
prefix/suffix filter at src/cli.rs:1162-1174 discards anything not directly
under the template prefix — so the impact is unbounded work or a denied command
triggered by a file the corpus author placed, not a forged verdict. Every
id-less invocation reaches this code: `check`, `audit`, and `propose` all call
`summary_ids` (src/cli.rs:834, 904, 987) when no ids are passed, which is the
documented usage. The corpus is user-controlled data that the tool is expected
to walk without supervision, and a stray symlink is a configuration mistake
rather than an attack. Note this recursion is new: it was introduced on
2026-08-10 in bfcb40b as the fix for the prior audit's
`custom-summary-template-inventory` finding, replacing a flat `read_dir`, so it
has not previously been reviewed for traversal safety.

```rust src/cli.rs:1157-1161
for entry in entries {
    let path = entry?.path();
    if path.is_dir() {
        walk_summaries(&path, prefix, suffix, root, ids)?;
    } else {
```

> Two symlinks in one directory, both pointing at `.`. I'm not trying to escape
> anything or forge a verdict — the prefix filter would throw my ids out anyway. I
> just want the branching factor. The kernel caps the chain at forty links, which
> sounds reassuring until you count what two-way branching does over forty levels.

Related [A missing or broken PDF tool is counted and exited as invalid evidence](#tool-failure-indistinguishable-from-invalid-evidence).

**Remediation:** Test `entry.file_type()` (which does not follow links) instead of
`path.is_dir()` and skip symlinked directories. That single change is the whole
fix: it eliminates the breadth explosion outright rather than merely capping its
depth, because a link that is never descended into cannot branch the path set.
Add an explicit depth counter as a second line of defence, with a ceiling
derived from the template's own `{id}` nesting — the template determines how
deep a valid summary can sit, so anything deeper cannot match `prefix`/`suffix`
anyway and need not be descended into. Return a contextual error when the
ceiling is exceeded rather than stopping silently, so a malformed tree is
reported instead of quietly yielding fewer summaries than the corpus contains.

*Effort (small):* Add a depth argument and swap is_dir() for a file_type() check; local to one function.

<div>&hairsp;</div>

### pdf.ocr.dpi is bounded only below, then silently clamped to 65535 before rendering {#ocr-dpi-has-no-upper-bound}

**moderate** · `src/config.rs:274-282` · effort: trivial · <img src="assets/sparkline-ocr-dpi-has-no-upper-bound.svg" height="14" alt="commit activity" />

`config::validate` rejects `dpi == 0` and stops there, while
`page_segmentation_mode` two checks below gets a genuine range test. The value
is declared `u32` and narrowed at src/pdf/mod.rs:102 with
`u16::try_from(ocr.dpi).unwrap_or(u16::MAX)`, so a corpus config saying `dpi:
4294967295` does not fail — it silently becomes 65535 and is handed to `mutool
draw -r 65535` (src/pdf/mutool.rs:70), which attempts to rasterize one page at
roughly 218x normal linear resolution. The subprocess attempts an allocation on
the order of a terabyte for a single page — a US Letter page at 65535 dpi is
roughly 5.6e5 by 7.2e5 pixels — and fails, is OOM-killed, or thrashes the host;
`run` then reports a failed subprocess with no hint that the configured dpi
caused it. The input crosses the same untrusted boundary as every other
`receipts.yaml` value, and the clamp converts what should be a validation error
into resource exhaustion in an external process.

```rust src/config.rs:274-282
if config.pdf.ocr.dpi == 0 {
    bail!("pdf.ocr.dpi must be greater than zero");
}
if config.pdf.ocr.lang.is_empty() {
    bail!("pdf.ocr.lang must not be empty");
}
if config.pdf.ocr.page_segmentation_mode > 13 {
    bail!("pdf.ocr.page_segmentation_mode must be 0–13");
}
```

> `dpi: 4294967295` isn't rejected, it's *narrowed*. The tool quietly hands 65535
> to `mutool draw -r` and asks it to rasterize a letter page at half a trillion
> pixels. The error the operator eventually sees will be about a failed subprocess,
> which tells them nothing about the line of YAML that caused it.

**Remediation:** Give `dpi` a real range in `config::validate` — `1..=1200` covers every
legitimate OCR render and matches the shape of the `page_segmentation_mode`
check beside it — and change the narrowing at src/pdf/mod.rs:102 from
`unwrap_or(u16::MAX)` to a hard error, so an out-of-range value can never reach
`mutool` even if validation is bypassed.

*Effort (trivial):* One range check next to an existing one, plus turning a clamp into an error.

<div>&hairsp;</div>

*Verdict: This is the surface that decides whether the tool is worth running. The cache is the break: matches_request compares four fields an attacker copies verbatim, and nothing binds ocr.tsv to the PDF. The cache.root escape turns that from a local-machine concern into a corpus-supplied one, because receipts.yaml is itself corpus content. The containment guard that check applies to sources is not applied by the three commands that read summaries and markdown, and the discovery walk that finds those summaries follows symlinks without bound. Fix the cache authentication first — a digest of ocr.tsv in the manifest closes the critical path — then bring cache.root under the same containment rule as every other configured path.*

<div>&nbsp;</div>

---

## The External Tool Surface

*receipts delegates its ground truth to two binaries it locates by bare name, never version-checks, and parses with serde defaults that convert upstream schema drift into an empty page rather than an error.*

### mutool and tesseract executables are hardcoded bare names with no configurable path {#external-tool-paths-resolved-from-ambient-path}

**significant** · `src/pdf/mutool.rs:14-26` · effort: medium · <img src="assets/sparkline-external-tool-paths-resolved-from-ambient-path.svg" height="14" alt="commit activity" />

`mutool` and `tesseract` are hard dependencies of this crate's core function,
but they are not in Cargo.lock, not version-pinned, and not addressable by
absolute path. `Mutool::default()` sets `executable` to the bare string
`"mutool"`, and `Tesseract::default()` / `Tesseract::new` do the same with
`"tesseract"` (src/pdf/tesseract.rs:88 and src/pdf/tesseract.rs:100). The field
is private and no constructor takes it, so `PdfTools::new` (src/pdf/mod.rs:101)
has no way to override it. `src/config.rs` validates `pdf.ocr.dpi`,
`pdf.ocr.lang`, and `pdf.ocr.page_segmentation_mode`, but exposes no key for
either tool's location — the binary is an assumption, not a parameter, even
though the trait boundary (`PageRenderer`, `OcrEngine`) is already correctly
abstracted. Every extraction therefore executes whatever the ambient PATH
resolves at that moment. This matters more here than in a typical CLI because
the product claim is deterministic evidence validation and the OCR cache is
keyed on the tool *version strings* recorded in `OcrProfile`: two different
binaries reporting the same version string (a Homebrew build vs. a distro build
vs. a wrapper shim earlier in PATH) produce an identical cache key over
non-identical output, so a stale cache entry can be served for a different
engine. Nothing in the codebase records which file was actually executed.

```rust src/pdf/mutool.rs:14-26
/// `MuPDF` command adapter.
#[derive(Debug, Clone)]
pub struct Mutool {
    executable: String,
}

impl Default for Mutool {
    fn default() -> Self {
        Self {
            executable: "mutool".to_owned(),
        }
    }
}
```

> `Command::new("mutool")` — no path, no version gate. Whatever the resolver finds
> first is what this tool treats as the truth about the document. I don't have to
> compromise `receipts` at all; I have to be earlier in `PATH` than the real
> binary, on a machine where somebody already decided to run untrusted corpora.

Enables [serde defaults on MuPDF stext.json containers turn upstream schema drift into a silent empty page](#stext-schema-drift-yields-silent-empty-extraction). Related [doctor probes external tool versions but never checks them against a supported range](#external-tool-versions-probed-never-validated).

**Remediation:** Add optional config keys (e.g. `pdf.tools.mutool`, `pdf.tools.tesseract`)
holding absolute paths, plumb them through `PdfTools::new` into the two
adapters, and default to bare-name PATH lookup when unset. Resolve the
executable to an absolute path once at startup, report that resolved path in
`doctor` output alongside the version, and include it (or a digest of the
binary) in `OcrProfile` so cache keys distinguish two binaries that report the
same version string.

*Effort (medium):* Config schema addition plus plumbing through PdfTools::new and both adapters; recording the resolved path in OcrProfile changes the cache key and invalidates existing cache entries, which needs a profile version bump.

<div>&hairsp;</div>

### serde defaults on MuPDF stext.json containers turn upstream schema drift into a silent empty page {#stext-schema-drift-yields-silent-empty-extraction}

**significant** · `src/pdf/mutool.rs:144-167` · effort: small · <img src="assets/sparkline-stext-schema-drift-yields-silent-empty-extraction.svg" height="14" alt="commit activity" />

MuPDF's `stext.json` output is a parsing contract owned by an upstream project
this crate does not pin. `#[serde(default)]` on the three structural container
fields makes that contract non-enforcing in exactly the places where drift would
occur. The top-level `pages` default is caught — `parse_stext_json` bails when
the page list is empty (src/pdf/mutool.rs:96-98) — and `text: String` is
required, so a rename there fails loudly. But `blocks` and `lines` have no such
guard: if a future MuPDF release renames or restructures either level, every
page deserializes successfully with zero lines, yielding an `ExtractedPage` with
empty `text` and no spans, and no error anywhere. For a tool whose entire
purpose is deciding whether a claim is supported by a PDF, a corpus-wide silent
extraction failure presents to the user as "your summary's claims are
unsupported" rather than "your toolchain broke" — the failure is safe in that it
cannot affirm a false claim, but it is indistinguishable from a genuine negative
verdict. The `bbox` default is a quieter version of the same: drift there
silently degrades every locator to no coordinates while validation still reports
success. Because no version gate exists (see
`external-tool-versions-probed-never-validated`), nothing upstream of the parser
would catch the drift either.

```rust src/pdf/mutool.rs:144-167
#[derive(Debug, Deserialize)]
struct StextDocument {
    #[serde(default)]
    pages: Vec<StextPage>,
}

#[derive(Debug, Deserialize)]
struct StextPage {
    #[serde(default)]
    blocks: Vec<StextBlock>,
}

#[derive(Debug, Deserialize)]
struct StextBlock {
    #[serde(default)]
    lines: Vec<StextLine>,
}

#[derive(Debug, Deserialize)]
struct StextLine {
    #[serde(default)]
    bbox: Option<StextBbox>,
    text: String,
}
```

Enabled by [doctor probes external tool versions but never checks them against a supported range](#external-tool-versions-probed-never-validated). Enabled by [mutool and tesseract executables are hardcoded bare names with no configurable path](#external-tool-paths-resolved-from-ambient-path).

**Remediation:** Drop `#[serde(default)]` from `blocks` and `lines` so a structural rename is a
deserialization error rather than an empty result, or keep the defaults and add
an explicit post-parse check that rejects a document in which every page
produced zero lines — the same shape as the existing empty-`pages` bail. For
`bbox`, keep the `Option` but surface "no geometry available for this backend"
in the output rather than letting it read as an ordinary absent box.

*Effort (small):* Attribute removal plus a guard clause and a test for the drifted-schema case; the empty-pages bail already establishes the pattern to follow.

<div>&hairsp;</div>

### doctor probes external tool versions but never checks them against a supported range {#external-tool-versions-probed-never-validated}

**moderate** · `src/cli.rs:594-601` · effort: small · <img src="assets/sparkline-external-tool-versions-probed-never-validated.svg" height="14" alt="commit activity" />

Both adapters implement a `version()` probe, and `doctor` is documented in
README.md:33 as the pre-flight check to "Run it first." But the probe result is
only ever printed or hashed into a cache key — it is never compared against a
supported version range. `mutool_ok` and `tesseract_ok` are true whenever the
subprocess exits 0, so `doctor` reports `ok` for any `mutool` that answers `-v`,
including one whose `stext.json` schema this crate's parser cannot read, and for
any `tesseract` whose TSV column set has changed. The project's only documented
install path is `brew install mupdf tesseract` (README.md:28) — unpinned,
whatever Homebrew ships on the day of install — and no supported range is stated
in the README, in config, or as a constant anywhere in the source. The result is
that the one untracked half of the supply chain is checked for presence but not
for compatibility, and `doctor`'s pass is weaker than a reader would reasonably
assume from its documented role.

```rust src/cli.rs:594-601
let (mutool_ok, mutool_value) = match tools.mutool.version() {
    Ok(version) => (true, version),
    Err(error) => (false, error.to_string()),
};
let (tesseract_ok, tesseract_value) = match tools.tesseract.version() {
    Ok(version) => (true, version),
    Err(error) => (false, error.to_string()),
};
```

Enables [serde defaults on MuPDF stext.json containers turn upstream schema drift into a silent empty page](#stext-schema-drift-yields-silent-empty-extraction). Related [mutool and tesseract executables are hardcoded bare names with no configurable path](#external-tool-paths-resolved-from-ambient-path).

**Remediation:** Define supported version ranges for MuPDF and Tesseract as constants next to the
adapters, parse the numeric version out of each probe's output, and give
`doctor` three states instead of two — missing, present-but-unsupported, and ok
— so an unsupported toolchain fails the pre-flight rather than surfacing later
as an extraction anomaly. Document the supported ranges in the README's Runtime
dependencies section.

*Effort (small):* Version-string parsing plus a range constant per tool and one extra branch in the doctor report; no structural change.

<div>&hairsp;</div>

*Verdict: The delegation itself is the right design — mutool and tesseract are the correct tools and reimplementing them would be worse. What is missing is the contract around them. A bare Command::new("mutool") resolves through the ambient PATH with no configurable override, so what the tool treats as authoritative depends on the invoking shell's environment. doctor already probes both versions but only prints them. And the stext.json parse defaults containers to empty, so a MuPDF release that renames a field produces a clean, confident, wrong answer: zero text on the page, reported to the user as a missing citation. Version-gate the tools, make the paths configurable, and make schema drift loud.*

<div>&nbsp;</div>

---

## The Failure Mode Surface

*The tool's failure paths do not distinguish between 'the evidence is bad' and 'I could not check the evidence', and two of them terminate the process in ways that bypass the error reporting the crate otherwise does well.*

### Inline-HTML `<br>` probe byte-slices a str and panics on multi-byte input {#inline-html-byte-slice-panics-on-multibyte-markdown}

**significant** · `src/markdown.rs:135-143` · effort: trivial · <img src="assets/sparkline-inline-html-byte-slice-panics-on-multibyte-markdown.svg" height="14" alt="commit activity" />

`parse_units` is the single entry point for every Markdown file the tool reads —
called from `validate_document` (src/validate.rs:142), `locate`
(src/cli.rs:686), and `propose_document` (src/propose.rs:141). The
`Event::InlineHtml` arm tests whether the span opens a `<br>` by slicing the
first three bytes of a `&str` whose contents come verbatim from the corpus file.
`trimmed.len() >= 3` proves the slice is in range but proves nothing about UTF-8
char boundaries, and `str`'s `Index<Range>` impl panics with "byte index 3 is
not a char boundary" when byte 3 lands inside a multi-byte scalar. The reachable
input is an inline HTML processing instruction: pulldown-cmark 0.13's
`scan_inline_html_processing` (registry
pulldown-cmark-0.13.4/src/scanners.rs:1505) accepts `<?` followed by arbitrary
bytes up to `?>`, and `parse.rs:2262` emits the raw span as `Event::InlineHtml`.
A paragraph containing `text <?é?> more` therefore yields `trimmed = "<?é?>"`,
where `é` occupies bytes 2..4 and `trimmed[..3]` splits it. The panic is not
caught anywhere: `main` has no `catch_unwind`, so `librebar::crash::install`
writes a crash dump and the process aborts with status 101 instead of reporting
a Markdown problem. Clippy does not flag this because `clippy::indexing_slicing`
is a restriction lint and Cargo.toml enables only `all` + `pedantic`;
`missing_panics_doc` is explicitly allowed, so the panic is also undocumented on
the public `parse_units`.

```rust src/markdown.rs:135-143
Event::InlineHtml(tag) if image_depth == 0 && html_block_depth == 0 => {
    let trimmed = tag.trim_start();
    if trimmed.len() >= 3
        && trimmed[..3].eq_ignore_ascii_case("<br")
        && let Some(builder) = active.as_mut()
    {
        builder.text.push(' ');
    }
}
```

> `<?é?>` — six bytes, and the third one is the middle of a character. The probe
> slices to byte three without asking whether that's a boundary. It costs me one
> processing instruction in a Markdown file the tool was built to accept from
> strangers, and the process aborts with a crash dump on disk instead of an error
> message about my document.

Related [Every report path uses `println!`, so a closed stdout panics and files a crash dump](#broken-pipe-panic-writes-crash-dump).

**Remediation:** Replace the byte slice with a boundary-safe test — `trimmed.as_bytes()` compared
against `b"<br"` case-insensitively, or `trimmed.get(..3)` with an `is_some_and`
guard. Both are one-line changes that keep the ASCII-only semantics the
comparison already assumes. Consider enabling `clippy::indexing_slicing` and
`clippy::string_slice` at warn level in `[lints.clippy]` so the next byte-index
slice on text-derived data is caught by `just clippy`. Dynamic verification (not
run in this static audit): a unit test feeding `parse_units("para <?é?> tail")`
should pass rather than panic.

*Effort (trivial):* One expression plus a regression test; no signature or behavior change.

<div>&hairsp;</div>

### A missing or broken PDF tool is counted and exited as invalid evidence {#tool-failure-indistinguishable-from-invalid-evidence}

**moderate** · `src/validate.rs:275-281` · effort: medium · <img src="assets/sparkline-tool-failure-indistinguishable-from-invalid-evidence.svg" height="14" alt="commit activity" />

`extract_pdf_page` converts every failure from the PDF backend into
`Err(String)` — a missing `mutool` binary, a non-zero exit, non-UTF-8 stdout, an
unreadable OCR cache, and a genuinely absent page all arrive here identically.
The result is pushed as a `Severity::Error` issue, which makes
`ValidationReport::is_valid` false, which increments `report.invalid` and drives
`bail!("{} summary or summaries have invalid evidence")` in `check`
(src/cli.rs:876-881) and `bail!("evidence audit failed")` in `audit`
(src/cli.rs:979-981). `main` maps every `Err` to `exit(1)` (src/main.rs:3-6), so
a CI gate running `receipts check` sees exactly the same signal — status 1, "N
summaries have invalid evidence" — whether the summary genuinely misquotes its
source or the runner simply lacks `mutool` on PATH. For a tool whose entire
value is being an authority on whether a claim is supported, reporting "the
evidence is invalid" when the truth is "the checker could not run" is a false
accusation, and it is the failure mode most likely to occur on a fresh CI image.
The `pdf_extraction_failed` code does survive into JSON output, so a consumer
parsing the report can tell the difference; nothing in the exit status, the
`invalid` counter, or the human output can. README documents only "non-zero exit
on failure" (README.md:141, 150), so no documented contract is being violated —
this is a design gap, not a doc/code mismatch.

```rust src/validate.rs:275-281
Err(error) => issues.push(issue(
    "pdf_extraction_failed",
    Severity::Error,
    error.clone(),
    Some(entry.claim),
    Some(locator_index),
)),
```

> I don't have to break the evidence. I have to break the *check*. A missing
> `tesseract` and a fabricated citation produce the same count, the same exit code,
> and the same red line in CI — so if the tool can be made to fail on its own
> tooling, every claim in the corpus reads as unsupported and nobody can tell which
> failures were real.

Related [`propose` aborts the whole corpus on one bad summary, discarding completed work](#propose-aborts-the-batch-on-one-unreadable-summary).

**Remediation:** Separate environment failure from evidence failure. Either (a) give
infrastructure issues their own severity or counter so `check`/`audit` can exit
with a distinct status (e.g. 2 = could not evaluate, 1 = evidence invalid),
matching the `ErrorMetadata` exit-code vocabulary the CLI schema already
declares; or (b) fail fast: probe the backends once per run (the `doctor` logic
already exists) and abort with a tool error before any summary is judged.
Preserving the `anyhow` chain instead of `error.to_string()` would also let the
caller test for the underlying `io::ErrorKind::NotFound`.

*Effort (medium):* Touches the issue model, both aggregate commands, and the declared exit-code schema.

<div>&hairsp;</div>

### Every report path uses `println!`, so a closed stdout panics and files a crash dump {#broken-pipe-panic-writes-crash-dump}

**moderate** · `src/output.rs:1-8` · effort: small · <img src="assets/sparkline-broken-pipe-panic-writes-crash-dump.svg" height="14" alt="commit activity" />

`print_json` returns `Result` but routes its only write through `println!`,
whose expansion panics ("failed printing to stdout") when the underlying write
errors. Rust sets SIGPIPE to SIG_IGN at startup, and neither `main` nor librebar
0.6.0 restores the default disposition (no `signal`/`libc` SIGPIPE handling
exists anywhere in the librebar source), so writing to a closed pipe returns
EPIPE rather than killing the process — and EPIPE becomes a panic. The same
applies to all 17 bare `println!` calls in src/cli.rs (doctor at 645, audit at
971, propose at 1030-1093, the check report at 1200-1228, locate at 1262-1264).
Because src/main.rs:2 installs `librebar::crash::install` before dispatch, the
panic does not merely print a message: the hook serializes a `CrashInfo` with
message, location, and backtrace into a persistent crash dump under the cache
directory (registry librebar-0.6.0/src/crash.rs) and chains to the default hook.
So the routine `receipts audit --format json | head` or `receipts propose |
less` (quit early) produces a backtrace, an on-disk crash artifact, and exit
status 101 — indistinguishable from a real defect for anyone reading CI logs or
the dump directory later.

```rust src/output.rs:1-8
use anyhow::Result;
use serde::Serialize;

/// Print any serializable report as stable pretty JSON.
pub fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
```

Related [Inline-HTML `<br>` probe byte-slices a str and panics on multi-byte input](#inline-html-byte-slice-panics-on-multibyte-markdown).

**Remediation:** Write reports through `writeln!` on a locked `io::Stdout` and propagate the
`io::Error` into the existing `anyhow::Result`, then treat
`ErrorKind::BrokenPipe` as a clean exit (status 0 or 141) in `main` rather than
an error. Human output in src/cli.rs should move to the same helper so no
`println!` remains on a report path. Dynamic verification (not run here):
`receipts audit --format json | head -1` must exit without a crash dump.

*Effort (small):* New print helper plus mechanical replacement of println! call sites in cli.rs.

<div>&hairsp;</div>

### `propose` aborts the whole corpus on one bad summary, discarding completed work {#propose-aborts-the-batch-on-one-unreadable-summary}

**advisory** · `src/cli.rs:993-1002` · effort: small · <img src="assets/sparkline-propose-aborts-the-batch-on-one-unreadable-summary.svg" height="14" alt="commit activity" />

With no ids on the command line, `propose` enumerates the entire corpus via
`summary_ids` (src/cli.rs:986-990) and then propagates the first per-summary
failure out of `run`. Its sibling commands do the opposite: `check`
(src/cli.rs:844-862) and `audit` (src/cli.rs:909-941) catch the same two
conditions and record them as `summary_parse_failed` / `id_mismatch` issues
against the offending id while continuing the sweep. The inconsistency has two
observable costs. In human mode, proposals for the summaries already processed
have been printed to stdout (src/cli.rs:1005-1007) before the abort, so the
operator gets a silently truncated list plus an error. In JSON mode nothing is
printed at all — the `print_json` call sits after the loop
(src/cli.rs:1010-1016) — so one malformed YAML file in a corpus of a hundred
yields zero output and no indication of which summaries would have succeeded.
This is distinct from the prior audit's accepted
`propose-silently-suppresses-source-failures`: that finding covers unreadable
Markdown and PDF *sources* inside `propose_document`, which are deliberately
tolerated. The behavior here is the opposite polarity — an advisory tool that
shrugs off a missing PDF but hard-aborts the entire run over one summary it
could equally well skip — so the accepted rationale ("fewer candidates rather
than a hard failure") argues for changing this path, not for leaving it.

```rust src/cli.rs:993-1002
let mut reports = Vec::new();
for id in &ids {
    let summary = read_summary(corpus, id)?;
    if summary.id != *id {
        bail!(
            "summary ID {:?} does not match filename ID {:?}",
            summary.id,
            id
        );
    }
```

Related [A missing or broken PDF tool is counted and exited as invalid evidence](#tool-failure-indistinguishable-from-invalid-evidence).

**Remediation:** Match `check`/`audit`: on `read_summary` failure or id mismatch, record the id
and its error in the report and continue. The JSON envelope already has a
multi-summary shape (`{"summaries": [...]}`) that can carry a per-id error
entry, and human mode can print the failure to stderr without losing the
proposals already computed.

*Effort (small):* Mirror the existing error_report pattern from check into the propose loop.

<div>&hairsp;</div>

### Vocabulary localization discards `Result`s with no stated reason {#vocabulary-rename-results-discarded-without-rationale}

**note** · `src/terms.rs:113-119` · effort: trivial · <img src="assets/sparkline-vocabulary-rename-results-discarded-without-rationale.svg" height="14" alt="commit activity" />

Six `drop(rename(...))` calls across `localize`, `localize_entry`, and
`localize_locate` (src/terms.rs:99, 107, 109, 117, 118, 132) discard a `Result`
with no comment. The discards are currently sound — `rename` only ever returns
`Err` on the `strict && object.contains_key(to)` branch (src/terms.rs:150-152)
and all six callers pass `strict = false` — but that proof lives in a different
function, so a future change making non-strict renames fallible would be
silently swallowed on the output path, producing evidence records written under
the wrong vocabulary keys. The same shape appears at src/hash.rs:14 (`let _ =
write!(s, "{b:02x}")`), where the discard is infallible-by-construction for
`String` but likewise unexplained. Criterion 5.3 asks for an explicit rationale
at the discard site; neither has one.

```rust src/terms.rs:113-119
pub fn localize_entry(&self, entry: &mut Value) {
    if self.is_canonical() {
        return;
    }
    drop(rename(entry, "claim_sha256", &self.hash_key(), false));
    drop(rename(entry, "claim", &self.claim, false));
}
```

**Remediation:** Encode the invariant in the type rather than the comment: split `rename` into a
`rename_strict(...) -> Result<()>` and an infallible `rename(...)` that returns
`()`, so the localization paths have no `Result` to discard. For src/hash.rs:14,
`write!` into a `String` cannot fail — either add a one-line comment saying so
or use a hex helper that does not return a `Result` at all. Neither change
alters behavior.

*Effort (trivial):* Split one helper into strict/non-strict variants; six call sites lose their drop().

<div>&hairsp;</div>

*Verdict: The happy path is careful; the failure paths are not yet. A missing PDF tool is counted and exited as invalid evidence, which is the worst possible conflation for an evidence gate — the user reads 'this claim is unsupported' when the truth is 'I never looked'. The inline-HTML byte slice panics on a `<?…?>` processing instruction whose first character is multi-byte — narrow, but it is corpus data the tool is designed to accept without supervision, and the result is a crash dump rather than a Markdown error. And every report path uses println!, so a closed stdout — receipts check | head is enough — panics and files a crash dump instead of exiting quietly. All three are small fixes with disproportionate effect on how the tool reads in practice.*

<div>&nbsp;</div>

---

## The Published Contract Surface

*The README and the CLI Spec are the tool's contract with its consumers, and the code has drifted from that contract in both directions — declaring codes it never emits, emitting codes it never declared, and changing an output shape without saying so.*

### Five declared error codes are unreachable; the codes actually emitted embed the corpus source name {#schema-declares-source-error-codes-the-code-never-emits}

**significant** · `src/cli.rs:553-582` · effort: small · <img src="assets/sparkline-schema-declares-source-error-codes-the-code-never-emits.svg" height="14" alt="commit activity" />

README's schema section promises the CLI Spec describes "every ... error code",
and the Vocabulary section promises that "a script consuming `--format json` is
portable across corpora" because codes are stable identifiers. Neither holds for
source-file validation. `src/validate.rs:112-113` builds a per-source label —
`format!("{source_name}/markdown")` — and `validate_source_path`,
`validate_file_hash`, and `validate_resolved_source` then interpolate that label
into the code itself: `format!("{label}_source_mismatch")`,
`format!("{label}_hash_mismatch")`, `format!("{label}_read_failed")`,
`format!("{label}_source_outside_repo")`,
`format!("{label}_source_unresolvable")`. The emitted values are therefore
strings like `default/markdown_hash_mismatch` and
`supplement/pdf_source_outside_repo` — corpus-specific, never the five declared
names. Comparing the declaration set against every literal the code emits
confirms both directions fail: the five `source_*` kinds declared in
`src/cli.rs:553-582` are emitted nowhere, and no declaration matches what a
consumer actually receives. Note also the naming drift: the declaration says
`source_hash_mismatch` but the runtime suffix is `_hash_mismatch` (no
`source_`), while the declaration says `source_outside_repo` and the runtime
suffix is `_source_outside_repo`. librebar's `ErrorMetadata.kind` is documented
as a "Stable machine-readable error kind" and has no pattern or wildcard
facility, so these five entries cannot be read as declared templates. The prior
remediation recorded these five entries as declarations of "5 dynamic code
patterns emitted at runtime"
(record/audits/2026-08-10-16-full-repo/actions-taken.md:47), but
`ErrorMetadata.kind` is a literal string with no pattern facility, so the
declaration and the emission never meet. An agent that switch-matches on `code`
per the CLI Spec — the documented "primary interface for agents integrating with
`receipts` programmatically" — will silently fall through on every
source-integrity failure.

```rust src/cli.rs:553-582
.error(
    ErrorMetadata::new("source_hash_mismatch")
        .exit_code(1)
        .retryable(false)
        .description("Source file SHA-256 disagrees with recorded hash"),
)
.error(
    ErrorMetadata::new("source_mismatch")
        .exit_code(1)
        .retryable(false)
        .description("Recorded source path does not match expected path"),
)
.error(
    ErrorMetadata::new("source_read_failed")
        .exit_code(1)
        .retryable(false)
        .description("Source file could not be read for hashing"),
)
.error(
    ErrorMetadata::new("source_outside_repo")
        .exit_code(1)
        .retryable(false)
        .description("Resolved source path escapes the corpus root"),
)
.error(
    ErrorMetadata::new("source_unresolvable")
        .exit_code(1)
        .retryable(false)
        .description("Source path cannot be resolved to a real path"),
)
```

Related [`summary_parse_failed` and `id_mismatch` are emitted in JSON output but declared nowhere in the CLI Spec](#runtime-error-codes-absent-from-cli-spec).

**Remediation:** Make the emitted codes match the declaration: keep the five `source_*` kinds as
literal, corpus-independent codes and move the per-source label out of `code`
and into `message` (which already carries the path) plus a new structured field
on `EvidenceIssue` (e.g. `source: Option<String>`), the same way `claim` and
`locator` are already carried as separate fields rather than baked into the code
string. Concretely, change `validate_source_path` to emit `"source_mismatch"`,
`validate_file_hash` to emit `"source_hash_mismatch"` / `"source_read_failed"`,
and `validate_resolved_source` to emit `"source_outside_repo"` /
`"source_unresolvable"`, threading `source_name` through as data. Add a test
that asserts the set of codes reachable at runtime is a subset of the set
declared in `schema_metadata()`, so this cannot regress.

*Effort (small):* Five call sites in `src/validate.rs`, one optional field on `EvidenceIssue`, and a declaration-vs-emission test. No change to the declared schema itself.

<div>&hairsp;</div>

### `summary_parse_failed` and `id_mismatch` are emitted in JSON output but declared nowhere in the CLI Spec {#runtime-error-codes-absent-from-cli-spec}

**significant** · `src/cli.rs:844-862` · effort: trivial · <img src="assets/sparkline-runtime-error-codes-absent-from-cli-spec.svg" height="14" alt="commit activity" />

README says `receipts schema` "describ[es] every subcommand, flag, type,
default, output field, and error code" and calls it "the primary interface for
agents integrating with `receipts` programmatically". `check` and `audit` both
surface unparseable and misnamed summaries as `EvidenceIssue` entries inside
`summaries[].issues[]` with codes `summary_parse_failed` and `id_mismatch` (the
identical pair is emitted again from `audit` at `src/cli.rs:917` and
`src/cli.rs:933`). Neither code appears among the 42 `ErrorMetadata`
declarations in `schema_metadata()`. These are not exotic paths — a summary with
a YAML typo, an unknown field inside `evidence`, or a filename that drifted from
its `id` produces them, and both fail the run with a non-zero exit. A consumer
built from the declared schema has no branch for either, so the two most common
authoring mistakes in the corpus are exactly the ones it cannot classify.

```rust src/cli.rs:844-862
let summary = match read_summary(corpus, &id) {
    Ok(summary) => summary,
    Err(error) => {
        report.invalid += 1;
        report
            .summaries
            .push(error_report(id, "summary_parse_failed", error.to_string()));
        continue;
    }
};
if summary.id != id {
    report.invalid += 1;
    report.summaries.push(error_report(
        id,
        "id_mismatch",
        format!("summary ID {:?} does not match filename", summary.id),
    ));
    continue;
}
```

Related [Five declared error codes are unreachable; the codes actually emitted embed the corpus source name](#schema-declares-source-error-codes-the-code-never-emits).

**Remediation:** Add two `ErrorMetadata` declarations to `schema_metadata()` in `src/cli.rs`
alongside the existing set — `summary_parse_failed` (exit_code 1, non-retryable,
"Summary document could not be read or parsed") and `id_mismatch` (exit_code 1,
non-retryable, "Summary `id` does not match the ID resolved from its filename").
Pair this with the declaration-vs-emission test proposed in
`schema-declares-source-error-codes-the-code-never-emits` so the two sets stay
locked together.

*Effort (trivial):* Two declarations appended to the existing builder chain.

<div>&hairsp;</div>

### `propose --format json` returns two different top-level shapes, and only one is declared {#propose-json-shape-varies-by-summary-count}

**significant** · `src/cli.rs:1010-1016` · effort: trivial · <img src="assets/sparkline-propose-json-shape-varies-by-summary-count.svg" height="14" alt="commit activity" />

The prior audit fixed `propose-multi-id-invalid-json` by wrapping multi-ID
output in `{"summaries": [...]}`, deliberately leaving single-ID output
unwrapped "for backward compatibility". That fix changed observable output, but
neither the CLI Spec nor the README was updated to match. `schema_metadata()`
still declares `propose`'s output as exactly two fields, `id` (string) and
`claims` (object[]) — the single-summary shape only. The `summaries` field is
undeclared, so an agent generating a parser from the schema sees no trace of the
wrapper. Worse, the shape is data-dependent rather than flag-dependent: bare
`receipts propose` (the documented `[ID...]` form with no IDs) walks the whole
corpus, so the same command emits `{id, claims}` against a one-summary corpus
and `{"summaries": [...]}` against a two-summary corpus — and `{"summaries":
[]}` against an empty one, since `reports.len() == 0` also takes the else
branch. README's `propose` section documents the human output in detail and says
nothing about the JSON shape at all.

```rust src/cli.rs:1010-1016
if json {
    if reports.len() == 1 {
        print_json(&reports.into_iter().next().unwrap())?;
    } else {
        print_json(&serde_json::json!({ "summaries": reports }))?;
    }
}
```

**Remediation:** Pick one shape and declare it. The lowest-risk fix that keeps the schema honest
is to always emit `{"summaries": [...]}` and declare a single `summaries` output
field (`object[]`, "Per-summary proposal reports: {id, claims}") in
`schema_metadata()` — an unconditional shape is what an agent integration needs,
and `propose` is advisory output with no persisted consumers. If the unwrapped
single-summary form must be kept, declare both shapes explicitly and document
the branch condition in README's `propose` section. Either way, add an
integration test asserting the top-level key set for zero, one, and two
summaries.

*Effort (trivial):* One branch collapsed or one output field declared, plus a shape test.

<div>&hairsp;</div>

### The declared error-code registry and the emitted codes are kept in sync by hand {#error-code-registry-is-hand-maintained}

**moderate** · `src/cli.rs:539-552` · effort: medium · <img src="assets/sparkline-error-code-registry-is-hand-maintained.svg" height="14" alt="commit activity" />

`schema_metadata` declares 42 error codes as string literals across
src/cli.rs:331-582. The codes themselves are emitted as unrelated string
literals in four other places — `issue("empty_markdown_candidates", ...)` at
src/validate.rs:90, `issue("weak_section_only", ...)` at src/validate.rs:328,
the seventeen codes in `src/evidence.rs:197-403`, `validate_review`
(src/review.rs:106-187), and the `summary_parse_failed` / `id_mismatch` codes
constructed inline in `check` and `audit` (src/cli.rs:850, 858, 917, 933) — plus
five dynamically composed families such as `format!("{label}_hash_mismatch")`
(src/validate.rs:427). Nothing connects the two sets: no shared constant, no
enum, no test. The prior audit found the registry declaring 18 of the codes
actually emitted and the fix was to type the other 19 in by hand, which is the
same hand-synchronization that produced the gap. A new `issue("...")` call is an
undeclared code, and a deleted code path leaves a phantom entry in `--schema`
output; both are invisible to `just check`.

```rust src/cli.rs:539-552
        .description("PDF text extraction failed"),
)
.error(
    ErrorMetadata::new("empty_markdown_candidates")
        .exit_code(1)
        .retryable(false)
        .description("No markdown template candidates for a source"),
)
.error(
    ErrorMetadata::new("weak_section_only")
        .exit_code(0)
        .retryable(false)
        .description("Every locator sits under a weak section heading (warning)"),
)
```

Related [Every JSON output shape except single-ID propose is asserted nowhere](#structured-output-contracts-untested). Related [Three copies of the EvidenceIssue constructor plus two inline literals](#duplicated-issue-constructors).

**Remediation:** Make the codes a single source of truth. The cheapest version that closes the
loop: define the static codes as `pub const` items (or a `#[non_exhaustive]`
enum with an `as_str`) in the module that emits them, have `schema_metadata`
build its `ErrorMetadata` entries from that list, and construct issues from the
same constants. Failing that, add a test that greps the emitted-code literals
out of the source and asserts every one appears in `schema_metadata`, so the
sync is at least enforced rather than remembered.

*Effort (medium):* Mechanical but wide: 42 declarations and ~44 emission sites must move to shared constants.

<div>&hairsp;</div>

### The documented "default YAML-ready output" of `locate` and `propose` becomes JSON the moment it is redirected {#default-output-becomes-json-when-redirected}

**moderate** · `src/cli.rs:183-185` · effort: trivial · <img src="assets/sparkline-default-output-becomes-json-when-redirected.svg" height="14" alt="commit activity" />

README describes `locate`'s default output as "YAML-ready", keeping the
PDF-match diagnostic "in a comment so [it is] not persisted in the strict
evidence contract", and describes `propose`'s human output as "commented YAML
safe to paste and edit". Both descriptions assume the user captures that output.
But the default format is librebar's `OutputFormat::Auto`, and `resolve_for`
maps `Auto` to `ResolvedOutputFormat::Json` whenever stdout is not a terminal.
So `receipts locate smith-2019 --claim 0 --exact "..." --page 3 > record.yaml` —
the natural way to act on a "YAML-ready" record — writes JSON, and `receipts
propose > candidates.yaml` writes JSON rather than the commented YAML with its
pasteable `receipts locate` command. The same applies to any pipe: `| tee`, `|
pbcopy`, running under `make`/`just`, or capturing in CI. The documented
affordance works only in the one case where the user cannot easily capture it.

```rust src/cli.rs:183-185
let format = cli.common.output_format();
let json = format == ResolvedOutputFormat::Json;
let quiet = cli.common.quiet;
```

Related [`propose --format json` returns two different top-level shapes, and only one is declared](#propose-json-shape-varies-by-summary-count).

**Remediation:** Make the two YAML-producing commands honor the documented default regardless of
where stdout points. Resolve the format for `Locate` and `Propose` with
`cli.common.output_format_for(true)` (librebar exposes exactly this for
deterministic rendering) so `auto` means text for them, leaving `--format json`
as the explicit opt-in the README already documents. `doctor`, `check`, and
`audit` are status commands where auto-JSON-on-redirect is the right behavior
and should keep `output_format()`. Add an integration test that runs `locate`
with stdout captured and asserts the first line begins with `# PDF match
diagnostic:`.

*Effort (trivial):* One-line format resolution change on two subcommand paths, plus a capture test.

<div>&hairsp;</div>

### Config discovery silently merges a user-level file and `RECEIPTS_*` environment variables the README never mentions {#undocumented-user-config-and-environment-layers}

**moderate** · `src/config.rs:185-199` · effort: small · <img src="assets/sparkline-undocumented-user-config-and-environment-layers.svg" height="14" alt="commit activity" />

README gives what reads as an exhaustive account of configuration discovery:
walk up from the working directory checking `.config/receipts.yaml`,
`.receipts.yaml`, then `receipts.yaml`, stop at a `.git` boundary, fall back to
defaults, and use `--config FILE` to name one explicitly. The project-file
search matches librebar exactly. What README omits is that `ConfigLoader::new`
defaults to `include_user_config: true` and `environment_source:
Some(ProcessEnvironment)`, and `load()` disables neither. `load_inner` therefore
merges a user config at the XDG path (`~/Library/Application
Support/receipts/config.{yaml,toml,json}` on macOS) beneath the project file,
then overlays `RECEIPTS_*` environment variables — double-underscore path
segments, e.g. `RECEIPTS_PDF__OCR__ENABLED=false` or
`RECEIPTS_CORPUS__SUMMARIES=other/{id}.yaml` — on top of both. Two documented
guarantees break. First, README frames `receipts.yaml` as the sole declaration
of layout, but a stray user file or exported variable changes token-coverage
severity, OCR settings, or path templates for every corpus on the machine.
Second, README says `doctor` reports "which config file was used", and the
`doctor` command prints `config:` from `Corpus::config_file()` — which
`src/config.rs:204-208` populates from `sources.project_file` alone. A corpus
reconfigured by the user layer or an environment variable still reports `config:
ok (defaults)`, so the tool's own diagnostic actively misreports provenance.
That undercuts the determinism claim the README opens with.

```rust src/config.rs:185-199
pub fn load(start: &Path, explicit: Option<&Path>) -> Result<Discovered> {
    let search = to_utf8(start)?;
    let mut loader = librebar::config::ConfigLoader::new("receipts").with_project_search(&search);
    if let Some(path) = explicit {
        loader = loader.with_file(to_utf8(path)?);
    }
    // Loaded as a `Value` so desugaring sees only what the files declared:
    // seeding the merge with `Config::default()` would make every corpus look
    // like it had declared `sources`.
    let (mut raw, sources): (Value, _) = loader
        .load()
        .map_err(|error| anyhow!("failed to load receipts configuration: {error}"))?;
    if raw.is_null() {
        raw = Value::Object(Map::new());
    }
```

**Remediation:** Restrict the loader to what README documents: chain
`.with_user_config(false).without_environment()` onto the `ConfigLoader` in
`src/config.rs:187`, making the project file (or `--config FILE`) the only
source of configuration, exactly as the README describes. If the extra layers
are wanted, the alternative is to keep them and make `doctor` honest — carry
`ConfigSources { user_file, environment_variables, .. }` through `Discovered`
into `Corpus`, report each applied layer as its own `doctor` line and JSON
field, and document all three layers plus the `RECEIPTS_*` naming convention in
README's Configuration section. Disabling is the smaller and more
determinism-preserving change.

*Effort (small):* Two builder calls if disabling; a plumbing pass through Discovered/Corpus/doctor if surfacing.

<div>&hairsp;</div>

### Summary IDs are silently restricted to lowercase-alphanumeric-hyphen with a 3-character minimum {#summary-id-constraints-undocumented}

**advisory** · `src/corpus.rs:168-179` · effort: trivial · <img src="assets/sparkline-summary-id-constraints-undocumented.svg" height="14" alt="commit activity" />

Every command that resolves a path calls `validate_id`, which admits only
lowercase ASCII letters, digits, and interior hyphens, with a minimum length of
three. README uses `ID` freely throughout the Run section and the evidence
contract without ever stating the rule, and the sole worked example
(`smith-2019`) happens to satisfy it. A corpus using `Smith2019`, `smith_2019`,
`10.1234/xyz`, or a two-character key gets `invalid summary ID "..."` with no
pointer to the rule it violated, and the constraint is not discoverable from
`--help` or the CLI Spec either — the declared `invalid_id` error says only
"Summary ID is not safe for template expansion". Adopting `receipts` against an
existing corpus means renaming files to satisfy an unwritten rule.

```rust src/corpus.rs:168-179
fn validate_id(id: &str) -> Result<()> {
    let valid = id.len() >= 3
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !valid {
        bail!("invalid summary ID {id:?}");
    }
    Ok(())
}
```

**Remediation:** State the rule where users encounter IDs. Add one sentence to README's Evidence
contract section — IDs are at least three characters of lowercase ASCII letters,
digits, and interior hyphens — and widen the `bail!` in `validate_id` to name
the constraint it enforced rather than only echoing the rejected value (e.g.
"invalid summary ID {id:?}: IDs must be 3+ characters of lowercase letters,
digits, and interior hyphens"). Consider extending the `invalid_id`
`ErrorMetadata` description in `schema_metadata()` with the same wording so
agent integrations can pre-validate.

*Effort (trivial):* One README sentence, one error-message string, one schema description.

<div>&hairsp;</div>

*Verdict: This project's own rule is that when documentation and code disagree, the documentation wins and the fix is to write the code. Applied here, that produces a clear worklist rather than a debate. Five declared error codes are unreachable and two emitted ones are undeclared, which is what a hand-maintained registry produces over time — the root cause is that nothing ties the declaration to the emission. Two shape surprises are worse than the code drift because they break parsers silently: propose --format json returns an object or a bare array depending on how many summaries it was handed, and the documented default output of locate and propose flips to JSON the moment it is redirected. The user-level config file and RECEIPTS_* environment layer are real, functioning features the README never mentions.*

<div>&nbsp;</div>

---

## The Type and API Surface

*In a tool whose entire job is to keep coordinates straight, line, column, page, and claim index are all bare usize — and the whole crate is exported as a stable-looking public API on a publishable manifest.*

### Every internal module is exported as a stable-looking public API on a publishable crate {#entire-crate-is-a-published-public-api}

**advisory** · `src/lib.rs:1-17` · effort: medium · <img src="assets/sparkline-entire-crate-is-a-published-public-api.svg" height="14" alt="commit activity" />

The prior audit accepted `public-apis-erase-error-types`,
`unvalidated-discovered-state`, and `cache-manifest-invariants-are-bypassable`
on the rationale that "there are no library consumers — the `pub` surface exists
for internal/test convenience only." That rationale is factually correct and is
precisely why the surface itself, not each downstream symptom, is the finding.
`lib.rs` is a bare list of `pub mod` with no `pub use` façade and no
`#[doc(hidden)]`, so every one of ~240 `pub` items is a load-bearing commitment:
`cli::run`, `output::print_json`, `normalize::normalize`,
`Corpus::from_discovered`, `CacheManifest`'s public fields, and the raw
`Mutool`/`Tesseract` adapters are all indistinguishable to a consumer from the
three functions that are actually the product (`validate_document`,
`propose_document`, `parse_summary`). Meanwhile `Cargo.toml` carries full
crates.io publish metadata (`license`, `repository`, `keywords`, `categories`,
`description`) and no `publish = false`, so a single `cargo publish` turns the
entire internal structure into a semver contract that 0.1.0 is not ready to
hold. Each accepted finding is a rational local decision; together they describe
a crate that has not decided whether it is a library.

```rust src/lib.rs:1-17
//! Deterministic evidence validation for LLM summaries of PDF sources.

pub mod cli;
pub mod config;
pub mod corpus;
pub mod evidence;
pub mod hash;
pub mod markdown;
pub mod normalize;
pub mod output;
pub mod pdf;
pub mod propose;
pub mod review;
pub mod sections;
pub mod terms;
pub mod tokens;
pub mod validate;
```

Enables [Line, column, page, and claim index are all bare `usize` and swap silently](#coordinate-primitives-are-interchangeable-usize). Enables [A byte offset is renamed `column` and published in the evidence schema](#byte-offset-published-as-a-markdown-column). Enables [Every output report derives `Serialize` alone, so the crate cannot read its own JSON](#report-types-are-write-only). Related [public-apis-erase-error-types](#public-apis-erase-error-types). Related [unvalidated-discovered-state](#unvalidated-discovered-state). Related [cache-manifest-invariants-are-bypassable](#cache-manifest-invariants-are-bypassable).

**Remediation:** Decide the posture once and encode it. If this is a binary with a test-only
library target, add `publish = false` to `Cargo.toml` — that one line removes
the compatibility obligation outright and costs nothing. Module-level demotion
buys much less than it looks: `tests/` imports 13 of the 15 modules, several at
depth (`pdf::cache::{CacheManifest, OcrCache, OcrProfile, cache_key}`,
`pdf::tesseract::{OcrEngine, PageRenderer, ocr_page, parse_tsv, ...}`,
`propose::{propose_document, split_sentences}`, `validate::validate_document`,
`normalize::normalize`), leaving only `cli` and `output` demotable as whole
modules. The reachable win is per-item: `#[doc(hidden)]` on the test-only
surface plus a curated `pub use` façade for the three product entry points. If
it is a library, curate `lib.rs` into a deliberate façade — `pub use` the
handful of entry types, mark the rest `pub(crate)` or `#[doc(hidden)]` — and
then the three previously-accepted encapsulation findings become in-scope rather
than moot. Either way, the choice belongs in `lib.rs` and `Cargo.toml`, not
distributed across per-symbol judgment calls.

*Effort (medium):* Mechanical but wide: demoting visibility touches every module and will surface which items the integration tests genuinely need. The decision is the hard part; the edit is compiler-guided.

<div>&hairsp;</div>

### Line, column, page, and claim index are all bare `usize` and swap silently {#coordinate-primitives-are-interchangeable-usize}

**significant** · `src/markdown.rs:163-174` · effort: medium · <img src="assets/sparkline-coordinate-primitives-are-interchangeable-usize.svg" height="14" alt="commit activity" />

The domain's central failure mode is confusing one integer coordinate for
another, and the type system prevents none of it. `MarkdownLocator.line`,
`MarkdownLocator.column`, `PdfLocator.page`, `ClaimEvidence.claim`,
`CoverageScore.matched`, `CoverageScore.required`, and the byte offsets in
`UnitBuilder.start` / `build_line_starts` are all `usize`. `resolve_unit` takes
`line` and `column` as adjacent positional `usize`; transposing them at
`validate.rs:169-170` compiles and produces a plausible-looking
`markdown_unit_missing` error rather than a type error. The same shape recurs in
`evidence::issue(..., claim: Option<usize>, locator: Option<usize>)` and
`validate::issue` with the same two adjacent `Option<usize>` parameters —
transposing those mislabels every issue in the JSON report while every test
still passes. The one-based invariant that all four coordinates share is
enforced by scattered runtime `== 0` checks in `markdown.rs:169`,
`evidence.rs:383-409`, `mutool.rs:31`, `mutool.rs:66`, `tesseract.rs:238`, and
`cache.rs:47` — six independent restatements of a property one newtype
constructor would hold once.

```rust src/markdown.rs:163-174
pub fn resolve_unit(
    units: &[MarkdownUnit],
    kind: UnitKind,
    line: usize,
    column: usize,
) -> Result<&MarkdownUnit> {
    if line == 0 || column == 0 {
        bail!("Markdown line and column must be one-based");
    }
    let mut matches = units
        .iter()
        .filter(|unit| unit.kind == kind && unit.line == line && unit.column == column);
```

Enables [A byte offset is renamed `column` and published in the evidence schema](#byte-offset-published-as-a-markdown-column). Enabled by [Every internal module is exported as a stable-looking public API on a publishable crate](#entire-crate-is-a-published-public-api).

**Remediation:** Introduce zero-cost newtypes for the coordinate vocabulary — `Line`, `Column`,
`Page`, `ClaimIndex`, `LocatorIndex` — each a `#[repr(transparent)]` tuple
struct over `usize` with a fallible constructor that rejects zero, and
`Serialize`/`Deserialize` forwarded transparently so the on-disk schema is
unchanged. Every `== 0` guard then collapses into the constructor, and the
transposition class of bug becomes a compile error. Start with `Line`/`Column`
and `Page`, which carry the shared one-based invariant and the most call sites;
`ClaimIndex`/`LocatorIndex` can follow.

*Effort (medium):* Compiler-guided once the newtypes exist, but they thread through the evidence schema, the CLI arguments, and the proposal types, so the change touches most modules in one pass.

<div>&hairsp;</div>

### propose defines field-for-field copies of the evidence locator types {#propose-duplicates-locator-types}

**significant** · `src/propose.rs:55-69` · effort: small · <img src="assets/sparkline-propose-duplicates-locator-types.svg" height="14" alt="commit activity" />

`MarkdownMatch` has exactly the fields of `evidence::MarkdownLocator`
(src/evidence.rs:70-76) and `PdfMatch` has exactly the fields of
`evidence::PdfLocator` (src/evidence.rs:80-83). These are not coincidentally
similar types — a proposal exists to be turned into a locator, and
`print_propose_human` (src/cli.rs:1049-1092) reads `candidate.markdown.line`,
`.column`, `.unit`, `.section` and `candidate.pdf.page`, `.backend` to print a
`receipts locate` command that writes precisely those fields. Any field added to
a locator (a second coordinate, a span offset, a backend variant qualifier) must
be added by hand in a second place or `propose` silently stops being able to
describe the locator it is proposing. Nothing in the type system or the test
suite ties the two definitions together, so the drift is silent.

```rust src/propose.rs:55-69
/// Where a candidate sits in the converted Markdown.
#[derive(Debug, Clone, Serialize)]
pub struct MarkdownMatch {
    pub line: usize,
    pub column: usize,
    pub unit: UnitKind,
    pub section: Vec<String>,
}

/// The single PDF page a candidate was found on.
#[derive(Debug, Clone, Serialize)]
pub struct PdfMatch {
    pub page: usize,
    pub backend: PdfBackend,
}
```

Related [Three copies of the EvidenceIssue constructor plus two inline literals](#duplicated-issue-constructors).

**Remediation:** Have `Candidate` hold `evidence::MarkdownLocator` and
`Option<evidence::PdfLocator>` directly and delete `MarkdownMatch` and
`PdfMatch`. The serialized JSON shape is unchanged for `markdown`
(`MarkdownLocator` already skips an empty `section`, which only removes a field
that carries no information) and identical for `pdf`. If the empty `section`
must stay in propose output for consumers, wrap rather than re-declare —
`#[serde(flatten)]` over the locator type keeps one definition.

*Effort (small):* Two type deletions and the construction site in propose_document; serialized shape is preserved.

<div>&hairsp;</div>

### A byte offset is renamed `column` and published in the evidence schema {#byte-offset-published-as-a-markdown-column}

**moderate** · `src/markdown.rs:247-254` · effort: small · <img src="assets/sparkline-byte-offset-published-as-a-markdown-column.svg" height="14" alt="commit activity" />

`parse_units` drives `Parser::into_offset_iter`, whose `range.start` is a byte
offset. `line_column_from_index` subtracts the line-start byte offset and calls
the result `column`, which then becomes `MarkdownUnit.column`,
`MarkdownLocator.column` in the on-disk evidence schema, and the `--column` CLI
flag. For any unit that begins after a multi-byte character on the same line — a
second table cell in a row whose first cell holds an em dash or an accented
word, a nested list item, a blockquote paragraph — the published "column" is a
byte index, not a character position, and disagrees with what an editor, an LLM,
or a human counting characters would produce. The system is internally
consistent because the same function both writes and re-derives the value, so
validation never fails; the cost lands entirely on anyone computing a column
from outside the tool. Nothing in the code, the README, or the tests names the
unit: `tests/markdown.rs` only ever asserts `column == 1`, so the
byte-versus-character distinction is untested in either direction.

```rust src/markdown.rs:247-254
fn line_column_from_index(line_starts: &[usize], offset: usize) -> (usize, usize) {
    let line_index = line_starts
        .partition_point(|&start| start <= offset)
        .saturating_sub(1);
    let line_start = line_starts[line_index];
    let column = offset.saturating_sub(line_start) + 1;
    (line_index + 1, column)
}
```

Enabled by [Line, column, page, and claim index are all bare `usize` and swap silently](#coordinate-primitives-are-interchangeable-usize). Enabled by [Every internal module is exported as a stable-looking public API on a publishable crate](#entire-crate-is-a-published-public-api).

**Remediation:** Decide which unit the schema means and make the type say so. Either keep byte
semantics and name the field for it (a `ByteColumn` newtype, documented in the
README and the CLI `--column` help), or convert to a character column at the
boundary via `source[line_start..offset].chars().count() + 1`. Whichever you
pick, replace the `(usize, usize)` return with a named struct or the
`Line`/`Column` newtypes from `coordinate-primitives-are-interchangeable-usize`,
and add a regression test with a non-ASCII table row so the choice is pinned.

*Effort (small):* The conversion or the rename is a few lines in one function; the work is picking the semantics and writing the non-ASCII regression test.

<div>&hairsp;</div>

### Domain enums render through `Debug` in validation messages, contradicting their own `as_str` {#debug-formatting-leaks-into-user-facing-messages}

**moderate** · `src/validate.rs:189-198` · effort: trivial · <img src="assets/sparkline-debug-formatting-leaks-into-user-facing-messages.svg" height="14" alt="commit activity" />

`UnitKind::as_str` carries the doc comment "The serialized name, so human output
and JSON agree," and `PdfBackend::as_str` exists for the same reason. Both are
used correctly at five call sites (`validate.rs:225,258,269`,
`cli.rs:1057,1067`) and bypassed at `validate.rs:193`, where `{:?}` emits
`ListItem`/`TableCell`/`CodeBlock` instead of the `list_item`/`table_cell`/
`code_block` the user typed into their YAML. `Verdict` has no `as_str` at all,
so `review.rs:180` tells the user their verdict is `Unsupported` while the file
they must edit says `unsupported`. None of the three enums implements `Display`,
which is what leaves `{:?}` as the only formatting available at a message site
and guarantees this drifts again. In a tool whose entire product is a precise
error message pointing at a schema value, a message that names a value the
schema does not accept sends the reader looking for the wrong string.

```rust src/validate.rs:189-198
0 => issues.push(issue(
    "markdown_missing",
    Severity::Error,
    format!(
        "exact text does not occur in the recorded {:?} unit",
        locator.markdown.unit
    ),
    Some(entry.claim),
    Some(locator_index),
)),
```

Related [vocabulary-leaks-in-validation-messages](#vocabulary-leaks-in-validation-messages).

**Remediation:** Implement `Display` on `UnitKind`, `PdfBackend`, and `Verdict`, delegating to
the existing `as_str` (and adding one to `Verdict`), then switch every
user-facing site from `{:?}` to `{}`. That removes the choice at the call site
rather than relying on each message author to remember, and makes the
`serde(rename_all)` attribute and the human output share one definition.
`validate.rs:181` (`section` paths) can keep `{:?}` — quoting a `Vec<String>` is
the intent there.

*Effort (trivial):* Three `Display` impls delegating to existing `as_str`, one new `as_str` for `Verdict`, and two format-string edits.

<div>&hairsp;</div>

### `Candidate.pdf` is an `Option` the constructor can never leave empty {#candidate-pdf-option-is-never-none}

**advisory** · `src/propose.rs:176-197` · effort: trivial · <img src="assets/sparkline-candidate-pdf-option-is-never-none.svg" height="14" alt="commit activity" />

The `let … else { continue; }` guard means no `Candidate` reaches the vector
without a verified single PDF page — the module doc comment states this as the
contract ("verified against both the Markdown unit that holds them and the
native PDF text"). The `Option<PdfMatch>` field therefore models a state the
code cannot produce, and it costs at both ends: `cli.rs:1055-1058` carries a `"
(no native PDF match)"` branch that can never print, and `cli.rs:1072-1073`
guards the pasteable `receipts locate` command behind an `Option` that is always
`Some`. The declared schema (`cli.rs:315-330`) describes `propose` output only
down to the `claims` field list, so a consumer reading the type gets the
optionality from the serialized `pdf` key alone — which in practice never
appears as `null`. A reader auditing whether unverified candidates can escape
has to re-derive the guard rather than read the type.

```rust src/propose.rs:176-197
let pages = source_pages.get(&raw.source);
let Some(page) = pages.and_then(|pages| verify_pdf(pages, &raw.exact)) else {
    continue;
};
candidates.push(Candidate {
    source: raw.source,
    exact: raw.exact,
    coverage: CoverageScore {
        matched: raw.matched,
        required: claim_tokens.required.len(),
    },
    markdown: MarkdownMatch {
        line: raw.line,
        column: raw.column,
        unit: raw.unit,
        section: raw.section,
    },
    pdf: Some(PdfMatch {
        page,
        backend: PdfBackend::MutoolNative,
    }),
});
```

Related [Every output report derives `Serialize` alone, so the crate cannot read its own JSON](#report-types-are-write-only).

**Remediation:** Change the field to `pub pdf: PdfMatch` and drop the `Some(...)` wrapper, the
`map_or_else` in `print_propose_human`, and the `first.pdf.as_ref()` check at
`cli.rs:1073`. If the intent is to eventually offer Markdown-only candidates,
keep the `Option` but add the code path that produces `None` — an unreachable
variant that documents a future is indistinguishable from a bug.

*Effort (trivial):* One field type, one construction site, two display sites. The JSON output shape is unchanged.

<div>&hairsp;</div>

### Every output report derives `Serialize` alone, so the crate cannot read its own JSON {#report-types-are-write-only}

**advisory** · `src/propose.rs:21-36` · effort: trivial · <img src="assets/sparkline-report-types-are-write-only.svg" height="14" alt="commit activity" />

`ProposalReport`, `ClaimProposal`, `Candidate`, `CoverageScore`,
`MarkdownMatch`, `PdfMatch`, `ValidationReport`, `EvidenceIssue`, `Severity`,
and `PdfBbox` all derive `Serialize` with no `Deserialize`. Seven of the ten —
every type in the `propose` report family plus `ValidationReport` — also lack
`PartialEq`; only `Severity` (src/evidence.rs:103), `EvidenceIssue`
(src/evidence.rs:111), and `PdfBbox` (src/pdf/mod.rs:29) derive it. The
schema-bearing input types next door (`Evidence`, `Locator`, `ClaimEvidence`,
`ReviewEntry`) derive both directions, so the asymmetry is a per-type oversight
rather than a policy. The cost is already visible in the crate's own suite:
`tests/cli.rs:385` decodes `propose --format json` into an untyped
`serde_json::Value` and asserts through string indexing
(`report["claims"][0]["required_tokens"]`), which means a field rename in
`ClaimProposal` breaks the JSON contract while every test still compiles and
passes. The missing `PartialEq` on the report family compounds it — a decoded
`ProposalReport` or `ValidationReport` still cannot be compared with
`assert_eq!`, so round-trip tests are not expressible either.

```rust src/propose.rs:21-36
/// Every claim considered for one summary, with its candidates.
#[derive(Debug, Clone, Serialize)]
pub struct ProposalReport {
    pub id: String,
    pub claims: Vec<ClaimProposal>,
}

/// Candidates offered for one claim, and the tokens none of them reach.
#[derive(Debug, Clone, Serialize)]
pub struct ClaimProposal {
    pub claim: usize,
    pub required_tokens: Vec<String>,
    pub uncovered_tokens: Vec<String>,
    pub advisory_tokens: Vec<String>,
    pub candidates: Vec<Candidate>,
}
```

Enabled by [Every internal module is exported as a stable-looking public API on a publishable crate](#entire-crate-is-a-published-public-api). Related [`Candidate.pdf` is an `Option` the constructor can never leave empty](#candidate-pdf-option-is-never-none).

**Remediation:** Add `Deserialize` and `PartialEq` to the report family. That lets `tests/cli.rs`
decode into `ProposalReport` and assert on typed fields, turning a JSON schema
break into a compile error, and it gives any future consumer — including a
`receipts` invocation piped into another tool — a supported way to read the
output. `PdfBbox` also derives `Copy`, so it costs nothing there. `f64` in
`PdfBbox`/`mean_confidence` blocks `Eq`, not `PartialEq`.

*Effort (trivial):* Derive-list edits plus optionally tightening `tests/cli.rs` to use the typed form.

<div>&hairsp;</div>

*Verdict: The domain's central failure mode is confusing one coordinate for another, and the type system currently offers no defence against it: every one of them is usize, and they compose and swap silently. One instance already shipped — a byte offset is renamed column in the published evidence schema, which is correct only for ASCII. The prior audit accepted the pub-surface findings on the grounds that there are no library consumers, and that reasoning is sound today; the finding here is narrower and still stands, because the manifest carries a description, keywords, categories, and a repository field, and the moment 0.1.0 is published every internal module becomes a compatibility obligation. Newtypes are the highest-leverage change on this surface and they are mechanical.*

<div>&nbsp;</div>

---

## The Performance Surface

*Five performance findings were fixed hours before this audit and all five held; what remains is a loop nest that repeats claim-independent work per claim, and a subprocess spawned per cited page where one per document would do.*

### Native PDF validation spawns one `mutool` process per cited page instead of one per document {#validate-spawns-one-mutool-per-cited-page}

**significant** · `src/validate.rs:233-248` · effort: medium · <img src="assets/sparkline-validate-spawns-one-mutool-per-cited-page.svg" height="14" alt="commit activity" />

The `pdf_pages` memo dedupes by `(source, backend, page)`, so each *distinct*
page cited by a summary enters `extract_pdf_page` exactly once — but for
`PdfBackend::MutoolNative` that call is `native_pages(pdf, Some(page))`, which
forks and execs `mutool draw -F stext.json` and makes MuPDF re-open, re-parse
the xref, and rebuild the page tree of the same PDF from scratch. Unlike the OCR
path, the native path has no disk cache (`src/pdf/cache.rs` is keyed on an
`OcrProfile` and used only by `ocr_page_with_profile`), and the memo is
re-created per `validate_document` call, so nothing survives the summary. For a
corpus run of N summaries whose claims cite D distinct pages each, `check` and
`audit` pay N×D process spawns and N×D document opens. `propose_document`
already demonstrates the alternative on the same trait: `native_pages(&pdf_path,
None)` returns every page of the document from a single spawn, so the same
evidence set costs N spawns instead of N×D. On a summary citing 8 pages of one
paper that is 8 forks, 8 PDF opens, and 8 JSON parses where 1 of each would do.

```rust src/validate.rs:233-248
let Some(pdf_path) = source_pdf_path.get(source_name) else {
    continue;
};
let key = (source_name, locator.pdf.backend, locator.pdf.page);
let extracted = pdf_pages.entry(key).or_insert_with(|| {
    extract_pdf_page(
        provider,
        pdf_path,
        source_pdf_sha256
            .get(source_name)
            .and_then(Option::as_deref)
            .unwrap_or_default(),
        locator.pdf.backend,
        locator.pdf.page,
    )
});
```

Related [The `max_candidates` bound on PDF verification only counts passes, so failing candidates still sweep every page](#pdf-verification-unbounded-when-candidates-fail).

**Remediation:** Extract native text per source PDF rather than per cited page: after the source
loop has resolved `source_pdf_path`, collect the distinct native pages each
source is asked for and, when that count exceeds one, call `native_pages(pdf,
None)` once and index the returned `Vec<ExtractedPage>` by `page` into the
existing `pdf_pages` map, falling back to the single-page call when exactly one
page is cited. If the whole-document form is adopted unconditionally, note the
memory trade: full-document `stext.json` for a large PDF is materially larger
than one page's worth, so bound it on page count rather than always taking the
batch path. Hoisting the memo above the per-summary loop in `cli::check` and
`cli::audit` would additionally deduplicate across summaries that share a
source.

*Effort (medium):* Requires restructuring the extraction memo to be populated in bulk before the claim loop, plus a page-count heuristic. The `PdfTextProvider` trait already supports both shapes, so no interface change and no test-double change.

<div>&hairsp;</div>

### Candidate span generation is redone for every claim although it depends only on the Markdown {#propose-regenerates-claim-independent-spans-per-claim}

**significant** · `src/propose.rs:159-169` · effort: medium · <img src="assets/sparkline-propose-regenerates-claim-independent-spans-per-claim.svg" height="14" alt="commit activity" />

`generate_candidates` is called inside the per-claim loop and rebuilds the
entire span universe from scratch each time: for every Markdown unit it calls
`candidate_spans` (which runs `split_sentences`, allocating a fresh normalized
`String` per sentence plus one more for the whole-unit span) and then
`exact_count(&unit.text, &span)` for each span, an O(|unit text|) substring
sweep. None of that depends on the claim — only the `matched` count does. With K
claims, U units per source, S spans per unit, and T the mean unit length, the
run costs O(K × U × S × T) byte comparisons and O(K × U × S) `String`
allocations, where O(U × S × T) once plus O(K × U × S × |tokens|) would suffice.
Every surviving candidate additionally pays `source.to_owned()` and
`unit.section.clone()` (a `Vec<String>` deep clone) per claim rather than once.
For a 40-claim summary over a 600-unit converted paper this is roughly a 40x
multiplier on the sentence-splitting and uniqueness-checking phase, which is the
whole in-process cost of `propose` once the PDF pages are in hand.

```rust src/propose.rs:159-169
for (index, claim_text) in summary.claims.iter().enumerate() {
    if !all_claims && settled.contains(&index) {
        continue;
    }

    let claim_tokens = tokens::extract(claim_text);
    let mut raw_candidates = Vec::new();
    for (source_name, units) in &source_units {
        raw_candidates.extend(generate_candidates(source_name, units, &claim_tokens));
    }
    rank_raw_candidates(&mut raw_candidates);
```

Enables [The `max_candidates` bound on PDF verification only counts passes, so failing candidates still sweep every page](#pdf-verification-unbounded-when-candidates-fail).

**Remediation:** Hoist span generation out of the claim loop. In the existing per-source setup
loop, build the unique spans once per source — span text, line, column, unit
kind, and section — and store them alongside `source_units`. Inside the claim
loop, iterate that pre-built list and compute only `matched`, materializing a
`RawCandidate` (and its `source`/`section` clones) only for spans with `matched
> 0`. Storing the section path as an `Rc<[String]>` or an index into a section
table removes the per-candidate `Vec<String>` clone as well.

*Effort (medium):* Split `generate_candidates` into a claim-independent build phase and a claim-dependent scoring phase; the ranking key and output ordering must stay byte-identical, so the existing propose tests are the regression gate.

<div>&hairsp;</div>

### `resolve_unit` linearly scans every Markdown unit, and validation calls it twice for the same locator {#resolve-unit-linear-scan-called-twice-per-locator}

**moderate** · `src/markdown.rs:172-181` · effort: small · <img src="assets/sparkline-resolve-unit-linear-scan-called-twice-per-locator.svg" height="14" alt="commit activity" />

`resolve_unit` filters the whole `&[MarkdownUnit]` slice and, because it must
prove no second unit shares the coordinates, consumes the entire iterator on
every successful lookup — an O(U) pass over every unit in the document for one
coordinate lookup. `validate_document` then performs that lookup a second time
for any corpus that configures `sections.weak`: once in the
Markdown-verification block and again in the weak-section block below, which
re-resolves the exact same `(unit, line, column)` triple it has already
resolved. That block is skipped entirely when the weak list is empty, which is
the default (`src/config.rs:165-170`), and `.all()` short-circuits on the first
non-weak locator, so the doubling is a configured-corpus cost rather than a
universal one. Across such a corpus that is up to N × K × L × 2 × U unit
comparisons where a hash lookup would make it N × K × L × 2. A converted paper
commonly yields several hundred units, so a 40-claim summary with 3 locators
each spends tens of thousands of comparisons on work that is O(1) per lookup
with the right structure — and on a weak-section corpus half of it is a straight
repeat. This is invisible to clippy: the code is idiomatic iterator style, the
cost is purely structural.

```rust src/markdown.rs:172-181
let mut matches = units
    .iter()
    .filter(|unit| unit.kind == kind && unit.line == line && unit.column == column);
let Some(found) = matches.next() else {
    bail!("no {kind:?} unit starts at line {line}, column {column}");
};
if matches.next().is_some() {
    bail!("multiple {kind:?} units start at line {line}, column {column}");
}
Ok(found)
```

Related [Weak-section matching lowercases the configured list again for every heading comparison](#weak-section-list-lowercased-per-comparison).

**Remediation:** Build the coordinate index once where the units are parsed: a
`HashMap<(UnitKind, usize, usize), …>` (or a `BTreeMap` if determinism of the
error text matters) populated during `parse_units`, recording the ambiguity flag
at insert time so `resolve_unit` keeps its exact "no unit"/"multiple units"
error semantics with an O(1) probe. Store that index next to the `Vec` in
`source_units`. Separately, have the locator loop keep the `&MarkdownUnit` it
already resolved (for example in a small per-claim `Vec<Option<&MarkdownUnit>>`)
and let the weak-section check consume it instead of resolving again. `UnitKind`
(src/markdown.rs:8) derives neither `Hash` nor `Ord`, so add `Hash` to its
derive list for the `HashMap` form (or `PartialOrd, Ord` for the `BTreeMap`
form) — both are safe on a fieldless enum and neither changes the serialized
representation.

*Effort (small):* The index is built in the function that already owns the units; `resolve_unit` keeps its signature by taking the index alongside the slice, or by moving both into a small struct. Reusing the resolved unit in the weak-section block is a local change inside `validate_document`.

<div>&hairsp;</div>

### The `max_candidates` bound on PDF verification only counts passes, so failing candidates still sweep every page {#pdf-verification-unbounded-when-candidates-fail}

**moderate** · `src/propose.rs:171-179` · effort: small · <img src="assets/sparkline-pdf-verification-unbounded-when-candidates-fail.svg" height="14" alt="commit activity" />

This is the residual of the fix recorded as
`proposal-ranking-verifies-unbounded-candidate-set`, not a regression: the
sort-then-verify restructuring did land and is correct, and the loop does stop
early once `max_candidates` candidates have been *accepted*. But `continue` on a
failed verification does not advance `candidates.len()`, so the bound never
fires for a claim whose spans do not survive PDF verification — exactly the case
that motivates `propose` in the first place, where the Markdown conversion and
the native PDF text disagree. Each attempt runs `verify_pdf`, which does
`exact_count` over each page's normalized text until it finds a second match or
exhausts the map, so a rejected candidate costs a scan of all P pages. With R
raw candidates for a claim and K claims, the worst case is O(K × R × P × |page
text|) substring scans; the actions-taken note's "verifies ~3-10 against the PDF
instead of all 200" holds only when at least `max_candidates` candidates pass.
Combined with the per-claim regeneration of `raw_candidates`, the R factor is
itself re-derived K times.

```rust src/propose.rs:171-179
let mut candidates = Vec::new();
for raw in raw_candidates {
    if candidates.len() >= max_candidates {
        break;
    }
    let pages = source_pages.get(&raw.source);
    let Some(page) = pages.and_then(|pages| verify_pdf(pages, &raw.exact)) else {
        continue;
    };
```

Enabled by [Candidate span generation is redone for every claim although it depends only on the Markdown](#propose-regenerates-claim-independent-spans-per-claim).

**Remediation:** Bound the attempts as well as the acceptances: track a verification-attempt
counter alongside `candidates.len()` and stop at a multiple of `max_candidates`
(the candidates are already in rank order, so anything reached after that budget
is low-value by construction), or stop after a run of consecutive failures.
Cheaper still, memoize verification by `exact` text within the summary — the
same span is frequently generated for several claims — so a span that failed
verification for one claim is not re-swept for the next.

*Effort (small):* A counter plus a budget constant, or a memo keyed on the span text. Output ordering is unaffected as long as the budget is generous enough not to change which candidates are accepted in the passing case.

<div>&hairsp;</div>

### `sort_keys` borrows an already-owned `Value` and rebuilds it by cloning every node {#owned-json-value-borrowed-then-deep-cloned}

**moderate** · `src/review.rs:46-56` · effort: trivial · <img src="assets/sparkline-owned-json-value-borrowed-then-deep-cloned.svg" height="14" alt="commit activity" />

`serde_json::to_value` produces an owned `Value` that `canonical_locators_json`
never uses again after the `sort_keys` call. `sort_keys` nevertheless takes
`&Value` and reconstructs the entire tree by cloning:
`sorted.insert(key.clone(), …)` for every key at every depth,
`arr.iter().map(sort_keys)` for every array element, and `other =>
other.clone()` for every leaf string and number. For each locator that means
duplicating the `exact` text, the section path `Vec<String>`, and every scalar —
all to reorder keys that could have been moved. `evidence_sha256` runs this once
per reviewed claim over that claim's full locator set, so the cost scales with
corpus size on the `check --require-review` path. This is the textbook shape of
criterion 3.1: the borrow buys nothing, because the caller has no use for the
original. The open question about map ordering resolves against the function:
`Cargo.lock` carries `serde_json` 1.0.151 with no `indexmap` dependency, so
`preserve_order` is off and `serde_json::Map` is `BTreeMap`-backed. The keys are
already sorted before `sort_keys` runs, which means the function performs no
reordering whatever — its entire observable effect is the deep clone.

```rust src/review.rs:46-56
fn canonical_locators_json(locators: &[crate::evidence::Locator]) -> String {
    let values: Vec<serde_json::Value> = locators
        .iter()
        .map(|loc| {
            let mut locator = serde_json::to_value(loc).expect("Locator is serializable");
            normalize_exact_in_value(&mut locator);
            sort_keys(&locator)
        })
        .collect();
    serde_json::to_string(&values).expect("JSON array is serializable")
}
```

**Remediation:** Change `sort_keys` to take `Value` by value and move its contents —
`Value::Object(map) => map.into_iter()` collected into a `serde_json::Map`
(which is `BTreeMap`-backed under the `preserve_order` default being off, so the
sort may become free), `Value::Array(arr) => arr.into_iter().map(sort_keys)`,
and `other => other` for leaves. The call site becomes `sort_keys(locator)` with
no other change. The map type is already key-ordered in this build (no
`indexmap` in `Cargo.lock`), so the object arm exists only to survive a future
`preserve_order` feature unification. Keep it — as a by-value move it costs
nothing — rather than deleting the function and depending on a feature flag
another crate could flip.

*Effort (trivial):* One signature change and three match arms; the call site is unchanged apart from dropping an `&`.

<div>&hairsp;</div>

### `source_names` clones every key so callers can run a linear scan over a `BTreeMap` {#source-names-allocates-a-vec-to-answer-a-lookup}

**advisory** · `src/corpus.rs:127-131` · effort: trivial · <img src="assets/sparkline-source-names-allocates-a-vec-to-answer-a-lookup.svg" height="14" alt="commit activity" />

Both callers want a question answered, not a collection. `validate.rs:61` does
`configured.iter().any(|name| name == source_name)` — an O(n) scan over a
freshly heap-allocated `Vec<String>` to replace a `BTreeMap::contains_key` the
underlying data already supports. `propose.rs:136` iterates the result and
immediately reborrows each element as `&source_name`. Neither stores an owned
`String`. The API shape forces an allocation per `validate_document` call and
per `propose_document` call, and it hands callers a snapshot that has silently
lost its ordering guarantee and its lookup complexity. This is criterion 3.3
inverted: the function returns ownership the callee never needed to give up.

```rust src/corpus.rs:127-131
/// Source names this corpus declares templates for.
#[must_use]
pub fn source_names(&self) -> Vec<String> {
    self.layout.sources.keys().cloned().collect()
}
```

Enabled by [Every internal module is exported as a stable-looking public API on a publishable crate](#entire-crate-is-a-published-public-api).

**Remediation:** Return `impl Iterator<Item = &str> + '_` for the iteration case and add a
`declares_source(&self, name: &str) -> bool` that delegates to
`self.layout.sources.contains_key(name)` for the membership case.
`validate.rs:61` becomes `if !corpus.declares_source(source_name)`, and
`propose.rs:136` iterates borrowed names. The two `tests/foundation.rs`
assertions (:176 and :193) collect explicitly instead.

*Effort (trivial):* Two small methods on `Corpus`, two call-site edits, and two tests that need an explicit `collect` (tests/foundation.rs:176 and tests/foundation.rs:193).

<div>&hairsp;</div>

### Weak-section matching lowercases the configured list again for every heading comparison {#weak-section-list-lowercased-per-comparison}

**advisory** · `src/sections.rs:14-24` · effort: trivial · <img src="assets/sparkline-weak-section-list-lowercased-per-comparison.svg" height="14" alt="commit activity" />

`weak_list` comes from `SectionsConfig::weak` and is fixed for the entire
process, yet `weak.to_lowercase()` allocates a fresh `String` in the innermost
loop — once per (heading, weak-entry) pair. `is_weak_section` is called from the
`all_weak` predicate for every locator of every claim, and the heading path of a
unit is re-lowercased on each of those calls too. For N summaries × K claims × L
locators × D headings in the section path × W configured weak entries, that is
N·K·L·D·W transient heap allocations for data that could be lowercased once at
configuration load. The per-allocation constant is small, so this is an advisory
rather than a hot-loop emergency — but it sits inside the same per-locator nest
as the duplicated `resolve_unit` call, so both are paid together.

```rust src/sections.rs:14-24
pub fn is_weak_section(section: &[String], weak_list: &[String]) -> bool {
    if weak_list.is_empty() {
        return false;
    }
    section.iter().any(|heading| {
        let lower = heading.to_lowercase();
        weak_list
            .iter()
            .any(|weak| lower.contains(&weak.to_lowercase()))
    })
}
```

Related [`resolve_unit` linearly scans every Markdown unit, and validation calls it twice for the same locator](#resolve-unit-linear-scan-called-twice-per-locator).

**Remediation:** Lowercase the weak list once — either at config deserialization, or by having
`Corpus` expose a pre-lowered `Vec<String>` derived from
`sections_config().weak` — and pass that to `is_weak_section`. Inside the
function, lowercase each heading once per call (as it already does) and compare
against the pre-lowered entries. An `eq_ignore_ascii_case`-style substring
search would remove the heading allocation too, but the entries are
user-supplied headings that may be non-ASCII, so keeping `to_lowercase` on the
heading and hoisting only the list is the correct-and-cheap version.

*Effort (trivial):* Hoist one `to_lowercase` out of a closure; the call site owns the list.

<div>&hairsp;</div>

### No `[profile.release]`: the in-process scanning phase ships at codegen-units 16 with LTO off {#release-profile-left-at-cargo-defaults}

**note** · `Cargo.toml:22-33` · effort: trivial · <img src="assets/sparkline-release-profile-left-at-cargo-defaults.svg" height="14" alt="commit activity" />

The manifest declares no release profile, so the binary ships with `opt-level =
3`, `lto = false`, `codegen-units = 16`, `panic = "unwind"`. For the
subprocess-dominated regime (OCR, `mutool`) this is genuinely irrelevant —
profile flags cannot make `tesseract` faster, and it would be wrong to file this
as a real cost there. It is not irrelevant for the in-process regime, which is
not small: `normalize` over every extracted page, `exact_count`/`match_indices`
sweeps over page-sized and unit-sized strings, `is_covered_case_insensitive`'s
byte loop, `parse_units`, and serde_json decoding of full-document `stext.json`.
Those are exactly the small hot leaf functions that cross module boundaries and
that `codegen-units = 16` prevents the optimizer from inlining across. This is a
reasoned trade-off rather than a defect, which is why it is filed at `note`: the
honest expected win is single-digit percent on the scanning phase, bought with
longer release builds.

```toml Cargo.toml:22-33
[dev-dependencies]
tempfile = "3.27"

[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
missing_errors_doc = "allow"
missing_panics_doc = "allow"
too_many_lines = "allow"
```

**Remediation:** Add `[profile.release]` with `lto = "thin"` and `codegen-units = 1`; both are
one-line, reversible, and target precisely the cross-module inlining the current
defaults block. Measure before keeping it — this surface's claim is a mechanism,
not a measurement, and a corpus run is the only honest benchmark. Do not reach
for `panic = "abort"` as part of the same change: `librebar` is pulled in with
its `crash` feature, which implies a panic/crash handler whose behaviour under
an aborting profile must be verified first, and the binary-size win does not
justify changing failure semantics for a validation tool.

*Effort (trivial):* Two lines in Cargo.toml; the cost is build time and a benchmark to confirm the win is real.

<div>&hairsp;</div>

*Verdict: The expensive thing in this tool is process spawning, and the two significant findings here are both about doing it or its equivalent more often than the work requires. Native validation spawns one mutool per cited page rather than extracting the document once, so cost scales with citations instead of documents. Candidate span generation in propose depends only on the markdown but is regenerated for every claim, making the work quadratic in claims per summary. The remaining items are real but small, and the release profile note is genuinely a note — the dominant cost is subprocess time, which no codegen setting touches. Verify the two significant items with a measurement before restructuring; the complexity argument is sound but the constant factors decide whether it matters at real corpus sizes.*

<div>&nbsp;</div>

---

## The Verification Gate Surface

*The repository documents a complete verification gate, but the gate reformats instead of verifying, skips the MSRV it declares, asserts almost none of the JSON contracts the tool publishes, and nothing runs it automatically.*

### Every JSON output shape except single-ID propose is asserted nowhere {#structured-output-contracts-untested}

**significant** · `src/cli.rs:632-642` · effort: small · <img src="assets/sparkline-structured-output-contracts-untested.svg" height="14" alt="commit activity" />

`tests/cli.rs` runs `receipts` with `--format json` exactly once, in
`propose_emits_candidates_for_claims_without_evidence` (line 381), and every
other CLI test passes `--format text`. The consequence is that three output
contracts changed in today's remediation commits carry no regression test. (1)
Doctor JSON was restructured from positional tuples to the seven named fields
above; the only doctor assertions (tests/cli.rs:109-113) read the human labels
`native profile:` and `OCR profile:`, which differ from the JSON keys
`native_profile` and `ocr_profile`, so renaming a JSON key breaks no test. (2)
The multi-ID `{"summaries": [...]}` wrapper added at src/cli.rs:1010-1016 is
never exercised — no test invokes `propose` with two IDs. (3) The
`advisory_tokens` field added to `ClaimProposal` (src/propose.rs:34) does not
appear anywhere under `tests/`. Beyond the three contracts changed today, the
`check`, `audit`, and `locate` JSON shapes are equally unasserted; of the six
`print_json` shapes the binary emits, exactly one is read by a test. All of them
exist to satisfy a declared machine contract, and a machine contract that no
test reads is a contract that will regress without anyone noticing.

```rust src/cli.rs:632-642
if json {
    let report = serde_json::json!({
        "corpus": corpus_value,
        "config": config_value,
        "mutool": mutool_value,
        "tesseract": tesseract_value,
        "native_profile": native_profile_value,
        "ocr_profile": ocr_profile_value,
        "cache": cache_value,
    });
    print_json(&report)?;
```

Related [The declared error-code registry and the emitted codes are kept in sync by hand](#error-code-registry-is-hand-maintained). Related [Recursive summary discovery has no test at any nesting depth](#summary-discovery-recursion-untested).

**Remediation:** Add three assertions to `tests/cli.rs`, all reusing the existing `receipts_json`
helper: parse `doctor --format json` and assert the seven key names are present;
run `propose` with two summary IDs and assert the payload is an object with a
`summaries` array of length two; extend the existing propose JSON test's fixture
claim with a two-word proper noun and assert `claims[0].advisory_tokens` is
non-empty.

*Effort (small):* Three tests against helpers that already exist in tests/cli.rs.

<div>&hairsp;</div>

### just check reformats instead of verifying, and MSRV is declared but never enforced {#just-check-is-not-a-complete-gate}

**moderate** · `justfile:1-10` · effort: trivial · <img src="assets/sparkline-just-check-is-not-a-complete-gate.svg" height="14" alt="commit activity" />

Three gaps in the sanctioned gate. (1) `msrv := "1.89.0"` is referenced by no
recipe; it duplicates `rust-version = "1.89"` in Cargo.toml and nothing ever
builds against it. Every recipe pins `+{{toolchain}}`, which resolves to 1.97.1
from rust-toolchain.toml, so a contributor using an API stabilized after 1.89
passes `just check` and breaks the declared minimum silently. (2) `check: fmt
clippy deny test doc-test doc` (justfile:40) opens with `fmt`, which *applies*
formatting (`cargo fmt --all --`, justfile:25) rather than verifying it.
Unformatted code is silently rewritten and the gate passes; `just ci`, which is
just `check` (justfile:42), therefore can never fail on formatting. (3)
`test-ci` (justfile:31-32) is the only recipe using the `ci` nextest profile
that `.config/nextest.toml` exists to define — including its `junit.xml` output
and `fail-fast = false` — and no recipe invokes it, so `just ci` runs the
default profile and the CI profile is dead configuration.

```make justfile:1-10
set shell := ["bash", "-c"]
set dotenv-load := true
toolchain := `taplo get -f rust-toolchain.toml toolchain.channel | tr -d '"'`
msrv := "1.89.0"

default:
  @just --list

clippy:
  cargo +{{toolchain}} clippy --all-targets --all-features --message-format=short -- -D warnings
```

Related [Every JSON output shape except single-ID propose is asserted nowhere](#structured-output-contracts-untested).

**Remediation:** Add `msrv-check: cargo +{{msrv}} check --all-targets --all-features` and include
it in `check`, which also gives the `msrv` variable a reader. Add `fmt-check:
cargo fmt --all --check -- --config-path .config/rustfmt.toml` and use that in
`check`, leaving `fmt` as the developer-facing mutating recipe. Change `ci:` to
`fmt-check clippy deny test-ci doc-test doc` so the profile in
`.config/nextest.toml` is actually used.

*Effort (trivial):* Three recipe lines; the MSRV recipe needs the 1.89 toolchain installed.

<div>&hairsp;</div>

### The repository documents a full verification gate but has no CI configuration to run it {#no-automated-ci-for-the-documented-gate}

**moderate** · `justfile:40-42` · effort: small · <img src="assets/sparkline-no-automated-ci-for-the-documented-gate.svg" height="14" alt="commit activity" />

The project has every ingredient of a 2026-era Rust verification gate — a pinned
toolchain in `rust-toolchain.toml`, `cargo-deny` with a checked-in
`.config/deny.toml`, a `nextest` CI profile in `.config/nextest.toml`, a
`test-ci` recipe wired to it, and a `ci: check` alias added by the prior audit
specifically to serve as the machine entry point. Nothing runs them: there is no
`.github/` directory (and `.gitignore` does not exclude one), no other pipeline
configuration anywhere in the tree, and no pre-commit or pre-push hook. Every
gate README advertises is manual. A contributor — or an agent making changes on
this repo's behalf — gets no signal on a pushed branch, and the `test-ci` recipe
and nextest CI profile are dead configuration written for a runner that does not
exist. The absence is conspicuous against the rest of the repo's rigor rather
than against a generic checklist: the entry point was built for CI and left
unconnected.

```make justfile:40-42
check: fmt clippy deny test doc-test doc

ci: check
```

**Remediation:** Add `.github/workflows/ci.yml` running on push and pull_request: check out,
install the pinned toolchain from `rust-toolchain.toml` with the `rustfmt` and
`clippy` components, install `just`, `cargo-nextest`, and `cargo-deny`, cache
`~/.cargo` and `target/`, then run formatting verification, `just clippy`, `just
deny`, `just test-ci`, `just doc-test`, and `just doc`. Note that `just check`
runs `fmt` in write mode, so CI should call the individual recipes (or gain a
`check-ci` recipe using `cargo fmt --check`) rather than `just ci` directly. The
`doctor` recipe should stay out of CI unless `mupdf` and `tesseract` are
installed on the runner.

*Effort (small):* One workflow file; all recipes and tool configuration already exist. A `check-ci` recipe using `cargo fmt --check` may be needed so CI does not mutate the tree.

<div>&hairsp;</div>

### The justfile loads a repo-supplied .env into every recipe, including cargo builds {#justfile-loads-repo-supplied-dotenv}

**moderate** · `justfile:1-4` · effort: trivial · <img src="assets/sparkline-justfile-loads-repo-supplied-dotenv.svg" height="14" alt="commit activity" />

`set dotenv-load := true` makes `just` read `.env` from the working directory
and export every entry into the environment of every recipe. `.env` is absent
from `.gitignore` (which lists only `/target`, `/.cache`, `/dist`, `commit.txt`,
`scratch/`, `.DS_Store`, `.crustoleum`), so a `.env` committed to a fork or a PR
branch is present the moment the branch is checked out. The recipes it feeds are
`cargo` invocations, and cargo reads execution-controlling variables straight
from the environment: `RUSTC`, `CARGO_BUILD_RUSTC_WRAPPER`, and
`CARGO_TARGET_<TRIPLE>_RUNNER` each name a binary cargo then executes. A `.env`
containing `CARGO_BUILD_RUSTC_WRAPPER=./pwn.sh` turns `just check`, `just test`,
or `just clippy` into arbitrary code execution on the reviewer's machine — the
sanctioned entry points a maintainer runs first when evaluating a contribution.
No recipe in the file reads an environment variable, so the setting buys nothing
and is pure exposure. It arrived on 2026-08-10 in "chore: update justfile and
config to match fleet baseline," which suggests the same line is present across
the fleet's other justfiles.

```makefile justfile:1-4
set shell := ["bash", "-c"]
set dotenv-load := true
toolchain := `taplo get -f rust-toolchain.toml toolchain.channel | tr -d '"'`
msrv := "1.89.0"
```

> `set dotenv-load := true`. I commit a `.env`, and it reaches every recipe in the
> file — including the ones that shell out to `cargo`. The person who runs
> `just check` on my branch is reading the justfile for what it runs, not for what
> it silently sourced first.

**Remediation:** Delete line 2. No recipe references an environment variable, so removal is
behaviour-neutral. If a future recipe needs configuration, prefer `just`
variables passed on the command line (`just foo bar=baz`) or an explicitly named
file loaded inside the one recipe that needs it. Add `.env` to `.gitignore`
regardless, so a stray local file is never committed. Since the line came from a
shared fleet baseline, fix it at the baseline rather than only here.

*Effort (trivial):* Delete one line and add one .gitignore entry.

<div>&hairsp;</div>

### deny.toml allows licenses for crates that are not in this tree, with the warning suppressed {#license-allowlist-inherited-from-absent-dependency-tree}

**moderate** · `.config/deny.toml:28-42` · effort: trivial · <img src="assets/sparkline-license-allowlist-inherited-from-absent-dependency-tree.svg" height="14" alt="commit activity" />

This policy file was copied from another project's baseline — its header still
names a different crate, and it landed here in a single commit on 2026-08-10 as
a fleet-config sync rather than as a decision about this crate's dependency
tree. The residue is not cosmetic. `OpenSSL` and `CDLA-Permissive-2.0` are
allowed with inline comments justifying them by `aws-lc-sys`, `reqwest`, and
`webpki-roots`, and none of those crates — nor `openssl`, `rustls`, or `hyper` —
appear anywhere in Cargo.lock's 122 entries. `receipts` makes no network calls
and links no TLS stack, so the OpenSSL allowance in particular grants a
non-permissive, historically contentious license a standing pass for a use case
this crate does not have. The normal safety net for exactly this — cargo-deny's
unused-allowed-license warning — is explicitly turned off at line 42, so the
allowlist can drift arbitrarily far from the real tree without ever producing a
signal. `just deny` currently passes with a single warning (a transitive `syn`
2/3 straddle), which correctly reports the current tree as compliant, but it
says nothing about how much wider the policy is than the tree it governs.

```toml .config/deny.toml:28-42
   # Unicode/text processing
    "Unicode-3.0",

    # Crypto libraries
    "OpenSSL",      # Used by aws-lc-sys (via reqwest)

    # Data licenses (for embedded certificate bundles)
    "CDLA-Permissive-2.0",  # webpki-roots (Mozilla root certificates)
]

# How to handle license detection confidence
confidence-threshold = 0.8

# Don't warn about licenses in allow list that aren't currently used
unused-allowed-license = "allow"
```

Related [The single cargo-deny warning is a transitive syn 2/3 straddle with no local fix](#transitive-syn-duplicate-is-not-actionable).

**Remediation:** Remove the `OpenSSL` and `CDLA-Permissive-2.0` entries and their stale
justifications, and flip `unused-allowed-license` to `"warn"` so the allowlist
is pressured back toward the licenses actually present — the same discipline the
file's own `skip` list comment already argues for. Re-run `just deny` and re-add
only what it demands. Rewrite the file header so it names this crate.

*Effort (trivial):* Delete two allowlist entries, change one setting, correct the header comment, then confirm `just deny` still reports licenses ok.

<div>&hairsp;</div>

### Recursive summary discovery has no test at any nesting depth {#summary-discovery-recursion-untested}

**moderate** · `src/cli.rs:1157-1176` · effort: small · <img src="assets/sparkline-summary-discovery-recursion-untested.svg" height="14" alt="commit activity" />

`walk_summaries` was rewritten today to replace a flat `read_dir` with recursive
traversal plus prefix/suffix `{id}` extraction, specifically so nested templates
like `records/{id}/summary.yaml` are discoverable. No test reaches this code
with a nested template. `tests/foundation.rs:85` declares exactly that template
but only asserts `summary_path` and `summaries_dir` — it never lists IDs. Every
CLI test that triggers discovery (`audit` and `check` with no IDs) uses the
default flat `summaries/{id}.yaml`, which the previous flat implementation also
handled. So the entire behavior the rewrite was written to add is unverified, as
are its two guards: `!id.contains('/')`, which is what stops a deeper stray file
from being reported as an ID, and the recursion's handling of a directory tree
that contains non-summary files.

```rust src/cli.rs:1157-1176
for entry in entries {
    let path = entry?.path();
    if path.is_dir() {
        walk_summaries(&path, prefix, suffix, root, ids)?;
    } else {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(id) = relative
            .strip_prefix(prefix)
            .and_then(|r| r.strip_suffix(suffix))
            && !id.is_empty()
            && !id.contains('/')
        {
            ids.push(id.to_owned());
        }
    }
}
```

Related [Every JSON output shape except single-ID propose is asserted nowhere](#structured-output-contracts-untested).

**Remediation:** Add a test to `tests/cli.rs` that builds a corpus with `corpus.summaries:
"records/{id}/summary.yaml"`, writes `records/doc-001/summary.yaml` and a decoy
`records/doc-001/notes.md`, runs `receipts audit` with no IDs, and asserts the
output accounts for exactly one summary. Add a second case with a summary two
levels down that must not be discovered, pinning the `!id.contains('/')` guard.

*Effort (small):* One fixture corpus and two assertions, following the shape of the existing audit tests.

<div>&hairsp;</div>

### No test pins a Markdown coordinate other than (1, 1) {#markdown-coordinates-only-pinned-at-line-one}

**moderate** · `tests/markdown.rs:20-28` · effort: trivial · <img src="assets/sparkline-markdown-coordinates-only-pinned-at-line-one.svg" height="14" alt="commit activity" />

Coordinate resolution was rewritten today from repeated prefix rescans to a
`build_line_starts` index plus `partition_point` lookup
(src/markdown.rs:237-254). Line 27 above is the only place in the suite that
asserts a literal `(line, column)` pair, and it asserts the origin, where the
index math is trivially correct. Every other coordinate assertion is
self-referential: `resolves_units_by_exact_coordinates_and_kind`
(tests/markdown.rs:64-75) feeds `second.line` and `second.column` straight back
into `resolve_unit`, and the `Fixture` helper in tests/validate.rs:839-840 does
the same, so a systematic off-by-one in `line_column_from_index` would
round-trip cleanly and the suite would stay green. The one place non-origin
coordinates appear is tests/validate.rs:606-607, which supplies literal lines 3
and 7 via `weak_locator` — and it is not a regression gate: the test asserts
only the absence of `weak_section_only`, so a coordinate off-by-one would make
`resolve_unit` fail, `is_ok_and` return false, `all_weak` become false, and the
assertion still pass while unrelated `markdown_unit_missing` errors go
unchecked. Columns are never exercised above 1 at all, despite nested list items
and blockquotes producing units whose start offset sits past the line start.

```rust tests/markdown.rs:20-28
#[test]
fn preserves_inline_text_and_collapses_breaks() {
    let units = parse_units("A **strong** [linked](https://example.test)\nline.");

    assert_eq!(units.len(), 1);
    assert_eq!(units[0].kind, UnitKind::Paragraph);
    assert_eq!(units[0].text, "A strong linked line.");
    assert_eq!((units[0].line, units[0].column), (1, 1));
}
```

**Remediation:** Add one test asserting literal coordinates for a document with several units at
known offsets — e.g. `parse_units("first\n\nsecond\n")` must report the second
paragraph at `(3, 1)` — and one asserting a column above 1. A top-level list
item will not do it: its `-` marker sits at the line start, so `"# H\n\n- an
item\n"` yields `(3, 1)` as well. Use a unit whose start offset is genuinely
past its line start — a nested list item, a paragraph inside a blockquote, or a
GFM table cell — and pin the literal pair the parser reports for it. Both are
single `assert_eq!` lines against values a reader can count off the fixture.

*Effort (trivial):* Two assertions against existing fixtures.

<div>&hairsp;</div>

### The single cargo-deny warning is a transitive syn 2/3 straddle with no local fix {#transitive-syn-duplicate-is-not-actionable}

**note** · `.config/deny.toml:61-79` · effort: trivial · <img src="assets/sparkline-transitive-syn-duplicate-is-not-actionable.svg" height="14" alt="commit activity" />

Recorded so the reader is not left wondering about it: `just deny` exits 0 and
reports advisories, bans, licenses, and sources all ok, with exactly one warning
— duplicate `syn` 2.0.119 and 3.0.3. Both arrivals are transitive and neither is
reachable from this crate's own declarations. `syn` 2 enters solely through
`librebar → tracing → tracing-attributes`; `syn` 3 enters through `clap_derive`,
`serde_derive`, and `thiserror-impl`. Nothing in `receipts` can collapse them;
the straddle resolves upstream when `tracing-attributes` moves to `syn` 3. The
cost is proc-macro compile time only — neither copy reaches the binary. I also
verified the one `skip` entry is not stale: `cargo tree -d` confirms
`owo-colors` v4.3.0 still pulls both `supports-color` v2.1.0 and v3.0.2, so the
file's stated keep-this-list-honest discipline is being met.

```toml .config/deny.toml:61-79
[bans]
# Warn about multiple versions instead of denying for development flexibility
multiple-versions = "warn"
wildcards = "allow"
highlight = "all"

# We don't ban any specific crates
deny = []

# Skip duplicate version checks for transitive dependencies we can't control.
#
# Keep this list honest. cargo-deny warns about entries it did not need, so a
# stale skip is not free — it is noise on every run, and it hides the day a
# duplicate you do care about appears. Most of the original list existed for
# candle and tokenizers, which left the tree when token counting moved to
# ah-ah-ah, so seventeen entries went with them.
skip = [
    { crate = "supports-color", reason = "owo-colors pulls both v2 and v3" },
]
```

Related [deny.toml allows licenses for crates that are not in this tree, with the warning suppressed](#license-allowlist-inherited-from-absent-dependency-tree).

**Remediation:** No action. Do not add a `syn` skip entry — that would suppress the signal on a
duplicate the project genuinely wants to see disappear, and cargo-deny would
then warn about the unused skip once upstream converges. Re-check after the next
`librebar` bump.

*Effort (trivial):* Informational; the recommended action is to leave it alone.

<div>&hairsp;</div>

*Verdict: This is the surface that determines whether the other seven stay fixed. just check runs fmt in write mode, so a contributor with unformatted code gets it silently rewritten rather than rejected; the msrv variable is declared in the justfile and consumed by no recipe; and there is no CI configuration at all, so the documented gate runs only when someone remembers. On the test side, the structured output shapes — the tool's actual contract with downstream consumers — are asserted for exactly one case, and markdown coordinates are only ever pinned at (1, 1), which is the one position where a line/column bug is invisible. Two of the fixes from the prior audit landed in exactly these untested areas. The dotenv load in the justfile belongs here too: set dotenv-load means a repo-supplied .env reaches every recipe including cargo builds.*

<div>&nbsp;</div>

---

## The Structure Surface

*The module boundaries are sound, but four constructs are duplicated across them and one function has absorbed seven distinct concerns.*

### validate_document is a 326-line function nested six levels deep {#validate-document-carries-seven-concerns}

**moderate** · `src/validate.rs:161-176` · effort: medium · <img src="assets/sparkline-validate-document-carries-seven-concerns.svg" height="14" alt="commit activity" />

`validate_document` runs from src/validate.rs:41 to 366 and performs nine
separable jobs against three interleaved `BTreeMap` accumulators keyed by source
name: source-template resolution, recorded-path comparison, containment
checking, file hashing, Markdown unit resolution, PDF page extraction with
memoization, required-token coverage, weak-section warning, and review dispatch.
The excerpt shows the depth — `for` inside `for` inside `if let` inside `match`
inside `match` inside `if` — reached before any of the four issue-pushing arms.
`too_many_lines` is set to `allow` in Cargo.toml, so clippy is explicitly
silenced on exactly this. The practical cost is that the three maps
(`source_units`, `source_pdf_path`, `source_pdf_sha256`) are populated in one
loop and read in another 100 lines away, with `continue` statements in the first
loop deciding which keys the second loop will silently skip via `let Some(...)
else { continue }` at src/validate.rs:233-235 — a cross-loop invariant a reader
has to reconstruct by hand before they can change anything safely.

```rust src/validate.rs:161-176
for entry in &evidence.claims {
    for (locator_index, locator) in entry.locators.iter().enumerate() {
        let source_name = locator.source.as_str();

        if let Some(units) = source_units.get(source_name) {
            match resolve_unit(
                units,
                locator.markdown.unit,
                locator.markdown.line,
                locator.markdown.column,
            ) {
                Ok(unit) => match exact_count(&unit.text, &locator.exact) {
                    1 => {
                        if !locator.markdown.section.is_empty()
                            && !sections::paths_match(&locator.markdown.section, &unit.section)
                        {
```

**Remediation:** Split along the seams already present. Extract the first loop into a
`resolve_sources(corpus, evidence, summary, &mut issues) -> BTreeMap<&str,
ResolvedSource>` returning one struct per source (units, pdf path, pdf hash)
instead of three parallel maps — that alone removes the cross-loop invariant.
Then extract `validate_locator_against_sources`, `check_token_coverage`, and
`check_weak_sections` as functions taking the resolved map and returning
`Vec<EvidenceIssue>`.

*Effort (medium):* Pure extraction with a strong test suite behind it, but the three-map-to-one-struct change touches every read site.

<div>&hairsp;</div>

### check and audit reimplement the same summary loop with different report types {#check-and-audit-duplicate-the-summary-loop}

**moderate** · `src/cli.rs:909-926` · effort: medium · <img src="assets/sparkline-check-and-audit-duplicate-the-summary-loop.svg" height="14" alt="commit activity" />

`check` (src/cli.rs:823-883) and `audit` (src/cli.rs:888-983) walk the same ID
list, resolve summaries the same way, and share four decision points in the same
order: parse failure, ID/filename mismatch, absent evidence, then
`validate_document`. The parse-failure and mismatch branches carry identical
codes and identical message text (`"summary ID {:?} does not match filename"`
appears at src/cli.rs:859 and src/cli.rs:935), but one builds `ValidationReport`
through `error_report` and the other builds `AuditSummary` through an inline
struct literal. The two also print through near-identical functions,
`print_issues` (src/cli.rs:1209-1219) and `print_audit_issues`
(src/cli.rs:1221-1231), whose bodies differ only in which struct field the `id`
comes from. A change to how a summary is resolved or classified has to land
twice, and a message reworded in one place makes the two commands disagree about
the same condition.

```rust src/cli.rs:909-926
let summary = match read_summary(corpus, &id) {
    Ok(summary) => summary,
    Err(error) => {
        report.invalid += 1;
        report.summaries.push(AuditSummary {
            id,
            status: AuditStatus::Invalid,
            issues: vec![crate::evidence::EvidenceIssue {
                code: "summary_parse_failed".to_owned(),
                severity: Severity::Error,
                message: error.to_string(),
                claim: None,
                locator: None,
            }],
        });
        continue;
    }
};
```

Related [Three copies of the EvidenceIssue constructor plus two inline literals](#duplicated-issue-constructors).

**Remediation:** Extract the shared step as a function returning a small `enum SummaryOutcome {
ParseFailed(EvidenceIssue), IdMismatch(EvidenceIssue), NoEvidence,
Validated(ValidationReport) }`, and let `check` and `audit` each map that
outcome onto their own counters and report type. Collapse `print_issues` and
`print_audit_issues` into one function taking `(&str, &[EvidenceIssue])` per
summary.

*Effort (medium):* Touches both command bodies; the outcome enum needs care so counters stay correct.

<div>&hairsp;</div>

### Three copies of the EvidenceIssue constructor plus two inline literals {#duplicated-issue-constructors}

**moderate** · `src/validate.rs:486-500` · effort: small · <img src="assets/sparkline-duplicated-issue-constructors.svg" height="14" alt="commit activity" />

This function is byte-identical to `issue` in src/evidence.rs:412-426. A third
variant in src/review.rs:192-205 drops the `locator` parameter and hardcodes
`locator: None`. Two further construction sites build `EvidenceIssue` as a
struct literal inline (src/cli.rs:916-922 and src/cli.rs:932-938), and a sixth
wraps it as `error_report` (src/cli.rs:1233-1248). `EvidenceIssue` is the single
serialized type every consumer of `check` and `audit` reads, so a field added to
it — a source name, a suggested fix, a stable code namespace — has to be
threaded through six places, three of which are copies of one another. The
review variant's divergence is already the shape of the problem: a reviewer
reading `review.rs` cannot tell whether `locator` is unavailable there or merely
unimplemented.

```rust src/validate.rs:486-500
fn issue(
    code: impl Into<String>,
    severity: Severity,
    message: impl Into<String>,
    claim: Option<usize>,
    locator: Option<usize>,
) -> EvidenceIssue {
    EvidenceIssue {
        code: code.into(),
        severity,
        message: message.into(),
        claim,
        locator,
    }
}
```

Related [The declared error-code registry and the emitted codes are kept in sync by hand](#error-code-registry-is-hand-maintained). Related [check and audit reimplement the same summary loop with different report types](#check-and-audit-duplicate-the-summary-loop).

**Remediation:** Move one constructor onto the type as `EvidenceIssue::new(code, severity,
message)` with `.claim(usize)` and `.locator(usize)` builders, or keep the free
function but define it once in `evidence.rs` and make it `pub(crate)`. Delete
the copies in `validate.rs` and `review.rs` and replace the two inline literals
in `cli.rs` with calls to it.

*Effort (small):* One definition, five call-site rewrites, no behavior change.

<div>&hairsp;</div>

### The subprocess runner is copied verbatim between the two PDF backends {#duplicated-subprocess-runner}

**advisory** · `src/pdf/tesseract.rs:361-372` · effort: trivial · <img src="assets/sparkline-duplicated-subprocess-runner.svg" height="14" alt="commit activity" />

This is byte-identical to `run` in src/pdf/mutool.rs:131-142. Both are the
single choke point through which every external tool invocation in the project
passes, and both define the same thing: how a failed external tool turns into an
error message a user reads. Any improvement made at that choke point — capturing
stdout on failure, including the exit status, truncating a multi-megabyte
stderr, adding a timeout — lands in one backend and not the other, and the two
backends then report failures differently for no reason a reader can discover.

```rust src/pdf/tesseract.rs:361-372
fn run(command: &mut Command, label: &str) -> Result<Output> {
    let output = command
        .output()
        .with_context(|| format!("failed to execute {label}"))?;
    if !output.status.success() {
        bail!(
            "{label} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output)
}
```

Related [Three copies of the EvidenceIssue constructor plus two inline literals](#duplicated-issue-constructors).

**Remediation:** Move `run` to `src/pdf/mod.rs` as `pub(crate) fn run(command: &mut Command,
label: &str) -> Result<Output>` and import it in both backends. Both call sites
already pass a descriptive label, so no signature change is needed.

*Effort (trivial):* One move, two imports, two deletions.

<div>&hairsp;</div>

### Terms::localize and ValidationReport::has_warnings have no callers {#dead-public-vocabulary-and-report-helpers}

**advisory** · `src/terms.rs:88-99` · effort: trivial · <img src="assets/sparkline-dead-public-vocabulary-and-report-helpers.svg" height="14" alt="commit activity" />

`Terms::localize` is called from tests/terms.rs:70 and nowhere in `src/`. Its
two helpers, `localize_entry` and `localize_locate`, are the ones production
uses (src/cli.rs:761). This matters beyond tidiness because the module doc at
src/terms.rs:4 states that documents are "localized back on output" — the
function that would do that for a whole document exists, passes a round-trip
test, and is wired to nothing, so a reader trusting the doc comment will
conclude `check --format json` localizes its vocabulary when it does not.
`ValidationReport::has_warnings` (src/validate.rs:33-36) is a second case with
no caller in `src/` or `tests/` at all; it survives from the Phase 2 plan
(record/superpowers/plans/2026-08-09-phase2-material-tokens-and-section-paths.md:114).

```rust src/terms.rs:88-99
/// Rewrite the canonical vocabulary back to the configured one, in place.
pub fn localize(&self, value: &mut Value) {
    if self.is_canonical() {
        return;
    }
    if let Some(evidence) = value.get_mut("evidence") {
        if let Some(entries) = evidence.get_mut("claims").and_then(Value::as_array_mut) {
            for entry in entries {
                self.localize_entry(entry);
            }
        }
        drop(rename(evidence, "claims", &self.claims, false));
```

**Remediation:** Decide the intent for each. If whole-document localization is wanted, wire
`localize` into the `check` and `audit` JSON paths and give it a CLI-level test;
if not, delete it and narrow the module doc at src/terms.rs:4 to say only
`locate` output is localized. Delete `has_warnings` unless a caller is imminent
— `is_valid` already covers the decision every current consumer makes.

*Effort (trivial):* Two deletions, or one wiring plus a doc correction.

<div>&hairsp;</div>

*Verdict: Nothing here is dangerous and nothing here is urgent; it is the ordinary consolidation a two-week-old codebase accumulates while its shape is still settling. The one worth doing early is validate_document, because it is the function every correctness finding in this report eventually routes through, and at 326 lines and six levels of nesting it is the hardest place in the crate to make a safe change. The duplications are each a small extraction, and the two dead items are a deletion.*

<div>&nbsp;</div>

---

## Remediation Ledger

| Finding | Concern | Location | Effort | Chains |
|---------|---------|----------|--------|--------|
| **The Evidence Trust Surface** | | | | |
| [ocr-cache-entries-are-unauthenticated-evidence](#ocr-cache-entries-are-unauthenticated-evidence) | critical | `src/pdf/cache.rs:115-133` | medium | enabled by: [cache-root-escapes-corpus-containment](#cache-root-escapes-corpus-containment); related: [summary-and-markdown-reads-skip-containment-guard](#summary-and-markdown-reads-skip-containment-guard) |
| [cache-root-escapes-corpus-containment](#cache-root-escapes-corpus-containment) | significant | `src/corpus.rs:36-45` | small | enables: [ocr-cache-entries-are-unauthenticated-evidence](#ocr-cache-entries-are-unauthenticated-evidence); related: [summary-and-markdown-reads-skip-containment-guard](#summary-and-markdown-reads-skip-containment-guard) |
| [summary-and-markdown-reads-skip-containment-guard](#summary-and-markdown-reads-skip-containment-guard) | moderate | `src/cli.rs:1119-1125` | small | related: [cache-root-escapes-corpus-containment](#cache-root-escapes-corpus-containment); related: [summary-walk-follows-directory-symlinks](#summary-walk-follows-directory-symlinks) |
| [summary-walk-recurses-without-a-depth-bound](#summary-walk-recurses-without-a-depth-bound) | moderate | `src/cli.rs:1157-1161` | small | related: [tool-failure-indistinguishable-from-invalid-evidence](#tool-failure-indistinguishable-from-invalid-evidence) |
| [ocr-dpi-has-no-upper-bound](#ocr-dpi-has-no-upper-bound) | moderate | `src/config.rs:274-282` | trivial | — |
| **The External Tool Surface** | | | | |
| [external-tool-paths-resolved-from-ambient-path](#external-tool-paths-resolved-from-ambient-path) | significant | `src/pdf/mutool.rs:14-26` | medium | enables: [stext-schema-drift-yields-silent-empty-extraction](#stext-schema-drift-yields-silent-empty-extraction); related: [external-tool-versions-probed-never-validated](#external-tool-versions-probed-never-validated) |
| [stext-schema-drift-yields-silent-empty-extraction](#stext-schema-drift-yields-silent-empty-extraction) | significant | `src/pdf/mutool.rs:144-167` | small | enabled by: [external-tool-versions-probed-never-validated](#external-tool-versions-probed-never-validated); enabled by: [external-tool-paths-resolved-from-ambient-path](#external-tool-paths-resolved-from-ambient-path) |
| [external-tool-versions-probed-never-validated](#external-tool-versions-probed-never-validated) | moderate | `src/cli.rs:594-601` | small | enables: [stext-schema-drift-yields-silent-empty-extraction](#stext-schema-drift-yields-silent-empty-extraction); related: [external-tool-paths-resolved-from-ambient-path](#external-tool-paths-resolved-from-ambient-path) |
| **The Failure Mode Surface** | | | | |
| [inline-html-byte-slice-panics-on-multibyte-markdown](#inline-html-byte-slice-panics-on-multibyte-markdown) | significant | `src/markdown.rs:135-143` | trivial | related: [broken-pipe-panic-writes-crash-dump](#broken-pipe-panic-writes-crash-dump) |
| [tool-failure-indistinguishable-from-invalid-evidence](#tool-failure-indistinguishable-from-invalid-evidence) | moderate | `src/validate.rs:275-281` | medium | related: [propose-aborts-the-batch-on-one-unreadable-summary](#propose-aborts-the-batch-on-one-unreadable-summary) |
| [broken-pipe-panic-writes-crash-dump](#broken-pipe-panic-writes-crash-dump) | moderate | `src/output.rs:1-8` | small | related: [inline-html-byte-slice-panics-on-multibyte-markdown](#inline-html-byte-slice-panics-on-multibyte-markdown) |
| [propose-aborts-the-batch-on-one-unreadable-summary](#propose-aborts-the-batch-on-one-unreadable-summary) | advisory | `src/cli.rs:993-1002` | small | related: [tool-failure-indistinguishable-from-invalid-evidence](#tool-failure-indistinguishable-from-invalid-evidence) |
| [vocabulary-rename-results-discarded-without-rationale](#vocabulary-rename-results-discarded-without-rationale) | note | `src/terms.rs:113-119` | trivial | — |
| **The Published Contract Surface** | | | | |
| [schema-declares-source-error-codes-the-code-never-emits](#schema-declares-source-error-codes-the-code-never-emits) | significant | `src/cli.rs:553-582` | small | related: [runtime-error-codes-absent-from-cli-spec](#runtime-error-codes-absent-from-cli-spec) |
| [runtime-error-codes-absent-from-cli-spec](#runtime-error-codes-absent-from-cli-spec) | significant | `src/cli.rs:844-862` | trivial | related: [schema-declares-source-error-codes-the-code-never-emits](#schema-declares-source-error-codes-the-code-never-emits) |
| [propose-json-shape-varies-by-summary-count](#propose-json-shape-varies-by-summary-count) | significant | `src/cli.rs:1010-1016` | trivial | — |
| [error-code-registry-is-hand-maintained](#error-code-registry-is-hand-maintained) | moderate | `src/cli.rs:539-552` | medium | related: [structured-output-contracts-untested](#structured-output-contracts-untested); related: [duplicated-issue-constructors](#duplicated-issue-constructors) |
| [default-output-becomes-json-when-redirected](#default-output-becomes-json-when-redirected) | moderate | `src/cli.rs:183-185` | trivial | related: [propose-json-shape-varies-by-summary-count](#propose-json-shape-varies-by-summary-count) |
| [undocumented-user-config-and-environment-layers](#undocumented-user-config-and-environment-layers) | moderate | `src/config.rs:185-199` | small | — |
| [summary-id-constraints-undocumented](#summary-id-constraints-undocumented) | advisory | `src/corpus.rs:168-179` | trivial | — |
| **The Type and API Surface** | | | | |
| [entire-crate-is-a-published-public-api](#entire-crate-is-a-published-public-api) | advisory | `src/lib.rs:1-17` | medium | enables: [coordinate-primitives-are-interchangeable-usize](#coordinate-primitives-are-interchangeable-usize); enables: [byte-offset-published-as-a-markdown-column](#byte-offset-published-as-a-markdown-column); enables: [report-types-are-write-only](#report-types-are-write-only); related: [public-apis-erase-error-types](#public-apis-erase-error-types); related: [unvalidated-discovered-state](#unvalidated-discovered-state); related: [cache-manifest-invariants-are-bypassable](#cache-manifest-invariants-are-bypassable) |
| [coordinate-primitives-are-interchangeable-usize](#coordinate-primitives-are-interchangeable-usize) | significant | `src/markdown.rs:163-174` | medium | enables: [byte-offset-published-as-a-markdown-column](#byte-offset-published-as-a-markdown-column); enabled by: [entire-crate-is-a-published-public-api](#entire-crate-is-a-published-public-api) |
| [propose-duplicates-locator-types](#propose-duplicates-locator-types) | significant | `src/propose.rs:55-69` | small | related: [duplicated-issue-constructors](#duplicated-issue-constructors) |
| [byte-offset-published-as-a-markdown-column](#byte-offset-published-as-a-markdown-column) | moderate | `src/markdown.rs:247-254` | small | enabled by: [coordinate-primitives-are-interchangeable-usize](#coordinate-primitives-are-interchangeable-usize); enabled by: [entire-crate-is-a-published-public-api](#entire-crate-is-a-published-public-api) |
| [debug-formatting-leaks-into-user-facing-messages](#debug-formatting-leaks-into-user-facing-messages) | moderate | `src/validate.rs:189-198` | trivial | related: [vocabulary-leaks-in-validation-messages](#vocabulary-leaks-in-validation-messages) |
| [candidate-pdf-option-is-never-none](#candidate-pdf-option-is-never-none) | advisory | `src/propose.rs:176-197` | trivial | related: [report-types-are-write-only](#report-types-are-write-only) |
| [report-types-are-write-only](#report-types-are-write-only) | advisory | `src/propose.rs:21-36` | trivial | enabled by: [entire-crate-is-a-published-public-api](#entire-crate-is-a-published-public-api); related: [candidate-pdf-option-is-never-none](#candidate-pdf-option-is-never-none) |
| **The Performance Surface** | | | | |
| [validate-spawns-one-mutool-per-cited-page](#validate-spawns-one-mutool-per-cited-page) | significant | `src/validate.rs:233-248` | medium | related: [pdf-verification-unbounded-when-candidates-fail](#pdf-verification-unbounded-when-candidates-fail) |
| [propose-regenerates-claim-independent-spans-per-claim](#propose-regenerates-claim-independent-spans-per-claim) | significant | `src/propose.rs:159-169` | medium | enables: [pdf-verification-unbounded-when-candidates-fail](#pdf-verification-unbounded-when-candidates-fail) |
| [resolve-unit-linear-scan-called-twice-per-locator](#resolve-unit-linear-scan-called-twice-per-locator) | moderate | `src/markdown.rs:172-181` | small | related: [weak-section-list-lowercased-per-comparison](#weak-section-list-lowercased-per-comparison) |
| [pdf-verification-unbounded-when-candidates-fail](#pdf-verification-unbounded-when-candidates-fail) | moderate | `src/propose.rs:171-179` | small | enabled by: [propose-regenerates-claim-independent-spans-per-claim](#propose-regenerates-claim-independent-spans-per-claim) |
| [owned-json-value-borrowed-then-deep-cloned](#owned-json-value-borrowed-then-deep-cloned) | moderate | `src/review.rs:46-56` | trivial | — |
| [source-names-allocates-a-vec-to-answer-a-lookup](#source-names-allocates-a-vec-to-answer-a-lookup) | advisory | `src/corpus.rs:127-131` | trivial | enabled by: [entire-crate-is-a-published-public-api](#entire-crate-is-a-published-public-api) |
| [weak-section-list-lowercased-per-comparison](#weak-section-list-lowercased-per-comparison) | advisory | `src/sections.rs:14-24` | trivial | related: [resolve-unit-linear-scan-called-twice-per-locator](#resolve-unit-linear-scan-called-twice-per-locator) |
| [release-profile-left-at-cargo-defaults](#release-profile-left-at-cargo-defaults) | note | `Cargo.toml:22-33` | trivial | — |
| **The Verification Gate Surface** | | | | |
| [structured-output-contracts-untested](#structured-output-contracts-untested) | significant | `src/cli.rs:632-642` | small | related: [error-code-registry-is-hand-maintained](#error-code-registry-is-hand-maintained); related: [summary-discovery-recursion-untested](#summary-discovery-recursion-untested) |
| [just-check-is-not-a-complete-gate](#just-check-is-not-a-complete-gate) | moderate | `justfile:1-10` | trivial | related: [structured-output-contracts-untested](#structured-output-contracts-untested) |
| [no-automated-ci-for-the-documented-gate](#no-automated-ci-for-the-documented-gate) | moderate | `justfile:40-42` | small | — |
| [justfile-loads-repo-supplied-dotenv](#justfile-loads-repo-supplied-dotenv) | moderate | `justfile:1-4` | trivial | — |
| [license-allowlist-inherited-from-absent-dependency-tree](#license-allowlist-inherited-from-absent-dependency-tree) | moderate | `.config/deny.toml:28-42` | trivial | related: [transitive-syn-duplicate-is-not-actionable](#transitive-syn-duplicate-is-not-actionable) |
| [summary-discovery-recursion-untested](#summary-discovery-recursion-untested) | moderate | `src/cli.rs:1157-1176` | small | related: [structured-output-contracts-untested](#structured-output-contracts-untested) |
| [markdown-coordinates-only-pinned-at-line-one](#markdown-coordinates-only-pinned-at-line-one) | moderate | `tests/markdown.rs:20-28` | trivial | — |
| [transitive-syn-duplicate-is-not-actionable](#transitive-syn-duplicate-is-not-actionable) | note | `.config/deny.toml:61-79` | trivial | related: [license-allowlist-inherited-from-absent-dependency-tree](#license-allowlist-inherited-from-absent-dependency-tree) |
| **The Structure Surface** | | | | |
| [validate-document-carries-seven-concerns](#validate-document-carries-seven-concerns) | moderate | `src/validate.rs:161-176` | medium | — |
| [check-and-audit-duplicate-the-summary-loop](#check-and-audit-duplicate-the-summary-loop) | moderate | `src/cli.rs:909-926` | medium | related: [duplicated-issue-constructors](#duplicated-issue-constructors) |
| [duplicated-issue-constructors](#duplicated-issue-constructors) | moderate | `src/validate.rs:486-500` | small | related: [error-code-registry-is-hand-maintained](#error-code-registry-is-hand-maintained); related: [check-and-audit-duplicate-the-summary-loop](#check-and-audit-duplicate-the-summary-loop) |
| [duplicated-subprocess-runner](#duplicated-subprocess-runner) | advisory | `src/pdf/tesseract.rs:361-372` | trivial | related: [duplicated-issue-constructors](#duplicated-issue-constructors) |
| [dead-public-vocabulary-and-report-helpers](#dead-public-vocabulary-and-report-helpers) | advisory | `src/terms.rs:88-99` | trivial | — |

<sub>
Generated 2026-08-10 at commit b78d869.
Intermediate artifacts: recon.yaml, findings.yaml, report.html.
Verification records: review-1.md, review-2.md, review-3.md.
</sub>

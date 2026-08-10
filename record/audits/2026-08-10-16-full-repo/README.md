---
audit_date: 2026-08-10
project: receipts
commit: 3c98f512425a89876c82fe52d98d21ba33ee6a55
scope: Full repository audit — Rust source, tests, dependencies, configuration, and documented behavior
auditor: codex-gpt-5
findings:
  critical: 0
  significant: 8
  moderate: 8
  advisory: 4
  note: 0
---

# Audit: receipts

`receipts` has a disciplined core: unsafe code is forbidden, the static lint
baseline is clean, and its advisory, license, ban, and source-policy gates pass.
**The Configuration and Cache Integrity Surface** permits checked invariants to
be bypassed through public construction. **The Error Architecture Surface** is
mostly explicit but loses operational failures in proposal and OCR paths. **The
Dependency Reachability Surface** is secure and fully used, with two avoidable
default-feature sets. **The Repeated-Work Performance Surface** multiplies work
by page and candidate cardinality. **The Documented Agent Contract Surface** is
the broadest gap: several runtime outputs do not match the advertised CLI Spec
or configuration model. The architecture is sound; the boundary contracts need
to become as deterministic as the evidence matcher.

---

## The Configuration and Cache Integrity Surface

*File-loaded configuration is carefully checked, but public construction and
deserialization can bypass the invariants that keep corpus and cache paths
contained.*

> If public construction can mint the same state as validated configuration, I
> do not need to beat the path checks. I can simply enter downstream of them.

### cache-manifest-invariants-are-bypassable

CacheManifest exposes the fields its constructors validate.

**significant** · `src/pdf/cache.rs:27-35` · effort: medium ·
<img src="assets/sparkline-cache-manifest-invariants-are-bypassable.svg" height="14" alt="12-month commit activity" />

`CacheManifest::new` and `with_render_rotation` validate hashes, page numbers,
profile names, the derived toolchain hash, and rotation. Public fields and
derived deserialization bypass both methods. `OcrCache::entry_dir` later joins
those components into filesystem paths, so an invalid value can escape the
configured cache root before `store` writes it.

```rust src/pdf/cache.rs:27-35
/// Manifest that makes a cached TSV self-validating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheManifest {
    pub pdf_sha256: String,
    pub page: usize,
    pub profile: OcrProfile,
    pub toolchain_sha256: String,
    pub render_rotation_degrees: i16,
```

Related to [unvalidated-discovered-state](#unvalidated-discovered-state).

**Remediation:** Make the fields private, expose read-only accessors, and
deserialize through a private raw representation followed by `TryFrom`
validation. Use validated newtypes for filesystem components and one-based page
numbers.

### unvalidated-discovered-state

from_discovered accepts configuration without preserving its validation proof.

**moderate** · `src/corpus.rs:29-56` · effort: small ·
<img src="assets/sparkline-unvalidated-discovered-state.svg" height="14" alt="12-month commit activity" />

`config::load` validates templates and OCR settings, but `Discovered` exposes
its `Config` and `Corpus::from_discovered` never revalidates it. A caller can
construct absolute, parent-traversing, or empty templates and obtain a `Corpus`
that violates the documented containment contract.

```rust src/corpus.rs:29-56
    /// Build a corpus from an already-resolved configuration.
    pub fn from_discovered(discovered: Discovered) -> Result<Self> {
        let Discovered {
            config,
            root,
            config_file,
        } = discovered;
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
        };
        Ok(Self {
            root,
            layout: config.corpus,
            cache_root,
            config_file,
            terms: config.terms,
            ocr_config: config.pdf.ocr,
            coverage_config: config.coverage,
            sections_config: config.sections,
        })
```

Enables
[unchecked-empty-markdown-candidates-panic](#unchecked-empty-markdown-candidates-panic).
Related to
[cache-manifest-invariants-are-bypassable](#cache-manifest-invariants-are-bypassable).

**Remediation:** Re-run the shared validator in `from_discovered` and make
`Discovered` opaque, or introduce a private validated-state type that only
checked constructors can produce. Cover traversal and empty-template cases.

### unchecked-empty-markdown-candidates-panic

Programmatic corpus configuration can trigger an indexing panic.

**significant** · `src/validate.rs:84-87` · effort: small ·
<img src="assets/sparkline-unchecked-empty-markdown-candidates-panic.svg" height="14" alt="12-month commit activity" />

The file-loading path rejects an empty Markdown-template list, but the public
constructor accepts unchecked state. With an empty list, the eager `unwrap_or`
fallback evaluates `markdown_candidates[0]` and panics instead of returning a
validation issue.

```rust src/validate.rs:84-87
        let markdown_path = markdown_candidates
            .iter()
            .find(|path| path.is_file())
            .unwrap_or(&markdown_candidates[0]);
```

Enabled by [unvalidated-discovered-state](#unvalidated-discovered-state).
Related to [public-apis-erase-error-types](#public-apis-erase-error-types).

**Remediation:** Validate `Discovered` at the constructor boundary and replace
the direct index with `first()` plus a structured configuration error or
validation issue. Add a regression test through the public constructor.

*Verdict: The CLI path protects the filesystem boundary, but the public API does
not preserve proof that those checks ran. Make validated state unforgeable and
the remaining panic disappears with it.*

---

## The Error Architecture Surface

*Most failures remain explicit, but proposal generation and OCR cleanup erase
operational errors while public APIs erase their categories.*

> A missing PDF and a real absence of evidence both become an empty candidate
> list. From automation, those states are indistinguishable—and only one is a
> valid research result.

### propose-silently-suppresses-source-failures

Proposal generation converts source failures into successful empty results.

**significant** · `src/propose.rs:135-155` · effort: small ·
<img src="assets/sparkline-propose-silently-suppresses-source-failures.svg" height="14" alt="12-month commit activity" />

Missing Markdown, read failures, missing PDFs, and native extraction failures
all skip their source without a diagnostic. `propose_document` then returns
`Ok(ProposalReport)` with fewer or no candidates, making operational failure
look like genuine absence of evidence.

```rust src/propose.rs:135-155
    for source_name in corpus.source_names() {
        let markdown_candidates = corpus.markdown_candidates_for(&summary.id, &source_name)?;
        if let Some(path) = markdown_candidates.iter().find(|path| path.is_file())
            && let Ok(markdown) = fs::read_to_string(path)
        {
            source_units.insert(source_name.clone(), parse_units(&markdown));
        }

        let pdf_path = corpus.pdf_path_for(&summary.id, &source_name)?;
        if pdf_path.is_file()
            && let Ok(pages) = provider.native_pages(&pdf_path, None)
        {
            source_pages.insert(
                source_name,
                pages
                    .into_iter()
                    .map(|page| (page.page, normalize(&page.text)))
                    .collect(),
            );
        }
    }
```

Related to [public-apis-erase-error-types](#public-apis-erase-error-types).

**Remediation:** Require each configured source to resolve and propagate
read/extraction failures with source and path context. If partial output is
intentional, include typed per-source diagnostics and fail the CLI whenever a
configured source could not be evaluated.

### ocr-temporary-file-cleanup-errors-discarded

OCR temporary-file cleanup failures are silently discarded.

**moderate** · `src/pdf/tesseract.rs:246-250` · effort: small ·
<img src="assets/sparkline-ocr-temporary-file-cleanup-errors-discarded.svg" height="14" alt="12-month commit activity" />

Both temporary-image removal results are dropped without a diagnostic.
Permission changes, filesystem faults, or concurrent interference can therefore
accumulate images while callers receive successful OCR results.

```rust src/pdf/tesseract.rs:246-250
            Ok::<_, anyhow::Error>(tsv)
        })();
        let _ = fs::remove_file(&initial);
        let _ = fs::remove_file(&rotated);
        let tsv = result?;
```

Related to
[propose-silently-suppresses-source-failures](#propose-silently-suppresses-source-failures).

**Remediation:** Centralize cleanup in a guard or helper. Propagate cleanup
failure after successful OCR; when an extraction error already exists, attach
cleanup failure as additional context rather than replacing the primary cause.

### public-apis-erase-error-types

Public library APIs erase failure categories behind anyhow::Error.

**advisory** · `src/output.rs:1-6` · effort: medium ·
<img src="assets/sparkline-public-apis-erase-error-types.svg" height="14" alt="12-month commit activity" />

Public corpus, parsing, output, cache, extraction, and provider methods use
`anyhow::Result`. Callers cannot exhaustively distinguish invalid input, missing
tools, subprocess failure, filesystem failure, or serialization failure without
inspecting display strings.

```rust src/output.rs:1-6
use anyhow::Result;
use serde::Serialize;

/// Print any serializable report as stable pretty JSON.
pub fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
```

Related to
[unchecked-empty-markdown-candidates-panic](#unchecked-empty-markdown-candidates-panic)
and
[propose-silently-suppresses-source-failures](#propose-silently-suppresses-source-failures).

**Remediation:** Define stable typed errors at configuration, corpus, evidence,
and PDF boundaries while preserving underlying sources. Convert domain errors to
`anyhow::Error` only at the CLI edge.

*Verdict: The affected functions already return `Result`; this is policy drift,
not structural debt. Propagate source failures, define cleanup semantics, and
keep application-oriented erasure at the application boundary.*

---

## The Dependency Reachability Surface

*The dependency tree is advisory-clean and fully used, but two default-feature
sets compile code that receipts never calls.*

### unscoped-librebar-default-features

Librebar defaults compile an unused cache subsystem.

**moderate** · `Cargo.toml:16` · effort: small ·
<img src="assets/sparkline-unscoped-librebar-default-features.svg" height="14" alt="12-month commit activity" />

Librebar's explicit `diagnostics` feature already activates logging. Cargo's
default-feature behavior adds the cache feature beyond that explicit closure,
but receipts does not use Librebar's cache API. The result is extra compile work
and reachable dependency surface without exercised behavior.

```toml Cargo.toml:16
librebar = { version = "=0.6.0", features = ["cli", "config", "crash", "diagnostics"] }
```

Related to
[pulldown-cmark-unused-default-features](#pulldown-cmark-unused-default-features).

**Remediation:** Set `default-features = false` while preserving the existing
explicit feature list. Regenerate the lockfile and inspect `cargo tree -e
features -i librebar`. Treat removal of explicitly selected `diagnostics` as a
separate decision after confirming the intended crash path.

### pulldown-cmark-unused-default-features

Pulldown-cmark enables HTML rendering and CLI parsing for parser-only use.

**advisory** · `Cargo.toml:17` · effort: trivial ·
<img src="assets/sparkline-pulldown-cmark-unused-default-features.svg" height="14" alt="12-month commit activity" />

Pulldown-cmark enables `getopts` and `html` by default, while receipts consumes
only event-parser APIs. The graph compiles CLI parsing and HTML rendering support
that this crate never calls.

```toml Cargo.toml:17
pulldown-cmark = "0.13"
```

Related to
[unscoped-librebar-default-features](#unscoped-librebar-default-features).

**Remediation:** Declare `{ version = "0.13", default-features = false }`, run
the Markdown parser tests, and confirm the narrower graph with `cargo tree -e
features`.

*Verdict: RustSec, licenses, bans, sources, Clippy, machete, and udeps are clean.
Narrow the two feature closures; no dependency replacement is warranted.
Binary-size attribution was not measured, so this audit makes no size claim.*

---

## The Repeated-Work Performance Surface

*Corpus-wide validation repeats prefix scans, subprocess probes, allocations,
and candidate verification at page-and-candidate cardinality.*

### ocr-cache-hit-spawns-version-probes

Every OCR cache lookup launches two version-probe subprocesses.

**significant** · `src/pdf/tesseract.rs:206-223` · effort: medium ·
<img src="assets/sparkline-ocr-cache-hit-spawns-version-probes.svg" height="14" alt="12-month commit activity" />

Every OCR-backed page runs `mutool -v` and `tesseract --version` before the
cache lookup. Validating N already-cached pages therefore launches 2N processes,
defeating the cache's low-latency purpose on corpus-wide commands.

```rust src/pdf/tesseract.rs:206-223
    let lang = tesseract.lang();
    let dpi = tesseract.dpi();
    let psm = tesseract.psm();
    let profile = OcrProfile {
        name: profile_name(lang, dpi),
        language: lang.to_owned(),
        dpi,
        page_segmentation_mode: psm,
        mutool_version: mutool.version()?,
        tesseract_version: tesseract.version()?,
        render_command: render_command(dpi),
        orientation_command: ORIENTATION_COMMAND.to_owned(),
        recognition_command: recognition_command(lang, psm),
    };
    let manifest = CacheManifest::new(pdf_sha256.to_owned(), page, profile)?;
    if let Some(tsv) = cache.load(&manifest)? {
        return extracted_page(page, &tsv);
    }
```

Related to
[tesseract-tsv-allocates-three-vectors-per-row](#tesseract-tsv-allocates-three-vectors-per-row).

**Remediation:** Resolve tool versions and the immutable OCR profile once per
provider, lazily if construction must remain infallible, and reuse the profile
when deriving page manifests.

### proposal-ranking-verifies-unbounded-candidate-set

Proposal ranking verifies every candidate before applying the output limit.

**significant** · `src/propose.rs:157-194` · effort: medium ·
<img src="assets/sparkline-proposal-ranking-verifies-unbounded-candidate-set.svg" height="14" alt="12-month commit activity" />

Every token-bearing Markdown span is materialized and verified by scanning the
normalized PDF pages. Only after all verified candidates are owned and sorted
does the code retain N, producing work proportional to claims × candidates ×
PDF text despite a default output limit of three.

```rust src/propose.rs:157-194
    let mut claims = Vec::new();
    for (index, claim_text) in summary.claims.iter().enumerate() {
        if !all_claims && settled.contains(&index) {
            continue;
        }

        let claim_tokens = tokens::extract(claim_text);
        let mut candidates = Vec::new();
        for (source_name, units) in &source_units {
            let pages = source_pages.get(source_name);
            for raw in generate_candidates(source_name, units, &claim_tokens) {
                // A span the reader cannot find in the PDF is not evidence.
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
            }
        }

        rank_candidates(&mut candidates);
        candidates.truncate(max_candidates);
```

Related to
[case-insensitive-token-check-allocates-per-comparison](#case-insensitive-token-check-allocates-per-comparison).

**Remediation:** Score borrowed spans first, order them deterministically, and
verify in rank order until `max_candidates` unambiguous matches are accepted,
continuing only when a higher-ranked candidate fails verification.

### markdown-coordinate-resolution-rescans-prefixes

Markdown parsing rescans the document prefix for every semantic unit.

**moderate** · `src/markdown.rs:236-245` · effort: small ·
<img src="assets/sparkline-markdown-coordinate-resolution-rescans-prefixes.svg" height="14" alt="12-month commit activity" />

Semantic units finish in source order, but coordinate reconstruction counts
from the beginning for every unit. For U units across N bytes this performs
O(U×N) prefix work and approaches quadratic behavior on long converted
documents used by every evidence command.

```rust src/markdown.rs:236-245
fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, tail)| tail)
        .chars()
        .count()
        + 1;
    (line, column)
```

**Remediation:** Track monotonic source offset and line state while consuming
events, or build line-start offsets once and resolve each unit with
`partition_point`.

### case-insensitive-token-check-allocates-per-comparison

Number-word coverage allocates two lowercase strings per comparison.

**advisory** · `src/tokens.rs:57-59` · effort: small ·
<img src="assets/sparkline-case-insensitive-token-check-allocates-per-comparison.svg" height="14" alt="12-month commit activity" />

Proposal scoring calls this helper for every required number-word token and
candidate span. The token is already lowercase, yet each comparison allocates
it again plus a lowercase copy of the potentially long span.

```rust src/tokens.rs:57-59
pub fn is_covered_case_insensitive(text: &str, token: &str) -> bool {
    is_covered(&text.to_lowercase(), &token.to_lowercase())
}
```

Enabled by
[proposal-ranking-verifies-unbounded-candidate-set](#proposal-ranking-verifies-unbounded-candidate-set).

**Remediation:** Scan ASCII bytes with `eq_ignore_ascii_case`, preserving the
current alphanumeric boundary rules without allocating either operand.

### tesseract-tsv-allocates-three-vectors-per-row

Cached Tesseract TSV parsing allocates three temporary vectors per word row.

**advisory** · `src/pdf/tesseract.rs:273-301` · effort: small ·
<img src="assets/sparkline-tesseract-tsv-allocates-three-vectors-per-row.svg" height="14" alt="12-month commit activity" />

Every nonempty TSV row allocates a field vector, recreates four geometry indexes
as another vector, and collects four coordinates into a third. This path also
runs on cache hits, where OCR no longer masks parsing cost.

```rust src/pdf/tesseract.rs:273-301
    for line in lines {
        let fields: Vec<_> = line.split('\t').collect();
        let Some(text) = fields.get(text_index).map(|value| value.trim()) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        words.push(text);
        let bbox = geometry_indexes
            .iter()
            .copied()
            .collect::<Option<Vec<_>>>()
            .and_then(|indexes| {
                let values = indexes
                    .into_iter()
                    .map(|index| fields.get(index)?.parse::<f64>().ok())
                    .collect::<Option<Vec<_>>>()?;
                Some(PdfBbox {
                    x: values[0],
                    y: values[1],
                    width: values[2],
                    height: values[3],
                })
            });
        spans.push(TextSpan {
            text: text.to_owned(),
            bbox,
        });
```

Related to
[ocr-cache-hit-spawns-version-probes](#ocr-cache-hit-spawns-version-probes).

**Remediation:** Convert header positions once into `Option<[usize; 4]>`,
stream row fields while capturing only required indexes, and parse coordinates
into `[f64; 4]`.

*Verdict: Profile the two structural findings after remediation; they dominate
the local allocation costs. The fixes preserve determinism and change only when
work occurs, not which evidence wins.*

---

## The Documented Agent Contract Surface

*The command set is broad and reachable, but several machine-readable and
corpus-wide promises diverge from runtime behavior.*

> The schema says it is exhaustive, so I build an exhaustive handler. Runtime
> then emits a code or shape the schema never mentioned. The strict tool has
> pushed ambiguity one layer outward into its integration contract.

### cli-schema-omits-runtime-errors

CLI schema omits runtime evidence error codes.

**significant** · `src/evidence.rs:206-224` · effort: medium ·
<img src="assets/sparkline-cli-schema-omits-runtime-errors.svg" height="14" alt="12-month commit activity" />

The README calls `receipts schema` the primary agent interface and says it lists
every error code. The schema registers 18 while runtime paths emit at least 20
more, including source-qualified dynamic codes. An agent cannot discover or
exhaustively handle actual validation outcomes.

```rust src/evidence.rs:206-224
        if evidence.sources.is_empty() {
            issues.push(issue(
                "empty_sources",
                Severity::Error,
                "evidence declares no sources",
                None,
                None,
            ));
        }
        for (name, pair) in &evidence.sources {
            if !validate_source_name(name) {
                issues.push(issue(
                    "invalid_source_name",
                    Severity::Error,
                    format!("source name {name:?} is not a valid path component"),
                    None,
                    None,
                ));
            }
```

Related to
[doctor-json-shape-contradicts-schema](#doctor-json-shape-contradicts-schema).

**Remediation:** Define the issue vocabulary once and generate both issue
construction and metadata from that registry. Use stable kinds plus structured
source data, then test that every emitted kind appears in CLI Spec.

### custom-summary-template-inventory

Corpus-wide commands cannot inventory supported custom summary templates.

**significant** · `src/cli.rs:969-984` · effort: medium ·
<img src="assets/sparkline-custom-summary-template-inventory.svg" height="14" alt="12-month commit activity" />

Configuration accepts any relative summary template containing `{id}`, and tests
support `records/{id}/summary.yaml`. Bare `check`, `audit`, and `propose` scan only
immediate `.yaml` children and treat each filename stem as the ID. Supported
nested or decorated templates therefore disappear from inventory.

```rust src/cli.rs:969-984
fn summary_ids(corpus: &Corpus) -> Result<Vec<String>> {
    let directory = corpus.summaries_dir();
    let mut ids = Vec::new();
    for entry in fs::read_dir(&directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("yaml")
            && let Some(id) = path.file_stem().and_then(|value| value.to_str())
        {
            ids.push(id.to_owned());
        }
    }
    ids.sort();
    Ok(ids)
}
```

**Remediation:** Derive discovery from the full template: traverse below its
static prefix, match the static prefix/suffix, extract `{id}`, and confirm the
resolved path. Add bare-command CLI coverage for a nested template.

### propose-multi-id-invalid-json

Multi-ID propose output is not a valid JSON document.

**significant** · `src/cli.rs:834-860` · effort: small ·
<img src="assets/sparkline-propose-multi-id-invalid-json.svg" height="14" alt="12-month commit activity" />

The command accepts `[ID...]` and piped output defaults to JSON, but the loop
prints one pretty top-level object per summary. Two IDs produce adjacent JSON
documents: neither valid single JSON nor documented JSON Lines.

```rust src/cli.rs:834-860
fn propose_cmd(corpus: &Corpus, args: &ProposeArgs, json: bool, quiet: bool) -> Result<()> {
    let ids = if args.ids.is_empty() {
        summary_ids(corpus)?
    } else {
        args.ids.clone()
    };
    let tools = PdfTools::new(corpus.cache_root().to_path_buf(), corpus.ocr_config());

    for id in &ids {
        let summary = read_summary(corpus, id)?;
        if summary.id != *id {
            bail!(
                "summary ID {:?} does not match filename ID {:?}",
                summary.id,
                id
            );
        }
        let report =
            crate::propose::propose_document(corpus, &summary, &tools, args.candidates, args.all)?;
        if json {
            print_json(&report)?;
        } else if !quiet {
            print_propose_human(&report, &summary, corpus.terms());
        }
    }
    Ok(())
}
```

**Remediation:** Collect reports and serialize one stable aggregate object with
a `summaries` array. Update CLI Spec fields and parse multi-ID stdout with
`serde_json` in an integration test.

### advisory-tokens-never-reported

Advertised advisory tokens never reach check or propose output.

**moderate** · `.handoffs/2026-08-09-phase-2-complete.md:157-160` · effort: small ·
<img src="assets/sparkline-advisory-tokens-never-reported.svg" height="14" alt="12-month commit activity" />

Token extraction computes advisory values, but validation reports contain only
issues and proposal claims expose only required/uncovered tokens and candidates.
The settled reporting contract is absent from both structured outputs, including
when required-token enforcement is off.

```markdown .handoffs/2026-08-09-phase-2-complete.md:157-160
- **Advisory tokens are never enforced.** They appear in `--json` output
  and will appear in `propose` output (Phase 3), but they never produce
  issues and never affect exit status. `coverage.tokens: off` disables
  required-token enforcement but leaves advisory extraction intact.
```

**Remediation:** Add per-claim advisory-token data to validation and proposal
models, populate it independently of enforcement, declare it in CLI Spec, and
cover `coverage.tokens: off`.

### doctor-json-shape-contradicts-schema

Doctor JSON is an array of tuples, not the declared object fields.

**moderate** · `src/cli.rs:445-472` · effort: small ·
<img src="assets/sparkline-doctor-json-shape-contradicts-schema.svg" height="14" alt="12-month commit activity" />

The JSON path serializes checks as `[label, valid, detail]` arrays. CLI Spec
declares named top-level string fields such as `corpus`, `config`,
`native_profile`, and `ocr_profile`; runtime labels also use different casing.

```rust src/cli.rs:445-472
    let mut checks = vec![
        ("corpus", true, corpus.root().display().to_string()),
        (
            "config",
            true,
            corpus
                .config_file()
                .map_or_else(|| "defaults".to_owned(), |path| path.display().to_string()),
        ),
        match tools.mutool.version() {
            Ok(version) => ("mutool", true, version),
            Err(error) => ("mutool", false, error.to_string()),
        },
        match tools.tesseract.version() {
            Ok(version) => ("tesseract", true, version),
            Err(error) => ("tesseract", false, error.to_string()),
        },
        (
            "native profile",
            true,
            crate::pdf::mutool::PROFILE_NAME.to_owned(),
        ),
        (
            "OCR profile",
            true,
            crate::pdf::tesseract::profile_name(&ocr.lang, ocr_dpi),
        ),
    ];
```

Related to [cli-schema-omits-runtime-errors](#cli-schema-omits-runtime-errors).

**Remediation:** Implement a serializable `DoctorReport` whose named fields
match the authoritative CLI Spec at `src/cli.rs:210-232`, serialize it at the
existing JSON boundary, and add a schema-conformance integration test.

### vocabulary-leaks-in-validation-messages

Review validation ignores configured claim vocabulary.

**moderate** · `src/review.rs:104-137` · effort: small ·
<img src="assets/sparkline-vocabulary-leaks-in-validation-messages.svg" height="14" alt="12-month commit activity" />

The README says human-readable messages follow configured vocabulary, but
review validation does not receive `Terms` and hard-codes `claim` throughout.
Token, section, missing-review, and locate messages contain similar literals.

```rust src/review.rs:104-137
    for entry in &review.claims {
        if entry.claim >= claims.len() {
            issues.push(issue(
                "unknown_review_claim",
                Severity::Error,
                format!(
                    "review entry references claim {} but only {} claims exist",
                    entry.claim,
                    claims.len()
                ),
                Some(entry.claim),
            ));
            continue;
        }

        if !reviewed.insert(entry.claim) {
            issues.push(issue(
                "duplicate_review_claim",
                Severity::Error,
                format!("duplicate review entry for claim {}", entry.claim),
                Some(entry.claim),
            ));
            continue;
        }

        let expected_claim_hash = sha256_bytes(claims[entry.claim].as_bytes());
        if entry.claim_sha256 != expected_claim_hash {
            issues.push(issue(
                "stale_review_claim",
                Severity::Error,
                format!(
                    "review claim_sha256 is stale for claim {}: expected {expected_claim_hash}, found {}",
                    entry.claim, entry.claim_sha256
                ),
```

**Remediation:** Pass `&Terms` through review validation and use it in every
human-readable message while keeping codes fixed. Cover review, token, section,
and locate failures with custom vocabulary.

### missing-just-ci-entry-point

README advertises a nonexistent just ci command.

**moderate** · `README.md:337-344` · effort: trivial ·
<img src="assets/sparkline-missing-just-ci-entry-point.svg" height="14" alt="12-month commit activity" />

The `justfile` defines `check`, `test`, and `doctor` but no `ci`, so the
documented command fails immediately. The adjacent `check` description is also
stale because that recipe already runs the full validation gate.

````markdown README.md:337-344
## Development

```bash
just check   # fmt, clippy, build
just test    # full suite
just ci      # both
just doctor  # probe tools
```
````

**Remediation:** Add a `ci` alias for the canonical gate, then align recipe
comments and the README with `just --list`.

*Verdict: The command implementation is substantially complete; the failures
cluster in contract generation, aggregation, and discovery. Generate metadata
from runtime vocabularies and test runtime JSON against it so agents can trust
the interface without reverse-engineering source.*

---

## Remediation Ledger

| Finding | Concern | Location | Effort | Chains |
|---------|---------|----------|--------|--------|
| | | **Configuration and Cache Integrity Surface** | | |
| [cache-manifest-invariants-are-bypassable](#cache-manifest-invariants-are-bypassable) | significant | `src/pdf/cache.rs:27-35` | medium | related: unvalidated-discovered-state |
| [unvalidated-discovered-state](#unvalidated-discovered-state) | moderate | `src/corpus.rs:29-56` | small | enables: unchecked-empty-markdown-candidates-panic |
| [unchecked-empty-markdown-candidates-panic](#unchecked-empty-markdown-candidates-panic) | significant | `src/validate.rs:84-87` | small | enabled by: unvalidated-discovered-state |
| | | **Error Architecture Surface** | | |
| [propose-silently-suppresses-source-failures](#propose-silently-suppresses-source-failures) | significant | `src/propose.rs:135-155` | small | related: public-apis-erase-error-types |
| [ocr-temporary-file-cleanup-errors-discarded](#ocr-temporary-file-cleanup-errors-discarded) | moderate | `src/pdf/tesseract.rs:246-250` | small | related: propose-silently-suppresses-source-failures |
| [public-apis-erase-error-types](#public-apis-erase-error-types) | advisory | `src/output.rs:1-6` | medium | related: two error findings |
| | | **Dependency Reachability Surface** | | |
| [unscoped-librebar-default-features](#unscoped-librebar-default-features) | moderate | `Cargo.toml:16` | small | related: pulldown-cmark-unused-default-features |
| [pulldown-cmark-unused-default-features](#pulldown-cmark-unused-default-features) | advisory | `Cargo.toml:17` | trivial | related: unscoped-librebar-default-features |
| | | **Repeated-Work Performance Surface** | | |
| [ocr-cache-hit-spawns-version-probes](#ocr-cache-hit-spawns-version-probes) | significant | `src/pdf/tesseract.rs:206-223` | medium | related: tesseract-tsv-allocates-three-vectors-per-row |
| [proposal-ranking-verifies-unbounded-candidate-set](#proposal-ranking-verifies-unbounded-candidate-set) | significant | `src/propose.rs:157-194` | medium | related: case-insensitive-token-check-allocates-per-comparison |
| [markdown-coordinate-resolution-rescans-prefixes](#markdown-coordinate-resolution-rescans-prefixes) | moderate | `src/markdown.rs:236-245` | small | — |
| [case-insensitive-token-check-allocates-per-comparison](#case-insensitive-token-check-allocates-per-comparison) | advisory | `src/tokens.rs:57-59` | small | enabled by: proposal-ranking-verifies-unbounded-candidate-set |
| [tesseract-tsv-allocates-three-vectors-per-row](#tesseract-tsv-allocates-three-vectors-per-row) | advisory | `src/pdf/tesseract.rs:273-301` | small | related: ocr-cache-hit-spawns-version-probes |
| | | **Documented Agent Contract Surface** | | |
| [cli-schema-omits-runtime-errors](#cli-schema-omits-runtime-errors) | significant | `src/evidence.rs:206-224` | medium | related: doctor-json-shape-contradicts-schema |
| [custom-summary-template-inventory](#custom-summary-template-inventory) | significant | `src/cli.rs:969-984` | medium | — |
| [propose-multi-id-invalid-json](#propose-multi-id-invalid-json) | significant | `src/cli.rs:834-860` | small | — |
| [advisory-tokens-never-reported](#advisory-tokens-never-reported) | moderate | `.handoffs/2026-08-09-phase-2-complete.md:157-160` | small | — |
| [doctor-json-shape-contradicts-schema](#doctor-json-shape-contradicts-schema) | moderate | `src/cli.rs:445-472` | small | related: cli-schema-omits-runtime-errors |
| [vocabulary-leaks-in-validation-messages](#vocabulary-leaks-in-validation-messages) | moderate | `src/review.rs:104-137` | small | — |
| [missing-just-ci-entry-point](#missing-just-ci-entry-point) | moderate | `README.md:337-344` | trivial | — |

<sub>
Generated 2026-08-10 at commit 3c98f51. Intermediate artifacts:
recon.yaml and findings.yaml. Primary report: report.html.
</sub>

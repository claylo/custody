# Phase 3: `propose` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `propose` subcommand that emits ranked candidate locators for claims lacking evidence, deterministically — no model, no randomness, stable ordering.

**Architecture:** New `src/propose.rs` module with the candidate generation algorithm (sentence splitting, scoring, PDF verification, ranking). CLI subcommand in `src/cli.rs` with both human-readable commented-YAML and JSON output. Reuses `tokens::extract` for required-token extraction and `parse_units` for Markdown parsing.

**Tech Stack:** Rust, existing deps only. Reuses `tokens`, `markdown`, `normalize`, `pdf` modules.

**Spec reference:** `record/superpowers/specs/2026-07-30-multi-source-and-support-design.md`, Phase 3 section (lines 352–434).

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/propose.rs` | Core algorithm: sentence splitting, candidate generation, scoring, PDF verification, ranking |
| Modify | `src/lib.rs` | Register `propose` module |
| Modify | `src/cli.rs` | `Propose` subcommand, args, output formatting |
| Create | `tests/propose.rs` | Unit tests for sentence splitting, candidate generation, scoring, ranking |
| Modify | `tests/cli.rs` | Integration test for the CLI command |

---

### Task 1: Sentence splitting and data types

Create `src/propose.rs` with the output data types and sentence splitting logic.

**Files:**
- Create: `src/propose.rs`
- Modify: `src/lib.rs`
- Create: `tests/propose.rs`

- [ ] **Step 1: Create `src/propose.rs` with data types**

```rust
use serde::Serialize;

use crate::evidence::PdfBackend;
use crate::markdown::UnitKind;

/// One candidate locator for a claim.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub source: String,
    pub exact: String,
    pub coverage: CoverageScore,
    pub markdown: MarkdownMatch,
    pub pdf: Option<PdfMatch>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverageScore {
    pub matched: usize,
    pub required: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarkdownMatch {
    pub line: usize,
    pub column: usize,
    pub unit: UnitKind,
    pub section: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PdfMatch {
    pub page: usize,
    pub backend: PdfBackend,
}

/// Proposal for one claim.
#[derive(Debug, Clone, Serialize)]
pub struct ClaimProposal {
    pub claim: usize,
    pub required_tokens: Vec<String>,
    pub uncovered_tokens: Vec<String>,
    pub candidates: Vec<Candidate>,
}

/// Full proposal report for one summary document.
#[derive(Debug, Clone, Serialize)]
pub struct ProposalReport {
    pub id: String,
    pub claims: Vec<ClaimProposal>,
}
```

- [ ] **Step 2: Implement sentence splitting**

The spec says: "Sentences split on terminal punctuation followed by whitespace." The split is deliberately naive.

```rust
use crate::normalize::normalize;

/// Split text into sentence spans.
///
/// A sentence boundary is terminal punctuation (`.`, `!`, `?`) followed by
/// whitespace. Deliberately naive: `et al.`, `Fig. 3`, and similar will
/// mis-split, which is acceptable because every candidate is verified.
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut start = 0;

    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'.' | b'!' | b'?') {
            // Look for whitespace after the punctuation
            let after = i + 1;
            if after < bytes.len() && bytes[after].is_ascii_whitespace() {
                let raw = &text[start..=i];
                let normalized = normalize(raw);
                if !normalized.is_empty() {
                    sentences.push(normalized);
                }
                start = after;
                // Skip the whitespace
                i = after;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                start = i;
                continue;
            }
        }
        i += 1;
    }

    // Remaining text after last sentence boundary
    if start < text.len() {
        let normalized = normalize(&text[start..]);
        if !normalized.is_empty() {
            sentences.push(normalized);
        }
    }

    sentences
}
```

- [ ] **Step 3: Register module in `src/lib.rs`**

Add `pub mod propose;` to `src/lib.rs`.

- [ ] **Step 4: Write sentence splitting tests**

Create `tests/propose.rs`:

```rust
use receipts::propose::split_sentences;

#[test]
fn splits_on_terminal_punctuation_followed_by_whitespace() {
    let sentences = split_sentences("First sentence. Second sentence. Third.");
    assert_eq!(sentences, ["First sentence.", "Second sentence.", "Third."]);
}

#[test]
fn does_not_split_on_decimal_numbers() {
    let sentences = split_sentences("The value was 0.05 in the control group.");
    assert_eq!(sentences, ["The value was 0.05 in the control group."]);
}

#[test]
fn splits_on_exclamation_and_question() {
    let sentences = split_sentences("Really? Yes! Confirmed.");
    assert_eq!(sentences, ["Really?", "Yes!", "Confirmed."]);
}

#[test]
fn handles_single_sentence() {
    let sentences = split_sentences("Just one sentence");
    assert_eq!(sentences, ["Just one sentence"]);
}

#[test]
fn handles_empty_input() {
    let sentences = split_sentences("");
    assert!(sentences.is_empty());
}

#[test]
fn normalizes_whitespace_within_sentences() {
    let sentences = split_sentences("Spread   out. Tight.");
    assert_eq!(sentences, ["Spread out.", "Tight."]);
}

#[test]
fn abbreviation_with_following_capital_splits() {
    // This IS a mis-split — the spec says that's acceptable
    let sentences = split_sentences("et al. Found the result.");
    assert_eq!(sentences.len(), 2);
}
```

- [ ] **Step 5: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 6: Commit**

```
feat(propose): add data types and sentence splitting

Sentence boundaries are terminal punctuation followed by whitespace.
Deliberately naive splitting is acceptable because every candidate
is verified against both sources.
```

---

### Task 2: Candidate generation and scoring

Build candidate spans from Markdown units, score by token coverage, filter by uniqueness within the unit.

**Files:**
- Modify: `src/propose.rs`
- Modify: `tests/propose.rs`

- [ ] **Step 1: Add candidate generation**

In `src/propose.rs`, add a function that generates scored candidates from a set of Markdown units for one source:

```rust
use crate::markdown::{MarkdownUnit, exact_count};
use crate::tokens::{self, ExtractedTokens};

/// A raw candidate before PDF verification.
#[derive(Debug, Clone)]
struct RawCandidate {
    source: String,
    exact: String,
    matched_count: usize,
    required_count: usize,
    line: usize,
    column: usize,
    unit: UnitKind,
    section: Vec<String>,
}

/// Generate scored candidates from one source's Markdown units.
fn generate_candidates(
    source_name: &str,
    units: &[MarkdownUnit],
    tokens: &ExtractedTokens,
) -> Vec<RawCandidate> {
    let mut candidates = Vec::new();

    for unit in units {
        let spans = candidate_spans(&unit.text);
        for span in &spans {
            // Must occur exactly once within the unit
            if exact_count(&unit.text, span) != 1 {
                continue;
            }
            let matched = count_covered_tokens(span, &tokens.required);
            if matched == 0 {
                continue;
            }
            candidates.push(RawCandidate {
                source: source_name.to_owned(),
                exact: span.clone(),
                matched_count: matched,
                required_count: tokens.required.len(),
                line: unit.line,
                column: unit.column,
                unit: unit.kind,
                section: unit.section.clone(),
            });
        }
    }

    candidates
}

/// Build candidate spans from a unit's text.
///
/// Each sentence is a candidate, and the whole unit text is a fallback
/// candidate (unless the unit is a single sentence).
fn candidate_spans(text: &str) -> Vec<String> {
    let sentences = split_sentences(text);
    let mut spans = sentences.clone();
    // Add the whole unit as a fallback if it produced multiple sentences
    if sentences.len() > 1 {
        spans.push(text.to_owned());
    }
    spans
}

/// Count how many required tokens are covered by a candidate span.
fn count_covered_tokens(span: &str, required: &[String]) -> usize {
    required
        .iter()
        .filter(|token| {
            if tokens::is_number_word(token) {
                tokens::is_covered_case_insensitive(span, token)
            } else {
                tokens::is_covered(span, token)
            }
        })
        .count()
}
```

- [ ] **Step 2: Write candidate generation tests**

In `tests/propose.rs`, add:

```rust
use receipts::markdown::{UnitKind, parse_units};
use receipts::propose::{split_sentences, propose_candidates};

// Note: propose_candidates is the public test helper we'll expose.
// For now, test through the full propose flow in Task 4.
// Here we focus on split_sentences and candidate_spans behavior.

#[test]
fn whole_unit_is_fallback_when_multiple_sentences() {
    let sentences = split_sentences("Alpha result. Beta result.");
    assert_eq!(sentences.len(), 2);
    // The whole unit text would also be a candidate (tested in integration)
}
```

We'll add more comprehensive tests in Task 5 when the full `propose_document` function exists.

- [ ] **Step 3: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 4: Commit**

```
feat(propose): candidate generation with token scoring

Each sentence within a unit is a candidate, plus the whole unit as
fallback. Candidates are scored by distinct required-token coverage
and filtered to those occurring exactly once in their unit.
```

---

### Task 3: PDF verification and ranking

Verify candidates against native PDF extraction. Rank by token count descending, span length ascending, source name, line, column.

**Files:**
- Modify: `src/propose.rs`

- [ ] **Step 1: Add PDF verification**

```rust
use std::collections::HashMap;
use std::path::Path;

use crate::normalize::normalize;
use crate::pdf::PdfTextProvider;

/// Verify a candidate against native PDF pages.
///
/// Returns the page number if the candidate appears on exactly one page.
/// Returns None for zero or multiple matches.
fn verify_pdf(
    normalized_pages: &HashMap<usize, String>,
    exact: &str,
) -> Option<usize> {
    let matches: Vec<usize> = normalized_pages
        .iter()
        .filter(|(_, text)| exact_count(text, exact) > 0)
        .map(|(page, _)| *page)
        .collect();
    match matches.as_slice() {
        [page] => Some(*page),
        _ => None,
    }
}
```

- [ ] **Step 2: Add ranking**

```rust
/// Rank candidates: token count desc, span length asc, source name asc,
/// line asc, column asc. Fully ordered for reproducible output.
fn rank_candidates(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| {
        b.coverage
            .matched
            .cmp(&a.coverage.matched)
            .then_with(|| a.exact.len().cmp(&b.exact.len()))
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.markdown.line.cmp(&b.markdown.line))
            .then_with(|| a.markdown.column.cmp(&b.markdown.column))
    });
}
```

- [ ] **Step 3: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 4: Commit**

```
feat(propose): PDF verification and deterministic ranking

Candidates are verified against native PDF extraction — only
offered when found on exactly one page. Ranking is fully ordered:
token count desc, span length asc, source name, line, column.
```

---

### Task 4: Top-level `propose_document` function

Wire everything together: load sources, parse Markdown, extract tokens, generate candidates, verify PDF, rank, limit.

**Files:**
- Modify: `src/propose.rs`
- Modify: `tests/propose.rs`

- [ ] **Step 1: Implement `propose_document`**

```rust
use std::collections::BTreeMap;

use anyhow::Result;

use crate::corpus::Corpus;
use crate::evidence::SummaryDocument;

/// Generate candidate locators for claims in a summary document.
///
/// Processes claims that lack evidence entries, or all claims if
/// `all_claims` is true. Returns at most `max_candidates` per claim.
pub fn propose_document(
    corpus: &Corpus,
    summary: &SummaryDocument,
    provider: &impl PdfTextProvider,
    max_candidates: usize,
    all_claims: bool,
) -> Result<ProposalReport> {
    let evidence = summary.evidence.as_ref();

    // Which claim indices already have locators?
    let has_evidence: std::collections::BTreeSet<usize> = evidence
        .map(|ev| {
            ev.claims
                .iter()
                .filter(|e| !e.locators.is_empty())
                .map(|e| e.claim)
                .collect()
        })
        .unwrap_or_default();

    // Resolve and parse each source's Markdown, once.
    let source_names = corpus.source_names();
    let mut source_units: BTreeMap<String, Vec<MarkdownUnit>> = BTreeMap::new();
    let mut source_pdf_pages: BTreeMap<String, HashMap<usize, String>> = BTreeMap::new();

    for source_name in &source_names {
        // Parse Markdown
        let markdown_candidates = corpus.markdown_candidates_for(&summary.id, source_name)?;
        let markdown_path = markdown_candidates
            .iter()
            .find(|p| p.is_file())
            .unwrap_or(&markdown_candidates[0]);
        if let Ok(markdown_source) = std::fs::read_to_string(markdown_path) {
            source_units.insert(source_name.clone(), parse_units(&markdown_source));
        }

        // Extract all native PDF pages once
        let pdf_path = corpus.pdf_path_for(&summary.id, source_name)?;
        if pdf_path.is_file() {
            if let Ok(pages) = provider.native_pages(&pdf_path, None) {
                let normalized: HashMap<usize, String> = pages
                    .into_iter()
                    .map(|p| (p.page, normalize(&p.text)))
                    .collect();
                source_pdf_pages.insert(source_name.clone(), normalized);
            }
        }
    }

    // Process each claim
    let mut claim_proposals = Vec::new();
    for (index, claim_text) in summary.claims.iter().enumerate() {
        if !all_claims && has_evidence.contains(&index) {
            continue;
        }

        let extracted = tokens::extract(claim_text);
        let mut all_candidates = Vec::new();

        for (source_name, units) in &source_units {
            let raw = generate_candidates(source_name, units, &extracted);
            let pdf_pages = source_pdf_pages.get(source_name.as_str());

            for raw_candidate in raw {
                let pdf = pdf_pages
                    .and_then(|pages| verify_pdf(pages, &raw_candidate.exact))
                    .map(|page| PdfMatch {
                        page,
                        backend: PdfBackend::MutoolNative,
                    });

                all_candidates.push(Candidate {
                    source: raw_candidate.source,
                    exact: raw_candidate.exact,
                    coverage: CoverageScore {
                        matched: raw_candidate.matched_count,
                        required: raw_candidate.required_count,
                    },
                    markdown: MarkdownMatch {
                        line: raw_candidate.line,
                        column: raw_candidate.column,
                        unit: raw_candidate.unit,
                        section: raw_candidate.section,
                    },
                    pdf,
                });
            }
        }

        // Only keep candidates that have a PDF match
        all_candidates.retain(|c| c.pdf.is_some());
        rank_candidates(&mut all_candidates);
        all_candidates.truncate(max_candidates);

        // Compute uncovered tokens
        let uncovered: Vec<String> = extracted
            .required
            .iter()
            .filter(|token| {
                !all_candidates.iter().any(|c| {
                    if tokens::is_number_word(token) {
                        tokens::is_covered_case_insensitive(&c.exact, token)
                    } else {
                        tokens::is_covered(&c.exact, token)
                    }
                })
            })
            .cloned()
            .collect();

        claim_proposals.push(ClaimProposal {
            claim: index,
            required_tokens: extracted.required,
            uncovered_tokens: uncovered,
            candidates: all_candidates,
        });
    }

    Ok(ProposalReport {
        id: summary.id.clone(),
        claims: claim_proposals,
    })
}
```

- [ ] **Step 2: Write unit tests for `propose_document`**

In `tests/propose.rs`, add tests using a `FakePdf` provider (same pattern as `tests/validate.rs`):

```rust
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};

use receipts::corpus::Corpus;
use receipts::evidence::{
    ClaimEvidence, DEFAULT_SOURCE, Evidence, Locator, MarkdownLocator, PdfBackend, PdfLocator,
    SourcePair, SourceRecord, SummaryDocument,
};
use receipts::hash::sha256_bytes;
use receipts::markdown::UnitKind;
use receipts::pdf::{ExtractedPage, PdfTextProvider};
use receipts::propose::propose_document;

struct FakePdf {
    pages: Vec<ExtractedPage>,
}

impl PdfTextProvider for FakePdf {
    fn native_pages(&self, _pdf: &Path, page: Option<usize>) -> Result<Vec<ExtractedPage>> {
        match page {
            None => Ok(self.pages.clone()),
            Some(p) => Ok(self.pages.iter().filter(|pg| pg.page == p).cloned().collect()),
        }
    }

    fn ocr_page(&self, _pdf: &Path, _sha256: &str, _page: usize) -> Result<ExtractedPage> {
        bail!("OCR not used by propose")
    }
}

#[test]
fn proposes_candidates_for_claims_without_evidence() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("md/smith-2019")).unwrap();
    fs::create_dir_all(temp.path().join("pdfs")).unwrap();
    fs::write(temp.path().join("receipts.yaml"), "cache:\n  root: \".cache/pdf-text\"\n").unwrap();
    let markdown = "The rate was 67.5% in the control group.\n";
    fs::write(temp.path().join("md/smith-2019/smith-2019.md"), markdown).unwrap();
    fs::write(temp.path().join("pdfs/smith-2019.pdf"), b"fake").unwrap();
    let corpus = Corpus::discover_from(temp.path(), None).unwrap();
    let summary = SummaryDocument {
        id: "smith-2019".to_owned(),
        claims: vec!["The rate was 67.5% in controls.".to_owned()],
        evidence: None,
    };

    let report = propose_document(
        &corpus,
        &summary,
        &FakePdf {
            pages: vec![ExtractedPage {
                page: 1,
                text: "The rate was 67.5% in the control group.".to_owned(),
                spans: Vec::new(),
                mean_confidence: None,
            }],
        },
        3,
        false,
    )
    .unwrap();

    assert_eq!(report.claims.len(), 1);
    assert_eq!(report.claims[0].claim, 0);
    assert!(!report.claims[0].candidates.is_empty());
    // The best candidate should cover "67.5%"
    assert!(report.claims[0].candidates[0].coverage.matched > 0);
}

#[test]
fn skips_claims_with_existing_evidence_unless_all() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("md/smith-2019")).unwrap();
    fs::create_dir_all(temp.path().join("pdfs")).unwrap();
    fs::write(temp.path().join("receipts.yaml"), "cache:\n  root: \".cache/pdf-text\"\n").unwrap();
    let markdown = "Supported once.\n";
    fs::write(temp.path().join("md/smith-2019/smith-2019.md"), markdown).unwrap();
    fs::write(temp.path().join("pdfs/smith-2019.pdf"), b"fake").unwrap();
    let corpus = Corpus::discover_from(temp.path(), None).unwrap();
    let claim = "Claim with 5 things.".to_owned();
    let summary = SummaryDocument {
        id: "smith-2019".to_owned(),
        claims: vec![claim.clone()],
        evidence: Some(Evidence {
            sources: BTreeMap::from([(
                DEFAULT_SOURCE.to_owned(),
                SourcePair {
                    markdown: SourceRecord {
                        source: "md/smith-2019/smith-2019.md".to_owned(),
                        sha256: sha256_bytes(markdown.as_bytes()),
                    },
                    pdf: SourceRecord {
                        source: "pdfs/smith-2019.pdf".to_owned(),
                        sha256: sha256_bytes(b"fake"),
                    },
                },
            )]),
            claims: vec![ClaimEvidence {
                claim: 0,
                claim_sha256: sha256_bytes(claim.as_bytes()),
                locators: vec![Locator {
                    source: DEFAULT_SOURCE.to_owned(),
                    exact: "Supported once".to_owned(),
                    markdown: MarkdownLocator {
                        line: 1, column: 1, unit: UnitKind::Paragraph, section: vec![],
                    },
                    pdf: PdfLocator { page: 1, backend: PdfBackend::MutoolNative },
                }],
            }],
        }),
    };

    let fake = FakePdf {
        pages: vec![ExtractedPage {
            page: 1, text: "Supported once.".to_owned(),
            spans: Vec::new(), mean_confidence: None,
        }],
    };

    let without_all = propose_document(&corpus, &summary, &fake, 3, false).unwrap();
    assert!(without_all.claims.is_empty());

    let with_all = propose_document(&corpus, &summary, &fake, 3, true).unwrap();
    assert_eq!(with_all.claims.len(), 1);
}

#[test]
fn ranking_is_deterministic() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("md/smith-2019")).unwrap();
    fs::create_dir_all(temp.path().join("pdfs")).unwrap();
    fs::write(temp.path().join("receipts.yaml"), "cache:\n  root: \".cache/pdf-text\"\n").unwrap();
    let markdown = "Found 3 cases. Also 3 cases reported here.\n";
    fs::write(temp.path().join("md/smith-2019/smith-2019.md"), markdown).unwrap();
    fs::write(temp.path().join("pdfs/smith-2019.pdf"), b"fake").unwrap();
    let corpus = Corpus::discover_from(temp.path(), None).unwrap();
    let summary = SummaryDocument {
        id: "smith-2019".to_owned(),
        claims: vec!["There were 3 total cases.".to_owned()],
        evidence: None,
    };

    let fake = FakePdf {
        pages: vec![ExtractedPage {
            page: 1, text: markdown.to_owned(),
            spans: Vec::new(), mean_confidence: None,
        }],
    };

    let report1 = propose_document(&corpus, &summary, &fake, 10, false).unwrap();
    let report2 = propose_document(&corpus, &summary, &fake, 10, false).unwrap();

    let json1 = serde_json::to_string(&report1).unwrap();
    let json2 = serde_json::to_string(&report2).unwrap();
    assert_eq!(json1, json2, "ranking must be deterministic");
}

#[test]
fn propose_never_modifies_files() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("md/smith-2019")).unwrap();
    fs::create_dir_all(temp.path().join("pdfs")).unwrap();
    fs::write(temp.path().join("receipts.yaml"), "cache:\n  root: \".cache/pdf-text\"\n").unwrap();
    let markdown = "The result was 42.8% effective.\n";
    fs::write(temp.path().join("md/smith-2019/smith-2019.md"), markdown).unwrap();
    fs::write(temp.path().join("pdfs/smith-2019.pdf"), b"fake").unwrap();
    let corpus = Corpus::discover_from(temp.path(), None).unwrap();

    // Hash everything before
    let md_before = fs::read(temp.path().join("md/smith-2019/smith-2019.md")).unwrap();
    let pdf_before = fs::read(temp.path().join("pdfs/smith-2019.pdf")).unwrap();
    let config_before = fs::read(temp.path().join("receipts.yaml")).unwrap();

    let summary = SummaryDocument {
        id: "smith-2019".to_owned(),
        claims: vec!["Effectiveness was 42.8% overall.".to_owned()],
        evidence: None,
    };

    let _ = propose_document(
        &corpus,
        &summary,
        &FakePdf {
            pages: vec![ExtractedPage {
                page: 1, text: markdown.to_owned(),
                spans: Vec::new(), mean_confidence: None,
            }],
        },
        3,
        false,
    );

    assert_eq!(fs::read(temp.path().join("md/smith-2019/smith-2019.md")).unwrap(), md_before);
    assert_eq!(fs::read(temp.path().join("pdfs/smith-2019.pdf")).unwrap(), pdf_before);
    assert_eq!(fs::read(temp.path().join("receipts.yaml")).unwrap(), config_before);
}
```

- [ ] **Step 3: Make necessary items public**

The `propose_document` function and all output types (`ProposalReport`, `ClaimProposal`, `Candidate`, `CoverageScore`, `MarkdownMatch`, `PdfMatch`) must be `pub`. Internal helpers (`generate_candidates`, `verify_pdf`, `rank_candidates`, `candidate_spans`, `count_covered_tokens`, `RawCandidate`) stay private.

`split_sentences` should be `pub` for its unit tests.

- [ ] **Step 4: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 5: Commit**

```
feat(propose): deterministic candidate locator generation

propose_document loads sources once, generates candidate spans,
verifies against native PDF, and returns ranked results. No model,
no randomness, stable ordering. Never writes to any file.
```

---

### Task 5: CLI subcommand and output formatting

Add the `Propose` subcommand to the CLI with both JSON and human-readable commented-YAML output.

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Add `ProposeArgs` and `Propose` variant**

```rust
#[derive(Debug, Subcommand)]
enum Command {
    /// Probe corpus paths and external PDF tools.
    Doctor,
    /// Generate one YAML-ready exact evidence record.
    Locate(LocateArgs),
    /// Validate selected summaries, or every evidence-bearing summary.
    Check(CheckArgs),
    /// Inventory valid, missing, and invalid evidence records.
    Audit(AuditArgs),
    /// Suggest candidate locators for claims lacking evidence.
    Propose(ProposeArgs),
}

#[derive(Debug, Args)]
struct ProposeArgs {
    ids: Vec<String>,
    /// Process all claims, not just those without evidence.
    #[arg(long)]
    all: bool,
    /// Maximum candidates per claim.
    #[arg(long, default_value = "3")]
    candidates: usize,
}
```

- [ ] **Step 2: Add dispatch in `run()`**

```rust
Command::Propose(args) => propose_cmd(&corpus, &args, json, quiet),
```

- [ ] **Step 3: Implement `propose_cmd`**

```rust
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
        let report = crate::propose::propose_document(
            corpus,
            &summary,
            &tools,
            args.candidates,
            args.all,
        )?;

        if json {
            print_json(&report)?;
        } else if !quiet {
            print_propose_human(&report, &summary, corpus.terms());
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Implement human-readable output**

The spec defines commented YAML output:

```rust
fn print_propose_human(
    report: &crate::propose::ProposalReport,
    summary: &crate::evidence::SummaryDocument,
    terms: &Terms,
) {
    for proposal in &report.claims {
        let claim_text = summary
            .claims
            .get(proposal.claim)
            .map(String::as_str)
            .unwrap_or("<unknown>");
        let truncated = if claim_text.len() > 72 {
            format!("{}...", &claim_text[..72])
        } else {
            claim_text.to_owned()
        };
        println!(
            "# {} {}  {:?}",
            terms.claim, proposal.claim, truncated
        );
        if !proposal.required_tokens.is_empty() {
            println!(
                "#   required tokens: {}",
                proposal.required_tokens.join(", ")
            );
        }
        if !proposal.uncovered_tokens.is_empty() {
            println!(
                "#   uncovered tokens: {}",
                proposal.uncovered_tokens.join(", ")
            );
        }

        for (i, candidate) in proposal.candidates.iter().enumerate() {
            let label = (b'a' + i as u8) as char;
            let section_str = if candidate.markdown.section.is_empty() {
                String::new()
            } else {
                format!(" [{}]", candidate.markdown.section.join(" > "))
            };
            let pdf_str = if let Some(ref pdf) = candidate.pdf {
                format!("  p.{} {}", pdf.page, pdf.backend.as_str())
            } else {
                "  (no native PDF match)".to_owned()
            };
            println!(
                "#   [{}] {}/{}  {}  md:{}:{} {:?}{}{}",
                label,
                candidate.coverage.matched,
                candidate.coverage.required,
                candidate.source,
                candidate.markdown.line,
                candidate.markdown.column,
                candidate.markdown.unit,
                section_str,
                pdf_str,
            );
            println!("#       {:?}", candidate.exact);
        }

        // Print accept command for the first candidate
        if let Some(first) = proposal.candidates.first() {
            if let Some(ref pdf) = first.pdf {
                println!("#");
                println!("#   accept [a]:");
                println!(
                    "#     receipts locate {} --{} {} \\",
                    report.id, terms.claim, proposal.claim
                );
                println!(
                    "#       --exact {:?} --page {}",
                    first.exact, pdf.page
                );
            }
        }
        println!();
    }
}
```

- [ ] **Step 5: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 6: Commit**

```
feat(cli): add propose subcommand with human and JSON output

receipts propose [ID...] [--all] [--candidates N] [--json]
emits ranked candidate locators for claims lacking evidence.
Human-readable output is commented YAML safe to paste and edit.
```

---

### Task 6: CLI integration test

Add an integration test that exercises `propose` through the CLI binary.

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add CLI integration test**

Following the pattern of existing CLI tests in `tests/cli.rs`, add a test that creates a temp corpus with a summary that has no evidence, runs `propose`, and verifies the output.

```rust
#[test]
fn propose_emits_candidates_for_claims_without_evidence() {
    // Create temp corpus with a summary with claims but no evidence
    // Run `receipts propose --format json <id>`
    // Verify JSON output has candidates
}
```

The existing CLI tests use `assert_cmd` or direct `Command` invocation — follow whichever pattern is already established.

- [ ] **Step 2: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 3: Commit**

```
test(cli): add propose integration test
```

---

### Task 7: Final verification and handoff

**Files:**
- Create: `.handoffs/2026-08-09-phase-3-complete.md`

- [ ] **Step 1: Run full verification suite**

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --locked
cargo test --locked
cargo run --locked -- --format text doctor
```

All must pass.

- [ ] **Step 2: Write handoff document**

- [ ] **Step 3: Commit the handoff**

```
docs: add handoff for Phase 3 completion
```

---

## Verification

After all tasks:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --locked
cargo test --locked
cargo run --locked -- --format text doctor
```

Expected: new `propose` subcommand works end-to-end with deterministic output. No files modified by propose. All existing tests still pass.

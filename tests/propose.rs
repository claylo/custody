use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use receipts::{
    coordinate::{ClaimIndex, Column, Line, Page},
    corpus::Corpus,
    evidence::{
        ClaimEvidence, DEFAULT_SOURCE, Evidence, Locator, MarkdownLocator, PdfBackend, PdfLocator,
        SourcePair, SourceRecord, SummaryDocument,
    },
    hash::sha256_bytes,
    markdown::UnitKind,
    pdf::{ExtractedPage, PdfTextProvider},
    propose::{propose_document, split_sentences},
};

#[test]
fn splits_on_terminal_punctuation_followed_by_whitespace() {
    assert_eq!(
        split_sentences("First sentence. Second sentence. Third."),
        ["First sentence.", "Second sentence.", "Third."]
    );
}

#[test]
fn does_not_split_on_decimal_numbers() {
    assert_eq!(
        split_sentences("The value was 0.05 in the control group."),
        ["The value was 0.05 in the control group."]
    );
}

#[test]
fn splits_on_exclamation_and_question() {
    assert_eq!(
        split_sentences("Really? Yes! Confirmed."),
        ["Really?", "Yes!", "Confirmed."]
    );
}

#[test]
fn handles_single_sentence() {
    assert_eq!(split_sentences("Just one sentence"), ["Just one sentence"]);
}

#[test]
fn handles_empty_input() {
    assert!(split_sentences("").is_empty());
}

#[test]
fn normalizes_whitespace_within_sentences() {
    assert_eq!(
        split_sentences("Spread   out.\n Tight."),
        ["Spread out.", "Tight."]
    );
}

#[test]
fn abbreviation_with_following_capital_splits() {
    // A mis-split, and an accepted one: verification drops what does not hold.
    assert_eq!(
        split_sentences("Reported by Smith et al. Found in three cohorts."),
        ["Reported by Smith et al.", "Found in three cohorts."]
    );
}

#[test]
fn proposes_candidates_for_claims_without_evidence() {
    let markdown = "The rate was 67.5% in the control group.\n";
    let fixture = Fixture::new(markdown);

    let report = propose_document(
        &fixture.corpus,
        &summary(&["The rate was 67.5% in controls."]),
        &FakePdf {
            pages: vec![page(1, markdown)],
        },
        3,
        false,
    )
    .unwrap();

    assert_eq!(report.id, "smith-2019");
    assert_eq!(report.claims.len(), 1);
    let proposal = &report.claims[0];
    assert_eq!(proposal.claim, 0);
    assert_eq!(proposal.required_tokens, ["67.5%"]);
    assert!(proposal.uncovered_tokens.is_empty());

    let best = &proposal.candidates[0];
    assert_eq!(best.exact, "The rate was 67.5% in the control group.");
    assert_eq!(best.source, DEFAULT_SOURCE);
    assert_eq!(best.coverage.matched, 1);
    assert_eq!(best.coverage.required, 1);
    assert_eq!(best.markdown.line, 1);
    assert_eq!(best.markdown.unit, UnitKind::Paragraph);
    let pdf = best.pdf.as_ref().unwrap();
    assert_eq!(pdf.page, 1);
    assert_eq!(pdf.backend, PdfBackend::MutoolNative);

    let accepted = Locator {
        source: best.source.clone(),
        exact: best.exact.clone(),
        markdown: best.markdown.clone(),
        pdf: best.pdf.clone().unwrap(),
    };
    assert_eq!(accepted.markdown.line, 1);
}

#[test]
fn shorter_span_outranks_the_whole_unit_at_equal_coverage() {
    let markdown = "Enrollment reached 240 patients. Follow-up ran for two years.\n";
    let fixture = Fixture::new(markdown);

    let report = propose_document(
        &fixture.corpus,
        &summary(&["The trial enrolled 240 patients."]),
        &FakePdf {
            pages: vec![page(1, markdown)],
        },
        10,
        false,
    )
    .unwrap();

    let candidates = &report.claims[0].candidates;
    assert_eq!(candidates[0].exact, "Enrollment reached 240 patients.");
    // The sentence covering nothing is dropped; the whole unit stays as a
    // longer fallback that covers the same token.
    assert_eq!(
        candidates
            .iter()
            .map(|c| c.exact.as_str())
            .collect::<Vec<_>>(),
        [
            "Enrollment reached 240 patients.",
            "Enrollment reached 240 patients. Follow-up ran for two years."
        ]
    );
}

#[test]
fn candidates_absent_from_the_pdf_are_not_offered() {
    let markdown = "The rate was 67.5% in the control group.\n";
    let fixture = Fixture::new(markdown);

    let report = propose_document(
        &fixture.corpus,
        &summary(&["The rate was 67.5% in controls."]),
        &FakePdf {
            pages: vec![page(1, "An unrelated 67.5% figure appears here.")],
        },
        3,
        false,
    )
    .unwrap();

    assert!(report.claims[0].candidates.is_empty());
    assert_eq!(report.claims[0].uncovered_tokens, ["67.5%"]);
}

#[test]
fn a_span_on_two_pages_is_too_ambiguous_to_offer() {
    let markdown = "The rate was 67.5% in the control group.\n";
    let fixture = Fixture::new(markdown);

    let report = propose_document(
        &fixture.corpus,
        &summary(&["The rate was 67.5% in controls."]),
        &FakePdf {
            pages: vec![page(1, markdown), page(2, markdown)],
        },
        3,
        false,
    )
    .unwrap();

    assert!(report.claims[0].candidates.is_empty());
}

#[test]
fn skips_claims_with_existing_evidence_unless_all() {
    let markdown = "Supported once in 5 trials.\n";
    let fixture = Fixture::new(markdown);
    let summary = summary_with_evidence(markdown, "Claim with 5 things.");
    let provider = FakePdf {
        pages: vec![page(1, markdown)],
    };

    let skipped = propose_document(&fixture.corpus, &summary, &provider, 3, false).unwrap();
    assert!(skipped.claims.is_empty());

    let forced = propose_document(&fixture.corpus, &summary, &provider, 3, true).unwrap();
    assert_eq!(forced.claims.len(), 1);
    assert_eq!(forced.claims[0].claim, 0);
    assert!(!forced.claims[0].candidates.is_empty());
}

#[test]
fn ranking_is_deterministic() {
    let markdown = "Found 3 cases. Also 3 cases reported here.\n";
    let fixture = Fixture::new(markdown);
    let summary = summary(&["There were 3 total cases."]);
    let provider = FakePdf {
        pages: vec![page(1, markdown)],
    };

    let first = propose_document(&fixture.corpus, &summary, &provider, 10, false).unwrap();
    let second = propose_document(&fixture.corpus, &summary, &provider, 10, false).unwrap();

    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    assert_eq!(first.claims[0].candidates.len(), 3);
}

#[test]
fn max_candidates_truncates_after_ranking() {
    let markdown = "Found 3 cases. Also 3 cases reported here.\n";
    let fixture = Fixture::new(markdown);

    let report = propose_document(
        &fixture.corpus,
        &summary(&["There were 3 total cases."]),
        &FakePdf {
            pages: vec![page(1, markdown)],
        },
        1,
        false,
    )
    .unwrap();

    let candidates = &report.claims[0].candidates;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].exact, "Found 3 cases.");
}

#[test]
fn propose_never_modifies_files() {
    let markdown = "The result was 42.8% effective.\n";
    let fixture = Fixture::new(markdown);
    let before = snapshot(fixture.root());

    propose_document(
        &fixture.corpus,
        &summary(&["Effectiveness was 42.8% overall."]),
        &FakePdf {
            pages: vec![page(1, markdown)],
        },
        3,
        false,
    )
    .unwrap();

    assert_eq!(snapshot(fixture.root()), before);
}

struct FakePdf {
    pages: Vec<ExtractedPage>,
}

impl PdfTextProvider for FakePdf {
    fn native_pages(&self, _pdf: &Path, page: Option<Page>) -> Result<Vec<ExtractedPage>> {
        Ok(match page {
            None => self.pages.clone(),
            Some(number) => self
                .pages
                .iter()
                .filter(|page| page.page == number)
                .cloned()
                .collect(),
        })
    }

    fn ocr_page(&self, _pdf: &Path, _pdf_sha256: &str, _page: Page) -> Result<ExtractedPage> {
        bail!("proposal never falls back to OCR")
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    corpus: Corpus,
}

impl Fixture {
    // The cache is pinned inside the temp dir so the suite stays hermetic.
    fn new(markdown: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("md/smith-2019")).unwrap();
        fs::create_dir_all(temp.path().join("pdfs")).unwrap();
        fs::write(
            temp.path().join("receipts.yaml"),
            "cache:\n  root: \".cache/pdf-text\"\n",
        )
        .unwrap();
        fs::write(temp.path().join("md/smith-2019/smith-2019.md"), markdown).unwrap();
        fs::write(temp.path().join("pdfs/smith-2019.pdf"), b"fixture PDF").unwrap();
        let corpus = Corpus::discover_from(temp.path(), None).unwrap();
        Self {
            _temp: temp,
            corpus,
        }
    }

    fn root(&self) -> &Path {
        self.corpus.root()
    }
}

fn page(number: usize, text: &str) -> ExtractedPage {
    ExtractedPage {
        page: Page::new(number).unwrap(),
        text: text.to_owned(),
        spans: Vec::new(),
        mean_confidence: None,
    }
}

fn summary(claims: &[&str]) -> SummaryDocument {
    SummaryDocument {
        id: "smith-2019".to_owned(),
        claims: claims.iter().map(|claim| (*claim).to_owned()).collect(),
        evidence: None,
        review: None,
    }
}

fn summary_with_evidence(markdown: &str, claim: &str) -> SummaryDocument {
    let mut document = summary(&[claim]);
    document.evidence = Some(Evidence {
        sources: BTreeMap::from([(
            DEFAULT_SOURCE.to_owned(),
            SourcePair {
                markdown: SourceRecord {
                    source: "md/smith-2019/smith-2019.md".to_owned(),
                    sha256: sha256_bytes(markdown.as_bytes()),
                },
                pdf: SourceRecord {
                    source: "pdfs/smith-2019.pdf".to_owned(),
                    sha256: sha256_bytes(b"fixture PDF"),
                },
            },
        )]),
        claims: vec![ClaimEvidence {
            claim: ClaimIndex::new(0),
            claim_sha256: sha256_bytes(claim.as_bytes()),
            locators: vec![Locator {
                source: DEFAULT_SOURCE.to_owned(),
                exact: "Supported once".to_owned(),
                markdown: MarkdownLocator {
                    line: Line::new(1).unwrap(),
                    column: Column::new(1).unwrap(),
                    unit: UnitKind::Paragraph,
                    section: Vec::new(),
                },
                pdf: PdfLocator {
                    page: Page::new(1).unwrap(),
                    backend: PdfBackend::MutoolNative,
                },
            }],
        }],
    });
    document
}

/// Every file under `root`, keyed by its path relative to it.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];

    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                files.insert(relative, fs::read(&path).unwrap());
            }
        }
    }

    files
}

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fs,
    os::unix::fs::symlink,
    path::Path,
};

use anyhow::{Result, bail};
use receipts::{
    coordinate::{ClaimIndex, Column, Line, Page},
    corpus::Corpus,
    evidence::{
        ClaimEvidence, DEFAULT_SOURCE, Evidence, Locator, MarkdownLocator, PdfBackend, PdfLocator,
        Severity, SourcePair, SourceRecord, SummaryDocument,
    },
    hash::{sha256_bytes, sha256_file},
    markdown::{UnitKind, parse_units},
    pdf::{ExtractedPage, PdfTextProvider},
    review::{Review, ReviewEntry, Verdict, evidence_sha256},
    validate::validate_document,
};

#[derive(Default)]
struct FakePdf {
    pages: HashMap<(PdfBackend, usize), String>,
}

impl PdfTextProvider for FakePdf {
    fn native_pages(&self, _pdf: &Path, page: Option<Page>) -> Result<Vec<ExtractedPage>> {
        let page = page.expect("validation requests one page");
        Ok(vec![ExtractedPage {
            page,
            text: self
                .pages
                .get(&(PdfBackend::MutoolNative, page.get()))
                .cloned()
                .unwrap_or_default(),
            spans: Vec::new(),
            mean_confidence: None,
        }])
    }

    fn ocr_page(&self, _pdf: &Path, _pdf_sha256: &str, page: Page) -> Result<ExtractedPage> {
        let Some(text) = self.pages.get(&(PdfBackend::TesseractOcr, page.get())) else {
            bail!("missing fake OCR page {page}");
        };
        Ok(ExtractedPage {
            page,
            text: text.clone(),
            spans: Vec::new(),
            mean_confidence: None,
        })
    }
}

/// Resolves pages by PDF file name, so a locator sent to the wrong source's
/// PDF fails instead of silently matching.
struct PerSourcePdf {
    pages: HashMap<(String, usize), String>,
}

impl PdfTextProvider for PerSourcePdf {
    fn native_pages(&self, pdf: &Path, page: Option<Page>) -> Result<Vec<ExtractedPage>> {
        let page = page.expect("validation requests one page");
        let name = pdf.file_name().unwrap().to_string_lossy().into_owned();
        let Some(text) = self.pages.get(&(name.clone(), page.get())) else {
            bail!("missing fake page {page} for {name}");
        };
        Ok(vec![ExtractedPage {
            page,
            text: text.clone(),
            spans: Vec::new(),
            mean_confidence: None,
        }])
    }

    fn ocr_page(&self, _pdf: &Path, _pdf_sha256: &str, _page: Page) -> Result<ExtractedPage> {
        bail!("OCR extraction was not requested")
    }
}

#[test]
fn locator_requires_one_match_in_each_source() {
    let fixture = Fixture::new("Supported once in both sources.");
    let report = validate_document(
        &fixture.corpus,
        &fixture.summary("Supported once", PdfBackend::MutoolNative),
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "Supported once in the PDF.".to_owned(),
            )]),
        },
        false,
    );

    assert!(report.issues.is_empty(), "{:?}", report.issues);
}

#[test]
fn duplicate_pdf_matches_are_invalid() {
    let fixture = Fixture::new("Supported once in Markdown.");
    let report = validate_document(
        &fixture.corpus,
        &fixture.summary("Supported once", PdfBackend::MutoolNative),
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "Supported once, then Supported once again.".to_owned(),
            )]),
        },
        false,
    );

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "pdf_ambiguous")
    );
}

#[test]
fn native_validation_batches_multiple_cited_pages() {
    struct CountingPdf {
        calls: RefCell<Vec<Option<Page>>>,
    }

    impl PdfTextProvider for CountingPdf {
        fn native_pages(&self, _pdf: &Path, page: Option<Page>) -> Result<Vec<ExtractedPage>> {
            self.calls.borrow_mut().push(page);
            let pages = page.map_or_else(
                || vec![Page::new(1).unwrap(), Page::new(2).unwrap()],
                |page| vec![page],
            );
            Ok(pages
                .into_iter()
                .map(|page| ExtractedPage {
                    page,
                    text: "Supported once.".to_owned(),
                    spans: Vec::new(),
                    mean_confidence: None,
                })
                .collect())
        }

        fn ocr_page(&self, _pdf: &Path, _pdf_sha256: &str, _page: Page) -> Result<ExtractedPage> {
            bail!("OCR extraction was not requested")
        }
    }

    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    let second = {
        let first = &summary.evidence.as_ref().unwrap().claims[0].locators[0];
        let mut second = first.clone();
        second.pdf.page = Page::new(2).unwrap();
        second
    };
    summary.evidence.as_mut().unwrap().claims[0]
        .locators
        .push(second);
    let provider = CountingPdf {
        calls: RefCell::new(Vec::new()),
    };

    let report = validate_document(&fixture.corpus, &summary, &provider, false);

    assert!(report.issues.is_empty(), "{:?}", report.issues);
    assert_eq!(*provider.calls.borrow(), [None]);
}

#[test]
fn locator_is_bounded_to_the_recorded_markdown_unit() {
    let fixture = Fixture::new("First unit.\n\nSupported once in a different unit.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    summary.evidence.as_mut().unwrap().claims[0].locators[0]
        .markdown
        .as_mut()
        .unwrap()
        .line = Line::new(1).unwrap();
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );

    let issue = report
        .issues
        .iter()
        .find(|issue| issue.code == "markdown_missing")
        .unwrap();
    assert!(issue.message.contains("recorded paragraph unit"));
}

#[test]
fn source_hash_mismatch_is_reported() {
    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    default_pair(&mut summary).pdf.sha256 = "0".repeat(64);
    let report = validate_document(&fixture.corpus, &summary, &FakePdf::default(), false);

    let issue = report
        .issues
        .iter()
        .find(|issue| issue.code == "source_hash_mismatch")
        .expect("source hash mismatch is reported with a stable code");
    assert_eq!(issue.source.as_deref(), Some("default"));
}

#[test]
fn actual_pdf_hash_not_recorded_hash_keys_ocr() {
    struct HashCheckingPdf {
        expected: String,
    }

    impl PdfTextProvider for HashCheckingPdf {
        fn native_pages(&self, _pdf: &Path, _page: Option<Page>) -> Result<Vec<ExtractedPage>> {
            bail!("native extraction was not requested")
        }

        fn ocr_page(&self, _pdf: &Path, pdf_sha256: &str, page: Page) -> Result<ExtractedPage> {
            if pdf_sha256 != self.expected {
                bail!("unsafe PDF hash reached provider: {pdf_sha256}");
            }
            Ok(ExtractedPage {
                page,
                text: "Supported once.".to_owned(),
                spans: Vec::new(),
                mean_confidence: None,
            })
        }
    }

    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::TesseractOcr);
    default_pair(&mut summary).pdf.sha256 = "../recorded-hash".to_owned();
    let actual = sha256_file(&fixture.corpus.root().join("pdfs/smith-2019.pdf")).unwrap();
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &HashCheckingPdf { expected: actual },
        false,
    );

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "invalid_sha256")
    );
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.code == "pdf_extraction_failed"),
        "{:?}",
        report.issues
    );
}

#[test]
fn ocr_disabled_rejects_an_ocr_backend_locator() {
    let fixture = Fixture::with_config(
        "Supported once.",
        "cache:\n  root: \".cache/pdf-text\"\npdf:\n  ocr:\n    enabled: false\n",
    );
    let report = validate_document(
        &fixture.corpus,
        &fixture.summary("Supported once", PdfBackend::TesseractOcr),
        &FakePdf {
            pages: HashMap::from([((PdfBackend::TesseractOcr, 1), "Supported once.".to_owned())]),
        },
        false,
    );

    assert!(!report.is_valid());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "ocr_disabled"),
        "{:?}",
        report.issues
    );
}

#[test]
fn rejects_canonical_source_symlinks_that_escape_the_corpus() {
    let fixture = Fixture::new("Supported once.");
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), "Supported once.").unwrap();
    let markdown_path = fixture.corpus.root().join("md/smith-2019/smith-2019.md");
    fs::remove_file(&markdown_path).unwrap();
    symlink(outside.path(), &markdown_path).unwrap();
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    default_pair(&mut summary).markdown.as_mut().unwrap().sha256 =
        sha256_file(outside.path()).unwrap();
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );

    let issue = report
        .issues
        .iter()
        .find(|issue| issue.code == "source_outside_repo")
        .unwrap_or_else(|| panic!("stable source escape issue missing: {:?}", report.issues));
    assert_eq!(issue.source.as_deref(), Some("default"));
}

#[test]
fn each_named_source_resolves_against_its_own_files() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("md/smith-2019")).unwrap();
    fs::create_dir_all(temp.path().join("pdfs")).unwrap();
    fs::write(
        temp.path().join("receipts.yaml"),
        concat!(
            "cache:\n  root: \".cache/pdf-text\"\n",
            "corpus:\n",
            "  sources:\n",
            "    default:\n",
            "      markdown: [\"md/{id}/{id}.md\"]\n",
            "      pdf: \"pdfs/{id}.pdf\"\n",
            "    supplement:\n",
            "      markdown: [\"md/{id}-supp.md\"]\n",
            "      pdf: \"pdfs/{id}-supp.pdf\"\n",
        ),
    )
    .unwrap();
    let primary_markdown = "Reported in the primary text.";
    let supplement_markdown = "Reported in the appendix text.";
    fs::write(
        temp.path().join("md/smith-2019/smith-2019.md"),
        primary_markdown,
    )
    .unwrap();
    fs::write(
        temp.path().join("md/smith-2019-supp.md"),
        supplement_markdown,
    )
    .unwrap();
    fs::write(temp.path().join("pdfs/smith-2019.pdf"), b"primary PDF").unwrap();
    fs::write(
        temp.path().join("pdfs/smith-2019-supp.pdf"),
        b"supplement PDF",
    )
    .unwrap();
    let corpus = Corpus::discover_from(temp.path(), None).unwrap();

    let claim = "A claim bound to both sources.".to_owned();
    let summary = SummaryDocument {
        exempt_words: Vec::new(),
        id: "smith-2019".to_owned(),
        claims: vec![claim.clone()],
        evidence: Some(Evidence {
            sources: BTreeMap::from([
                (
                    DEFAULT_SOURCE.to_owned(),
                    source_pair(
                        &corpus,
                        "md/smith-2019/smith-2019.md",
                        "pdfs/smith-2019.pdf",
                    ),
                ),
                (
                    "supplement".to_owned(),
                    source_pair(&corpus, "md/smith-2019-supp.md", "pdfs/smith-2019-supp.pdf"),
                ),
            ]),
            claims: vec![ClaimEvidence {
                claim: ClaimIndex::new(0),
                claim_sha256: sha256_bytes(claim.as_bytes()),
                locators: vec![
                    locator(DEFAULT_SOURCE, "the primary text"),
                    locator("supplement", "the appendix text"),
                ],
            }],
        }),
        review: None,
    };

    // Keyed by file name so a locator routed to the wrong source's PDF misses.
    let report = validate_document(
        &corpus,
        &summary,
        &PerSourcePdf {
            pages: HashMap::from([
                (
                    ("smith-2019.pdf".to_owned(), 1),
                    "Reported in the primary text.".to_owned(),
                ),
                (
                    ("smith-2019-supp.pdf".to_owned(), 1),
                    "Reported in the appendix text.".to_owned(),
                ),
            ]),
        },
        false,
    );

    assert!(report.issues.is_empty(), "{:?}", report.issues);
}

#[test]
fn a_source_without_corpus_templates_is_reported() {
    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    let evidence = summary.evidence.as_mut().unwrap();
    let orphan = evidence.sources.get(DEFAULT_SOURCE).unwrap().clone();
    evidence.sources.insert("supplement".to_owned(), orphan);
    let mut cited = evidence.claims[0].locators[0].clone();
    cited.source = "supplement".to_owned();
    evidence.claims[0].locators.push(cited);

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );

    assert_eq!(
        report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<Vec<_>>(),
        ["unknown_source_template"],
        "{:?}",
        report.issues
    );
}

#[test]
fn stale_section_is_reported_when_path_changes() {
    let fixture = Fixture::new("# Results\n\n## Onset\n\nSupported once in the text.\n");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    summary.evidence.as_mut().unwrap().claims[0].locators[0]
        .markdown
        .as_mut()
        .unwrap()
        .section = vec!["Results".to_owned(), "Discussion".to_owned()];

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "stale_section"),
        "{:?}",
        report.issues
    );
}

#[test]
fn matching_section_produces_no_issue() {
    let fixture = Fixture::new("# Results\n\n## Onset\n\nSupported once in the text.\n");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    summary.evidence.as_mut().unwrap().claims[0].locators[0]
        .markdown
        .as_mut()
        .unwrap()
        .section = vec!["Results".to_owned(), "Onset".to_owned()];

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );

    assert!(report.issues.is_empty(), "{:?}", report.issues);
}

#[test]
fn uncovered_token_is_reported_when_number_missing_from_locators() {
    let fixture = Fixture::new("The effect was 42.8% within tolerance.\n");
    let summary = fixture.summary_for(
        "The result showed 42.8% accuracy across 3 trials.",
        "42.8% within tolerance",
        PdfBackend::MutoolNative,
    );

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "The effect was 42.8% within tolerance.".to_owned(),
            )]),
        },
        false,
    );

    let uncovered: Vec<_> = report
        .issues
        .iter()
        .filter(|issue| issue.code == "uncovered_token")
        .collect();
    assert!(
        uncovered
            .iter()
            .any(|issue| issue.message.contains("\"3\"")),
        "{:?}",
        report.issues
    );
    assert!(
        !uncovered
            .iter()
            .any(|issue| issue.message.contains("42.8%")),
        "{:?}",
        report.issues
    );
}

#[test]
fn coverage_tokens_off_skips_token_check() {
    let fixture = Fixture::with_config(
        "The finding text.\n",
        "cache:\n  root: \".cache/pdf-text\"\ncoverage:\n  tokens: off\n",
    );
    let summary = fixture.summary_for(
        "There were 5 total findings.",
        "finding text",
        PdfBackend::MutoolNative,
    );

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "The finding text.".to_owned(),
            )]),
        },
        false,
    );

    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.code == "uncovered_token"),
        "{:?}",
        report.issues
    );
}

#[test]
fn coverage_tokens_warn_produces_warnings_not_errors() {
    let fixture = Fixture::with_config(
        "The finding text.\n",
        "cache:\n  root: \".cache/pdf-text\"\ncoverage:\n  tokens: warn\n",
    );
    let summary = fixture.summary_for(
        "There were 5 findings total.",
        "finding text",
        PdfBackend::MutoolNative,
    );

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "The finding text.".to_owned(),
            )]),
        },
        false,
    );

    let token_issues: Vec<_> = report
        .issues
        .iter()
        .filter(|issue| issue.code == "uncovered_token")
        .collect();
    assert!(!token_issues.is_empty(), "{:?}", report.issues);
    assert!(
        token_issues
            .iter()
            .all(|issue| issue.severity == Severity::Warning)
    );
    assert!(report.is_valid(), "warnings should not make report invalid");
}

#[test]
fn weak_section_only_fires_when_all_locators_are_weak() {
    let fixture = Fixture::with_config(
        "# Limitations\n\nSupported once in the text.\n",
        "cache:\n  root: \".cache/pdf-text\"\nsections:\n  weak:\n    - Limitations\n",
    );
    let summary = fixture.summary("Supported once", PdfBackend::MutoolNative);

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );

    let weak: Vec<_> = report
        .issues
        .iter()
        .filter(|issue| issue.code == "weak_section_only")
        .collect();
    assert_eq!(weak.len(), 1, "{:?}", report.issues);
    assert_eq!(weak[0].severity, Severity::Warning);
    assert!(
        report.is_valid(),
        "weak_section_only is a warning, not an error"
    );
}

#[test]
fn weak_section_does_not_fire_when_one_locator_is_not_weak() {
    let fixture = Fixture::with_config(
        "# Results\n\nFirst locator text.\n\n# Limitations\n\nSecond locator text.\n",
        "cache:\n  root: \".cache/pdf-text\"\nsections:\n  weak:\n    - Limitations\n",
    );
    let claim = "A claim bound to literal source evidence.".to_owned();
    let summary = SummaryDocument {
        exempt_words: Vec::new(),
        id: "smith-2019".to_owned(),
        claims: vec![claim.clone()],
        evidence: Some(Evidence {
            sources: BTreeMap::from([(
                DEFAULT_SOURCE.to_owned(),
                source_pair(
                    &fixture.corpus,
                    "md/smith-2019/smith-2019.md",
                    "pdfs/smith-2019.pdf",
                ),
            )]),
            claims: vec![ClaimEvidence {
                claim: ClaimIndex::new(0),
                claim_sha256: sha256_bytes(claim.as_bytes()),
                locators: vec![
                    weak_locator("First locator text", 3),
                    weak_locator("Second locator text", 7),
                ],
            }],
        }),
        review: None,
    };

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "First locator text. Second locator text.".to_owned(),
            )]),
        },
        false,
    );

    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.code == "weak_section_only"),
        "{:?}",
        report.issues
    );
}

#[test]
fn stale_review_claim_detected_during_validation() {
    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    let evidence = summary.evidence.as_ref().unwrap();
    let ev_hash = evidence_sha256(&evidence.claims[0]);
    summary.review = Some(Review {
        claims: vec![ReviewEntry {
            claim: ClaimIndex::new(0),
            claim_sha256: "0".repeat(64),
            evidence_sha256: ev_hash,
            verdict: Verdict::Supported,
            reviewer: "test".to_owned(),
            note: None,
            at: None,
        }],
    });
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );
    assert!(
        report.issues.iter().any(|i| i.code == "stale_review_claim"),
        "{:?}",
        report.issues
    );
}

#[test]
fn stale_review_evidence_detected_during_validation() {
    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    let claim_hash = sha256_bytes(summary.claims[0].as_bytes());
    summary.review = Some(Review {
        claims: vec![ReviewEntry {
            claim: ClaimIndex::new(0),
            claim_sha256: claim_hash,
            evidence_sha256: "0".repeat(64),
            verdict: Verdict::Supported,
            reviewer: "test".to_owned(),
            note: None,
            at: None,
        }],
    });
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == "stale_review_evidence"),
        "{:?}",
        report.issues
    );
}

#[test]
fn valid_review_produces_no_issues_during_validation() {
    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    let claim_hash = sha256_bytes(summary.claims[0].as_bytes());
    let ev_hash = evidence_sha256(&summary.evidence.as_ref().unwrap().claims[0]);
    summary.review = Some(Review {
        claims: vec![ReviewEntry {
            claim: ClaimIndex::new(0),
            claim_sha256: claim_hash,
            evidence_sha256: ev_hash,
            verdict: Verdict::Supported,
            reviewer: "test".to_owned(),
            note: None,
            at: None,
        }],
    });
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );
    assert!(report.issues.is_empty(), "{:?}", report.issues);
}

fn weak_locator(exact: &str, line: usize) -> Locator {
    let mut locator = locator(DEFAULT_SOURCE, exact);
    locator.markdown.as_mut().unwrap().line = Line::new(line).unwrap();
    locator
}

fn source_pair(corpus: &Corpus, markdown: &str, pdf: &str) -> SourcePair {
    SourcePair {
        markdown: Some(SourceRecord {
            source: markdown.to_owned(),
            sha256: sha256_file(&corpus.root().join(markdown)).unwrap(),
        }),
        pdf: SourceRecord {
            source: pdf.to_owned(),
            sha256: sha256_file(&corpus.root().join(pdf)).unwrap(),
        },
    }
}

fn locator(source: &str, exact: &str) -> Locator {
    Locator {
        source: source.to_owned(),
        exact: exact.to_owned(),
        markdown: Some(MarkdownLocator {
            line: Line::new(1).unwrap(),
            column: Column::new(1).unwrap(),
            unit: UnitKind::Paragraph,
            section: Vec::new(),
        }),
        pdf: PdfLocator {
            page: Page::new(1).unwrap(),
            backend: PdfBackend::MutoolNative,
        },
    }
}

fn default_pair(summary: &mut SummaryDocument) -> &mut SourcePair {
    summary
        .evidence
        .as_mut()
        .unwrap()
        .sources
        .get_mut(DEFAULT_SOURCE)
        .unwrap()
}

struct Fixture {
    _temp: tempfile::TempDir,
    corpus: Corpus,
    markdown: String,
}

impl Fixture {
    // Every config pins the cache inside the temp dir so the suite stays hermetic.
    fn new(markdown: &str) -> Self {
        Self::with_config(markdown, "cache:\n  root: \".cache/pdf-text\"\n")
    }

    fn with_config(markdown: &str, config: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("summaries")).unwrap();
        fs::create_dir_all(temp.path().join("md/smith-2019")).unwrap();
        fs::create_dir_all(temp.path().join("pdfs")).unwrap();
        fs::write(temp.path().join("receipts.yaml"), config).unwrap();
        fs::write(temp.path().join("md/smith-2019/smith-2019.md"), markdown).unwrap();
        fs::write(temp.path().join("pdfs/smith-2019.pdf"), b"fixture PDF").unwrap();
        let corpus = Corpus::discover_from(temp.path(), None).unwrap();
        Self {
            _temp: temp,
            corpus,
            markdown: markdown.to_owned(),
        }
    }

    fn summary(&self, exact: &str, backend: PdfBackend) -> SummaryDocument {
        self.summary_for("A claim bound to literal source evidence.", exact, backend)
    }

    fn summary_for(&self, claim: &str, exact: &str, backend: PdfBackend) -> SummaryDocument {
        let unit = parse_units(&self.markdown)
            .into_iter()
            .find(|unit| unit.text.contains(exact))
            .unwrap();
        let claim = claim.to_owned();
        SummaryDocument {
            exempt_words: Vec::new(),
            id: "smith-2019".to_owned(),
            claims: vec![claim.clone()],
            evidence: Some(Evidence {
                sources: BTreeMap::from([(
                    DEFAULT_SOURCE.to_owned(),
                    SourcePair {
                        markdown: Some(SourceRecord {
                            source: "md/smith-2019/smith-2019.md".to_owned(),
                            sha256: sha256_bytes(self.markdown.as_bytes()),
                        }),
                        pdf: SourceRecord {
                            source: "pdfs/smith-2019.pdf".to_owned(),
                            sha256: sha256_file(&self.corpus.root().join("pdfs/smith-2019.pdf"))
                                .unwrap(),
                        },
                    },
                )]),
                claims: vec![ClaimEvidence {
                    claim: ClaimIndex::new(0),
                    claim_sha256: sha256_bytes(claim.as_bytes()),
                    locators: vec![Locator {
                        source: DEFAULT_SOURCE.to_owned(),
                        exact: exact.to_owned(),
                        markdown: Some(MarkdownLocator {
                            line: unit.line,
                            column: unit.column,
                            unit: UnitKind::Paragraph,
                            section: Vec::new(),
                        }),
                        pdf: PdfLocator {
                            page: Page::new(1).unwrap(),
                            backend,
                        },
                    }],
                }],
            }),
            review: None,
        }
    }
}

#[test]
fn coverage_words_reports_a_qualifier_the_claim_added() {
    let fixture = Fixture::with_config(
        "The fearful prototype is characterized by an avoidance of close relationships.\n",
        "cache:\n  root: \".cache/pdf-text\"\ncoverage:\n  words: error\n",
    );
    let summary = fixture.summary_for(
        "The fearful interview prototype is characterized by an avoidance of close relationships.",
        "The fearful prototype is characterized by an avoidance of close relationships.",
        PdfBackend::MutoolNative,
    );

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([(
                (PdfBackend::MutoolNative, 1),
                "The fearful prototype is characterized by an avoidance of close relationships."
                    .to_owned(),
            )]),
        },
        false,
    );

    let word_issues: Vec<_> = report
        .issues
        .iter()
        .filter(|issue| issue.code == "uncovered_word")
        .collect();
    assert_eq!(word_issues.len(), 1, "{:?}", report.issues);
    assert!(
        word_issues[0].message.contains("\"interview\""),
        "{word_issues:?}"
    );
    assert!(
        !word_issues[0].message.contains("\"fearful\""),
        "{word_issues:?}"
    );
    assert_eq!(word_issues[0].severity, Severity::Error);
}

#[test]
fn coverage_words_is_off_by_default_and_honours_the_allowlist() {
    let fixture = Fixture::new("The fearful prototype is characterized by avoidance.\n");
    let summary = fixture.summary_for(
        "The authors report that the fearful interview prototype is characterized by avoidance.",
        "The fearful prototype is characterized by avoidance.",
        PdfBackend::MutoolNative,
    );
    let pages = HashMap::from([(
        (PdfBackend::MutoolNative, 1),
        "The fearful prototype is characterized by avoidance.".to_owned(),
    )]);

    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: pages.clone(),
        },
        false,
    );
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.code == "uncovered_word"),
        "{:?}",
        report.issues
    );

    let fixture = Fixture::with_config(
        "The fearful prototype is characterized by avoidance.\n",
        "cache:\n  root: \".cache/pdf-text\"\ncoverage:\n  words: warn\n  allowed_words: [authors, report]\n",
    );
    let summary = fixture.summary_for(
        "The authors report that the fearful interview prototype is characterized by avoidance.",
        "The fearful prototype is characterized by avoidance.",
        PdfBackend::MutoolNative,
    );
    let report = validate_document(&fixture.corpus, &summary, &FakePdf { pages }, false);
    let word_issues: Vec<_> = report
        .issues
        .iter()
        .filter(|issue| issue.code == "uncovered_word")
        .collect();
    assert_eq!(word_issues.len(), 1, "{:?}", report.issues);
    assert_eq!(word_issues[0].severity, Severity::Warning);
    assert!(word_issues[0].message.contains("\"interview\""));
    assert!(!word_issues[0].message.contains("\"authors\""));
}

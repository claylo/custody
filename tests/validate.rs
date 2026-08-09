use std::{
    collections::{BTreeMap, HashMap},
    fs,
    os::unix::fs::symlink,
    path::Path,
};

use anyhow::{Result, bail};
use receipts::{
    corpus::Corpus,
    evidence::{
        ClaimEvidence, DEFAULT_SOURCE, Evidence, Locator, MarkdownLocator, PdfBackend, PdfLocator,
        SourcePair, SourceRecord, SummaryDocument,
    },
    hash::{sha256_bytes, sha256_file},
    markdown::{UnitKind, parse_units},
    pdf::{ExtractedPage, PdfTextProvider},
    validate::validate_document,
};

#[derive(Default)]
struct FakePdf {
    pages: HashMap<(PdfBackend, usize), String>,
}

impl PdfTextProvider for FakePdf {
    fn native_pages(&self, _pdf: &Path, page: Option<usize>) -> Result<Vec<ExtractedPage>> {
        let page = page.expect("validation requests one page");
        Ok(vec![ExtractedPage {
            page,
            text: self
                .pages
                .get(&(PdfBackend::MutoolNative, page))
                .cloned()
                .unwrap_or_default(),
            spans: Vec::new(),
            mean_confidence: None,
        }])
    }

    fn ocr_page(&self, _pdf: &Path, _pdf_sha256: &str, page: usize) -> Result<ExtractedPage> {
        let Some(text) = self.pages.get(&(PdfBackend::TesseractOcr, page)) else {
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
    fn native_pages(&self, pdf: &Path, page: Option<usize>) -> Result<Vec<ExtractedPage>> {
        let page = page.expect("validation requests one page");
        let name = pdf.file_name().unwrap().to_string_lossy().into_owned();
        let Some(text) = self.pages.get(&(name.clone(), page)) else {
            bail!("missing fake page {page} for {name}");
        };
        Ok(vec![ExtractedPage {
            page,
            text: text.clone(),
            spans: Vec::new(),
            mean_confidence: None,
        }])
    }

    fn ocr_page(&self, _pdf: &Path, _pdf_sha256: &str, _page: usize) -> Result<ExtractedPage> {
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
    );

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "pdf_ambiguous")
    );
}

#[test]
fn locator_is_bounded_to_the_recorded_markdown_unit() {
    let fixture = Fixture::new("First unit.\n\nSupported once in a different unit.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    summary.evidence.as_mut().unwrap().claims[0].locators[0]
        .markdown
        .line = 1;
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
    );

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "markdown_missing")
    );
}

#[test]
fn source_hash_mismatch_is_reported() {
    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    default_pair(&mut summary).pdf.sha256 = "0".repeat(64);
    let report = validate_document(&fixture.corpus, &summary, &FakePdf::default());

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "default/pdf_hash_mismatch")
    );
}

#[test]
fn actual_pdf_hash_not_recorded_hash_keys_ocr() {
    struct HashCheckingPdf {
        expected: String,
    }

    impl PdfTextProvider for HashCheckingPdf {
        fn native_pages(&self, _pdf: &Path, _page: Option<usize>) -> Result<Vec<ExtractedPage>> {
            bail!("native extraction was not requested")
        }

        fn ocr_page(&self, _pdf: &Path, pdf_sha256: &str, page: usize) -> Result<ExtractedPage> {
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
    default_pair(&mut summary).markdown.sha256 = sha256_file(outside.path()).unwrap();
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
    );

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "default/markdown_source_outside_repo"),
        "{:?}",
        report.issues
    );
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

    let claim = "A claim bound to two sources.".to_owned();
    let summary = SummaryDocument {
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
                claim: 0,
                claim_sha256: sha256_bytes(claim.as_bytes()),
                locators: vec![
                    locator(DEFAULT_SOURCE, "the primary text"),
                    locator("supplement", "the appendix text"),
                ],
            }],
        }),
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

fn source_pair(corpus: &Corpus, markdown: &str, pdf: &str) -> SourcePair {
    SourcePair {
        markdown: SourceRecord {
            source: markdown.to_owned(),
            sha256: sha256_file(&corpus.root().join(markdown)).unwrap(),
        },
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
        markdown: MarkdownLocator {
            line: 1,
            column: 1,
            unit: UnitKind::Paragraph,
        },
        pdf: PdfLocator {
            page: 1,
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
        let unit = parse_units(&self.markdown)
            .into_iter()
            .find(|unit| unit.text.contains(exact))
            .unwrap();
        let claim = "A claim bound to literal source evidence.".to_owned();
        SummaryDocument {
            id: "smith-2019".to_owned(),
            claims: vec![claim.clone()],
            evidence: Some(Evidence {
                sources: BTreeMap::from([(
                    DEFAULT_SOURCE.to_owned(),
                    SourcePair {
                        markdown: SourceRecord {
                            source: "md/smith-2019/smith-2019.md".to_owned(),
                            sha256: sha256_bytes(self.markdown.as_bytes()),
                        },
                        pdf: SourceRecord {
                            source: "pdfs/smith-2019.pdf".to_owned(),
                            sha256: sha256_file(&self.corpus.root().join("pdfs/smith-2019.pdf"))
                                .unwrap(),
                        },
                    },
                )]),
                claims: vec![ClaimEvidence {
                    claim: 0,
                    claim_sha256: sha256_bytes(claim.as_bytes()),
                    locators: vec![Locator {
                        source: DEFAULT_SOURCE.to_owned(),
                        exact: exact.to_owned(),
                        markdown: MarkdownLocator {
                            line: unit.line,
                            column: unit.column,
                            unit: UnitKind::Paragraph,
                        },
                        pdf: PdfLocator { page: 1, backend },
                    }],
                }],
            }),
        }
    }
}

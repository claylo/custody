use std::{collections::HashMap, fs, os::unix::fs::symlink, path::Path};

use anyhow::{Result, bail};
use receipts::{
    corpus::Corpus,
    evidence::{
        ClaimEvidence, Evidence, Locator, MarkdownLocator, PdfBackend, PdfLocator, SourceRecord,
        SummaryDocument,
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
    summary.evidence.as_mut().unwrap().pdf.sha256 = "0".repeat(64);
    let report = validate_document(&fixture.corpus, &summary, &FakePdf::default());

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "pdf_hash_mismatch")
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
    summary.evidence.as_mut().unwrap().pdf.sha256 = "../recorded-hash".to_owned();
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
    summary.evidence.as_mut().unwrap().markdown.sha256 = sha256_file(outside.path()).unwrap();
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
            .any(|issue| issue.code == "markdown_source_outside_repo"),
        "{:?}",
        report.issues
    );
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
                markdown: SourceRecord {
                    source: "md/smith-2019/smith-2019.md".to_owned(),
                    sha256: sha256_bytes(self.markdown.as_bytes()),
                },
                pdf: SourceRecord {
                    source: "pdfs/smith-2019.pdf".to_owned(),
                    sha256: sha256_file(&self.corpus.root().join("pdfs/smith-2019.pdf")).unwrap(),
                },
                claims: vec![ClaimEvidence {
                    claim: 0,
                    claim_sha256: sha256_bytes(claim.as_bytes()),
                    locators: vec![Locator {
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

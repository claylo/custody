use std::{collections::HashMap, fs, path::Path};

use serde::Serialize;

use crate::{
    corpus::Corpus,
    evidence::{EvidenceIssue, PdfBackend, SummaryDocument},
    hash::sha256_file,
    markdown::{exact_count, parse_units, resolve_unit},
    normalize::normalize,
    pdf::PdfTextProvider,
};

/// Complete accumulated result for one summary.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub id: String,
    pub issues: Vec<EvidenceIssue>,
}

impl ValidationReport {
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Validate a parsed summary against its canonical Markdown and PDF sources.
#[must_use]
pub fn validate_document(
    corpus: &Corpus,
    summary: &SummaryDocument,
    provider: &impl PdfTextProvider,
) -> ValidationReport {
    let mut issues = summary.validate_evidence_structure();
    let Some(evidence) = summary.evidence.as_ref() else {
        return ValidationReport {
            id: summary.id.clone(),
            issues,
        };
    };

    let markdown_candidates = match corpus.markdown_candidates(&summary.id) {
        Ok(paths) => paths,
        Err(error) => {
            issues.push(issue("invalid_id", error.to_string(), None, None));
            return ValidationReport {
                id: summary.id.clone(),
                issues,
            };
        }
    };
    let markdown_path = markdown_candidates
        .iter()
        .find(|path| path.is_file())
        .unwrap_or(&markdown_candidates[0]);
    let pdf_path = match corpus.pdf_path(&summary.id) {
        Ok(path) => path,
        Err(error) => {
            issues.push(issue("invalid_id", error.to_string(), None, None));
            return ValidationReport {
                id: summary.id.clone(),
                issues,
            };
        }
    };

    validate_source_path(
        corpus,
        "markdown",
        &evidence.markdown.source,
        markdown_path,
        &mut issues,
    );
    validate_source_path(corpus, "pdf", &evidence.pdf.source, &pdf_path, &mut issues);
    let markdown_is_safe = validate_resolved_source(corpus, "markdown", markdown_path, &mut issues);
    let pdf_is_safe = validate_resolved_source(corpus, "pdf", &pdf_path, &mut issues);
    if !markdown_is_safe || !pdf_is_safe {
        return ValidationReport {
            id: summary.id.clone(),
            issues,
        };
    }
    let _markdown_sha256 = validate_file_hash(
        "markdown",
        markdown_path,
        &evidence.markdown.sha256,
        &mut issues,
    );
    let actual_pdf_sha256 = validate_file_hash("pdf", &pdf_path, &evidence.pdf.sha256, &mut issues);

    let markdown_source = match fs::read_to_string(markdown_path) {
        Ok(source) => source,
        Err(error) => {
            issues.push(issue(
                "markdown_read_failed",
                format!("failed to read {}: {error}", markdown_path.display()),
                None,
                None,
            ));
            return ValidationReport {
                id: summary.id.clone(),
                issues,
            };
        }
    };
    let units = parse_units(&markdown_source);
    let mut pdf_pages: HashMap<(PdfBackend, usize), Result<String, String>> = HashMap::new();

    for entry in &evidence.claims {
        for (locator_index, locator) in entry.locators.iter().enumerate() {
            match resolve_unit(
                &units,
                locator.markdown.unit,
                locator.markdown.line,
                locator.markdown.column,
            ) {
                Ok(unit) => match exact_count(&unit.text, &locator.exact) {
                    1 => {}
                    0 => issues.push(issue(
                        "markdown_missing",
                        format!(
                            "exact text does not occur in the recorded {:?} unit",
                            locator.markdown.unit
                        ),
                        Some(entry.claim),
                        Some(locator_index),
                    )),
                    count => issues.push(issue(
                        "markdown_ambiguous",
                        format!("exact text occurs {count} times in the recorded Markdown unit"),
                        Some(entry.claim),
                        Some(locator_index),
                    )),
                },
                Err(error) => issues.push(issue(
                    "markdown_unit_missing",
                    error.to_string(),
                    Some(entry.claim),
                    Some(locator_index),
                )),
            }

            let key = (locator.pdf.backend, locator.pdf.page);
            let extracted = pdf_pages.entry(key).or_insert_with(|| {
                extract_pdf_page(
                    provider,
                    &pdf_path,
                    actual_pdf_sha256.as_deref().unwrap_or_default(),
                    locator.pdf.backend,
                    locator.pdf.page,
                )
            });
            match extracted {
                Ok(text) => match exact_count(text, &locator.exact) {
                    1 => {}
                    0 => issues.push(issue(
                        "pdf_missing",
                        format!(
                            "exact text does not occur on PDF page {} through {}",
                            locator.pdf.page,
                            locator.pdf.backend.as_str()
                        ),
                        Some(entry.claim),
                        Some(locator_index),
                    )),
                    count => issues.push(issue(
                        "pdf_ambiguous",
                        format!(
                            "exact text occurs {count} times on PDF page {} through {}",
                            locator.pdf.page,
                            locator.pdf.backend.as_str()
                        ),
                        Some(entry.claim),
                        Some(locator_index),
                    )),
                },
                Err(error) => issues.push(issue(
                    "pdf_extraction_failed",
                    error.clone(),
                    Some(entry.claim),
                    Some(locator_index),
                )),
            }
        }
    }

    ValidationReport {
        id: summary.id.clone(),
        issues,
    }
}

fn extract_pdf_page(
    provider: &impl PdfTextProvider,
    pdf_path: &Path,
    pdf_sha256: &str,
    backend: PdfBackend,
    page: usize,
) -> Result<String, String> {
    let extracted = match backend {
        PdfBackend::MutoolNative => {
            provider
                .native_pages(pdf_path, Some(page))
                .and_then(|mut pages| {
                    if pages.len() == 1 {
                        Ok(pages.remove(0))
                    } else {
                        anyhow::bail!("native backend returned {} pages", pages.len())
                    }
                })
        }
        PdfBackend::TesseractOcr => provider.ocr_page(pdf_path, pdf_sha256, page),
    };
    extracted
        .map(|page| normalize(&page.text))
        .map_err(|error| error.to_string())
}

fn validate_source_path(
    corpus: &Corpus,
    label: &str,
    recorded: &str,
    expected: &Path,
    issues: &mut Vec<EvidenceIssue>,
) {
    let expected_relative = expected
        .strip_prefix(corpus.root())
        .unwrap_or(expected)
        .to_string_lossy()
        .replace('\\', "/");
    if recorded != expected_relative {
        issues.push(issue(
            format!("{label}_source_mismatch"),
            format!("recorded {label} source is {recorded:?}; expected {expected_relative:?}"),
            None,
            None,
        ));
    }
}

fn validate_file_hash(
    label: &str,
    path: &Path,
    expected: &str,
    issues: &mut Vec<EvidenceIssue>,
) -> Option<String> {
    match sha256_file(path) {
        Ok(actual) if actual == expected => Some(actual),
        Ok(actual) => {
            issues.push(issue(
                format!("{label}_hash_mismatch"),
                format!(
                    "{} hash is stale: expected {actual}, found {expected}",
                    path.display()
                ),
                None,
                None,
            ));
            Some(actual)
        }
        Err(error) => {
            issues.push(issue(
                format!("{label}_read_failed"),
                error.to_string(),
                None,
                None,
            ));
            None
        }
    }
}

fn validate_resolved_source(
    corpus: &Corpus,
    label: &str,
    path: &Path,
    issues: &mut Vec<EvidenceIssue>,
) -> bool {
    match path.canonicalize() {
        Ok(resolved) if resolved.starts_with(corpus.root()) => true,
        Ok(resolved) => {
            issues.push(issue(
                format!("{label}_source_outside_repo"),
                format!(
                    "{} resolves outside the corpus to {}",
                    path.display(),
                    resolved.display()
                ),
                None,
                None,
            ));
            false
        }
        Err(error) => {
            issues.push(issue(
                format!("{label}_source_unresolvable"),
                format!("failed to resolve {}: {error}", path.display()),
                None,
                None,
            ));
            false
        }
    }
}

fn issue(
    code: impl Into<String>,
    message: impl Into<String>,
    claim: Option<usize>,
    locator: Option<usize>,
) -> EvidenceIssue {
    EvidenceIssue {
        code: code.into(),
        message: message.into(),
        claim,
        locator,
    }
}

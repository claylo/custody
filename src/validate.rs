use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    corpus::Corpus,
    evidence::{EvidenceIssue, PdfBackend, Severity, SummaryDocument},
    hash::sha256_file,
    markdown::{MarkdownUnit, exact_count, parse_units, resolve_unit},
    normalize::normalize,
    pdf::PdfTextProvider,
    sections,
};

/// Complete accumulated result for one summary.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub id: String,
    pub issues: Vec<EvidenceIssue>,
}

impl ValidationReport {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Warning)
    }
}

/// Validate a parsed summary against its canonical Markdown and PDF sources.
#[must_use]
pub fn validate_document(
    corpus: &Corpus,
    summary: &SummaryDocument,
    provider: &impl PdfTextProvider,
) -> ValidationReport {
    let mut issues = summary.validate_evidence_structure(corpus.terms());
    let Some(evidence) = summary.evidence.as_ref() else {
        return ValidationReport {
            id: summary.id.clone(),
            issues,
        };
    };

    let configured = corpus.source_names();
    let mut source_units: BTreeMap<&str, Vec<MarkdownUnit>> = BTreeMap::new();
    let mut source_pdf_path: BTreeMap<&str, PathBuf> = BTreeMap::new();
    let mut source_pdf_sha256: BTreeMap<&str, Option<String>> = BTreeMap::new();

    for (source_name, pair) in &evidence.sources {
        if !configured.iter().any(|name| name == source_name) {
            issues.push(issue(
                "unknown_source_template",
                Severity::Error,
                format!("no configured templates for source {source_name:?}"),
                None,
                None,
            ));
            continue;
        }
        let markdown_candidates = match corpus.markdown_candidates_for(&summary.id, source_name) {
            Ok(paths) => paths,
            Err(error) => {
                issues.push(issue(
                    "invalid_id",
                    Severity::Error,
                    error.to_string(),
                    None,
                    None,
                ));
                continue;
            }
        };
        let markdown_path = markdown_candidates
            .iter()
            .find(|path| path.is_file())
            .unwrap_or(&markdown_candidates[0]);
        let pdf_path = match corpus.pdf_path_for(&summary.id, source_name) {
            Ok(path) => path,
            Err(error) => {
                issues.push(issue(
                    "invalid_id",
                    Severity::Error,
                    error.to_string(),
                    None,
                    None,
                ));
                continue;
            }
        };

        let markdown_label = format!("{source_name}/markdown");
        let pdf_label = format!("{source_name}/pdf");
        validate_source_path(
            corpus,
            &markdown_label,
            &pair.markdown.source,
            markdown_path,
            &mut issues,
        );
        validate_source_path(corpus, &pdf_label, &pair.pdf.source, &pdf_path, &mut issues);
        let markdown_is_safe =
            validate_resolved_source(corpus, &markdown_label, markdown_path, &mut issues);
        let pdf_is_safe = validate_resolved_source(corpus, &pdf_label, &pdf_path, &mut issues);
        // Never read a source that resolves outside the corpus.
        if !markdown_is_safe || !pdf_is_safe {
            continue;
        }

        let _markdown_sha256 = validate_file_hash(
            &markdown_label,
            markdown_path,
            &pair.markdown.sha256,
            &mut issues,
        );
        let pdf_sha256 = validate_file_hash(&pdf_label, &pdf_path, &pair.pdf.sha256, &mut issues);
        source_pdf_sha256.insert(source_name, pdf_sha256);
        source_pdf_path.insert(source_name, pdf_path);

        match fs::read_to_string(markdown_path) {
            Ok(markdown_source) => {
                source_units.insert(source_name, parse_units(&markdown_source));
            }
            Err(error) => issues.push(issue(
                "markdown_read_failed",
                Severity::Error,
                format!("failed to read {}: {error}", markdown_path.display()),
                None,
                None,
            )),
        }
    }

    let mut pdf_pages: HashMap<(&str, PdfBackend, usize), Result<String, String>> = HashMap::new();

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
                                issues.push(issue(
                                    "stale_section",
                                    Severity::Error,
                                    format!(
                                        "recorded section {:?} but unit is under {:?}",
                                        locator.markdown.section, unit.section
                                    ),
                                    Some(entry.claim),
                                    Some(locator_index),
                                ));
                            }
                        }
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
                        count => issues.push(issue(
                            "markdown_ambiguous",
                            Severity::Error,
                            format!(
                                "exact text occurs {count} times in the recorded Markdown unit"
                            ),
                            Some(entry.claim),
                            Some(locator_index),
                        )),
                    },
                    Err(error) => issues.push(issue(
                        "markdown_unit_missing",
                        Severity::Error,
                        error.to_string(),
                        Some(entry.claim),
                        Some(locator_index),
                    )),
                }
            }

            if locator.pdf.backend == PdfBackend::TesseractOcr && !corpus.ocr_config().enabled {
                issues.push(issue(
                    "ocr_disabled",
                    Severity::Error,
                    format!(
                        "locator uses {} but OCR is disabled in configuration",
                        locator.pdf.backend.as_str()
                    ),
                    Some(entry.claim),
                    Some(locator_index),
                ));
                continue;
            }

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
            match extracted {
                Ok(text) => match exact_count(text, &locator.exact) {
                    1 => {}
                    0 => issues.push(issue(
                        "pdf_missing",
                        Severity::Error,
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
                        Severity::Error,
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
                    Severity::Error,
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
            Severity::Error,
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
                Severity::Error,
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
                Severity::Error,
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
                Severity::Error,
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
                Severity::Error,
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

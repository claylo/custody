use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    config::TokenSeverity,
    coordinate::{ClaimIndex, LocatorIndex, Page},
    corpus::Corpus,
    evidence::{EvidenceIssue, IssueCode, PdfBackend, Severity, SummaryDocument, issue_code},
    hash::sha256_file,
    markdown::{MarkdownUnit, exact_count, parse_units, resolve_unit},
    normalize::normalize,
    pdf::PdfTextProvider,
    review, sections, tokens,
};

/// Complete accumulated result for one summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    require_review: bool,
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
                issue_code::UNKNOWN_SOURCE_TEMPLATE,
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
                    issue_code::INVALID_ID,
                    Severity::Error,
                    error.to_string(),
                    None,
                    None,
                ));
                continue;
            }
        };
        let Some(markdown_path) = markdown_candidates
            .iter()
            .find(|path| path.is_file())
            .or_else(|| markdown_candidates.first())
        else {
            issues.push(issue(
                issue_code::EMPTY_MARKDOWN_CANDIDATES,
                Severity::Error,
                format!("no markdown template candidates for source {source_name:?}"),
                None,
                None,
            ));
            continue;
        };
        let pdf_path = match corpus.pdf_path_for(&summary.id, source_name) {
            Ok(path) => path,
            Err(error) => {
                issues.push(issue(
                    issue_code::INVALID_ID,
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
            source_name,
            &markdown_label,
            &pair.markdown.source,
            markdown_path,
            &mut issues,
        );
        validate_source_path(
            corpus,
            source_name,
            &pdf_label,
            &pair.pdf.source,
            &pdf_path,
            &mut issues,
        );
        let markdown_is_safe = validate_resolved_source(
            corpus,
            source_name,
            &markdown_label,
            markdown_path,
            &mut issues,
        );
        let pdf_is_safe =
            validate_resolved_source(corpus, source_name, &pdf_label, &pdf_path, &mut issues);
        // Never read a source that resolves outside the corpus.
        if !markdown_is_safe || !pdf_is_safe {
            continue;
        }

        let _markdown_sha256 = validate_file_hash(
            source_name,
            &markdown_label,
            markdown_path,
            &pair.markdown.sha256,
            &mut issues,
        );
        let pdf_sha256 = validate_file_hash(
            source_name,
            &pdf_label,
            &pdf_path,
            &pair.pdf.sha256,
            &mut issues,
        );
        source_pdf_sha256.insert(source_name, pdf_sha256);
        source_pdf_path.insert(source_name, pdf_path);

        match corpus.read_contained_text(markdown_path) {
            Ok(markdown_source) => {
                source_units.insert(source_name, parse_units(&markdown_source));
            }
            Err(error) => issues.push(source_issue(
                issue_code::MARKDOWN_READ_FAILED,
                Severity::Error,
                format!("failed to read {}: {error}", markdown_path.display()),
                source_name,
                None,
                None,
            )),
        }
    }

    let mut pdf_pages: HashMap<(&str, PdfBackend, Page), Result<String, String>> = HashMap::new();
    let token_severity = match corpus.coverage_config().tokens {
        TokenSeverity::Error => Some(Severity::Error),
        TokenSeverity::Warn => Some(Severity::Warning),
        TokenSeverity::Off => None,
    };

    for entry in &evidence.claims {
        for (locator_index, locator) in entry.locators.iter().enumerate() {
            let locator_index = LocatorIndex::new(locator_index);
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
                                    issue_code::STALE_SECTION,
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
                            issue_code::MARKDOWN_MISSING,
                            Severity::Error,
                            format!(
                                "exact text does not occur in the recorded {} unit",
                                locator.markdown.unit
                            ),
                            Some(entry.claim),
                            Some(locator_index),
                        )),
                        count => issues.push(issue(
                            issue_code::MARKDOWN_AMBIGUOUS,
                            Severity::Error,
                            format!(
                                "exact text occurs {count} times in the recorded Markdown unit"
                            ),
                            Some(entry.claim),
                            Some(locator_index),
                        )),
                    },
                    Err(error) => issues.push(issue(
                        issue_code::MARKDOWN_UNIT_MISSING,
                        Severity::Error,
                        error.to_string(),
                        Some(entry.claim),
                        Some(locator_index),
                    )),
                }
            }

            if locator.pdf.backend == PdfBackend::TesseractOcr && !corpus.ocr_config().enabled {
                issues.push(issue(
                    issue_code::OCR_DISABLED,
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
                        issue_code::PDF_MISSING,
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
                        issue_code::PDF_AMBIGUOUS,
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
                    issue_code::PDF_EXTRACTION_FAILED,
                    Severity::Error,
                    error.clone(),
                    Some(entry.claim),
                    Some(locator_index),
                )),
            }
        }

        if let Some(severity) = token_severity
            && let Some(claim_text) = summary.claims.get(entry.claim.get())
        {
            for token in &tokens::extract(claim_text).required {
                let covered = entry.locators.iter().any(|locator| {
                    if tokens::is_number_word(token) {
                        tokens::is_covered_case_insensitive(&locator.exact, token)
                    } else {
                        tokens::is_covered(&locator.exact, token)
                    }
                });
                if !covered {
                    issues.push(issue(
                        issue_code::UNCOVERED_TOKEN,
                        severity,
                        format!(
                            "required token {:?} from claim {} appears in no locator",
                            token, entry.claim
                        ),
                        Some(entry.claim),
                        None,
                    ));
                }
            }
        }

        let weak_sections = &corpus.sections_config().weak;
        if !weak_sections.is_empty() && !entry.locators.is_empty() {
            let all_weak = entry.locators.iter().all(|loc| {
                let Some(units) = source_units.get(loc.source.as_str()) else {
                    return false;
                };
                // An unresolvable unit is already an error; don't judge weakness.
                resolve_unit(
                    units,
                    loc.markdown.unit,
                    loc.markdown.line,
                    loc.markdown.column,
                )
                .is_ok_and(|unit| sections::is_weak_section(&unit.section, weak_sections))
            });
            if all_weak {
                issues.push(issue(
                    issue_code::WEAK_SECTION_ONLY,
                    Severity::Warning,
                    format!(
                        "every locator for claim {} is under a weak section",
                        entry.claim
                    ),
                    Some(entry.claim),
                    None,
                ));
            }
        }
    }

    if let Some(rev) = summary.review.as_ref() {
        issues.extend(review::validate_review(
            rev,
            &summary.claims,
            summary.evidence.as_ref(),
            require_review,
            corpus.terms(),
        ));
    } else if require_review {
        let term = &corpus.terms().claim;
        for index in 0..summary.claims.len() {
            let index = ClaimIndex::new(index);
            issues.push(issue(
                issue_code::MISSING_REVIEW,
                Severity::Error,
                format!("{term} {index} has no review entry"),
                Some(index),
                None,
            ));
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
    page: Page,
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
    source_name: &str,
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
        issues.push(source_issue(
            issue_code::SOURCE_MISMATCH,
            Severity::Error,
            format!("recorded {label} source is {recorded:?}; expected {expected_relative:?}"),
            source_name,
            None,
            None,
        ));
    }
}

fn validate_file_hash(
    source_name: &str,
    label: &str,
    path: &Path,
    expected: &str,
    issues: &mut Vec<EvidenceIssue>,
) -> Option<String> {
    match sha256_file(path) {
        Ok(actual) if actual == expected => Some(actual),
        Ok(actual) => {
            issues.push(source_issue(
                issue_code::SOURCE_HASH_MISMATCH,
                Severity::Error,
                format!(
                    "{label} source {} hash is stale: expected {actual}, found {expected}",
                    path.display()
                ),
                source_name,
                None,
                None,
            ));
            Some(actual)
        }
        Err(error) => {
            issues.push(source_issue(
                issue_code::SOURCE_READ_FAILED,
                Severity::Error,
                format!("failed to read {label} source: {error}"),
                source_name,
                None,
                None,
            ));
            None
        }
    }
}

fn validate_resolved_source(
    corpus: &Corpus,
    source_name: &str,
    label: &str,
    path: &Path,
    issues: &mut Vec<EvidenceIssue>,
) -> bool {
    match corpus.resolve_contained_file(path) {
        Ok(crate::corpus::ResolvedCorpusFile::Contained(_)) => true,
        Ok(crate::corpus::ResolvedCorpusFile::Outside(resolved)) => {
            issues.push(source_issue(
                issue_code::SOURCE_OUTSIDE_REPO,
                Severity::Error,
                format!(
                    "{label} source {} resolves outside the corpus to {}",
                    path.display(),
                    resolved.display()
                ),
                source_name,
                None,
                None,
            ));
            false
        }
        Err(error) => {
            issues.push(source_issue(
                issue_code::SOURCE_UNRESOLVABLE,
                Severity::Error,
                format!(
                    "failed to resolve {label} source {}: {error}",
                    path.display()
                ),
                source_name,
                None,
                None,
            ));
            false
        }
    }
}

fn issue(
    code: IssueCode,
    severity: Severity,
    message: impl Into<String>,
    claim: Option<ClaimIndex>,
    locator: Option<LocatorIndex>,
) -> EvidenceIssue {
    EvidenceIssue {
        code: code.as_str().to_owned(),
        severity,
        message: message.into(),
        source: None,
        claim,
        locator,
    }
}

fn source_issue(
    code: IssueCode,
    severity: Severity,
    message: impl Into<String>,
    source: &str,
    claim: Option<ClaimIndex>,
    locator: Option<LocatorIndex>,
) -> EvidenceIssue {
    EvidenceIssue {
        code: code.as_str().to_owned(),
        severity,
        message: message.into(),
        source: Some(source.to_owned()),
        claim,
        locator,
    }
}

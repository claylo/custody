use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    hash::sha256_bytes, markdown::UnitKind, normalize::normalize, review::Review, terms::Terms,
};

/// The source name a document gets when it declares only one source pair.
pub const DEFAULT_SOURCE: &str = "default";

/// Summary fields needed for evidence validation.
#[derive(Debug, Clone, Deserialize)]
pub struct SummaryDocument {
    pub id: String,
    pub claims: Vec<String>,
    #[serde(default)]
    pub evidence: Option<Evidence>,
    #[serde(default)]
    pub review: Option<Review>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub sources: BTreeMap<String, SourcePair>,
    pub claims: Vec<ClaimEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePair {
    pub markdown: SourceRecord,
    pub pdf: SourceRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    pub source: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimEvidence {
    pub claim: usize,
    pub claim_sha256: String,
    pub locators: Vec<Locator>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locator {
    #[serde(default = "default_source_name")]
    pub source: String,
    pub exact: String,
    pub markdown: MarkdownLocator,
    pub pdf: PdfLocator,
}

fn default_source_name() -> String {
    DEFAULT_SOURCE.to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownLocator {
    pub line: usize,
    pub column: usize,
    pub unit: UnitKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub section: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfLocator {
    pub page: usize,
    pub backend: PdfBackend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PdfBackend {
    MutoolNative,
    TesseractOcr,
}

impl PdfBackend {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MutoolNative => "mutool-native",
            Self::TesseractOcr => "tesseract-ocr",
        }
    }
}

/// How much weight an issue carries: errors fail validation, warnings do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One accumulated evidence-contract problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceIssue {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<usize>,
}

/// One stable issue kind and its CLI schema metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IssueCode {
    kind: &'static str,
    exit_code: Option<u8>,
    description: &'static str,
}

impl IssueCode {
    pub(crate) const fn as_str(self) -> &'static str {
        self.kind
    }

    pub(crate) const fn exit_code(self) -> Option<u8> {
        self.exit_code
    }

    pub(crate) const fn description(self) -> &'static str {
        self.description
    }
}

macro_rules! define_issue_codes {
    ($($name:ident => ($kind:literal, $exit_code:expr, $description:literal)),+ $(,)?) => {
        pub(crate) mod issue_code {
            use super::IssueCode;

            $(pub const $name: IssueCode = IssueCode {
                kind: $kind,
                exit_code: $exit_code,
                description: $description,
            };)+

            pub const ALL: &[IssueCode] = &[$($name),+];
        }
    };
}

define_issue_codes! {
    MISSING_EVIDENCE => ("missing_evidence", Some(1), "Summary has no evidence section"),
    STALE_HASH => ("stale_hash", Some(1), "Claim or source SHA-256 does not match current content"),
    MARKDOWN_MISSING => ("markdown_missing", Some(1), "Exact text not found in the recorded Markdown unit"),
    MARKDOWN_AMBIGUOUS => ("markdown_ambiguous", Some(1), "Exact text occurs more than once in the Markdown unit"),
    PDF_MISSING => ("pdf_missing", Some(1), "Exact text not found on the PDF page"),
    PDF_AMBIGUOUS => ("pdf_ambiguous", Some(1), "Exact text occurs more than once on the PDF page"),
    UNKNOWN_SOURCE => ("unknown_source", Some(1), "Locator references an undeclared source"),
    UNUSED_SOURCE => ("unused_source", Some(1), "Declared source is not cited by any locator"),
    UNKNOWN_SOURCE_TEMPLATE => ("unknown_source_template", Some(1), "No configured templates for a declared source name"),
    UNCOVERED_TOKEN => ("uncovered_token", Some(1), "A required claim token appears in no locator"),
    STALE_SECTION => ("stale_section", Some(1), "Recorded section path disagrees with the source"),
    OCR_DISABLED => ("ocr_disabled", Some(1), "Locator uses OCR but OCR is disabled in configuration"),
    STALE_REVIEW_CLAIM => ("stale_review_claim", Some(1), "Review claim_sha256 does not match current claim text"),
    STALE_REVIEW_EVIDENCE => ("stale_review_evidence", Some(1), "Review evidence_sha256 does not match current locator set"),
    UNKNOWN_REVIEW_CLAIM => ("unknown_review_claim", Some(1), "Review entry references a nonexistent claim"),
    DUPLICATE_REVIEW_CLAIM => ("duplicate_review_claim", Some(1), "Two review entries for one claim"),
    MISSING_REVIEW => ("missing_review", Some(1), "Claim has no review entry (under --require-review)"),
    UNSUPPORTED_VERDICT => ("unsupported_verdict", Some(1), "Verdict is not 'supported' (under --require-review)"),
    DUPLICATE_ENTRY => ("duplicate_entry", Some(1), "Multiple evidence entries for one claim"),
    MISSING_EVIDENCE_ENTRY => ("missing_evidence_entry", Some(1), "Claim has no evidence entry"),
    ENTRY_OUT_OF_RANGE => ("entry_out_of_range", Some(1), "Evidence entry references a nonexistent claim"),
    EMPTY_SOURCES => ("empty_sources", Some(1), "Evidence declares no sources"),
    EMPTY_SOURCE => ("empty_source", Some(1), "Source record is missing markdown or pdf path"),
    EMPTY_LOCATORS => ("empty_locators", Some(1), "Evidence entry has no locators"),
    EMPTY_EXACT => ("empty_exact", Some(1), "Locator exact text is empty after normalization"),
    UNNORMALIZED_EXACT => ("unnormalized_exact", Some(1), "Locator exact text is not in normalized form"),
    INVALID_SOURCE_NAME => ("invalid_source_name", Some(1), "Source name is not a valid path component"),
    INVALID_SHA256 => ("invalid_sha256", Some(1), "SHA-256 value is not 64 lowercase hex characters"),
    INVALID_ID => ("invalid_id", Some(1), "Summary IDs must be 3+ characters of lowercase ASCII letters, digits, and interior hyphens"),
    INVALID_MARKDOWN_LINE => ("invalid_markdown_line", Some(1), "Markdown line coordinate is invalid"),
    INVALID_MARKDOWN_COLUMN => ("invalid_markdown_column", Some(1), "Markdown column coordinate is invalid"),
    INVALID_PDF_PAGE => ("invalid_pdf_page", Some(1), "PDF page number is invalid"),
    MARKDOWN_UNIT_MISSING => ("markdown_unit_missing", Some(1), "No semantic unit at the recorded Markdown coordinates"),
    MARKDOWN_READ_FAILED => ("markdown_read_failed", Some(1), "Markdown source file could not be read"),
    PDF_EXTRACTION_FAILED => ("pdf_extraction_failed", Some(1), "PDF text extraction failed"),
    EMPTY_MARKDOWN_CANDIDATES => ("empty_markdown_candidates", Some(1), "No markdown template candidates for a source"),
    WEAK_SECTION_ONLY => ("weak_section_only", None, "Every locator sits under a weak section heading (warning)"),
    SOURCE_HASH_MISMATCH => ("source_hash_mismatch", Some(1), "Source file SHA-256 disagrees with recorded hash"),
    SOURCE_MISMATCH => ("source_mismatch", Some(1), "Recorded source path does not match expected path"),
    SOURCE_READ_FAILED => ("source_read_failed", Some(1), "Source file could not be read for hashing"),
    SOURCE_OUTSIDE_REPO => ("source_outside_repo", Some(1), "Resolved source path escapes the corpus root"),
    SOURCE_UNRESOLVABLE => ("source_unresolvable", Some(1), "Source path cannot be resolved to a real path"),
    SUMMARY_PARSE_FAILED => ("summary_parse_failed", Some(1), "Summary document could not be read or parsed"),
    ID_MISMATCH => ("id_mismatch", Some(1), "Summary id does not match the ID resolved from its filename"),
}

/// Whether a source name is a safe single path component.
#[must_use]
pub fn validate_source_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

/// Parse a complete summary while retaining only validation-relevant fields.
pub fn parse_summary(content: &str, terms: &Terms) -> Result<SummaryDocument> {
    let mut value =
        librebar::config::parse_yaml(content).context("failed to parse summary YAML")?;
    terms.canonicalize(&mut value)?;
    desugar_evidence_sources(&mut value)?;
    serde_json::from_value(value).context("failed to decode summary evidence")
}

/// Rewrite the single-source shorthand into the general `sources` map.
///
/// This runs on the raw document so the typed structs only ever see one shape,
/// and so `deny_unknown_fields` still rejects genuinely unknown keys.
fn desugar_evidence_sources(value: &mut Value) -> Result<()> {
    let Some(evidence) = value.get_mut("evidence").and_then(Value::as_object_mut) else {
        return Ok(());
    };

    let bare = evidence.contains_key("markdown") || evidence.contains_key("pdf");
    if bare && evidence.contains_key("sources") {
        bail!("evidence declares both bare markdown/pdf and sources; use one form");
    }
    if bare {
        let mut pair = Map::new();
        for key in ["markdown", "pdf"] {
            if let Some(record) = evidence.remove(key) {
                pair.insert(key.to_owned(), record);
            }
        }
        let mut sources = Map::new();
        sources.insert(DEFAULT_SOURCE.to_owned(), Value::Object(pair));
        evidence.insert("sources".to_owned(), Value::Object(sources));
    }

    let Some(entries) = evidence.get_mut("claims").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for entry in entries {
        let Some(locators) = entry.get_mut("locators").and_then(Value::as_array_mut) else {
            continue;
        };
        for locator in locators {
            if let Some(locator) = locator.as_object_mut() {
                locator
                    .entry("source")
                    .or_insert_with(|| Value::String(DEFAULT_SOURCE.to_owned()));
            }
        }
    }

    Ok(())
}

impl SummaryDocument {
    pub fn claim_hash(&self, index: usize) -> Result<String> {
        let Some(claim) = self.claims.get(index) else {
            bail!("claim index {index} is out of range");
        };
        Ok(sha256_bytes(claim.as_bytes()))
    }

    /// Validate evidence shape and bind every entry to its current claim.
    #[must_use]
    pub fn validate_evidence_structure(&self, terms: &Terms) -> Vec<EvidenceIssue> {
        let Some(evidence) = self.evidence.as_ref() else {
            return vec![issue(
                issue_code::MISSING_EVIDENCE,
                Severity::Error,
                "summary is missing evidence section",
                None,
                None,
            )];
        };
        let mut issues = Vec::new();

        if evidence.sources.is_empty() {
            issues.push(issue(
                issue_code::EMPTY_SOURCES,
                Severity::Error,
                "evidence declares no sources",
                None,
                None,
            ));
        }
        for (name, pair) in &evidence.sources {
            if !validate_source_name(name) {
                issues.push(issue(
                    issue_code::INVALID_SOURCE_NAME,
                    Severity::Error,
                    format!("source name {name:?} is not a valid path component"),
                    None,
                    None,
                ));
            }
            validate_source(&format!("{name}/markdown"), &pair.markdown, &mut issues);
            validate_source(&format!("{name}/pdf"), &pair.pdf, &mut issues);
        }

        let mut referenced: BTreeSet<&str> = BTreeSet::new();
        let mut counts = vec![0_usize; self.claims.len()];
        for entry in &evidence.claims {
            if entry.claim >= self.claims.len() {
                issues.push(issue(
                    issue_code::ENTRY_OUT_OF_RANGE,
                    Severity::Error,
                    format!(
                        "evidence {} {} is out of range for {} {}",
                        terms.claim,
                        entry.claim,
                        self.claims.len(),
                        terms.claims
                    ),
                    Some(entry.claim),
                    None,
                ));
            } else {
                counts[entry.claim] += 1;
                let actual = self.claim_hash(entry.claim).expect("index was checked");
                if entry.claim_sha256 != actual {
                    issues.push(issue(
                        issue_code::STALE_HASH,
                        Severity::Error,
                        format!(
                            "{} {} hash is stale: expected {actual}, found {}",
                            terms.claim, entry.claim, entry.claim_sha256
                        ),
                        Some(entry.claim),
                        None,
                    ));
                }
            }
            if entry.locators.is_empty() {
                issues.push(issue(
                    issue_code::EMPTY_LOCATORS,
                    Severity::Error,
                    format!("{} {} has no locators", terms.claim, entry.claim),
                    Some(entry.claim),
                    None,
                ));
            }
            validate_hash(
                &format!("{}_sha256", terms.claim),
                &entry.claim_sha256,
                Some(entry.claim),
                &mut issues,
            );

            for (locator_index, locator) in entry.locators.iter().enumerate() {
                if evidence.sources.contains_key(&locator.source) {
                    referenced.insert(locator.source.as_str());
                } else {
                    issues.push(issue(
                        issue_code::UNKNOWN_SOURCE,
                        Severity::Error,
                        format!("locator references undeclared source {:?}", locator.source),
                        Some(entry.claim),
                        Some(locator_index),
                    ));
                }
                validate_locator(entry.claim, locator_index, locator, &mut issues);
            }
        }

        for (index, count) in counts.into_iter().enumerate() {
            match count {
                0 => issues.push(issue(
                    issue_code::MISSING_EVIDENCE_ENTRY,
                    Severity::Error,
                    format!("missing evidence for {} {index}", terms.claim),
                    Some(index),
                    None,
                )),
                1 => {}
                _ => issues.push(issue(
                    issue_code::DUPLICATE_ENTRY,
                    Severity::Error,
                    format!("{} {index} has {count} evidence entries", terms.claim),
                    Some(index),
                    None,
                )),
            }
        }

        for name in evidence.sources.keys() {
            if !referenced.contains(name.as_str()) {
                issues.push(issue(
                    issue_code::UNUSED_SOURCE,
                    Severity::Error,
                    format!("source {name:?} is declared but no locator references it"),
                    None,
                    None,
                ));
            }
        }

        issues
    }
}

fn validate_source(label: &str, source: &SourceRecord, issues: &mut Vec<EvidenceIssue>) {
    if source.source.is_empty() {
        issues.push(issue(
            issue_code::EMPTY_SOURCE,
            Severity::Error,
            format!("{label} source path is empty"),
            None,
            None,
        ));
    }
    validate_hash(&format!("{label} sha256"), &source.sha256, None, issues);
}

fn validate_hash(label: &str, hash: &str, claim: Option<usize>, issues: &mut Vec<EvidenceIssue>) {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        issues.push(issue(
            issue_code::INVALID_SHA256,
            Severity::Error,
            format!("{label} must be 64 lowercase hexadecimal characters"),
            claim,
            None,
        ));
    }
}

fn validate_locator(
    claim: usize,
    locator_index: usize,
    locator: &Locator,
    issues: &mut Vec<EvidenceIssue>,
) {
    let normalized = normalize(&locator.exact);
    if normalized.is_empty() {
        issues.push(issue(
            issue_code::EMPTY_EXACT,
            Severity::Error,
            "locator exact text is empty after normalization",
            Some(claim),
            Some(locator_index),
        ));
    } else if normalized != locator.exact {
        issues.push(issue(
            issue_code::UNNORMALIZED_EXACT,
            Severity::Error,
            format!("locator exact text must be normalized as {normalized:?}"),
            Some(claim),
            Some(locator_index),
        ));
    }
    if locator.markdown.line == 0 {
        issues.push(issue(
            issue_code::INVALID_MARKDOWN_LINE,
            Severity::Error,
            "Markdown line must be one-based",
            Some(claim),
            Some(locator_index),
        ));
    }
    if locator.markdown.column == 0 {
        issues.push(issue(
            issue_code::INVALID_MARKDOWN_COLUMN,
            Severity::Error,
            "Markdown column must be one-based",
            Some(claim),
            Some(locator_index),
        ));
    }
    if locator.pdf.page == 0 {
        issues.push(issue(
            issue_code::INVALID_PDF_PAGE,
            Severity::Error,
            "PDF page must be one-based",
            Some(claim),
            Some(locator_index),
        ));
    }
}

fn issue(
    code: IssueCode,
    severity: Severity,
    message: impl Into<String>,
    claim: Option<usize>,
    locator: Option<usize>,
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

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{hash::sha256_bytes, markdown::UnitKind, normalize::normalize, terms::Terms};

/// Summary fields needed for evidence validation.
#[derive(Debug, Clone, Deserialize)]
pub struct SummaryDocument {
    pub id: String,
    pub claims: Vec<String>,
    #[serde(default)]
    pub evidence: Option<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub markdown: SourceRecord,
    pub pdf: SourceRecord,
    pub claims: Vec<ClaimEvidence>,
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
    pub exact: String,
    pub markdown: MarkdownLocator,
    pub pdf: PdfLocator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownLocator {
    pub line: usize,
    pub column: usize,
    pub unit: UnitKind,
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

/// One accumulated evidence-contract problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceIssue {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<usize>,
}

/// Parse a complete summary while retaining only validation-relevant fields.
pub fn parse_summary(content: &str, terms: &Terms) -> Result<SummaryDocument> {
    let mut value =
        librebar::config::parse_yaml(content).context("failed to parse summary YAML")?;
    terms.canonicalize(&mut value)?;
    serde_json::from_value(value).context("failed to decode summary evidence")
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
                "missing_evidence",
                "summary is missing evidence section",
                None,
                None,
            )];
        };
        let mut issues = Vec::new();

        validate_source("Markdown", &evidence.markdown, &mut issues);
        validate_source("PDF", &evidence.pdf, &mut issues);

        let mut counts = vec![0_usize; self.claims.len()];
        for entry in &evidence.claims {
            if entry.claim >= self.claims.len() {
                issues.push(issue(
                    "entry_out_of_range",
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
                        "stale_hash",
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
                    "empty_locators",
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
                validate_locator(entry.claim, locator_index, locator, &mut issues);
            }
        }

        for (index, count) in counts.into_iter().enumerate() {
            match count {
                0 => issues.push(issue(
                    "missing_evidence_entry",
                    format!("missing evidence for {} {index}", terms.claim),
                    Some(index),
                    None,
                )),
                1 => {}
                _ => issues.push(issue(
                    "duplicate_entry",
                    format!("{} {index} has {count} evidence entries", terms.claim),
                    Some(index),
                    None,
                )),
            }
        }

        issues
    }
}

fn validate_source(label: &str, source: &SourceRecord, issues: &mut Vec<EvidenceIssue>) {
    if source.source.is_empty() {
        issues.push(issue(
            "empty_source",
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
            "invalid_sha256",
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
            "empty_exact",
            "locator exact text is empty after normalization",
            Some(claim),
            Some(locator_index),
        ));
    } else if normalized != locator.exact {
        issues.push(issue(
            "unnormalized_exact",
            format!("locator exact text must be normalized as {normalized:?}"),
            Some(claim),
            Some(locator_index),
        ));
    }
    if locator.markdown.line == 0 {
        issues.push(issue(
            "invalid_markdown_line",
            "Markdown line must be one-based",
            Some(claim),
            Some(locator_index),
        ));
    }
    if locator.markdown.column == 0 {
        issues.push(issue(
            "invalid_markdown_column",
            "Markdown column must be one-based",
            Some(claim),
            Some(locator_index),
        ));
    }
    if locator.pdf.page == 0 {
        issues.push(issue(
            "invalid_pdf_page",
            "PDF page must be one-based",
            Some(claim),
            Some(locator_index),
        ));
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

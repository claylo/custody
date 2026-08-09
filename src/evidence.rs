use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{hash::sha256_bytes, markdown::UnitKind, normalize::normalize, terms::Terms};

/// The source name a document gets when it declares only one source pair.
pub const DEFAULT_SOURCE: &str = "default";

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
                "missing_evidence",
                "summary is missing evidence section",
                None,
                None,
            )];
        };
        let mut issues = Vec::new();

        if evidence.sources.is_empty() {
            issues.push(issue(
                "empty_sources",
                "evidence declares no sources",
                None,
                None,
            ));
        }
        for (name, pair) in &evidence.sources {
            if !validate_source_name(name) {
                issues.push(issue(
                    "invalid_source_name",
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
                if evidence.sources.contains_key(&locator.source) {
                    referenced.insert(locator.source.as_str());
                } else {
                    issues.push(issue(
                        "unknown_source",
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

        for name in evidence.sources.keys() {
            if !referenced.contains(name.as_str()) {
                issues.push(issue(
                    "unused_source",
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

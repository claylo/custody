use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::evidence::{ClaimEvidence, Evidence, EvidenceIssue, Severity};
use crate::hash::sha256_bytes;
use crate::normalize::normalize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Supported,
    Partial,
    Unsupported,
    Unclear,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewEntry {
    pub claim: usize,
    pub claim_sha256: String,
    pub evidence_sha256: String,
    pub verdict: Verdict,
    pub reviewer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub claims: Vec<ReviewEntry>,
}

/// Compute the canonical SHA-256 of a claim's locator set.
#[must_use]
pub fn evidence_sha256(entry: &ClaimEvidence) -> String {
    let canonical = canonical_locators_json(&entry.locators);
    sha256_bytes(canonical.as_bytes())
}

fn canonical_locators_json(locators: &[crate::evidence::Locator]) -> String {
    let values: Vec<serde_json::Value> = locators
        .iter()
        .map(|loc| {
            let mut locator = serde_json::to_value(loc).expect("Locator is serializable");
            normalize_exact_in_value(&mut locator);
            sort_keys(&locator)
        })
        .collect();
    serde_json::to_string(&values).expect("JSON array is serializable")
}

fn normalize_exact_in_value(value: &mut serde_json::Value) {
    if let Some(obj) = value.as_object_mut()
        && let Some(exact) = obj.get("exact").and_then(|v| v.as_str())
    {
        let normalized = normalize(exact);
        obj.insert("exact".to_owned(), serde_json::Value::String(normalized));
    }
}

/// Rebuild a JSON value with object keys sorted lexicographically at every level.
fn sort_keys(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                sorted.insert(key.clone(), sort_keys(&map[key]));
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(sort_keys).collect())
        }
        other => other.clone(),
    }
}

/// Validate review entries against their claims and evidence.
///
/// Checks: unknown claim indices, duplicate review entries, stale claim
/// hashes, and stale evidence hashes. Does not check verdict semantics —
/// `unsupported` is a valid recorded verdict unless `require_review` is set.
#[must_use]
pub fn validate_review(
    review: &Review,
    claims: &[String],
    evidence: Option<&Evidence>,
    require_review: bool,
) -> Vec<EvidenceIssue> {
    let mut issues = Vec::new();
    let mut reviewed: BTreeSet<usize> = BTreeSet::new();

    let evidence_entries: std::collections::BTreeMap<usize, &ClaimEvidence> = evidence
        .map(|ev| ev.claims.iter().map(|e| (e.claim, e)).collect())
        .unwrap_or_default();

    for entry in &review.claims {
        if entry.claim >= claims.len() {
            issues.push(issue(
                "unknown_review_claim",
                Severity::Error,
                format!(
                    "review entry references claim {} but only {} claims exist",
                    entry.claim,
                    claims.len()
                ),
                Some(entry.claim),
            ));
            continue;
        }

        if !reviewed.insert(entry.claim) {
            issues.push(issue(
                "duplicate_review_claim",
                Severity::Error,
                format!("duplicate review entry for claim {}", entry.claim),
                Some(entry.claim),
            ));
            continue;
        }

        let expected_claim_hash = sha256_bytes(claims[entry.claim].as_bytes());
        if entry.claim_sha256 != expected_claim_hash {
            issues.push(issue(
                "stale_review_claim",
                Severity::Error,
                format!(
                    "review claim_sha256 is stale for claim {}: expected {expected_claim_hash}, found {}",
                    entry.claim, entry.claim_sha256
                ),
                Some(entry.claim),
            ));
        }

        if let Some(ev_entry) = evidence_entries.get(&entry.claim) {
            let expected_evidence_hash = evidence_sha256(ev_entry);
            if entry.evidence_sha256 != expected_evidence_hash {
                issues.push(issue(
                    "stale_review_evidence",
                    Severity::Error,
                    format!(
                        "review evidence_sha256 is stale for claim {}: expected {expected_evidence_hash}, found {}",
                        entry.claim, entry.evidence_sha256
                    ),
                    Some(entry.claim),
                ));
            }
        }
    }

    if require_review {
        for index in 0..claims.len() {
            if !reviewed.contains(&index) {
                issues.push(issue(
                    "missing_review",
                    Severity::Error,
                    format!("claim {index} has no review entry"),
                    Some(index),
                ));
            }
        }

        for entry in &review.claims {
            if entry.claim < claims.len() && entry.verdict != Verdict::Supported {
                issues.push(issue(
                    "unsupported_verdict",
                    Severity::Error,
                    format!(
                        "claim {} verdict is {:?}, not supported",
                        entry.claim, entry.verdict
                    ),
                    Some(entry.claim),
                ));
            }
        }
    }

    issues
}

fn issue(
    code: impl Into<String>,
    severity: Severity,
    message: impl Into<String>,
    claim: Option<usize>,
) -> EvidenceIssue {
    EvidenceIssue {
        code: code.into(),
        severity,
        message: message.into(),
        claim,
        locator: None,
    }
}

use serde::{Deserialize, Serialize};

use crate::evidence::ClaimEvidence;
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

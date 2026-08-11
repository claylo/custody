//! Configurable claim vocabulary.
//!
//! A corpus may use any word for a claim. Input is canonicalized to `claims` at
//! the parse boundary and localized back on output, so every internal type and
//! every validation path speaks one vocabulary.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The words this corpus uses for a claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Terms {
    /// Singular form: messages, the entry key, and the `_sha256` prefix.
    pub claim: String,
    /// Plural form: the document-level key.
    pub claims: String,
}

impl Default for Terms {
    fn default() -> Self {
        Self {
            claim: "claim".to_owned(),
            claims: "claims".to_owned(),
        }
    }
}

impl Terms {
    /// Whether the configured vocabulary already matches the canonical one.
    #[must_use]
    pub fn is_canonical(&self) -> bool {
        self.claim == "claim" && self.claims == "claims"
    }

    /// Reject empty or ambiguous vocabulary.
    pub fn validate(&self) -> Result<()> {
        if self.claim.is_empty() {
            bail!("terms.claim must not be empty");
        }
        if self.claims.is_empty() {
            bail!("terms.claims must not be empty");
        }
        if self.claim == self.claims {
            bail!("terms.claim and terms.claims must differ");
        }
        Ok(())
    }

    fn hash_key(&self) -> String {
        format!("{}_sha256", self.claim)
    }

    /// Rewrite the configured vocabulary to the canonical one, in place.
    ///
    /// Only four known paths are touched, so nothing deeper in a caller's own
    /// document is ever renamed by accident.
    pub fn canonicalize(&self, value: &mut Value) -> Result<()> {
        self.validate()?;
        if self.is_canonical() {
            return Ok(());
        }
        rename_strict(value, &self.claims, "claims")?;
        if let Some(evidence) = value.get_mut("evidence") {
            rename_strict(evidence, &self.claims, "claims")?;
            if let Some(entries) = evidence.get_mut("claims").and_then(Value::as_array_mut) {
                let hash_key = self.hash_key();
                for entry in entries {
                    rename_strict(entry, &self.claim, "claim")?;
                    rename_strict(entry, &hash_key, "claim_sha256")?;
                }
            }
        }
        if let Some(review) = value.get_mut("review") {
            rename_strict(review, &self.claims, "claims")?;
            if let Some(entries) = review.get_mut("claims").and_then(Value::as_array_mut) {
                let hash_key = self.hash_key();
                for entry in entries {
                    rename_strict(entry, &self.claim, "claim")?;
                    rename_strict(entry, &hash_key, "claim_sha256")?;
                }
            }
        }
        Ok(())
    }

    /// Rewrite the canonical vocabulary back to the configured one, in place.
    pub fn localize(&self, value: &mut Value) {
        if self.is_canonical() {
            return;
        }
        if let Some(evidence) = value.get_mut("evidence") {
            if let Some(entries) = evidence.get_mut("claims").and_then(Value::as_array_mut) {
                for entry in entries {
                    self.localize_entry(entry);
                }
            }
            rename(evidence, "claims", &self.claims);
        }
        if let Some(review) = value.get_mut("review") {
            if let Some(entries) = review.get_mut("claims").and_then(Value::as_array_mut) {
                for entry in entries {
                    self.localize_entry(entry);
                }
            }
            rename(review, "claims", &self.claims);
        }
        rename(value, "claims", &self.claims);
    }

    /// Rewrite one claim-evidence entry to the configured vocabulary, in place.
    pub fn localize_entry(&self, entry: &mut Value) {
        if self.is_canonical() {
            return;
        }
        rename(entry, "claim_sha256", &self.hash_key());
        rename(entry, "claim", &self.claim);
    }

    /// Rewrite a `locate` record to the configured vocabulary, in place.
    ///
    /// A locate record wraps a single entry under a `claim` field, so both the
    /// wrapper and the entry's own keys need renaming.
    pub fn localize_locate(&self, value: &mut Value) {
        if self.is_canonical() {
            return;
        }
        if let Some(entry) = value.get_mut("claim") {
            self.localize_entry(entry);
        }
        rename(value, "claim", &self.claim);
    }
}

fn rename_strict(value: &mut Value, from: &str, to: &str) -> Result<()> {
    if from != to
        && let Some(object) = value.as_object()
        && object.contains_key(from)
        && object.contains_key(to)
    {
        bail!("document uses both {from:?} and {to:?}; use one vocabulary");
    }
    rename(value, from, to);
    Ok(())
}

/// Move `from` to `to` within one object, preserving insertion order.
fn rename(value: &mut Value, from: &str, to: &str) {
    if from == to {
        return;
    }
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if !object.contains_key(from) {
        return;
    }
    // Rebuild in place so the renamed key keeps its original position rather
    // than moving to the end, which would churn `locate` output ordering.
    let existing = std::mem::take(object);
    *object = existing
        .into_iter()
        .map(|(key, entry)| {
            if key == from {
                (to.to_owned(), entry)
            } else {
                (key, entry)
            }
        })
        .collect();
}

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
        rename(value, &self.claims, "claims", true)?;
        if let Some(evidence) = value.get_mut("evidence") {
            rename(evidence, &self.claims, "claims", true)?;
            if let Some(entries) = evidence.get_mut("claims").and_then(Value::as_array_mut) {
                let hash_key = self.hash_key();
                for entry in entries {
                    rename(entry, &self.claim, "claim", true)?;
                    rename(entry, &hash_key, "claim_sha256", true)?;
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
            drop(rename(evidence, "claims", &self.claims, false));
        }
        drop(rename(value, "claims", &self.claims, false));
    }

    /// Rewrite one claim-evidence entry to the configured vocabulary, in place.
    pub fn localize_entry(&self, entry: &mut Value) {
        if self.is_canonical() {
            return;
        }
        drop(rename(entry, "claim_sha256", &self.hash_key(), false));
        drop(rename(entry, "claim", &self.claim, false));
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
        drop(rename(value, "claim", &self.claim, false));
    }
}

/// Move `from` to `to` within one object, preserving insertion order.
///
/// With `strict`, a pre-existing `to` key is a conflict rather than something to
/// overwrite.
fn rename(value: &mut Value, from: &str, to: &str, strict: bool) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    if !object.contains_key(from) {
        return Ok(());
    }
    if strict && object.contains_key(to) {
        bail!("document uses both {from:?} and {to:?}; use one vocabulary");
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
    Ok(())
}

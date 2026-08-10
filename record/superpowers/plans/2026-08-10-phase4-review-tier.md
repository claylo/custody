# Phase 4: Review Tier — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a review tier that records semantic verdicts (supported/partial/unsupported/unclear) bound to both claim text and evidence set via dual SHA-256, with staleness detection when either changes.

**Architecture:** New `src/review.rs` module for review data types, parsing/desugaring, evidence hashing, and structural validation. `SummaryDocument` gains an optional `review` field. `validate_document` gains review staleness checks. CLI gains `--require-review` on `check` and `audit`. Review data flows through the existing `parse_summary` → `desugar` → `validate` pipeline.

**Tech Stack:** Rust, existing deps only. Reuses `hash`, `evidence`, `normalize` modules. SHA-256 via `sha2` crate (already a dependency).

**Spec reference:** `record/superpowers/specs/2026-07-30-multi-source-and-support-design.md`, Phase 4 section (lines 436–495).

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/review.rs` | Review data types, verdict enum, evidence hash computation, structural validation |
| Modify | `src/lib.rs` | Register `review` module |
| Modify | `src/evidence.rs` | Add `review` field to `SummaryDocument`, wire desugaring |
| Modify | `src/validate.rs` | Review staleness checks during validation |
| Modify | `src/cli.rs` | `--require-review` flag on `check` and `audit`, review columns in audit |
| Create | `tests/review.rs` | Unit tests for evidence hashing and review validation |
| Modify | `tests/validate.rs` | Integration tests for review staleness during validation |
| Modify | `tests/cli.rs` | CLI integration tests for `--require-review` |

---

### Task 1: Review data types and evidence hashing

Create `src/review.rs` with the core types and the canonical evidence hash function.

**Files:**
- Create: `src/review.rs`
- Modify: `src/lib.rs`
- Create: `tests/review.rs`

- [ ] **Step 1: Create `src/review.rs` with data types**

```rust
use serde::{Deserialize, Serialize};

use crate::evidence::{ClaimEvidence, EvidenceIssue, Severity};
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
```

- [ ] **Step 2: Implement `evidence_sha256` — the canonical hash of a claim's locator set**

The spec says: SHA-256 of the canonical JSON serialization of that claim's `locators` array, with object keys sorted lexicographically and `exact` values normalized. Array order is significant.

```rust
/// Compute the canonical SHA-256 of a claim's locator set.
///
/// The hash covers the full JSON serialization of the `locators` array with
/// object keys sorted lexicographically and `exact` values normalized.
/// Array order is significant: reordering locators changes the hash.
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
    if let Some(obj) = value.as_object_mut() {
        if let Some(exact) = obj.get("exact").and_then(|v| v.as_str()) {
            let normalized = normalize(exact);
            obj.insert("exact".to_owned(), serde_json::Value::String(normalized));
        }
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
```

- [ ] **Step 3: Register module in `src/lib.rs`**

Add `pub mod review;` to `src/lib.rs`, keeping alphabetical order.

- [ ] **Step 4: Write evidence hash tests**

Create `tests/review.rs`:

```rust
use receipts::evidence::{
    ClaimEvidence, DEFAULT_SOURCE, Locator, MarkdownLocator, PdfBackend, PdfLocator,
};
use receipts::hash::sha256_bytes;
use receipts::markdown::UnitKind;
use receipts::review::evidence_sha256;

fn test_locator(exact: &str, page: usize) -> Locator {
    Locator {
        source: DEFAULT_SOURCE.to_owned(),
        exact: exact.to_owned(),
        markdown: MarkdownLocator {
            line: 1,
            column: 1,
            unit: UnitKind::Paragraph,
            section: vec![],
        },
        pdf: PdfLocator {
            page,
            backend: PdfBackend::MutoolNative,
        },
    }
}

fn test_entry(locators: Vec<Locator>) -> ClaimEvidence {
    let claim = "test claim".to_owned();
    ClaimEvidence {
        claim: 0,
        claim_sha256: sha256_bytes(claim.as_bytes()),
        locators,
    }
}

#[test]
fn evidence_hash_is_deterministic() {
    let entry = test_entry(vec![test_locator("some evidence text", 1)]);
    let hash1 = evidence_sha256(&entry);
    let hash2 = evidence_sha256(&entry);
    assert_eq!(hash1, hash2);
}

#[test]
fn evidence_hash_is_valid_sha256() {
    let entry = test_entry(vec![test_locator("some evidence text", 1)]);
    let hash = evidence_sha256(&entry);
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn evidence_hash_changes_when_locator_swapped() {
    let entry_a = test_entry(vec![test_locator("text alpha", 1)]);
    let entry_b = test_entry(vec![test_locator("text beta", 1)]);
    assert_ne!(evidence_sha256(&entry_a), evidence_sha256(&entry_b));
}

#[test]
fn evidence_hash_changes_when_locator_added() {
    let entry_one = test_entry(vec![test_locator("text alpha", 1)]);
    let entry_two = test_entry(vec![
        test_locator("text alpha", 1),
        test_locator("text beta", 2),
    ]);
    assert_ne!(evidence_sha256(&entry_one), evidence_sha256(&entry_two));
}

#[test]
fn evidence_hash_changes_when_locators_reordered() {
    let entry_ab = test_entry(vec![
        test_locator("text alpha", 1),
        test_locator("text beta", 2),
    ]);
    let entry_ba = test_entry(vec![
        test_locator("text beta", 2),
        test_locator("text alpha", 1),
    ]);
    assert_ne!(evidence_sha256(&entry_ab), evidence_sha256(&entry_ba));
}

#[test]
fn evidence_hash_changes_when_page_changes() {
    let entry_p1 = test_entry(vec![test_locator("same text", 1)]);
    let entry_p2 = test_entry(vec![test_locator("same text", 2)]);
    assert_ne!(evidence_sha256(&entry_p1), evidence_sha256(&entry_p2));
}

#[test]
fn evidence_hash_changes_when_source_changes() {
    let mut locator = test_locator("same text", 1);
    let entry_default = test_entry(vec![locator.clone()]);
    locator.source = "supplement".to_owned();
    let entry_supp = test_entry(vec![locator]);
    assert_ne!(evidence_sha256(&entry_default), evidence_sha256(&entry_supp));
}

#[test]
fn evidence_hash_normalizes_exact_whitespace() {
    let entry_spaces = test_entry(vec![test_locator("some  extra   spaces", 1)]);
    let entry_normal = test_entry(vec![test_locator("some extra spaces", 1)]);
    assert_eq!(
        evidence_sha256(&entry_spaces),
        evidence_sha256(&entry_normal),
        "whitespace normalization should produce identical hashes"
    );
}

#[test]
fn evidence_hash_empty_locators() {
    let entry = test_entry(vec![]);
    let hash = evidence_sha256(&entry);
    assert_eq!(hash.len(), 64, "empty locators should still produce a valid hash");
}
```

- [ ] **Step 5: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 6: Commit**

```
feat(review): add review data types and evidence hashing

evidence_sha256 produces a canonical SHA-256 of a claim's locator
set with sorted keys and normalized exact values. Array order is
significant: reordering locators invalidates the review.
```

---

### Task 2: Wire review into SummaryDocument parsing

Add the `review` field to `SummaryDocument` and handle desugaring through `parse_summary`.

**Files:**
- Modify: `src/evidence.rs`

- [ ] **Step 1: Add `review` field to `SummaryDocument`**

In `src/evidence.rs`, add the import and field:

```rust
// Add to the existing use statement at the top:
use crate::review::Review;
```

Add the field to `SummaryDocument`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct SummaryDocument {
    pub id: String,
    pub claims: Vec<String>,
    #[serde(default)]
    pub evidence: Option<Evidence>,
    #[serde(default)]
    pub review: Option<Review>,
}
```

- [ ] **Step 2: Add review canonicalization in `desugar_evidence_sources`**

The `review.claims` entries use the same `claim`/`claim_sha256` vocabulary as evidence entries, so they need the same canonicalization. Add at the end of `desugar_evidence_sources`, before the final `Ok(())`:

```rust
if let Some(review) = value.get_mut("review").and_then(Value::as_object_mut) {
    if let Some(entries) = review.get_mut("claims").and_then(Value::as_array_mut) {
        for entry in entries {
            if let Some(obj) = entry.as_object_mut() {
                obj.entry("claim")
                    .or_insert_with(|| Value::Null);
            }
        }
    }
}
```

Wait — review entries don't need the same `source` defaulting that evidence locators get. They only need vocabulary canonicalization. But vocabulary canonicalization happens in `terms.canonicalize()`, which already runs before `desugar_evidence_sources`. Let's check: `terms.canonicalize()` in `src/terms.rs` only renames keys within `evidence.claims[]` entries. Review entries sit under `review.claims[]`, which `canonicalize` doesn't touch.

We need to extend `Terms::canonicalize` to also handle review claim entries. Add at the end of the `canonicalize` method, before the final `Ok(())`:

```rust
if let Some(review) = value.get_mut("review") {
    rename(review, &self.claims, "claims", true)?;
    if let Some(entries) = review.get_mut("claims").and_then(Value::as_array_mut) {
        let hash_key = self.hash_key();
        for entry in entries {
            rename(entry, &self.claim, "claim", true)?;
            rename(entry, &hash_key, "claim_sha256", true)?;
        }
    }
}
```

And extend `localize` in `Terms` to also handle review:

```rust
if let Some(review) = value.get_mut("review") {
    if let Some(entries) = review.get_mut("claims").and_then(Value::as_array_mut) {
        for entry in entries {
            self.localize_entry(entry);
        }
    }
    drop(rename(review, "claims", &self.claims, false));
}
```

- [ ] **Step 3: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

All existing tests must still pass. The `review` field is `Option` with `#[serde(default)]`, so existing summaries without a `review` section parse unchanged.

- [ ] **Step 4: Commit**

```
feat(evidence): add optional review field to SummaryDocument

The review section is parsed alongside evidence and uses the same
vocabulary canonicalization for claim/claim_sha256 fields.
```

---

### Task 3: Review structural validation

Add validation for review entries: unknown claim indices, duplicate entries, stale claim hashes, stale evidence hashes.

**Files:**
- Modify: `src/review.rs`
- Modify: `tests/review.rs`

- [ ] **Step 1: Add `validate_review_structure` to `review.rs`**

```rust
use std::collections::BTreeSet;

use crate::evidence::{ClaimEvidence, Evidence, EvidenceIssue, Severity};
use crate::hash::sha256_bytes;

/// Validate review entries against their claims and evidence.
///
/// Checks: unknown claim indices, duplicate review entries, stale claim
/// hashes, and stale evidence hashes. Does not check verdict semantics —
/// `unsupported` is a valid recorded verdict.
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
            if entry.verdict != Verdict::Supported {
                issues.push(issue(
                    "unsupported_verdict",
                    Severity::Error,
                    format!(
                        "claim {} verdict is {:?}, not supported",
                        entry.claim,
                        entry.verdict
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
```

- [ ] **Step 2: Write structural validation tests**

Add to `tests/review.rs`:

```rust
use receipts::evidence::{
    ClaimEvidence, DEFAULT_SOURCE, Evidence, Locator, MarkdownLocator, PdfBackend, PdfLocator,
    SourcePair, SourceRecord,
};
use receipts::review::{Review, ReviewEntry, Verdict, evidence_sha256, validate_review};
use std::collections::BTreeMap;

fn review_entry(claim: usize, claims: &[String], evidence: Option<&Evidence>) -> ReviewEntry {
    let claim_sha256 = sha256_bytes(claims[claim].as_bytes());
    let evidence_sha256_val = evidence
        .and_then(|ev| ev.claims.iter().find(|e| e.claim == claim))
        .map(|e| evidence_sha256(e))
        .unwrap_or_else(|| "0".repeat(64));
    ReviewEntry {
        claim,
        claim_sha256,
        evidence_sha256: evidence_sha256_val,
        verdict: Verdict::Supported,
        reviewer: "test-reviewer".to_owned(),
        note: None,
        at: None,
    }
}

fn test_claims() -> Vec<String> {
    vec!["Claim zero text.".to_owned(), "Claim one text.".to_owned()]
}

fn test_evidence(claims: &[String]) -> Evidence {
    Evidence {
        sources: BTreeMap::from([(
            DEFAULT_SOURCE.to_owned(),
            SourcePair {
                markdown: SourceRecord {
                    source: "md/test.md".to_owned(),
                    sha256: "a".repeat(64),
                },
                pdf: SourceRecord {
                    source: "pdfs/test.pdf".to_owned(),
                    sha256: "b".repeat(64),
                },
            },
        )]),
        claims: claims
            .iter()
            .enumerate()
            .map(|(i, c)| ClaimEvidence {
                claim: i,
                claim_sha256: sha256_bytes(c.as_bytes()),
                locators: vec![test_locator("some evidence", 1)],
            })
            .collect(),
    }
}

#[test]
fn valid_review_produces_no_issues() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let review = Review {
        claims: claims
            .iter()
            .enumerate()
            .map(|(i, _)| review_entry(i, &claims, Some(&evidence)))
            .collect(),
    };
    let issues = validate_review(&review, &claims, Some(&evidence), false);
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn unknown_review_claim_is_reported() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let mut entry = review_entry(0, &claims, Some(&evidence));
    entry.claim = 99;
    let review = Review {
        claims: vec![entry],
    };
    let issues = validate_review(&review, &claims, Some(&evidence), false);
    assert!(issues.iter().any(|i| i.code == "unknown_review_claim"), "{issues:?}");
}

#[test]
fn duplicate_review_claim_is_reported() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let entry = review_entry(0, &claims, Some(&evidence));
    let review = Review {
        claims: vec![entry.clone(), entry],
    };
    let issues = validate_review(&review, &claims, Some(&evidence), false);
    assert!(issues.iter().any(|i| i.code == "duplicate_review_claim"), "{issues:?}");
}

#[test]
fn stale_review_claim_is_reported() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let mut entry = review_entry(0, &claims, Some(&evidence));
    entry.claim_sha256 = "0".repeat(64);
    let review = Review {
        claims: vec![entry],
    };
    let issues = validate_review(&review, &claims, Some(&evidence), false);
    assert!(issues.iter().any(|i| i.code == "stale_review_claim"), "{issues:?}");
}

#[test]
fn stale_review_evidence_is_reported() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let mut entry = review_entry(0, &claims, Some(&evidence));
    entry.evidence_sha256 = "0".repeat(64);
    let review = Review {
        claims: vec![entry],
    };
    let issues = validate_review(&review, &claims, Some(&evidence), false);
    assert!(issues.iter().any(|i| i.code == "stale_review_evidence"), "{issues:?}");
}

#[test]
fn missing_review_reported_only_with_require_review() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let review = Review {
        claims: vec![review_entry(0, &claims, Some(&evidence))],
    };
    let without = validate_review(&review, &claims, Some(&evidence), false);
    assert!(!without.iter().any(|i| i.code == "missing_review"), "{without:?}");

    let with = validate_review(&review, &claims, Some(&evidence), true);
    assert!(with.iter().any(|i| i.code == "missing_review"), "{with:?}");
}

#[test]
fn unsupported_verdict_reported_only_with_require_review() {
    let claims = vec!["Only claim.".to_owned()];
    let evidence = test_evidence(&claims);
    let mut entry = review_entry(0, &claims, Some(&evidence));
    entry.verdict = Verdict::Unsupported;
    let review = Review { claims: vec![entry] };

    let without = validate_review(&review, &claims, Some(&evidence), false);
    assert!(!without.iter().any(|i| i.code == "unsupported_verdict"), "{without:?}");

    let with = validate_review(&review, &claims, Some(&evidence), true);
    assert!(with.iter().any(|i| i.code == "unsupported_verdict"), "{with:?}");
}

#[test]
fn partial_verdict_reported_under_require_review() {
    let claims = vec!["Only claim.".to_owned()];
    let evidence = test_evidence(&claims);
    let mut entry = review_entry(0, &claims, Some(&evidence));
    entry.verdict = Verdict::Partial;
    let review = Review { claims: vec![entry] };

    let issues = validate_review(&review, &claims, Some(&evidence), true);
    assert!(issues.iter().any(|i| i.code == "unsupported_verdict"), "{issues:?}");
}

#[test]
fn unclear_verdict_reported_under_require_review() {
    let claims = vec!["Only claim.".to_owned()];
    let evidence = test_evidence(&claims);
    let mut entry = review_entry(0, &claims, Some(&evidence));
    entry.verdict = Verdict::Unclear;
    let review = Review { claims: vec![entry] };

    let issues = validate_review(&review, &claims, Some(&evidence), true);
    assert!(issues.iter().any(|i| i.code == "unsupported_verdict"), "{issues:?}");
}
```

- [ ] **Step 3: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 4: Commit**

```
feat(review): structural validation for review entries

Checks unknown claim indices, duplicate entries, stale claim hashes,
and stale evidence hashes. --require-review gates on missing reviews
and non-supported verdicts.
```

---

### Task 4: Wire review validation into `validate_document`

Call `validate_review` from the existing `validate_document` function when a review section is present.

**Files:**
- Modify: `src/validate.rs`
- Modify: `tests/validate.rs`

- [ ] **Step 1: Add review validation call in `validate_document`**

In `src/validate.rs`, add the import:

```rust
use crate::review;
```

At the end of `validate_document`, just before the final `ValidationReport { ... }` construction, add:

```rust
if let Some(rev) = summary.review.as_ref() {
    issues.extend(review::validate_review(
        rev,
        &summary.claims,
        summary.evidence.as_ref(),
        false,
    ));
}
```

Note: `require_review` is `false` by default. This will be plumbed from the CLI in Task 5. For now, `validate_document` needs a parameter to pass it through. Modify the signature:

Change `validate_document` to accept a `require_review: bool` parameter:

```rust
pub fn validate_document(
    corpus: &Corpus,
    summary: &SummaryDocument,
    provider: &impl PdfTextProvider,
    require_review: bool,
) -> ValidationReport {
```

And use it in the review validation call:

```rust
if let Some(rev) = summary.review.as_ref() {
    issues.extend(review::validate_review(
        rev,
        &summary.claims,
        summary.evidence.as_ref(),
        require_review,
    ));
} else if require_review {
    for index in 0..summary.claims.len() {
        issues.push(issue(
            "missing_review",
            Severity::Error,
            format!("claim {index} has no review entry"),
            Some(index),
            None,
        ));
    }
}
```

- [ ] **Step 2: Update all existing callers of `validate_document`**

In `src/cli.rs`, every call to `validate_document` now needs `false` (or the appropriate require_review value) as the fourth argument. For this step, add `false` to all existing calls:

In `check` function:
```rust
let validation = validate_document(corpus, &summary, &tools, false);
```

In `audit` function:
```rust
let validation = validate_document(corpus, &summary, &tools, false);
```

- [ ] **Step 3: Update all test callers of `validate_document`**

In `tests/validate.rs`, every call to `validate_document` needs `false` as the fourth argument. There are 9 calls to update. Add `false` after `&FakePdf { ... }` (or `&PerSourcePdf { ... }`) in each.

Example — change:
```rust
let report = validate_document(&fixture.corpus, &summary, &FakePdf { ... });
```
to:
```rust
let report = validate_document(&fixture.corpus, &summary, &FakePdf { ... }, false);
```

- [ ] **Step 4: Add integration test for review staleness during validation**

In `tests/validate.rs`, add:

```rust
use receipts::review::{Review, ReviewEntry, Verdict, evidence_sha256};

#[test]
fn stale_review_claim_detected_during_validation() {
    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    let evidence = summary.evidence.as_ref().unwrap();
    let ev_hash = evidence_sha256(&evidence.claims[0]);
    summary.review = Some(Review {
        claims: vec![ReviewEntry {
            claim: 0,
            claim_sha256: "0".repeat(64),
            evidence_sha256: ev_hash,
            verdict: Verdict::Supported,
            reviewer: "test".to_owned(),
            note: None,
            at: None,
        }],
    });
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );
    assert!(
        report.issues.iter().any(|i| i.code == "stale_review_claim"),
        "{:?}",
        report.issues
    );
}

#[test]
fn stale_review_evidence_detected_during_validation() {
    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    let claim_hash = sha256_bytes(summary.claims[0].as_bytes());
    summary.review = Some(Review {
        claims: vec![ReviewEntry {
            claim: 0,
            claim_sha256: claim_hash,
            evidence_sha256: "0".repeat(64),
            verdict: Verdict::Supported,
            reviewer: "test".to_owned(),
            note: None,
            at: None,
        }],
    });
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );
    assert!(
        report.issues.iter().any(|i| i.code == "stale_review_evidence"),
        "{:?}",
        report.issues
    );
}

#[test]
fn valid_review_produces_no_issues_during_validation() {
    let fixture = Fixture::new("Supported once.");
    let mut summary = fixture.summary("Supported once", PdfBackend::MutoolNative);
    let claim_hash = sha256_bytes(summary.claims[0].as_bytes());
    let ev_hash = evidence_sha256(&summary.evidence.as_ref().unwrap().claims[0]);
    summary.review = Some(Review {
        claims: vec![ReviewEntry {
            claim: 0,
            claim_sha256: claim_hash,
            evidence_sha256: ev_hash,
            verdict: Verdict::Supported,
            reviewer: "test".to_owned(),
            note: None,
            at: None,
        }],
    });
    let report = validate_document(
        &fixture.corpus,
        &summary,
        &FakePdf {
            pages: HashMap::from([((PdfBackend::MutoolNative, 1), "Supported once.".to_owned())]),
        },
        false,
    );
    assert!(report.issues.is_empty(), "{:?}", report.issues);
}
```

- [ ] **Step 5: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 6: Commit**

```
feat(validate): check review staleness during document validation

validate_document now checks review entries for stale claim hashes
and stale evidence hashes. A new require_review parameter gates
missing-review and non-supported-verdict errors.
```

---

### Task 5: CLI `--require-review` flag

Add `--require-review` to `check` and `audit` subcommands. Plumb through to `validate_document`.

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Add `--require-review` to `CheckArgs`**

```rust
#[derive(Debug, Args)]
struct CheckArgs {
    ids: Vec<String>,
    /// Require review entries with supported verdicts for all claims.
    #[arg(long)]
    require_review: bool,
}
```

- [ ] **Step 2: Add `--require-review` to `AuditArgs`**

```rust
#[derive(Debug, Args)]
struct AuditArgs {
    /// Treat summaries without evidence as invalid.
    #[arg(long)]
    strict: bool,
    /// Require review entries with supported verdicts for all claims.
    #[arg(long)]
    require_review: bool,
    ids: Vec<String>,
}
```

- [ ] **Step 3: Plumb `require_review` through `check`**

In the `check` function, pass it to `validate_document`:

```rust
fn check(corpus: &Corpus, ids: &[String], json: bool, quiet: bool, require_review: bool) -> Result<()> {
```

Update the call:
```rust
let validation = validate_document(corpus, &summary, &tools, require_review);
```

Update the dispatch in `run()`:
```rust
Command::Check(args) => check(&corpus, &args.ids, json, quiet, args.require_review),
```

- [ ] **Step 4: Plumb `require_review` through `audit`**

In the `audit` function:

```rust
fn audit(corpus: &Corpus, strict: bool, require_review: bool, ids: &[String], json: bool, quiet: bool) -> Result<()> {
```

Update the call:
```rust
let validation = validate_document(corpus, &summary, &tools, require_review);
```

Update the dispatch in `run()`:
```rust
Command::Audit(args) => audit(&corpus, args.strict, args.require_review, &args.ids, json, quiet),
```

- [ ] **Step 5: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 6: Commit**

```
feat(cli): add --require-review flag to check and audit

When set, missing review entries and non-supported verdicts are
reported as errors. Default behavior is unchanged.
```

---

### Task 6: CLI integration tests for review

Add CLI-level integration tests exercising `--require-review` and review staleness detection.

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add a test for `--require-review` failing on missing reviews**

```rust
#[test]
fn require_review_fails_when_review_is_missing() {
    let corpus = fixture_corpus();
    let temp = corpus.path();
    let claim = "A claim without evidence.";
    let claim_sha = sha256_bytes(claim.as_bytes());
    let md_sha = sha256_file(&temp.join("md/missing-evidence/missing-evidence.md")).unwrap();
    let pdf_sha = sha256_file(&temp.join("pdfs/missing-evidence.pdf")).unwrap();
    fs::write(
        temp.join("summaries/missing-evidence.yaml"),
        format!(
            "id: missing-evidence
claims:
  - \"{claim}\"
evidence:
  markdown:
    source: \"md/missing-evidence/missing-evidence.md\"
    sha256: \"{md_sha}\"
  pdf:
    source: \"pdfs/missing-evidence.pdf\"
    sha256: \"{pdf_sha}\"
  claims:
    - claim: 0
      claim_sha256: \"{claim_sha}\"
      locators:
        - exact: \"A source.\"
          markdown:
            line: 1
            column: 1
            unit: paragraph
          pdf:
            page: 1
            backend: mutool-native
"
        ),
    )
    .unwrap();

    let output = receipts(temp, &["check", "--require-review", "missing-evidence"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("missing_review"),
        "should report missing review: {}",
        stderr(&output)
    );
}
```

- [ ] **Step 2: Add a test for `--require-review` passing with a supported review**

```rust
#[test]
fn require_review_passes_with_supported_verdict() {
    let corpus = fixture_corpus();
    let temp = corpus.path();
    let claim = "A claim without evidence.";
    let claim_sha = sha256_bytes(claim.as_bytes());
    let md_sha = sha256_file(&temp.join("md/missing-evidence/missing-evidence.md")).unwrap();
    let pdf_sha = sha256_file(&temp.join("pdfs/missing-evidence.pdf")).unwrap();

    // Build the evidence section first to compute evidence_sha256
    let evidence_yaml = format!(
        "evidence:
  markdown:
    source: \"md/missing-evidence/missing-evidence.md\"
    sha256: \"{md_sha}\"
  pdf:
    source: \"pdfs/missing-evidence.pdf\"
    sha256: \"{pdf_sha}\"
  claims:
    - claim: 0
      claim_sha256: \"{claim_sha}\"
      locators:
        - exact: \"A source.\"
          markdown:
            line: 1
            column: 1
            unit: paragraph
          pdf:
            page: 1
            backend: mutool-native"
    );

    // Compute evidence_sha256 from the locator
    use receipts::evidence::{
        ClaimEvidence, Locator, MarkdownLocator, PdfBackend, PdfLocator,
    };
    use receipts::markdown::UnitKind;
    use receipts::review::evidence_sha256;

    let ev_entry = ClaimEvidence {
        claim: 0,
        claim_sha256: claim_sha.clone(),
        locators: vec![Locator {
            source: "default".to_owned(),
            exact: "A source.".to_owned(),
            markdown: MarkdownLocator {
                line: 1,
                column: 1,
                unit: UnitKind::Paragraph,
                section: vec![],
            },
            pdf: PdfLocator {
                page: 1,
                backend: PdfBackend::MutoolNative,
            },
        }],
    };
    let ev_sha = evidence_sha256(&ev_entry);

    fs::write(
        temp.join("summaries/missing-evidence.yaml"),
        format!(
            "id: missing-evidence
claims:
  - \"{claim}\"
{evidence_yaml}
review:
  claims:
    - claim: 0
      claim_sha256: \"{claim_sha}\"
      evidence_sha256: \"{ev_sha}\"
      verdict: supported
      reviewer: test-reviewer
"
        ),
    )
    .unwrap();

    let output = receipts(temp, &["check", "--require-review", "missing-evidence"]);
    assert!(
        output.status.success(),
        "should pass with supported review: {}",
        stderr(&output)
    );
}
```

- [ ] **Step 3: Add a test for `--require-review` failing on unsupported verdict**

```rust
#[test]
fn require_review_fails_on_unsupported_verdict() {
    let corpus = fixture_corpus();
    let temp = corpus.path();
    let claim = "A claim without evidence.";
    let claim_sha = sha256_bytes(claim.as_bytes());
    let md_sha = sha256_file(&temp.join("md/missing-evidence/missing-evidence.md")).unwrap();
    let pdf_sha = sha256_file(&temp.join("pdfs/missing-evidence.pdf")).unwrap();

    use receipts::evidence::{
        ClaimEvidence, Locator, MarkdownLocator, PdfBackend, PdfLocator,
    };
    use receipts::markdown::UnitKind;
    use receipts::review::evidence_sha256;

    let ev_entry = ClaimEvidence {
        claim: 0,
        claim_sha256: claim_sha.clone(),
        locators: vec![Locator {
            source: "default".to_owned(),
            exact: "A source.".to_owned(),
            markdown: MarkdownLocator {
                line: 1,
                column: 1,
                unit: UnitKind::Paragraph,
                section: vec![],
            },
            pdf: PdfLocator {
                page: 1,
                backend: PdfBackend::MutoolNative,
            },
        }],
    };
    let ev_sha = evidence_sha256(&ev_entry);

    fs::write(
        temp.join("summaries/missing-evidence.yaml"),
        format!(
            "id: missing-evidence
claims:
  - \"{claim}\"
evidence:
  markdown:
    source: \"md/missing-evidence/missing-evidence.md\"
    sha256: \"{md_sha}\"
  pdf:
    source: \"pdfs/missing-evidence.pdf\"
    sha256: \"{pdf_sha}\"
  claims:
    - claim: 0
      claim_sha256: \"{claim_sha}\"
      locators:
        - exact: \"A source.\"
          markdown:
            line: 1
            column: 1
            unit: paragraph
          pdf:
            page: 1
            backend: mutool-native
review:
  claims:
    - claim: 0
      claim_sha256: \"{claim_sha}\"
      evidence_sha256: \"{ev_sha}\"
      verdict: unsupported
      reviewer: test-reviewer
"
        ),
    )
    .unwrap();

    let output = receipts(temp, &["check", "--require-review", "missing-evidence"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("unsupported_verdict"),
        "should report unsupported verdict: {}",
        stderr(&output)
    );
}
```

- [ ] **Step 4: Add a test for review staleness through the CLI**

```rust
#[test]
fn check_detects_stale_review_evidence() {
    let corpus = fixture_corpus();
    let temp = corpus.path();
    let claim = "A claim without evidence.";
    let claim_sha = sha256_bytes(claim.as_bytes());
    let md_sha = sha256_file(&temp.join("md/missing-evidence/missing-evidence.md")).unwrap();
    let pdf_sha = sha256_file(&temp.join("pdfs/missing-evidence.pdf")).unwrap();

    fs::write(
        temp.join("summaries/missing-evidence.yaml"),
        format!(
            "id: missing-evidence
claims:
  - \"{claim}\"
evidence:
  markdown:
    source: \"md/missing-evidence/missing-evidence.md\"
    sha256: \"{md_sha}\"
  pdf:
    source: \"pdfs/missing-evidence.pdf\"
    sha256: \"{pdf_sha}\"
  claims:
    - claim: 0
      claim_sha256: \"{claim_sha}\"
      locators:
        - exact: \"A source.\"
          markdown:
            line: 1
            column: 1
            unit: paragraph
          pdf:
            page: 1
            backend: mutool-native
review:
  claims:
    - claim: 0
      claim_sha256: \"{claim_sha}\"
      evidence_sha256: \"{stale}\"
      verdict: supported
      reviewer: test-reviewer
",
            stale = "0".repeat(64)
        ),
    )
    .unwrap();

    let output = receipts(temp, &["check", "missing-evidence"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("stale_review_evidence"),
        "should detect stale review evidence: {}",
        stderr(&output)
    );
}
```

- [ ] **Step 5: Run tests**

```sh
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] **Step 6: Commit**

```
test(cli): add integration tests for --require-review

Tests cover missing review, supported verdict passing, unsupported
verdict failing, and stale evidence hash detection through the CLI.
```

---

### Task 7: Final verification and handoff

**Files:**
- Create: `.handoffs/2026-08-10-phase-4-complete.md`

- [ ] **Step 1: Run full verification suite**

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --locked
cargo test --locked
cargo run --locked -- --format text doctor
```

All must pass.

- [ ] **Step 2: Write handoff document**

Write `.handoffs/2026-08-10-phase-4-complete.md` following the format of previous handoffs.

- [ ] **Step 3: Commit the plan and handoff**

```
docs: add Phase 4 implementation plan and handoff
```

---

## Verification

After all tasks:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --locked
cargo test --locked
cargo run --locked -- --format text doctor
```

Expected: review section is parsed, validated for staleness, and `--require-review` gates on missing/non-supported verdicts. All existing tests still pass. No regressions in any subcommand.

use std::collections::BTreeMap;

use receipts::coordinate::{ClaimIndex, Column, Line, Page};
use receipts::evidence::{
    ClaimEvidence, DEFAULT_SOURCE, Evidence, Locator, MarkdownLocator, PdfBackend, PdfLocator,
    SourcePair, SourceRecord,
};
use receipts::hash::sha256_bytes;
use receipts::markdown::UnitKind;
use receipts::review::{Review, ReviewEntry, Verdict, evidence_sha256, validate_review};
use receipts::terms::Terms;

fn test_locator(exact: &str, page: usize) -> Locator {
    Locator {
        source: DEFAULT_SOURCE.to_owned(),
        exact: exact.to_owned(),
        markdown: Some(MarkdownLocator {
            line: Line::new(1).unwrap(),
            column: Column::new(1).unwrap(),
            unit: UnitKind::Paragraph,
            section: vec![],
        }),
        pdf: PdfLocator {
            page: Page::new(page).unwrap(),
            backend: PdfBackend::MutoolNative,
        },
    }
}

fn test_entry(locators: Vec<Locator>) -> ClaimEvidence {
    let claim = "test claim".to_owned();
    ClaimEvidence {
        claim: ClaimIndex::new(0),
        claim_sha256: sha256_bytes(claim.as_bytes()),
        locators,
    }
}

fn review_entry_for(claim: usize, claims: &[String], evidence: Option<&Evidence>) -> ReviewEntry {
    let claim_sha256 = sha256_bytes(claims[claim].as_bytes());
    let evidence_sha256_val = evidence
        .and_then(|ev| ev.claims.iter().find(|e| e.claim == claim))
        .map_or_else(|| "0".repeat(64), evidence_sha256);
    ReviewEntry {
        claim: ClaimIndex::new(claim),
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
                markdown: Some(SourceRecord {
                    source: "md/test.md".to_owned(),
                    sha256: "a".repeat(64),
                }),
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
                claim: ClaimIndex::new(i),
                claim_sha256: sha256_bytes(c.as_bytes()),
                locators: vec![test_locator("some evidence", 1)],
            })
            .collect(),
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
    assert_ne!(
        evidence_sha256(&entry_default),
        evidence_sha256(&entry_supp)
    );
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
    assert_eq!(
        hash.len(),
        64,
        "empty locators should still produce a valid hash"
    );
}

#[test]
fn valid_review_produces_no_issues() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let review = Review {
        claims: claims
            .iter()
            .enumerate()
            .map(|(i, _)| review_entry_for(i, &claims, Some(&evidence)))
            .collect(),
    };
    let issues = validate_review(&review, &claims, Some(&evidence), false, &Terms::default());
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn unknown_review_claim_is_reported() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let mut entry = review_entry_for(0, &claims, Some(&evidence));
    entry.claim = ClaimIndex::new(99);
    let review = Review {
        claims: vec![entry],
    };
    let issues = validate_review(&review, &claims, Some(&evidence), false, &Terms::default());
    assert!(
        issues.iter().any(|i| i.code == "unknown_review_claim"),
        "{issues:?}"
    );
}

#[test]
fn duplicate_review_claim_is_reported() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let entry = review_entry_for(0, &claims, Some(&evidence));
    let review = Review {
        claims: vec![entry.clone(), entry],
    };
    let issues = validate_review(&review, &claims, Some(&evidence), false, &Terms::default());
    assert!(
        issues.iter().any(|i| i.code == "duplicate_review_claim"),
        "{issues:?}"
    );
}

#[test]
fn stale_review_claim_is_reported() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let mut entry = review_entry_for(0, &claims, Some(&evidence));
    entry.claim_sha256 = "0".repeat(64);
    let review = Review {
        claims: vec![entry],
    };
    let issues = validate_review(&review, &claims, Some(&evidence), false, &Terms::default());
    assert!(
        issues.iter().any(|i| i.code == "stale_review_claim"),
        "{issues:?}"
    );
}

#[test]
fn stale_review_evidence_is_reported() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let mut entry = review_entry_for(0, &claims, Some(&evidence));
    entry.evidence_sha256 = "0".repeat(64);
    let review = Review {
        claims: vec![entry],
    };
    let issues = validate_review(&review, &claims, Some(&evidence), false, &Terms::default());
    assert!(
        issues.iter().any(|i| i.code == "stale_review_evidence"),
        "{issues:?}"
    );
}

#[test]
fn missing_review_reported_only_with_require_review() {
    let claims = test_claims();
    let evidence = test_evidence(&claims);
    let review = Review {
        claims: vec![review_entry_for(0, &claims, Some(&evidence))],
    };
    let without = validate_review(&review, &claims, Some(&evidence), false, &Terms::default());
    assert!(
        !without.iter().any(|i| i.code == "missing_review"),
        "{without:?}"
    );

    let with = validate_review(&review, &claims, Some(&evidence), true, &Terms::default());
    assert!(with.iter().any(|i| i.code == "missing_review"), "{with:?}");
}

#[test]
fn unsupported_verdict_reported_only_with_require_review() {
    let claims = vec!["Only claim.".to_owned()];
    let evidence = test_evidence(&claims);
    let mut entry = review_entry_for(0, &claims, Some(&evidence));
    entry.verdict = Verdict::Unsupported;
    let review = Review {
        claims: vec![entry],
    };

    let without = validate_review(&review, &claims, Some(&evidence), false, &Terms::default());
    assert!(
        !without.iter().any(|i| i.code == "unsupported_verdict"),
        "{without:?}"
    );

    let with = validate_review(&review, &claims, Some(&evidence), true, &Terms::default());
    let issue = with
        .iter()
        .find(|issue| issue.code == "unsupported_verdict")
        .unwrap();
    assert!(issue.message.contains("verdict is unsupported"));
}

#[test]
fn partial_verdict_reported_under_require_review() {
    let claims = vec!["Only claim.".to_owned()];
    let evidence = test_evidence(&claims);
    let mut entry = review_entry_for(0, &claims, Some(&evidence));
    entry.verdict = Verdict::Partial;
    let review = Review {
        claims: vec![entry],
    };

    let issues = validate_review(&review, &claims, Some(&evidence), true, &Terms::default());
    assert!(
        issues.iter().any(|i| i.code == "unsupported_verdict"),
        "{issues:?}"
    );
}

#[test]
fn unclear_verdict_reported_under_require_review() {
    let claims = vec!["Only claim.".to_owned()];
    let evidence = test_evidence(&claims);
    let mut entry = review_entry_for(0, &claims, Some(&evidence));
    entry.verdict = Verdict::Unclear;
    let review = Review {
        claims: vec![entry],
    };

    let issues = validate_review(&review, &claims, Some(&evidence), true, &Terms::default());
    assert!(
        issues.iter().any(|i| i.code == "unsupported_verdict"),
        "{issues:?}"
    );
}

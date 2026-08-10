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

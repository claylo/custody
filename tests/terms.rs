use receipts::terms::Terms;
use serde_json::json;

fn proposition() -> Terms {
    Terms {
        claim: "proposition".to_owned(),
        claims: "propositions".to_owned(),
    }
}

#[test]
fn default_terms_are_claim_and_claims() {
    let terms = Terms::default();
    assert_eq!(terms.claim, "claim");
    assert_eq!(terms.claims, "claims");
    assert!(terms.is_canonical());
}

#[test]
fn canonicalize_is_a_noop_for_default_terms() {
    let mut value = json!({"id": "smith-2019", "claims": ["one"]});
    let expected = value.clone();
    Terms::default().canonicalize(&mut value).unwrap();
    assert_eq!(value, expected);
}

#[test]
fn canonicalize_renames_the_document_level_key() {
    let mut value = json!({"id": "smith-2019", "propositions": ["one"]});
    proposition().canonicalize(&mut value).unwrap();
    assert_eq!(value, json!({"id": "smith-2019", "claims": ["one"]}));
}

#[test]
fn canonicalize_renames_nested_evidence_keys() {
    let mut value = json!({
        "id": "smith-2019",
        "propositions": ["one"],
        "evidence": {
            "markdown": {"source": "md/smith-2019.md", "sha256": "a"},
            "pdf": {"source": "pdfs/smith-2019.pdf", "sha256": "b"},
            "propositions": [
                {"proposition": 0, "proposition_sha256": "c", "locators": []}
            ]
        }
    });
    proposition().canonicalize(&mut value).unwrap();

    let evidence = value.get("evidence").unwrap();
    let entries = evidence.get("claims").unwrap().as_array().unwrap();
    assert_eq!(entries[0].get("claim").unwrap(), 0);
    assert_eq!(entries[0].get("claim_sha256").unwrap(), "c");
    assert!(evidence.get("propositions").is_none());
}

#[test]
fn localize_is_the_inverse_of_canonicalize() {
    let original = json!({
        "id": "smith-2019",
        "propositions": ["one"],
        "evidence": {
            "propositions": [
                {"proposition": 0, "proposition_sha256": "c", "locators": []}
            ]
        }
    });
    let mut value = original.clone();
    let terms = proposition();
    terms.canonicalize(&mut value).unwrap();
    terms.localize(&mut value);
    assert_eq!(value, original);
}

#[test]
fn canonicalize_rejects_a_document_using_both_vocabularies() {
    let mut value = json!({"claims": ["one"], "propositions": ["two"]});
    assert!(proposition().canonicalize(&mut value).is_err());
}

#[test]
fn terms_reject_empty_or_conflicting_words() {
    assert!(
        Terms {
            claim: String::new(),
            claims: "x".to_owned()
        }
        .validate()
        .is_err()
    );
    assert!(
        Terms {
            claim: "x".to_owned(),
            claims: String::new()
        }
        .validate()
        .is_err()
    );
    assert!(
        Terms {
            claim: "same".to_owned(),
            claims: "same".to_owned()
        }
        .validate()
        .is_err()
    );
}

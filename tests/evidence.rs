use std::fmt::Write as _;

use receipts::{
    evidence::{PdfBackend, parse_summary},
    hash::sha256_bytes,
    terms::Terms,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn parses_the_complete_contract_and_hashes_decoded_claim_values() {
    let claim = "A folded claim with one logical line.\n";
    let claim_hash = sha256_bytes(claim.as_bytes());
    let yaml = valid_yaml(&claim_hash);
    let summary = parse_summary(&yaml, &Terms::default()).unwrap();

    assert_eq!(summary.id, "smith-2019");
    assert_eq!(summary.claims, [claim]);
    assert_eq!(
        summary.evidence.as_ref().unwrap().claims[0].locators[0]
            .pdf
            .backend,
        PdfBackend::MutoolNative
    );
    assert_eq!(summary.claim_hash(0).unwrap(), claim_hash);
    assert!(
        summary
            .validate_evidence_structure(&Terms::default())
            .is_empty()
    );
}

#[test]
fn reports_missing_duplicate_and_out_of_range_claim_coverage() {
    let claim_hash = sha256_bytes(b"A folded claim with one logical line.\n");
    let mut yaml = valid_yaml(&claim_hash);
    // A duplicate entry for claim 0 and an out-of-range entry for claim 9.
    write!(
        yaml,
        "\n    - claim: 0\n      claim_sha256: \"{claim_hash}\"\n      locators: []\n\
         \n    - claim: 9\n      claim_sha256: \"{claim_hash}\"\n      locators: []\n"
    )
    .unwrap();
    let summary = parse_summary(&yaml, &Terms::default()).unwrap();
    let issues = summary.validate_evidence_structure(&Terms::default());

    assert!(issues.iter().any(|issue| issue.code == "duplicate_entry"));
    assert!(
        issues
            .iter()
            .any(|issue| issue.code == "entry_out_of_range")
    );
    assert!(issues.iter().any(|issue| issue.code == "empty_locators"));
}

#[test]
fn reports_stale_claim_hashes_and_invalid_coordinates() {
    let mut yaml = valid_yaml(HASH_A);
    yaml = yaml.replace("line: 1", "line: 0");
    yaml = yaml.replace("page: 1", "page: 0");
    let summary = parse_summary(&yaml, &Terms::default()).unwrap();
    let issues = summary.validate_evidence_structure(&Terms::default());

    assert!(issues.iter().any(|issue| issue.code == "stale_hash"));
    assert!(
        issues
            .iter()
            .any(|issue| issue.code == "invalid_markdown_line")
    );
    assert!(issues.iter().any(|issue| issue.code == "invalid_pdf_page"));
}

#[test]
fn rejects_unknown_evidence_fields() {
    let claim_hash = sha256_bytes(b"A folded claim with one logical line.\n");
    let yaml = valid_yaml(&claim_hash).replace(
        "            backend: mutool-native",
        "            backend: mutool-native\n            occurrence: 2",
    );

    assert!(parse_summary(&yaml, &Terms::default()).is_err());
}

#[test]
fn requires_evidence_when_structural_validation_is_requested() {
    let summary = parse_summary(
        "id: smith-2019\nclaims:\n  - \"A claim long enough to validate.\"\n",
        &Terms::default(),
    )
    .unwrap();
    let issues = summary.validate_evidence_structure(&Terms::default());

    assert!(issues.iter().any(|issue| issue.code == "missing_evidence"));
}

fn valid_yaml(claim_hash: &str) -> String {
    format!(
        r#"id: smith-2019
claims:
  - >
    A folded claim with one logical line.
evidence:
  markdown:
    source: md/smith-2019/smith-2019.md
    sha256: "{HASH_A}"
  pdf:
    source: pdfs/smith-2019.pdf
    sha256: "{HASH_B}"
  claims:
    - claim: 0
      claim_sha256: "{claim_hash}"
      locators:
        - exact: "logical line"
          markdown:
            line: 1
            column: 1
            unit: paragraph
          pdf:
            page: 1
            backend: mutool-native
"#
    )
}

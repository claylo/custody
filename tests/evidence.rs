use std::{collections::BTreeMap, fmt::Write as _};

use receipts::{
    evidence::{
        ClaimEvidence, Evidence, Locator, MarkdownLocator, PdfBackend, PdfLocator, SourcePair,
        SourceRecord, SummaryDocument, parse_summary,
    },
    hash::sha256_bytes,
    markdown::UnitKind,
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

#[test]
fn desugars_bare_evidence_into_sources_default() {
    let claim_hash = sha256_bytes(b"A folded claim with one logical line.\n");
    let summary = parse_summary(&valid_yaml(&claim_hash), &Terms::default()).unwrap();
    let evidence = summary.evidence.unwrap();

    assert_eq!(evidence.sources.len(), 1);
    let default = &evidence.sources["default"];
    assert_eq!(default.markdown.source, "md/smith-2019/smith-2019.md");
    assert_eq!(default.pdf.source, "pdfs/smith-2019.pdf");
    assert_eq!(evidence.claims[0].locators[0].source, "default");
}

#[test]
fn rejects_conflicting_source_form() {
    let yaml = format!(
        r#"id: smith-2019
claims:
  - "Transport remained laminar."
evidence:
  markdown: {{source: "a.md", sha256: "{HASH_A}"}}
  pdf: {{source: "a.pdf", sha256: "{HASH_B}"}}
  sources:
    default:
      markdown: {{source: "a.md", sha256: "{HASH_A}"}}
      pdf: {{source: "a.pdf", sha256: "{HASH_B}"}}
  claims: []
"#
    );

    assert!(parse_summary(&yaml, &Terms::default()).is_err());
}

#[test]
fn parses_explicit_named_sources() {
    let claim = "Transport remained laminar.";
    let claim_hash = sha256_bytes(claim.as_bytes());
    let yaml = format!(
        r#"id: smith-2019
claims:
  - "{claim}"
evidence:
  sources:
    default:
      markdown: {{source: "md/smith-2019.md", sha256: "{HASH_A}"}}
      pdf: {{source: "pdfs/smith-2019.pdf", sha256: "{HASH_B}"}}
    supplement:
      markdown: {{source: "md/smith-2019-supp.md", sha256: "{HASH_A}"}}
      pdf: {{source: "pdfs/smith-2019-supp.pdf", sha256: "{HASH_B}"}}
  claims:
    - claim: 0
      claim_sha256: "{claim_hash}"
      locators:
        - source: supplement
          exact: "remained laminar"
          markdown: {{line: 1, column: 1, unit: paragraph}}
          pdf: {{page: 1, backend: mutool-native}}
"#
    );
    let summary = parse_summary(&yaml, &Terms::default()).unwrap();
    let evidence = summary.evidence.as_ref().unwrap();

    assert_eq!(evidence.sources.len(), 2);
    assert_eq!(
        evidence.sources["supplement"].pdf.source,
        "pdfs/smith-2019-supp.pdf"
    );
    assert_eq!(evidence.claims[0].locators[0].source, "supplement");
    // "default" is declared but no locator cites it.
    let issues = summary.validate_evidence_structure(&Terms::default());
    assert!(
        issues
            .iter()
            .any(|issue| issue.code == "unused_source" && issue.message.contains("default")),
        "{issues:?}"
    );
}

#[test]
fn reports_unknown_and_unused_sources() {
    let claim = "Transport remained laminar.".to_owned();
    let summary = SummaryDocument {
        id: "smith-2019".to_owned(),
        claims: vec![claim.clone()],
        evidence: Some(Evidence {
            sources: BTreeMap::from([(
                "main".to_owned(),
                SourcePair {
                    markdown: SourceRecord {
                        source: "md/smith-2019.md".to_owned(),
                        sha256: HASH_A.to_owned(),
                    },
                    pdf: SourceRecord {
                        source: "pdfs/smith-2019.pdf".to_owned(),
                        sha256: HASH_B.to_owned(),
                    },
                },
            )]),
            claims: vec![ClaimEvidence {
                claim: 0,
                claim_sha256: sha256_bytes(claim.as_bytes()),
                locators: vec![Locator {
                    source: "other".to_owned(),
                    exact: "remained laminar".to_owned(),
                    markdown: MarkdownLocator {
                        line: 1,
                        column: 1,
                        unit: UnitKind::Paragraph,
                        section: Vec::new(),
                    },
                    pdf: PdfLocator {
                        page: 1,
                        backend: PdfBackend::MutoolNative,
                    },
                }],
            }],
        }),
        review: None,
    };
    let issues = summary.validate_evidence_structure(&Terms::default());

    assert!(
        issues.iter().any(|issue| issue.code == "unknown_source"),
        "{issues:?}"
    );
    assert!(
        issues.iter().any(|issue| issue.code == "unused_source"),
        "{issues:?}"
    );
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

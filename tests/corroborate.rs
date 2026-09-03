use std::{fs, path::Path, process::Command};

fn run(corpus: &Path, format: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_receipts"))
        .arg("-C")
        .arg(corpus)
        .args(["--format", format])
        .args(args)
        .output()
        .expect("receipts executes")
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const LITERAL: &str = "The rate was 67.5% in the control group.";

/// One page, one native text line.
const TEXT_PDF: &str = "%PDF-1.4\n\
    1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
    2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
    3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]\
    /Resources<</Font<</F1 4 0 R>>>>/Contents 5 0 R>>endobj\n\
    4 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n\
    5 0 obj<</Length 72>>stream\n\
    BT /F1 12 Tf 72 700 Td (The rate was 67.5% in the control group.) Tj ET\n\
    endstream\nendobj\n\
    trailer<</Root 1 0 R>>\n";

/// Three sources over one PDF: `native` is PDF-only, `fulltext` is an
/// independent Markdown, `default` is converter Markdown.
fn corpus(extra_config: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for path in ["summaries", "md", "fulltext", "pdfs"] {
        fs::create_dir_all(temp.path().join(path)).unwrap();
    }
    fs::write(
        temp.path().join("receipts.yaml"),
        format!(
            concat!(
                "corpus:\n",
                "  summaries: \"summaries/{{id}}.yaml\"\n",
                "  sources:\n",
                "    default:\n",
                "      markdown: [\"md/{{id}}.md\"]\n",
                "      pdf: \"pdfs/{{id}}.pdf\"\n",
                "    fulltext:\n",
                "      markdown: [\"fulltext/{{id}}.md\"]\n",
                "      pdf: \"pdfs/{{id}}.pdf\"\n",
                "    native:\n",
                "      pdf: \"pdfs/{{id}}.pdf\"\n",
                "cache:\n  root: \".cache/pdf-text\"\n",
                "{}"
            ),
            extra_config
        ),
    )
    .unwrap();
    fs::write(temp.path().join("pdfs/smith-2019.pdf"), TEXT_PDF).unwrap();
    fs::write(
        temp.path().join("summaries/smith-2019.yaml"),
        "id: smith-2019\nclaims:\n  - The control group rate was 67.5%.\n",
    )
    .unwrap();
    temp
}

fn locate_native(corpus: &Path) -> serde_json::Value {
    let output = run(
        corpus,
        "json",
        &[
            "locate",
            "smith-2019",
            "--claim",
            "0",
            "--source",
            "native",
            "--exact",
            LITERAL,
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Turn a locate record into a complete summary with evidence.
fn write_evidence(corpus: &Path, record: &serde_json::Value) {
    let evidence = serde_json::json!({
        "id": "smith-2019",
        "claims": ["The control group rate was 67.5%."],
        "evidence": {
            "sources": { "native": { "pdf": record["pdf"] } },
            "claims": [record["claim"]],
        }
    });
    // JSON is valid YAML, and the summary parser accepts it as such.
    fs::write(
        corpus.join("summaries/smith-2019.yaml"),
        serde_json::to_string_pretty(&evidence).unwrap(),
    )
    .unwrap();
}

#[test]
fn locate_on_a_pdf_only_source_has_no_markdown_half() {
    let corpus = corpus("");
    let record = locate_native(corpus.path());

    assert_eq!(record["source"], "native");
    assert!(record.get("markdown").is_none(), "{record}");
    assert_eq!(record["pdf"]["source"], "pdfs/smith-2019.pdf");
    let locator = &record["claim"]["locators"][0];
    assert!(locator.get("markdown").is_none(), "{locator}");
    assert_eq!(locator["pdf"]["page"], 1);
    assert_eq!(locator["pdf"]["backend"], "mutool-native");

    let text = run(
        corpus.path(),
        "text",
        &[
            "locate",
            "smith-2019",
            "--claim",
            "0",
            "--source",
            "native",
            "--exact",
            LITERAL,
        ],
    );
    assert!(text.status.success(), "{}", stderr(&text));
    assert!(!stdout(&text).contains("markdown:"), "{}", stdout(&text));
}

#[test]
fn pdf_only_source_rejects_markdown_coordinates() {
    let corpus = corpus("");
    let output = run(
        corpus.path(),
        "json",
        &[
            "locate",
            "smith-2019",
            "--claim",
            "0",
            "--source",
            "native",
            "--exact",
            LITERAL,
            "--line",
            "1",
        ],
    );
    assert!(!output.status.success());
    assert!(stderr(&output).contains("PDF-only"));
}

#[test]
fn pdf_only_evidence_validates_as_a_single_leg() {
    let corpus = corpus("");
    let record = locate_native(corpus.path());
    write_evidence(corpus.path(), &record);

    let output = run(corpus.path(), "json", &["check", "smith-2019"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["valid"], 1);
    assert_eq!(report["corroborated"], 0);
    assert_eq!(report["single_leg"], 1);
    let legs = &report["summaries"][0]["claims"][0];
    assert_eq!(legs["legs"], serde_json::json!(["pdf"]));
    assert_eq!(legs["corroborated"], false);

    let text = run(corpus.path(), "text", &["audit"]);
    assert!(text.status.success(), "{}", stderr(&text));
    assert!(stdout(&text).contains("corroborated claims: 0\nsingle-leg claims: 1"));
}

#[test]
fn an_independent_markdown_corroborates_without_being_cited() {
    let corpus = corpus("");
    fs::write(
        corpus.path().join("fulltext/smith-2019.md"),
        "# Results\n\nThe rate was 67.5% in the control group. Nothing else changed.\n",
    )
    .unwrap();
    let record = locate_native(corpus.path());
    write_evidence(corpus.path(), &record);

    let output = run(corpus.path(), "json", &["audit"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["corroborated"], 1);
    assert_eq!(report["single_leg"], 0);
    let legs = &report["summaries"][0]["claims"][0];
    assert_eq!(legs["legs"], serde_json::json!(["pdf", "fulltext"]));
    assert_eq!(legs["corroborated"], true);
}

#[test]
fn derived_markdown_does_not_corroborate() {
    // Declare `default` as derived from the PDF: its agreement means nothing.
    let corpus = corpus("");
    let config = fs::read_to_string(corpus.path().join("receipts.yaml")).unwrap();
    fs::write(
        corpus.path().join("receipts.yaml"),
        config.replace(
            "      markdown: [\"md/{id}.md\"]\n",
            "      markdown: [\"md/{id}.md\"]\n      corroborates: false\n",
        ),
    )
    .unwrap();
    fs::write(
        corpus.path().join("md/smith-2019.md"),
        "The rate was 67.5% in the control group.\n",
    )
    .unwrap();
    let record = locate_native(corpus.path());
    write_evidence(corpus.path(), &record);

    let output = run(corpus.path(), "json", &["check", "smith-2019"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["single_leg"], 1);
    assert_eq!(
        report["summaries"][0]["claims"][0]["legs"],
        serde_json::json!(["pdf"])
    );
}

#[test]
fn single_leg_severity_can_fail_check() {
    let corpus = corpus("corroborate:\n  single_leg: error\n");
    let record = locate_native(corpus.path());
    write_evidence(corpus.path(), &record);

    let output = run(corpus.path(), "text", &["check", "smith-2019"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("single_leg"),
        "{}",
        stderr(&output)
    );
    assert!(stderr(&output).contains("confirmed by pdf only"));
}

#[test]
fn ocr_leg_corroborates_a_native_match_when_enabled() {
    let corpus = corpus("corroborate:\n  ocr: true\n");
    let record = locate_native(corpus.path());
    write_evidence(corpus.path(), &record);

    let output = run(corpus.path(), "json", &["check", "smith-2019"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let legs = &report["summaries"][0]["claims"][0];
    assert_eq!(legs["legs"], serde_json::json!(["pdf", "ocr"]), "{report}");
    assert_eq!(legs["corroborated"], true);
}

#[test]
fn markdown_half_on_a_pdf_only_source_is_rejected() {
    let corpus = corpus("");
    let record = locate_native(corpus.path());
    let mut broken = record.clone();
    broken["claim"]["locators"][0]["markdown"] =
        serde_json::json!({"line": 1, "column": 1, "unit": "paragraph"});
    write_evidence(corpus.path(), &broken);

    let output = run(corpus.path(), "text", &["check", "smith-2019"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("markdown_locator_mismatch"));
}

#[test]
fn extract_refuses_to_write_into_a_pdf_only_source() {
    let corpus = corpus("");
    let output = run(
        corpus.path(),
        "text",
        &["extract", "smith-2019", "--source", "native", "--write"],
    );
    assert!(!output.status.success());
    assert!(stderr(&output).contains("PDF-only"));
}

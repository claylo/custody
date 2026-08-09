use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn receipts(corpus: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_receipts"))
        .arg("-C")
        .arg(corpus)
        .args(["--format", "text"])
        .args(args)
        .output()
        .expect("receipts executes")
}

fn receipts_with_path(corpus: &Path, args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_receipts"))
        .arg("-C")
        .arg(corpus)
        .args(["--format", "text"])
        .args(args)
        .env("PATH", path)
        .output()
        .expect("receipts executes")
}

#[test]
fn targeted_check_fails_when_evidence_is_missing() {
    let corpus = fixture_corpus();
    let output = receipts(corpus.path(), &["check", "missing-evidence"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("missing evidence"));
}

#[test]
fn default_audit_reports_missing_without_failing() {
    let corpus = fixture_corpus();
    let output = receipts(corpus.path(), &["audit"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("missing: 1"));
}

#[test]
fn strict_audit_fails_on_missing_evidence() {
    let corpus = fixture_corpus();
    let output = receipts(corpus.path(), &["audit", "--strict"]);

    assert!(!output.status.success());
    assert!(stdout(&output).contains("missing: 1"));
}

#[test]
fn targeted_audit_accepts_summary_ids() {
    let corpus = fixture_corpus();
    let output = receipts(corpus.path(), &["audit", "missing-evidence"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("missing: 1"));
}

#[test]
fn audit_rejects_a_filename_id_mismatch() {
    let corpus = fixture_corpus();
    fs::write(
        corpus.path().join("summaries/missing-evidence.yaml"),
        "id: different-id\nclaims:\n  - A claim without evidence.\n",
    )
    .unwrap();
    let output = receipts(corpus.path(), &["audit"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("id_mismatch"));
}

#[test]
fn quiet_audit_suppresses_non_error_output() {
    let corpus = fixture_corpus();
    let output = receipts(corpus.path(), &["--quiet", "audit"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).is_empty());
}

#[test]
fn doctor_reports_corpus_and_executables() {
    let corpus = fixture_corpus();
    let output = receipts(corpus.path(), &["doctor"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let output = stdout(&output);
    assert!(output.contains("corpus: ok"));
    assert!(output.contains("mutool: ok"));
    assert!(output.contains("tesseract: ok"));
    assert!(output.contains("native profile: ok (mutool-native)"));
    assert!(output.contains("OCR profile: ok (tesseract-eng-300dpi-v1)"));
}

#[test]
fn doctor_reports_configured_ocr_profile() {
    let corpus = fixture_corpus();
    fs::write(
        corpus.path().join("receipts.yaml"),
        "cache:\n  root: \".cache/pdf-text\"\npdf:\n  ocr:\n    dpi: 600\n    lang: deu\n",
    )
    .unwrap();
    let output = receipts(corpus.path(), &["doctor"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let output = stdout(&output);
    assert!(
        output.contains("OCR profile: ok (tesseract-deu-600dpi-v1)"),
        "doctor should show derived profile: {output}"
    );
}

#[test]
fn doctor_fails_when_runtime_tools_are_unavailable() {
    let corpus = fixture_corpus();
    let empty_path = tempfile::tempdir().unwrap();
    let output = receipts_with_path(corpus.path(), &["doctor"], empty_path.path());

    assert!(!output.status.success());
    assert!(stdout(&output).contains("mutool: error"));
    assert!(stdout(&output).contains("tesseract: error"));
}

fn fixture_corpus() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for path in ["summaries", "md/missing-evidence", "pdfs"] {
        fs::create_dir_all(temp.path().join(path)).unwrap();
    }
    // Pin the cache inside the temp dir. Without this the real binary writes to
    // the developer's platform cache directory and the suite stops being
    // hermetic.
    fs::write(
        temp.path().join("receipts.yaml"),
        "cache:\n  root: \".cache/pdf-text\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("summaries/missing-evidence.yaml"),
        "id: missing-evidence\nclaims:\n  - A claim without evidence.\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("md/missing-evidence/missing-evidence.md"),
        "A source.",
    )
    .unwrap();
    fs::write(temp.path().join("pdfs/missing-evidence.pdf"), b"fixture").unwrap();
    temp
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn reports_uncovered_claims_using_the_configured_vocabulary() {
    let temp = tempfile::tempdir().unwrap();
    for path in ["summaries", "md/smith-2019", "pdfs"] {
        fs::create_dir_all(temp.path().join(path)).unwrap();
    }
    fs::write(
        temp.path().join("receipts.yaml"),
        concat!(
            "cache:\n  root: \".cache/pdf-text\"\n",
            "terms:\n  claim: proposition\n  claims: propositions\n",
        ),
    )
    .unwrap();
    // An evidence block that covers nothing, so the uncovered-entry message
    // fires. Both the document key and the nested evidence key use the
    // configured vocabulary.
    let markdown_sha = "a".repeat(64);
    let pdf_sha = "b".repeat(64);
    fs::write(
        temp.path().join("summaries/smith-2019.yaml"),
        format!(
            "id: smith-2019
propositions:
  - \"Transport remained laminar.\"
evidence:
  markdown:
    source: \"md/smith-2019/smith-2019.md\"
    sha256: \"{markdown_sha}\"
  pdf:
    source: \"pdfs/smith-2019.pdf\"
    sha256: \"{pdf_sha}\"
  propositions: []
"
        ),
    )
    .unwrap();
    fs::write(
        temp.path().join("md/smith-2019/smith-2019.md"),
        "Transport remained laminar.\n",
    )
    .unwrap();
    fs::write(temp.path().join("pdfs/smith-2019.pdf"), b"%PDF-1.7\n").unwrap();

    let output = receipts(temp.path(), &["check", "smith-2019"]);
    let stderr = stderr(&output);

    assert!(
        !output.status.success(),
        "uncovered propositions must fail: {stderr}"
    );
    assert!(
        stderr.contains("proposition"),
        "message should use the configured vocabulary: {stderr}"
    );
    assert!(
        !stderr.contains("claim"),
        "message should not leak the canonical vocabulary: {stderr}"
    );
}

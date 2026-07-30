use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn receipts(corpus: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_receipts"))
        .arg("-C")
        .arg(corpus)
        .args(args)
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
fn doctor_reports_corpus_and_executables() {
    let corpus = fixture_corpus();
    let output = receipts(corpus.path(), &["doctor"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let output = stdout(&output);
    assert!(output.contains("corpus: ok"));
    assert!(output.contains("mutool: ok"));
    assert!(output.contains("tesseract: ok"));
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

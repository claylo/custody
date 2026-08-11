use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use receipts::evidence::{ClaimEvidence, Locator, MarkdownLocator, PdfBackend, PdfLocator};
use receipts::hash::{sha256_bytes, sha256_file};
use receipts::markdown::UnitKind;
use receipts::review::evidence_sha256;

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

fn receipts_json(corpus: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_receipts"))
        .arg("-C")
        .arg(corpus)
        .args(["--format", "json"])
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

#[cfg(unix)]
#[test]
fn audit_skips_symlinked_summary_entries() {
    use std::os::unix::fs::symlink;

    let corpus = fixture_corpus();
    symlink(
        corpus.path().join("does-not-exist.yaml"),
        corpus.path().join("summaries/broken.yaml"),
    )
    .unwrap();

    let output = receipts(corpus.path(), &["audit"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("missing: 1"));
}

#[test]
fn audit_rejects_summary_directories_deeper_than_the_template() {
    let corpus = fixture_corpus();
    fs::create_dir(corpus.path().join("summaries/unexpected")).unwrap();
    fs::write(
        corpus.path().join("summaries/unexpected/ignored.yaml"),
        "id: ignored\nclaims: []\n",
    )
    .unwrap();

    let output = receipts(corpus.path(), &["audit"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("summary discovery exceeded template depth"));
}

#[test]
fn audit_discovers_summaries_at_the_configured_template_depth() {
    let corpus = tempfile::tempdir().unwrap();
    fs::create_dir_all(corpus.path().join("records/missing-evidence")).unwrap();
    fs::write(
        corpus.path().join("receipts.yaml"),
        concat!(
            "corpus:\n  summaries: \"records/{id}/summary.yaml\"\n",
            "cache:\n  root: \".cache/pdf-text\"\n",
        ),
    )
    .unwrap();
    fs::write(
        corpus.path().join("records/missing-evidence/summary.yaml"),
        "id: missing-evidence\nclaims:\n  - A claim without evidence.\n",
    )
    .unwrap();

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

#[cfg(unix)]
#[test]
fn audit_rejects_a_summary_symlink_outside_the_corpus() {
    use std::os::unix::fs::symlink;

    let corpus = fixture_corpus();
    let outside = tempfile::tempdir().unwrap();
    let outside_summary = outside.path().join("missing-evidence.yaml");
    fs::write(
        &outside_summary,
        "id: missing-evidence\nclaims:\n  - A claim without evidence.\n",
    )
    .unwrap();
    let summary = corpus.path().join("summaries/missing-evidence.yaml");
    fs::remove_file(&summary).unwrap();
    symlink(&outside_summary, &summary).unwrap();

    let output = receipts(corpus.path(), &["audit", "missing-evidence"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("resolves outside the corpus"));
}

#[test]
fn audit_rejects_an_oversized_summary_before_reading_it() {
    let corpus = fixture_corpus();
    let summary = corpus.path().join("summaries/missing-evidence.yaml");
    fs::File::create(&summary)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();

    let output = receipts(corpus.path(), &["audit", "missing-evidence"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("exceeds the 67108864-byte corpus text limit"));
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
    assert!(output.contains("OCR profile: ok (tesseract-eng-300dpi-v2)"));
}

#[cfg(unix)]
#[test]
fn closed_stdout_is_a_clean_exit() {
    use std::{
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::Stdio,
    };

    let corpus = fixture_corpus();
    let (writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    let writer: OwnedFd = writer.into();
    let output = Command::new(env!("CARGO_BIN_EXE_receipts"))
        .arg("-C")
        .arg(corpus.path())
        .arg("doctor")
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stderr(&output).contains("panicked"));
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
        output.contains("OCR profile: ok (tesseract-deu-600dpi-v2)"),
        "doctor should show derived profile: {output}"
    );
}

#[cfg(unix)]
#[test]
fn doctor_uses_and_reports_configured_external_tool_paths() {
    use std::os::unix::fs::PermissionsExt;

    let corpus = fixture_corpus();
    let tools = tempfile::tempdir().unwrap();
    let mutool = tools.path().join("mutool-pinned");
    let tesseract = tools.path().join("tesseract-pinned");
    fs::write(&mutool, "#!/bin/sh\necho 'mutool version 1.28.2'\n").unwrap();
    fs::write(&tesseract, "#!/bin/sh\necho 'tesseract 5.5.3'\n").unwrap();
    fs::set_permissions(&mutool, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&tesseract, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        corpus.path().join("receipts.yaml"),
        format!(
            "cache:\n  root: \".cache/pdf-text\"\npdf:\n  tools:\n    mutool: \"{}\"\n    tesseract: \"{}\"\n",
            mutool.display(),
            tesseract.display(),
        ),
    )
    .unwrap();
    let empty_path = tempfile::tempdir().unwrap();

    let output = receipts_with_path(corpus.path(), &["doctor"], empty_path.path());
    let stdout = stdout(&output);
    let mutool = mutool.canonicalize().unwrap();
    let tesseract = tesseract.canonicalize().unwrap();

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout.contains(&format!("mutool path: ok ({})", mutool.display())));
    assert!(stdout.contains(&format!("tesseract path: ok ({})", tesseract.display())));
    assert!(stdout.contains("mutool: ok (mutool version 1.28.2)"));
    assert!(stdout.contains("tesseract: ok (tesseract 5.5.3)"));
}

#[cfg(unix)]
#[test]
fn doctor_rejects_unsupported_external_tool_versions() {
    use std::os::unix::fs::PermissionsExt;

    let corpus = fixture_corpus();
    let tools = tempfile::tempdir().unwrap();
    let mutool = tools.path().join("mutool-old");
    let tesseract = tools.path().join("tesseract-supported");
    fs::write(&mutool, "#!/bin/sh\necho 'mutool version 1.27.9'\n").unwrap();
    fs::write(&tesseract, "#!/bin/sh\necho 'tesseract 5.5.3'\n").unwrap();
    fs::set_permissions(&mutool, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&tesseract, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        corpus.path().join("receipts.yaml"),
        format!(
            "cache:\n  root: \".cache/pdf-text\"\npdf:\n  tools:\n    mutool: \"{}\"\n    tesseract: \"{}\"\n",
            mutool.display(),
            tesseract.display(),
        ),
    )
    .unwrap();
    let empty_path = tempfile::tempdir().unwrap();

    let output = receipts_with_path(corpus.path(), &["doctor"], empty_path.path());
    let stdout = stdout(&output);

    assert!(!output.status.success());
    assert!(stdout.contains("mutool: unsupported"));
    assert!(stdout.contains("supported >=1.28.0, <1.29.0"));
    assert!(stdout.contains("tesseract: ok (tesseract 5.5.3)"));
}

#[test]
fn doctor_fails_when_runtime_tools_are_unavailable() {
    let corpus = fixture_corpus();
    let empty_path = tempfile::tempdir().unwrap();
    let output = receipts_with_path(corpus.path(), &["doctor"], empty_path.path());

    assert!(!output.status.success());
    assert!(stdout(&output).contains("mutool: missing"));
    assert!(stdout(&output).contains("tesseract: missing"));
}

#[test]
fn aggregate_commands_abort_before_judging_when_pdf_tools_are_unavailable() {
    let corpus = fixture_corpus();
    let empty_path = tempfile::tempdir().unwrap();

    for args in [
        &["check", "missing-evidence"][..],
        &["audit", "missing-evidence"][..],
    ] {
        let output = receipts_with_path(corpus.path(), args, empty_path.path());
        let stderr = stderr(&output);

        assert!(!output.status.success());
        assert!(stderr.contains("PDF toolchain preflight failed"));
        assert!(!stderr.contains("invalid evidence"));
    }
}

#[test]
fn locate_refuses_ocr_fallback_when_disabled() {
    let corpus = fixture_corpus();
    fs::write(
        corpus.path().join("receipts.yaml"),
        "cache:\n  root: \".cache/pdf-text\"\npdf:\n  ocr:\n    enabled: false\n",
    )
    .unwrap();
    let output = receipts(
        corpus.path(),
        &[
            "locate",
            "missing-evidence",
            "--claim",
            "0",
            "--exact",
            "A source.",
            "--page",
            "1",
        ],
    );

    assert!(!output.status.success(), "{}", stdout(&output));
    let stderr = stderr(&output);
    assert!(
        stderr.contains("OCR is disabled"),
        "should refuse OCR fallback: {stderr}"
    );
}

#[test]
fn locate_source_flag_selects_named_source_templates() {
    let temp = tempfile::tempdir().unwrap();
    for path in ["summaries", "md/doc-001-supp", "pdfs"] {
        fs::create_dir_all(temp.path().join(path)).unwrap();
    }
    fs::write(
        temp.path().join("receipts.yaml"),
        concat!(
            "cache:\n  root: \".cache/pdf-text\"\n",
            "corpus:\n",
            "  sources:\n",
            "    default:\n",
            "      markdown:\n        - \"md/{id}/{id}.md\"\n",
            "      pdf: \"pdfs/{id}.pdf\"\n",
            "    supplement:\n",
            "      markdown:\n        - \"md/{id}-supp/{id}-supp.md\"\n",
            "      pdf: \"pdfs/{id}-supp.pdf\"\n",
        ),
    )
    .unwrap();
    fs::write(
        temp.path().join("summaries/doc-001.yaml"),
        "id: doc-001\nclaims:\n  - \"The finding was confirmed by supplementary data.\"\n",
    )
    .unwrap();
    // Only the supplement source has a Markdown file on disk; the default
    // source's template resolves to a path that does not exist. A run that
    // reaches PDF/OCR resolution instead of failing on Markdown lookup proves
    // `--source` picked the supplement templates.
    fs::write(
        temp.path().join("md/doc-001-supp/doc-001-supp.md"),
        "Supplementary data confirms the result.\n",
    )
    .unwrap();
    fs::write(temp.path().join("pdfs/doc-001-supp.pdf"), MINIMAL_PDF).unwrap();

    let exact_args = [
        "locate",
        "doc-001",
        "--claim",
        "0",
        "--exact",
        "Supplementary data confirms the result.",
        "--page",
        "1",
    ];

    let default_run = receipts(temp.path(), &exact_args);
    assert!(!default_run.status.success());
    let default_stderr = stderr(&default_run);
    assert!(
        default_stderr.contains("no canonical Markdown source found for doc-001 (source: default)"),
        "omitting --source should resolve against the default templates: {default_stderr}"
    );

    let mut supplement_args = exact_args.to_vec();
    supplement_args.extend(["--source", "supplement"]);
    let supplement_run = receipts(temp.path(), &supplement_args);
    assert!(!supplement_run.status.success());
    let supplement_stderr = stderr(&supplement_run);
    assert!(
        !supplement_stderr.contains("no canonical Markdown source found"),
        "--source supplement should resolve its own Markdown template: {supplement_stderr}"
    );
    assert!(
        supplement_stderr.contains("Tesseract"),
        "should reach PDF/OCR resolution once Markdown resolves via the named source: {supplement_stderr}"
    );
}

#[test]
fn audit_recognizes_all_configured_source_templates() {
    let temp = tempfile::tempdir().unwrap();
    for path in ["summaries", "md/doc-001", "md/doc-001-supp", "pdfs"] {
        fs::create_dir_all(temp.path().join(path)).unwrap();
    }
    fs::write(
        temp.path().join("receipts.yaml"),
        concat!(
            "cache:\n  root: \".cache/pdf-text\"\n",
            "corpus:\n",
            "  sources:\n",
            "    default:\n",
            "      markdown:\n        - \"md/{id}/{id}.md\"\n",
            "      pdf: \"pdfs/{id}.pdf\"\n",
            "    supplement:\n",
            "      markdown:\n        - \"md/{id}-supp/{id}-supp.md\"\n",
            "      pdf: \"pdfs/{id}-supp.pdf\"\n",
        ),
    )
    .unwrap();

    let md_path = temp.path().join("md/doc-001/doc-001.md");
    let md_supp_path = temp.path().join("md/doc-001-supp/doc-001-supp.md");
    let pdf_path = temp.path().join("pdfs/doc-001.pdf");
    let pdf_supp_path = temp.path().join("pdfs/doc-001-supp.pdf");
    fs::write(&md_path, "The primary finding was significant.\n").unwrap();
    fs::write(&md_supp_path, "Supplementary data confirms the result.\n").unwrap();
    fs::write(&pdf_path, MINIMAL_PDF).unwrap();
    fs::write(&pdf_supp_path, MINIMAL_PDF).unwrap();

    let md_sha = sha256_file(&md_path).unwrap();
    let md_supp_sha = sha256_file(&md_supp_path).unwrap();
    let pdf_sha = sha256_file(&pdf_path).unwrap();
    let pdf_supp_sha = sha256_file(&pdf_supp_path).unwrap();
    let claim = "The finding was confirmed by supplementary data.";
    let claim_sha = sha256_bytes(claim.as_bytes());

    fs::write(
        temp.path().join("summaries/doc-001.yaml"),
        format!(
            "id: doc-001
claims:
  - \"{claim}\"
evidence:
  sources:
    default:
      markdown:
        source: \"md/doc-001/doc-001.md\"
        sha256: \"{md_sha}\"
      pdf:
        source: \"pdfs/doc-001.pdf\"
        sha256: \"{pdf_sha}\"
    supplement:
      markdown:
        source: \"md/doc-001-supp/doc-001-supp.md\"
        sha256: \"{md_supp_sha}\"
      pdf:
        source: \"pdfs/doc-001-supp.pdf\"
        sha256: \"{pdf_supp_sha}\"
  claims:
    - claim: 0
      claim_sha256: \"{claim_sha}\"
      locators:
        - source: default
          exact: \"The primary finding was significant.\"
          markdown:
            line: 1
            column: 1
            unit: paragraph
          pdf:
            page: 1
            backend: mutool-native
        - source: supplement
          exact: \"Supplementary data confirms the result.\"
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

    let output = receipts(temp.path(), &["audit", "doc-001"]);
    let stderr_text = stderr(&output);

    assert!(
        !stderr_text.contains("unknown_source_template"),
        "both source templates should be recognized: {stderr_text}"
    );
    assert!(
        !stderr_text.contains("_source_mismatch"),
        "recorded source paths should match resolved paths: {stderr_text}"
    );
    assert!(
        !stderr_text.contains("hash_mismatch"),
        "recorded hashes should match the fixture files: {stderr_text}"
    );
    // MINIMAL_PDF carries no extractable text, so native/OCR extraction finds
    // nothing for either locator and the audit still fails overall — but for
    // a PDF-content reason, not a source-plumbing reason.
    assert!(!output.status.success());
    assert!(
        stderr_text.contains("pdf_missing") || stderr_text.contains("pdf_extraction_failed"),
        "expected a PDF-stage failure, not a source-plumbing failure: {stderr_text}"
    );
}

#[test]
fn propose_emits_candidates_for_claims_without_evidence() {
    let temp = tempfile::tempdir().unwrap();
    for path in ["summaries", "md/smith-2019", "pdfs"] {
        fs::create_dir_all(temp.path().join(path)).unwrap();
    }
    fs::write(
        temp.path().join("receipts.yaml"),
        "cache:\n  root: \".cache/pdf-text\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("summaries/smith-2019.yaml"),
        "id: smith-2019\nclaims:\n  - \"The rate was 67.5% in controls.\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("md/smith-2019/smith-2019.md"),
        "The rate was 67.5% in the control group.\n",
    )
    .unwrap();
    fs::write(temp.path().join("pdfs/smith-2019.pdf"), TEXT_PDF).unwrap();

    let output = receipts_json(temp.path(), &["propose", "smith-2019"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let stdout = stdout(&output);
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("propose emits JSON");
    assert_eq!(report["id"], "smith-2019");
    assert_eq!(report["claims"].as_array().unwrap().len(), 1);

    let claim = &report["claims"][0];
    assert_eq!(claim["claim"], 0);
    assert_eq!(claim["required_tokens"], serde_json::json!(["67.5%"]));
    assert_eq!(claim["uncovered_tokens"], serde_json::json!([]));
    assert!(
        !claim["candidates"].as_array().unwrap().is_empty(),
        "the claim's token appears in both sources: {stdout}"
    );

    let candidate = &claim["candidates"][0];
    assert_eq!(candidate["source"], "default");
    assert_eq!(
        candidate["exact"],
        "The rate was 67.5% in the control group."
    );
    assert_eq!(candidate["coverage"]["matched"], 1);
    assert_eq!(candidate["coverage"]["required"], 1);
    assert_eq!(candidate["markdown"]["line"], 1);
    assert_eq!(candidate["markdown"]["unit"], "paragraph");
    assert_eq!(candidate["pdf"]["page"], 1);
    assert_eq!(candidate["pdf"]["backend"], "mutool-native");
}

#[test]
fn propose_keeps_successes_when_other_summaries_cannot_be_read() {
    let temp = tempfile::tempdir().unwrap();
    for path in ["summaries", "md/smith-2019", "pdfs"] {
        fs::create_dir_all(temp.path().join(path)).unwrap();
    }
    fs::write(
        temp.path().join("receipts.yaml"),
        "cache:\n  root: \".cache/pdf-text\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("summaries/smith-2019.yaml"),
        "id: smith-2019\nclaims:\n  - \"The rate was 67.5% in controls.\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("summaries/broken.yaml"),
        "id: [unterminated\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("summaries/wrong-id.yaml"),
        "id: different-id\nclaims:\n  - A claim.\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("md/smith-2019/smith-2019.md"),
        "The rate was 67.5% in the control group.\n",
    )
    .unwrap();
    fs::write(temp.path().join("pdfs/smith-2019.pdf"), TEXT_PDF).unwrap();

    let output = receipts_json(temp.path(), &["propose"]);
    assert!(!output.status.success());

    let json_stdout = stdout(&output);
    let report: serde_json::Value =
        serde_json::from_str(&json_stdout).expect("propose preserves a JSON batch report");
    let summaries = report["summaries"].as_array().unwrap();
    assert_eq!(summaries.len(), 3);
    assert!(summaries.iter().any(|entry| {
        entry["id"] == "smith-2019"
            && entry["claims"]
                .as_array()
                .is_some_and(|claims| !claims.is_empty())
    }));
    assert!(summaries.iter().any(|entry| {
        entry["id"] == "broken" && entry["issues"][0]["code"] == "summary_parse_failed"
    }));
    assert!(
        summaries.iter().any(|entry| {
            entry["id"] == "wrong-id" && entry["issues"][0]["code"] == "id_mismatch"
        })
    );

    let human = receipts(temp.path(), &["propose"]);
    assert!(!human.status.success());
    assert!(stdout(&human).contains("# claim 0"));
    assert!(stderr(&human).contains("broken: summary_parse_failed"));
    assert!(stderr(&human).contains("wrong-id: id_mismatch"));
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
    fs::write(temp.path().join("pdfs/missing-evidence.pdf"), MINIMAL_PDF).unwrap();
    temp
}

/// One empty page, no xref. `MuPDF` repairs it and reports zero native text,
/// which is what drives `locate` down the OCR fallback path.
const MINIMAL_PDF: &str = "%PDF-1.4\n\
    1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
    2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
    3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n\
    trailer<</Root 1 0 R>>\n";

/// One page carrying a single line of native text, so `propose` can verify a
/// candidate span against real `MuPDF` extraction rather than a stub.
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

fn fixture_evidence_sha256(exact: &str) -> String {
    let entry = ClaimEvidence {
        claim: 0,
        claim_sha256: String::new(),
        locators: vec![Locator {
            source: "default".to_owned(),
            exact: exact.to_owned(),
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
    evidence_sha256(&entry)
}

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

#[test]
fn require_review_passes_with_supported_verdict() {
    let corpus = fixture_corpus();
    let temp = corpus.path();
    let claim = "A claim without evidence.";
    let claim_sha = sha256_bytes(claim.as_bytes());
    let exact = "The rate was 67.5% in the control group.";
    fs::write(
        temp.join("md/missing-evidence/missing-evidence.md"),
        format!("{exact}\n"),
    )
    .unwrap();
    fs::write(temp.join("pdfs/missing-evidence.pdf"), TEXT_PDF).unwrap();
    let md_sha = sha256_file(&temp.join("md/missing-evidence/missing-evidence.md")).unwrap();
    let pdf_sha = sha256_file(&temp.join("pdfs/missing-evidence.pdf")).unwrap();
    let ev_sha = fixture_evidence_sha256(exact);

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
        - exact: \"{exact}\"
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

#[test]
fn require_review_fails_on_unsupported_verdict() {
    let corpus = fixture_corpus();
    let temp = corpus.path();
    let claim = "A claim without evidence.";
    let claim_sha = sha256_bytes(claim.as_bytes());
    let md_sha = sha256_file(&temp.join("md/missing-evidence/missing-evidence.md")).unwrap();
    let pdf_sha = sha256_file(&temp.join("pdfs/missing-evidence.pdf")).unwrap();
    let ev_sha = fixture_evidence_sha256("A source.");

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

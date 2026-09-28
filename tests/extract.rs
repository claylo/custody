use std::{fs, path::Path, process::Command};

fn custody(corpus: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_custody"))
        .arg("-C")
        .arg(corpus)
        .args(["--format", "text"])
        .args(args)
        .output()
        .expect("custody executes")
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Two text objects on one page: a heading-like line and a two-line paragraph
/// whose first line ends in a hyphen. `MuPDF` groups the paragraph's lines into
/// one block, so dehyphenation must rejoin `adjust-` and `ment`.
const TWO_BLOCK_PDF: &str = "%PDF-1.4\n\
    1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
    2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
    3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]\
    /Resources<</Font<</F1 4 0 R>>>>/Contents 5 0 R>>endobj\n\
    4 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n\
    5 0 obj<</Length 170>>stream\n\
    BT /F1 18 Tf 72 720 Td (Results) Tj ET\n\
    BT /F1 12 Tf 72 600 Td (The sensor showed an adjust-) Tj 0 -14 Td (ment behaviour toward the target.) Tj ET\n\
    endstream\nendobj\n\
    trailer<</Root 1 0 R>>\n";

fn corpus() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for path in ["summaries", "md", "native", "pdfs"] {
        fs::create_dir_all(temp.path().join(path)).unwrap();
    }
    fs::write(
        temp.path().join("custody.yaml"),
        concat!(
            "corpus:\n",
            "  summaries: \"summaries/{id}.yaml\"\n",
            "  sources:\n",
            "    default:\n",
            "      markdown: [\"md/{id}.md\"]\n",
            "      pdf: \"pdfs/{id}.pdf\"\n",
            "    native:\n",
            "      markdown: [\"native/{id}.md\"]\n",
            "      pdf: \"pdfs/{id}.pdf\"\n",
            "cache:\n  root: \".cache/pdf-text\"\n",
        ),
    )
    .unwrap();
    fs::write(temp.path().join("pdfs/smith-2019.pdf"), TWO_BLOCK_PDF).unwrap();
    fs::write(
        temp.path().join("summaries/smith-2019.yaml"),
        "id: smith-2019\nclaims:\n  - The sensor showed adjustment behaviour toward the target.\n",
    )
    .unwrap();
    temp
}

#[test]
fn extract_prints_frontmatter_page_headings_and_dehyphenated_paragraphs() {
    let corpus = corpus();
    let output = custody(corpus.path(), &["extract", "smith-2019"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.starts_with("---\nid: smith-2019\nsource_format: pdf-native\n"));
    assert!(text.contains("source: pdfs/smith-2019.pdf\n"));
    assert!(text.contains("normalize: \"whitespace, dehyphenate=on, quotes=on\"\n"));
    assert!(text.contains("\n## Page 1\n"));
    assert!(text.contains("\nResults\n"));
    assert!(
        text.contains("\nThe sensor showed an adjustment behaviour toward the target.\n"),
        "{text}"
    );
    assert!(!corpus.path().join("native/smith-2019.md").exists());
}

#[test]
fn extract_write_targets_the_named_source_and_refuses_to_overwrite() {
    let corpus = corpus();
    let output = custody(
        corpus.path(),
        &["extract", "smith-2019", "--source", "native", "--write"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("wrote native/smith-2019.md (1 pages, 2 paragraphs)"));

    let written = fs::read_to_string(corpus.path().join("native/smith-2019.md")).unwrap();
    assert!(written.contains("adjustment behaviour"));
    assert!(!corpus.path().join("md/smith-2019.md").exists());

    let again = custody(
        corpus.path(),
        &["extract", "smith-2019", "--source", "native", "--write"],
    );
    assert!(!again.status.success());
    assert!(stderr(&again).contains("already exists; pass --force"));

    let forced = custody(
        corpus.path(),
        &[
            "extract",
            "smith-2019",
            "--source",
            "native",
            "--write",
            "--force",
        ],
    );
    assert!(forced.status.success(), "{}", stderr(&forced));
}

#[test]
fn extracted_markdown_binds_through_locate_and_check() {
    let corpus = corpus();
    let output = custody(
        corpus.path(),
        &["extract", "smith-2019", "--source", "native", "--write"],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    // The literal spans the rejoined hyphen on the PDF side, which only
    // works because both sides are dehyphenated by the same rule.
    let located = Command::new(env!("CARGO_BIN_EXE_custody"))
        .arg("-C")
        .arg(corpus.path())
        .args(["--format", "json", "locate", "smith-2019", "--claim", "0"])
        .args(["--source", "native"])
        .args(["--exact", "showed an adjustment behaviour"])
        .output()
        .unwrap();
    assert!(located.status.success(), "{}", stderr(&located));
    let record: serde_json::Value = serde_json::from_slice(&located.stdout).unwrap();
    assert_eq!(record["source"], "native");
    assert_eq!(record["pdf_match"]["page"], 1);
    assert_eq!(record["pdf_match"]["backend"], "mutool-native");
    assert!(record["pdf_match"]["bbox"].is_object(), "{record}");
    assert_eq!(
        record["claim"]["locators"][0]["markdown"]["section"][0],
        "Page 1"
    );
}

#[test]
fn extract_force_requires_write() {
    let corpus = corpus();
    let output = custody(corpus.path(), &["extract", "smith-2019", "--force"]);
    assert!(!output.status.success());
}

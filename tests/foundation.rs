use std::fs;

use receipts::{
    config,
    corpus::Corpus,
    hash::{sha256_bytes, sha256_file},
    normalize::normalize,
};

fn write_config(dir: &std::path::Path, body: &str) {
    fs::write(dir.join("receipts.yaml"), body).unwrap();
}

const TWO_SOURCE_CONFIG: &str = concat!(
    "cache:\n  root: \"c\"\ncorpus:\n",
    "  sources:\n",
    "    default:\n",
    "      markdown:\n        - \"md/{id}.md\"\n",
    "      pdf: \"pdfs/{id}.pdf\"\n",
    "    supplement:\n",
    "      markdown:\n        - \"md/{id}-supp.md\"\n",
    "      pdf: \"pdfs/{id}-supp.pdf\"\n",
);

#[test]
fn collapses_unicode_whitespace_without_rewriting_content() {
    assert_eq!(normalize("  A\u{00a0}\tB – ﬁ  "), "A B – ﬁ");
}

#[test]
fn hashes_bytes_and_files_with_sha256() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.txt");
    fs::write(&path, b"abc").unwrap();
    let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    assert_eq!(sha256_bytes(b"abc"), expected);
    assert_eq!(sha256_file(&path).unwrap(), expected);
}

#[test]
fn discovers_the_corpus_root_from_a_nested_directory() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), "corpus:\n  pdf: \"pdfs/{id}.pdf\"\n");
    let nested = dir.path().join("notes/deep");
    fs::create_dir_all(&nested).unwrap();

    let corpus = Corpus::discover_from(&nested, None).unwrap();
    let root = dir.path().canonicalize().unwrap();

    assert_eq!(corpus.root().canonicalize().unwrap(), root);
    assert_eq!(
        corpus.config_file().unwrap().file_name().unwrap(),
        "receipts.yaml"
    );
}

#[test]
fn resolves_default_layout_templates() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), "cache:\n  root: \".cache/pdf-text\"\n");
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();
    let root = corpus.root().to_path_buf();

    assert_eq!(
        corpus.summary_path("smith-2019").unwrap(),
        root.join("summaries/smith-2019.yaml")
    );
    assert_eq!(
        corpus.markdown_candidates("smith-2019").unwrap(),
        vec![
            root.join("md/smith-2019.md"),
            root.join("md/smith-2019/smith-2019.md"),
        ]
    );
    assert_eq!(
        corpus.pdf_path("smith-2019").unwrap(),
        root.join("pdfs/smith-2019.pdf")
    );
    assert_eq!(corpus.summaries_dir(), root.join("summaries"));
    assert_eq!(corpus.cache_root(), root.join(".cache/pdf-text"));
    assert!(corpus.cache_is_corpus_local());
}

#[test]
fn honors_custom_layout_templates() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        "corpus:\n  summaries: \"records/{id}/summary.yaml\"\n  markdown:\n    - \"text/{id}.md\"\n  pdf: \"sources/{id}.pdf\"\n",
    );
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();
    let root = corpus.root().to_path_buf();

    assert_eq!(
        corpus.summary_path("smith-2019").unwrap(),
        root.join("records/smith-2019/summary.yaml")
    );
    assert_eq!(
        corpus.markdown_candidates("smith-2019").unwrap(),
        vec![root.join("text/smith-2019.md")]
    );
    assert_eq!(corpus.summaries_dir(), root.join("records"));
}

#[test]
fn treats_dotconfig_parent_as_the_corpus_root() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".config")).unwrap();
    fs::write(
        dir.path().join(".config/receipts.yaml"),
        "cache:\n  root: \"c\"\n",
    )
    .unwrap();

    let corpus = Corpus::discover_from(dir.path(), None).unwrap();
    assert_eq!(
        corpus.root().canonicalize().unwrap(),
        dir.path().canonicalize().unwrap()
    );
}

#[test]
fn falls_back_to_the_git_boundary_without_config() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    let nested = dir.path().join("a/b");
    fs::create_dir_all(&nested).unwrap();

    let corpus = Corpus::discover_from(&nested, None).unwrap();
    assert_eq!(
        corpus.root().canonicalize().unwrap(),
        dir.path().canonicalize().unwrap()
    );
    assert!(
        corpus.config_file().is_none(),
        "no config file exists, so doctor must report defaults"
    );
}

#[test]
fn rejects_ids_that_escape_the_corpus() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), "cache:\n  root: \"c\"\n");
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();

    assert!(corpus.pdf_path("../escape").is_err());
    assert!(corpus.pdf_path("Upper").is_err());
    assert!(corpus.pdf_path("ab").is_err());
    assert!(corpus.pdf_path("-lead").is_err());
}

#[test]
fn rejects_templates_that_escape_the_corpus() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), "corpus:\n  pdf: \"../outside/{id}.pdf\"\n");
    assert!(Corpus::discover_from(dir.path(), None).is_err());
}

#[test]
fn rejects_templates_without_an_id_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), "corpus:\n  pdf: \"pdfs/fixed.pdf\"\n");
    assert!(Corpus::discover_from(dir.path(), None).is_err());
}

#[test]
fn desugars_bare_corpus_layout_into_sources_default() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        "cache:\n  root: \"c\"\ncorpus:\n  markdown:\n    - \"docs/{id}.md\"\n  pdf: \"files/{id}.pdf\"\n",
    );
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();
    let root = corpus.root().to_path_buf();

    assert_eq!(corpus.source_names(), vec!["default"]);
    assert_eq!(
        corpus.markdown_candidates("smith-2019").unwrap(),
        vec![root.join("docs/smith-2019.md")]
    );
    assert_eq!(
        corpus.pdf_path("smith-2019").unwrap(),
        root.join("files/smith-2019.pdf")
    );
}

#[test]
fn parses_explicit_corpus_sources() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), TWO_SOURCE_CONFIG);
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();

    let mut sources = corpus.source_names();
    sources.sort();
    assert_eq!(sources, vec!["default", "supplement"]);
}

#[test]
fn rejects_conflicting_corpus_source_form() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        concat!(
            "cache:\n  root: \"c\"\ncorpus:\n",
            "  markdown:\n    - \"md/{id}.md\"\n",
            "  pdf: \"pdfs/{id}.pdf\"\n",
            "  sources:\n    default:\n      markdown:\n        - \"x/{id}.md\"\n      pdf: \"x/{id}.pdf\"\n",
        ),
    );
    assert!(Corpus::discover_from(dir.path(), None).is_err());
}

#[test]
fn rejects_an_explicit_source_missing_a_template() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        concat!(
            "cache:\n  root: \"c\"\ncorpus:\n  sources:\n",
            "    default:\n      markdown:\n        - \"md/{id}.md\"\n      pdf: \"pdfs/{id}.pdf\"\n",
            "    supplement:\n      markdown:\n        - \"md/{id}-supp.md\"\n",
        ),
    );
    assert!(Corpus::discover_from(dir.path(), None).is_err());
}

#[test]
fn rejects_source_names_that_are_not_path_components() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        concat!(
            "cache:\n  root: \"c\"\ncorpus:\n  sources:\n",
            "    \"../escape\":\n      markdown:\n        - \"md/{id}.md\"\n      pdf: \"pdfs/{id}.pdf\"\n",
        ),
    );
    assert!(Corpus::discover_from(dir.path(), None).is_err());
}

#[test]
fn resolves_source_specific_paths() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), TWO_SOURCE_CONFIG);
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();
    let root = corpus.root().to_path_buf();

    assert_eq!(
        corpus
            .markdown_candidates_for("smith-2019", "default")
            .unwrap(),
        vec![root.join("md/smith-2019.md")]
    );
    assert_eq!(
        corpus
            .markdown_candidates_for("smith-2019", "supplement")
            .unwrap(),
        vec![root.join("md/smith-2019-supp.md")]
    );
    assert_eq!(
        corpus.pdf_path_for("smith-2019", "supplement").unwrap(),
        root.join("pdfs/smith-2019-supp.pdf")
    );
    assert!(corpus.pdf_path_for("smith-2019", "unknown").is_err());
    assert!(
        corpus
            .markdown_candidates_for("smith-2019", "unknown")
            .is_err()
    );
}

#[test]
fn resolves_the_platform_cache_root_by_default() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), "corpus:\n  pdf: \"pdfs/{id}.pdf\"\n");
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();

    assert_eq!(corpus.cache_root(), config::platform_cache_root().unwrap());
    assert!(corpus.cache_root().ends_with("pdf-text"));
    assert!(!corpus.cache_is_corpus_local());
}

#[test]
fn rejects_invalid_page_segmentation_mode() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        "cache:\n  root: \"c\"\npdf:\n  ocr:\n    page_segmentation_mode: 14\n",
    );
    assert!(Corpus::discover_from(dir.path(), None).is_err());
}

#[test]
fn exposes_ocr_config_from_corpus() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        "cache:\n  root: \"c\"\npdf:\n  ocr:\n    dpi: 600\n    lang: deu\n",
    );
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();

    assert_eq!(corpus.ocr_config().dpi, 600);
    assert_eq!(corpus.ocr_config().lang, "deu");
    assert!(corpus.ocr_config().enabled);
    assert_eq!(corpus.ocr_config().page_segmentation_mode, 3);
}

#[test]
fn coverage_and_sections_default_when_undeclared() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), "cache:\n  root: \"c\"\n");
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();

    assert_eq!(
        corpus.coverage_config().tokens,
        config::TokenSeverity::Error
    );
    assert!(corpus.sections_config().weak.is_empty());
}

#[test]
fn exposes_coverage_and_sections_config_from_corpus() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        "cache:\n  root: \"c\"\ncoverage:\n  tokens: warn\nsections:\n  weak:\n    - Abstract\n    - References\n",
    );
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();

    assert_eq!(corpus.coverage_config().tokens, config::TokenSeverity::Warn);
    assert_eq!(corpus.sections_config().weak, ["Abstract", "References"]);
}

/// The YAML layer resolves a bare `off` to the boolean `false`, so this pins
/// the spelling users actually write.
#[test]
fn accepts_unquoted_and_quoted_off_coverage_severity() {
    for declared in ["off", "\"off\""] {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            &format!("cache:\n  root: \"c\"\ncoverage:\n  tokens: {declared}\n"),
        );
        let corpus = Corpus::discover_from(dir.path(), None).unwrap();

        assert_eq!(corpus.coverage_config().tokens, config::TokenSeverity::Off);
    }
}

#[test]
fn rejects_an_unknown_coverage_severity() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        "cache:\n  root: \"c\"\ncoverage:\n  tokens: quiet\n",
    );
    assert!(Corpus::discover_from(dir.path(), None).is_err());
}

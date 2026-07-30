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
fn resolves_the_platform_cache_root_by_default() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path(), "corpus:\n  pdf: \"pdfs/{id}.pdf\"\n");
    let corpus = Corpus::discover_from(dir.path(), None).unwrap();

    assert_eq!(corpus.cache_root(), config::platform_cache_root().unwrap());
    assert!(corpus.cache_root().ends_with("pdf-text"));
}

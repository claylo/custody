use std::fs;

use receipts::pdf::{
    cache::{CacheManifest, OcrCache, OcrProfile, cache_key},
    tesseract::{parse_orientation, parse_tsv},
};

fn profile(mutool: &str, tesseract: &str) -> OcrProfile {
    OcrProfile {
        name: "tesseract-eng-300dpi-v1".to_owned(),
        language: "eng".to_owned(),
        dpi: 300,
        page_segmentation_mode: 3,
        mutool_version: mutool.to_owned(),
        tesseract_version: tesseract.to_owned(),
    }
}

#[test]
fn reconstructs_words_in_tsv_order() {
    let page =
        parse_tsv(include_str!("fixtures/tesseract-rotated-table.tsv")).expect("fixture parses");

    assert!(page.text.contains("42.8% within tolerance"));
    assert_eq!(page.mean_confidence, Some(91.5));
}

#[test]
fn cache_key_changes_with_tool_versions() {
    assert_ne!(
        cache_key(&profile("mutool 1.28", "tesseract 5.5")).unwrap(),
        cache_key(&profile("mutool 1.29", "tesseract 5.5")).unwrap()
    );
}

#[test]
fn matching_cache_manifest_reuses_tsv() {
    let root = tempfile::tempdir().unwrap();
    let cache = OcrCache::new(root.path().to_path_buf());
    let profile = profile("mutool 1.28", "tesseract 5.5");
    let manifest = CacheManifest::new("a".repeat(64), 7, profile).unwrap();
    cache
        .store(&manifest, "level\tpage_num\ttext\n5\t1\tcached\n")
        .unwrap();

    let loaded = cache.load(&manifest).unwrap().expect("cache hit");
    assert!(loaded.contains("cached"));
    assert!(
        cache
            .entry_dir(&manifest)
            .starts_with(root.path().join("a".repeat(64)))
    );
}

#[test]
fn changed_manifest_is_a_cache_miss() {
    let root = tempfile::tempdir().unwrap();
    let cache = OcrCache::new(root.path().to_path_buf());
    let first =
        CacheManifest::new("b".repeat(64), 2, profile("mutool 1.28", "tesseract 5.5")).unwrap();
    cache.store(&first, "tsv").unwrap();

    let changed =
        CacheManifest::new("b".repeat(64), 2, profile("mutool 1.29", "tesseract 5.5")).unwrap();
    assert!(cache.load(&changed).unwrap().is_none());

    let files = fs::read_dir(cache.entry_dir(&first)).unwrap().count();
    assert_eq!(files, 2);
}

#[test]
fn parses_tesseract_rotation_instruction() {
    assert_eq!(
        parse_orientation("Orientation in degrees: 270\nRotate: 90\n"),
        90
    );
    assert_eq!(parse_orientation("no orientation result"), 0);
}

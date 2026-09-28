use std::{cell::Cell, fs, path::Path};

use anyhow::Result;
use custody::{
    coordinate::Page,
    pdf::{
        cache::{CacheManifest, CacheReadPolicy, OcrCache, OcrProfile, cache_key},
        matching_bbox,
        tesseract::{
            OcrEngine, PageRenderer, ocr_page, parse_orientation, parse_tsv, profile_name,
            validate_version,
        },
    },
};

fn physical_page(value: usize) -> Page {
    Page::new(value).unwrap()
}

fn profile(mutool: &str, tesseract: &str) -> OcrProfile {
    OcrProfile {
        name: profile_name("eng", 300),
        language: "eng".to_owned(),
        dpi: 300,
        page_segmentation_mode: 3,
        mutool_executable: "/test/mutool".to_owned(),
        mutool_version: mutool.to_owned(),
        tesseract_executable: "/test/tesseract".to_owned(),
        tesseract_version: tesseract.to_owned(),
        render_command: "mutool draw -q -r 300 [-R ROTATION] -o OUTPUT PDF PAGE".to_owned(),
        orientation_command: "tesseract IMAGE stdout -l osd --psm 0".to_owned(),
        recognition_command: "tesseract IMAGE stdout -l eng --psm 3 tsv".to_owned(),
    }
}

#[test]
fn reconstructs_words_in_tsv_order() {
    let page =
        parse_tsv(include_str!("fixtures/tesseract-rotated-table.tsv")).expect("fixture parses");

    assert!(page.text.contains("42.8% within tolerance"));
    assert_eq!(page.mean_confidence, Some(91.5));
    let extracted = page.into_extracted_page(physical_page(1));
    assert_eq!(extracted.mean_confidence, Some(91.5));
    let bbox = matching_bbox(&extracted, "42.8% within tolerance").unwrap();
    assert_eq!(
        (bbox.x, bbox.y, bbox.width, bbox.height),
        (84.0, 110.0, 274.0, 32.0)
    );
}

#[test]
fn profile_name_derives_from_settings() {
    assert_eq!(profile_name("eng", 300), "tesseract-eng-300dpi-v2");
    assert_eq!(profile_name("deu", 600), "tesseract-deu-600dpi-v2");
}

#[test]
fn validates_the_supported_tesseract_version_line() {
    assert!(validate_version("tesseract 5.5.0").is_ok());
    assert!(validate_version("tesseract 5.5.99").is_ok());
    assert!(validate_version("tesseract 5.4.9").is_err());
    assert!(validate_version("tesseract 5.6.0").is_err());
    assert!(validate_version("tesseract unknown").is_err());
}

#[test]
fn cache_key_changes_with_tool_versions() {
    assert_ne!(
        cache_key(&profile("mutool 1.28", "tesseract 5.5")).unwrap(),
        cache_key(&profile("mutool 1.29", "tesseract 5.5")).unwrap()
    );
}

#[test]
fn cache_key_changes_with_tool_paths() {
    let first = profile("mutool 1.28", "tesseract 5.5");
    let mut second = first.clone();
    second.mutool_executable = "/opt/alternate/mutool".to_owned();

    assert_ne!(cache_key(&first).unwrap(), cache_key(&second).unwrap());
}

#[test]
fn matching_cache_manifest_reuses_tsv() {
    let root = tempfile::tempdir().unwrap();
    let cache = OcrCache::new(root.path().to_path_buf());
    let profile = profile("mutool 1.28", "tesseract 5.5");
    let manifest = CacheManifest::new("a".repeat(64), physical_page(7), profile).unwrap();
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
fn write_only_cache_never_reads_stored_tsv() {
    let root = tempfile::tempdir().unwrap();
    let trusted = OcrCache::new(root.path().to_path_buf());
    let manifest = CacheManifest::new(
        "f".repeat(64),
        physical_page(4),
        profile("mutool 1.28", "tesseract 5.5"),
    )
    .unwrap();
    trusted.store(&manifest, "attacker-controlled TSV").unwrap();

    let write_only =
        OcrCache::with_read_policy(root.path().to_path_buf(), CacheReadPolicy::WriteOnly);

    assert!(write_only.load(&manifest).unwrap().is_none());
    assert!(write_only.entry_dir(&manifest).join("ocr.tsv").is_file());
}

#[test]
fn changed_manifest_is_a_cache_miss() {
    let root = tempfile::tempdir().unwrap();
    let cache = OcrCache::new(root.path().to_path_buf());
    let first = CacheManifest::new(
        "b".repeat(64),
        physical_page(2),
        profile("mutool 1.28", "tesseract 5.5"),
    )
    .unwrap();
    cache.store(&first, "tsv").unwrap();

    let changed = CacheManifest::new(
        "b".repeat(64),
        physical_page(2),
        profile("mutool 1.29", "tesseract 5.5"),
    )
    .unwrap();
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

#[test]
fn cache_manifest_rejects_path_components_and_invalid_pages() {
    assert!(
        CacheManifest::new(
            "../escape".to_owned(),
            physical_page(1),
            profile("mutool 1.28", "tesseract 5.5")
        )
        .is_err()
    );
    assert!(Page::new(0).is_err());
    let mut unsafe_profile = profile("mutool 1.28", "tesseract 5.5");
    unsafe_profile.name = "../profile".to_owned();
    assert!(CacheManifest::new("a".repeat(64), physical_page(1), unsafe_profile).is_err());
}

#[test]
fn second_ocr_request_reuses_tsv_without_rendering_or_recognition() {
    struct CountingRenderer {
        renders: Cell<usize>,
    }
    impl PageRenderer for CountingRenderer {
        fn executable(&self) -> Result<&Path> {
            Ok(Path::new("/test/mutool"))
        }

        fn version(&self) -> Result<String> {
            Ok("mutool test".to_owned())
        }

        fn render_page(
            &self,
            _pdf: &Path,
            _page: Page,
            _dpi: u16,
            _rotation: i16,
            output: &Path,
        ) -> Result<()> {
            self.renders.set(self.renders.get() + 1);
            fs::write(output, b"image")?;
            Ok(())
        }
    }

    struct CountingOcr {
        orientations: Cell<usize>,
        recognitions: Cell<usize>,
    }
    impl OcrEngine for CountingOcr {
        fn executable(&self) -> Result<&Path> {
            Ok(Path::new("/test/tesseract"))
        }

        fn version(&self) -> Result<String> {
            Ok("tesseract test".to_owned())
        }

        fn rotation(&self, _image: &Path) -> Result<i16> {
            self.orientations.set(self.orientations.get() + 1);
            Ok(90)
        }

        fn tsv(&self, _image: &Path) -> Result<String> {
            self.recognitions.set(self.recognitions.get() + 1);
            Ok(include_str!("fixtures/tesseract-rotated-table.tsv").to_owned())
        }

        fn lang(&self) -> &'static str {
            "eng"
        }

        fn dpi(&self) -> u16 {
            300
        }

        fn psm(&self) -> u8 {
            3
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let pdf = temp.path().join("source.pdf");
    fs::write(&pdf, b"fixture").unwrap();
    let cache = OcrCache::new(temp.path().join("cache"));
    let renderer = CountingRenderer {
        renders: Cell::new(0),
    };
    let engine = CountingOcr {
        orientations: Cell::new(0),
        recognitions: Cell::new(0),
    };
    let hash = "c".repeat(64);

    ocr_page(&renderer, &engine, &cache, &pdf, &hash, physical_page(2)).unwrap();
    ocr_page(&renderer, &engine, &cache, &pdf, &hash, physical_page(2)).unwrap();

    assert_eq!(renderer.renders.get(), 2);
    assert_eq!(engine.orientations.get(), 1);
    assert_eq!(engine.recognitions.get(), 1);
    let request = CacheManifest::new(
        hash,
        physical_page(2),
        profile("mutool test", "tesseract test"),
    )
    .unwrap();
    let stored: CacheManifest =
        serde_json::from_slice(&fs::read(cache.entry_dir(&request).join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(stored.render_rotation_degrees, 90);
}

#[test]
fn failed_ocr_removes_temporary_page_renderings() {
    struct Renderer;
    impl PageRenderer for Renderer {
        fn executable(&self) -> Result<&Path> {
            Ok(Path::new("/test/mutool"))
        }

        fn version(&self) -> Result<String> {
            Ok("mutool test".to_owned())
        }

        fn render_page(
            &self,
            _pdf: &Path,
            _page: Page,
            _dpi: u16,
            _rotation: i16,
            output: &Path,
        ) -> Result<()> {
            fs::write(output, b"image")?;
            Ok(())
        }
    }

    struct FailingOcr;
    impl OcrEngine for FailingOcr {
        fn executable(&self) -> Result<&Path> {
            Ok(Path::new("/test/tesseract"))
        }

        fn version(&self) -> Result<String> {
            Ok("tesseract test".to_owned())
        }

        fn rotation(&self, _image: &Path) -> Result<i16> {
            anyhow::bail!("orientation failed")
        }

        fn tsv(&self, _image: &Path) -> Result<String> {
            unreachable!("recognition must not run after orientation failure")
        }

        fn lang(&self) -> &'static str {
            "eng"
        }

        fn dpi(&self) -> u16 {
            300
        }

        fn psm(&self) -> u8 {
            3
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let pdf = temp.path().join("source.pdf");
    fs::write(&pdf, b"fixture").unwrap();
    let cache_root = temp.path().join("cache");
    let cache = OcrCache::new(cache_root.clone());

    assert!(
        ocr_page(
            &Renderer,
            &FailingOcr,
            &cache,
            &pdf,
            &"d".repeat(64),
            physical_page(3),
        )
        .is_err()
    );
    assert!(!contains_png(&cache_root));
}

fn contains_png(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            contains_png(&path)
        } else {
            path.extension().and_then(|value| value.to_str()) == Some("png")
        }
    })
}

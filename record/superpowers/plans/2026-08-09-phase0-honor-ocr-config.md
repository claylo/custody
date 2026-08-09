# Phase 0: Honor the Existing OCR Config — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `pdf.ocr.enabled`, `dpi`, `lang`, and the new `page_segmentation_mode` config fields actually drive OCR behavior instead of being silently ignored.

**Architecture:** Thread `OcrConfig` from `Corpus` through `PdfTools` into `Tesseract` and `ocr_page`. Derive the profile name from settings so the filesystem stops lying. Refuse OCR when `enabled: false`.

**Tech Stack:** Rust, serde, clap, anyhow

**Spec:** `record/superpowers/specs/2026-07-30-multi-source-and-support-design.md` — Phase 0 section

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `src/config.rs` | Modify | Add `page_segmentation_mode` field to `OcrConfig`, validate it |
| `src/corpus.rs` | Modify | Store and expose `OcrConfig` |
| `src/pdf/tesseract.rs` | Modify | Remove hardcoded constants, accept config on `Tesseract` struct, derive profile name, update `ocr_page` signature |
| `src/pdf/mod.rs` | Modify | `PdfTools::new` takes `OcrConfig`, stores it, threads it through |
| `src/cli.rs` | Modify | Pass `OcrConfig` to `PdfTools`, refuse OCR in `locate_pdf` when disabled, derive doctor profile name |
| `src/validate.rs` | Modify | `ocr_disabled` issue when locator uses OCR backend but config says no |
| `tests/foundation.rs` | Modify | Add PSM validation test, add OCR config accessor test |
| `tests/tesseract_cache.rs` | Modify | Update `OcrProfile` construction to use derived profile names |
| `tests/cli.rs` | Modify | Update doctor profile assertion, add `ocr_disabled` test |
| `tests/validate.rs` | Modify | Add `ocr_disabled` validation test |

---

### Task 1: Add `page_segmentation_mode` to `OcrConfig`

**Files:**
- Modify: `src/config.rs:60-80` (OcrConfig struct and Default impl)
- Modify: `src/config.rs:122-148` (validate function)
- Test: `tests/foundation.rs`

- [ ] **Step 1: Write the failing test for PSM validation**

Add to `tests/foundation.rs`:

```rust
#[test]
fn rejects_invalid_page_segmentation_mode() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        dir.path(),
        "cache:\n  root: \"c\"\npdf:\n  ocr:\n    page_segmentation_mode: 14\n",
    );
    assert!(Corpus::discover_from(dir.path(), None).is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --locked --test foundation rejects_invalid_page_segmentation_mode`
Expected: FAIL — `page_segmentation_mode` is an unknown field (denied by `deny_unknown_fields`).

- [ ] **Step 3: Add `page_segmentation_mode` field to `OcrConfig`**

In `src/config.rs`, add the field to the struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OcrConfig {
    pub enabled: bool,
    pub dpi: u32,
    pub lang: String,
    pub page_segmentation_mode: u8,
}
```

Update the `Default` impl:

```rust
impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dpi: 300,
            lang: "eng".to_owned(),
            page_segmentation_mode: 3,
        }
    }
}
```

Add validation in `validate()`, after the `lang` check:

```rust
if config.pdf.ocr.page_segmentation_mode > 13 {
    bail!("pdf.ocr.page_segmentation_mode must be 0–13");
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --locked --test foundation rejects_invalid_page_segmentation_mode`
Expected: PASS

- [ ] **Step 5: Run full suite to verify no regressions**

Run: `cargo test --locked`
Expected: 56 tests pass. Existing configs omit `page_segmentation_mode`, which hits `#[serde(default)]` and gets `3`.

- [ ] **Step 6: Commit**

```
feat(config): add page_segmentation_mode to OcrConfig
```

---

### Task 2: Store `OcrConfig` on `Corpus` and expose it

**Files:**
- Modify: `src/corpus.rs:11-50` (Corpus struct and constructors)
- Test: `tests/foundation.rs`

- [ ] **Step 1: Write the failing test for OcrConfig accessor**

Add to `tests/foundation.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --locked --test foundation exposes_ocr_config`
Expected: FAIL — `ocr_config()` method does not exist.

- [ ] **Step 3: Add `ocr_config` field and accessor to `Corpus`**

In `src/corpus.rs`, add the field to the struct:

```rust
pub struct Corpus {
    root: PathBuf,
    layout: CorpusLayout,
    cache_root: PathBuf,
    config_file: Option<PathBuf>,
    terms: crate::terms::Terms,
    ocr_config: crate::config::OcrConfig,
}
```

Set it in `from_discovered`:

```rust
Ok(Self {
    root,
    layout: config.corpus,
    cache_root,
    config_file,
    terms: config.terms,
    ocr_config: config.pdf.ocr,
})
```

Add accessor:

```rust
#[must_use]
pub const fn ocr_config(&self) -> &crate::config::OcrConfig {
    &self.ocr_config
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --locked --test foundation exposes_ocr_config`
Expected: PASS

- [ ] **Step 5: Run full suite**

Run: `cargo test --locked`
Expected: all pass

- [ ] **Step 6: Commit**

```
feat(corpus): store and expose OcrConfig
```

---

### Task 3: Make `Tesseract` configurable and derive profile name

**Files:**
- Modify: `src/pdf/tesseract.rs:17-22` (remove constants), `46-76` (Tesseract struct), `92-116` (tsv/rotation methods), `149-204` (ocr_page)
- Test: `tests/tesseract_cache.rs`

- [ ] **Step 1: Write the failing test for profile name derivation**

Add to `tests/tesseract_cache.rs`:

```rust
use receipts::pdf::tesseract::profile_name;

#[test]
fn profile_name_derives_from_settings() {
    assert_eq!(profile_name("eng", 300), "tesseract-eng-300dpi-v1");
    assert_eq!(profile_name("deu", 600), "tesseract-deu-600dpi-v1");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --locked --test tesseract_cache profile_name_derives`
Expected: FAIL — `profile_name` function does not exist.

- [ ] **Step 3: Add `profile_name` function, make `Tesseract` configurable, update `ocr_page`**

In `src/pdf/tesseract.rs`, replace the constants with:

```rust
pub fn profile_name(lang: &str, dpi: u16) -> String {
    format!("tesseract-{lang}-{dpi}dpi-v1")
}
```

Replace the `Tesseract` struct and its `Default` impl:

```rust
#[derive(Debug, Clone)]
pub struct Tesseract {
    executable: String,
    lang: String,
    dpi: u16,
    psm: u8,
}

impl Default for Tesseract {
    fn default() -> Self {
        Self {
            executable: "tesseract".to_owned(),
            lang: "eng".to_owned(),
            dpi: 300,
            psm: 3,
        }
    }
}

impl Tesseract {
    #[must_use]
    pub fn new(lang: String, dpi: u16, psm: u8) -> Self {
        Self {
            executable: "tesseract".to_owned(),
            lang,
            dpi,
            psm,
        }
    }

    pub fn lang(&self) -> &str {
        &self.lang
    }

    pub fn dpi(&self) -> u16 {
        self.dpi
    }

    pub fn psm(&self) -> u8 {
        self.psm
    }
}
```

Update `Tesseract::tsv()` to use stored config:

```rust
fn tsv(&self, image: &Path) -> Result<String> {
    let output = run(
        Command::new(&self.executable)
            .arg(image)
            .args([
                "stdout",
                "-l",
                &self.lang,
                "--psm",
                &self.psm.to_string(),
                "tsv",
            ]),
        "Tesseract OCR",
    )?;
    String::from_utf8(output.stdout).context("Tesseract TSV was not UTF-8")
}
```

Note: `Tesseract::rotation()` keeps `-l osd --psm 0` — orientation detection always uses the OSD script, not the recognition language.

Update the command template constants to be functions that derive from config:

```rust
fn render_command(dpi: u16) -> String {
    format!("mutool draw -q -r {dpi} [-R ROTATION] -o OUTPUT PDF PAGE")
}

fn recognition_command(lang: &str, psm: u8) -> String {
    format!("tesseract IMAGE stdout -l {lang} --psm {psm} tsv")
}

const ORIENTATION_COMMAND: &str = "tesseract IMAGE stdout -l osd --psm 0";
```

Update `ocr_page` to build the profile from the tesseract's stored config:

```rust
pub fn ocr_page(
    mutool: &impl PageRenderer,
    tesseract: &Tesseract,
    cache: &OcrCache,
    pdf: &Path,
    pdf_sha256: &str,
    page: usize,
) -> Result<ExtractedPage> {
    if page == 0 {
        bail!("PDF pages are one-based");
    }
    let dpi = tesseract.dpi();
    let lang = tesseract.lang();
    let psm = tesseract.psm();
    let profile = OcrProfile {
        name: profile_name(lang, dpi),
        language: lang.to_owned(),
        dpi,
        page_segmentation_mode: psm,
        mutool_version: mutool.version()?,
        tesseract_version: tesseract.version()?,
        render_command: render_command(dpi),
        orientation_command: ORIENTATION_COMMAND.to_owned(),
        recognition_command: recognition_command(lang, psm),
    };
    let manifest = CacheManifest::new(pdf_sha256.to_owned(), page, profile)?;
    if let Some(tsv) = cache.load(&manifest)? {
        return extracted_page(page, &tsv);
    }

    let directory = cache.entry_dir(&manifest);
    fs::create_dir_all(&directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let initial = directory.join(format!(".page-{}-{nonce}.png", std::process::id()));
    let rotated = directory.join(format!(".page-{}-{nonce}-rotated.png", std::process::id()));
    let result = (|| {
        mutool.render_page(pdf, page, dpi, 0, &initial)?;
        let rotation = tesseract.rotation(&initial)?;
        let ocr_image = if rotation == 0 {
            &initial
        } else {
            mutool.render_page(pdf, page, dpi, rotation, &rotated)?;
            &rotated
        };
        let tsv = tesseract.tsv(ocr_image)?;
        let stored_manifest = manifest.clone().with_render_rotation(rotation)?;
        cache.store(&stored_manifest, &tsv)?;
        Ok::<_, anyhow::Error>(tsv)
    })();
    let _ = fs::remove_file(&initial);
    let _ = fs::remove_file(&rotated);
    let tsv = result?;
    extracted_page(page, &tsv)
}
```

`ocr_page` stays generic over `&impl OcrEngine` because `tests/tesseract_cache.rs` calls it with mock trait objects (`CountingOcr`, `FailingOcr`). The config getters must live on the trait.

Add to `OcrEngine`:

```rust
pub trait OcrEngine {
    fn version(&self) -> Result<String>;
    fn rotation(&self, image: &Path) -> Result<i16>;
    fn tsv(&self, image: &Path) -> Result<String>;
    fn lang(&self) -> &str;
    fn dpi(&self) -> u16;
    fn psm(&self) -> u8;
}
```

Implement on `Tesseract` using stored fields. The `ocr_page` signature stays:

```rust
pub fn ocr_page(
    mutool: &impl PageRenderer,
    tesseract: &impl OcrEngine,
    cache: &OcrCache,
    ...
```

And reads config via `tesseract.dpi()`, `tesseract.lang()`, `tesseract.psm()`.

- [ ] **Step 4: Update test doubles in `tests/tesseract_cache.rs`**

Both `CountingOcr` and `FailingOcr` implement `OcrEngine` and are passed to `ocr_page`. Add the three new trait methods to each:

```rust
// On CountingOcr:
fn lang(&self) -> &str { "eng" }
fn dpi(&self) -> u16 { 300 }
fn psm(&self) -> u8 { 3 }

// On FailingOcr:
fn lang(&self) -> &str { "eng" }
fn dpi(&self) -> u16 { 300 }
fn psm(&self) -> u8 { 3 }
```

These return defaults matching the profile constructed by `profile()` helper so the cache key assertions continue to pass.

- [ ] **Step 5: Update `profile()` helper and `OcrProfile` construction in `tests/tesseract_cache.rs`**

Update the `profile()` helper to use derived values:

```rust
use receipts::pdf::tesseract::profile_name;

fn profile(mutool: &str, tesseract: &str) -> OcrProfile {
    OcrProfile {
        name: profile_name("eng", 300),
        language: "eng".to_owned(),
        dpi: 300,
        page_segmentation_mode: 3,
        mutool_version: mutool.to_owned(),
        tesseract_version: tesseract.to_owned(),
        render_command: "mutool draw -q -r 300 [-R ROTATION] -o OUTPUT PDF PAGE".to_owned(),
        orientation_command: "tesseract IMAGE stdout -l osd --psm 0".to_owned(),
        recognition_command: "tesseract IMAGE stdout -l eng --psm 3 tsv".to_owned(),
    }
}
```

The command template strings must also match what `ocr_page` now produces from config. Since defaults are `lang="eng"`, `dpi=300`, `psm=3`, these strings stay identical to the old constants — the important thing is they're now computed consistently.

- [ ] **Step 6: Run tests**

Run: `cargo test --locked`
Expected: all pass. The profile name for default settings is still `tesseract-eng-300dpi-v1`, so existing cache behavior is unchanged when config is default.

- [ ] **Step 7: Commit**

```
feat(ocr): derive profile from config, remove hardcoded constants
```

---

### Task 4: Thread `OcrConfig` through `PdfTools` and update call sites

**Files:**
- Modify: `src/pdf/mod.rs:90-123` (PdfTools struct and impl)
- Modify: `src/cli.rs:170,294,392,442` (PdfTools::new call sites)
- Modify: `src/cli.rs:156` (doctor profile name)
- Test: `tests/cli.rs`

- [ ] **Step 1: Write the failing test for derived doctor profile name**

In `tests/cli.rs`, update the doctor assertion. The current test checks:

```rust
assert!(output.contains("OCR profile: ok (tesseract-eng-300dpi-v1)"));
```

This will continue to pass with defaults. Add a new test that uses a custom DPI:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --locked --test cli doctor_reports_configured_ocr_profile`
Expected: FAIL — doctor still shows the hardcoded profile name.

- [ ] **Step 3: Update `PdfTools` to accept and store `OcrConfig`**

In `src/pdf/mod.rs`:

```rust
pub struct PdfTools {
    pub mutool: mutool::Mutool,
    pub tesseract: tesseract::Tesseract,
    pub cache: cache::OcrCache,
    ocr_enabled: bool,
}

impl PdfTools {
    #[must_use]
    pub fn new(cache_root: std::path::PathBuf, ocr: &crate::config::OcrConfig) -> Self {
        let dpi = u16::try_from(ocr.dpi).unwrap_or(u16::MAX);
        Self {
            mutool: mutool::Mutool::default(),
            tesseract: tesseract::Tesseract::new(
                ocr.lang.clone(),
                dpi,
                ocr.page_segmentation_mode,
            ),
            cache: cache::OcrCache::new(cache_root),
            ocr_enabled: ocr.enabled,
        }
    }
}
```

Update `PdfTextProvider` impl:

```rust
impl PdfTextProvider for PdfTools {
    fn native_pages(&self, pdf: &Path, page: Option<usize>) -> Result<Vec<ExtractedPage>> {
        self.mutool.native_pages(pdf, page)
    }

    fn ocr_page(&self, pdf: &Path, pdf_sha256: &str, page: usize) -> Result<ExtractedPage> {
        if !self.ocr_enabled {
            anyhow::bail!("OCR is disabled in configuration");
        }
        tesseract::ocr_page(
            &self.mutool,
            &self.tesseract,
            &self.cache,
            pdf,
            pdf_sha256,
            page,
        )
    }
}
```

- [ ] **Step 4: Update all `PdfTools::new` call sites in `src/cli.rs`**

Every `PdfTools::new(corpus.cache_root().to_path_buf())` becomes `PdfTools::new(corpus.cache_root().to_path_buf(), corpus.ocr_config())`.

There are four call sites in `cli.rs`:
- `doctor()` — line ~170
- `locate()` — line ~294
- `check()` — line ~392
- `audit()` — line ~442

- [ ] **Step 5: Update doctor to show derived profile name**

In `cli.rs`, the doctor function currently has:

```rust
(
    "OCR profile",
    true,
    crate::pdf::tesseract::PROFILE_NAME.to_owned(),
),
```

Replace with:

```rust
{
    let ocr = corpus.ocr_config();
    let dpi = u16::try_from(ocr.dpi).unwrap_or(u16::MAX);
    (
        "OCR profile",
        true,
        crate::pdf::tesseract::profile_name(&ocr.lang, dpi),
    )
}
```

Note: the `PROFILE_NAME` constant no longer exists after Task 3. If the compiler errors in this step due to a missing constant, that's the signal to update this line.

- [ ] **Step 6: Run tests**

Run: `cargo test --locked`
Expected: all pass, including the new `doctor_reports_configured_ocr_profile`.

- [ ] **Step 7: Commit**

```
feat(ocr): thread OcrConfig through PdfTools and update doctor
```

---

### Task 5: Honor `enabled: false` — refuse OCR when disabled

**Files:**
- Modify: `src/cli.rs:335-383` (locate_pdf function)
- Modify: `src/validate.rs:194-218` (extract_pdf_page function)
- Test: `tests/cli.rs`, `tests/validate.rs`

- [ ] **Step 1: Write the failing test for `ocr_disabled` in validation**

Add to `tests/validate.rs`:

```rust
#[test]
fn ocr_disabled_rejects_ocr_backend_locator() {
    let temp = tempfile::tempdir().unwrap();
    for path in ["summaries", "md/smith-2019", "pdfs"] {
        fs::create_dir_all(temp.path().join(path)).unwrap();
    }
    fs::write(
        temp.path().join("receipts.yaml"),
        "cache:\n  root: \".cache/pdf-text\"\npdf:\n  ocr:\n    enabled: false\n",
    )
    .unwrap();
    let md_source = "No measurable turbulent mixing was observed.\n";
    let md_path = temp.path().join("md/smith-2019/smith-2019.md");
    fs::write(&md_path, md_source).unwrap();
    let pdf_path = temp.path().join("pdfs/smith-2019.pdf");
    fs::write(&pdf_path, b"%PDF-1.7\n").unwrap();

    let md_sha = receipts::hash::sha256_file(&md_path).unwrap();
    let pdf_sha = receipts::hash::sha256_file(&pdf_path).unwrap();
    let claim_sha = receipts::hash::sha256_bytes(
        b"Transport remained laminar across all three test regimes.",
    );

    let summary_yaml = format!(
        "id: smith-2019\n\
         claims:\n  - \"Transport remained laminar across all three test regimes.\"\n\
         evidence:\n\
         \x20 markdown:\n    source: \"md/smith-2019/smith-2019.md\"\n    sha256: \"{md_sha}\"\n\
         \x20 pdf:\n    source: \"pdfs/smith-2019.pdf\"\n    sha256: \"{pdf_sha}\"\n\
         \x20 claims:\n    - claim: 0\n      claim_sha256: \"{claim_sha}\"\n\
         \x20     locators:\n\
         \x20       - exact: \"no measurable turbulent mixing was observed\"\n\
         \x20         markdown:\n           line: 1\n           column: 1\n           unit: paragraph\n\
         \x20         pdf:\n           page: 1\n           backend: tesseract-ocr\n"
    );
    fs::write(
        temp.path().join("summaries/smith-2019.yaml"),
        &summary_yaml,
    )
    .unwrap();

    let corpus = receipts::corpus::Corpus::discover_from(temp.path(), None).unwrap();
    let summary = receipts::evidence::parse_summary(&summary_yaml, corpus.terms()).unwrap();
    let tools = receipts::pdf::PdfTools::new(
        corpus.cache_root().to_path_buf(),
        corpus.ocr_config(),
    );
    let report = receipts::validate::validate_document(&corpus, &summary, &tools);

    assert!(
        !report.is_valid(),
        "validation should fail when OCR is disabled but locator uses OCR backend"
    );
    assert!(
        report.issues.iter().any(|i| i.code == "ocr_disabled"),
        "should produce ocr_disabled issue: {:?}",
        report.issues
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --locked --test validate ocr_disabled`
Expected: FAIL — `ocr_disabled` issue is not produced. Instead, either OCR runs (ignoring the flag) or `ocr_page` errors with the bail added in Task 4 (which becomes a `pdf_extraction_failed`, not `ocr_disabled`).

- [ ] **Step 3: Add `ocr_disabled` check in `validate_document`**

The check must happen _before_ `extract_pdf_page` is called for an OCR backend. In `src/validate.rs`, update `validate_document` to accept OCR-enabled state.

Add `ocr_enabled: bool` parameter to `extract_pdf_page`:

```rust
fn extract_pdf_page(
    provider: &impl PdfTextProvider,
    pdf_path: &Path,
    pdf_sha256: &str,
    backend: PdfBackend,
    page: usize,
    ocr_enabled: bool,
) -> Result<String, String> {
    let extracted = match backend {
        PdfBackend::MutoolNative => {
            provider
                .native_pages(pdf_path, Some(page))
                .and_then(|mut pages| {
                    if pages.len() == 1 {
                        Ok(pages.remove(0))
                    } else {
                        anyhow::bail!("native backend returned {} pages", pages.len())
                    }
                })
        }
        PdfBackend::TesseractOcr if !ocr_enabled => {
            return Err("OCR is disabled".to_owned());
        }
        PdfBackend::TesseractOcr => provider.ocr_page(pdf_path, pdf_sha256, page),
    };
    extracted
        .map(|page| normalize(&page.text))
        .map_err(|error| error.to_string())
}
```

But this returns an error string, which the caller turns into a `pdf_extraction_failed` issue. We need `ocr_disabled` specifically. Change the approach — check _before_ calling `extract_pdf_page` in the locator loop within `validate_document`:

In `validate_document`, right before the `let key = ...` line inside the locator loop (around line 144), add:

```rust
if locator.pdf.backend == PdfBackend::TesseractOcr
    && !corpus.ocr_config().enabled
{
    issues.push(issue(
        "ocr_disabled",
        format!(
            "locator uses {} but OCR is disabled in configuration",
            locator.pdf.backend.as_str()
        ),
        Some(entry.claim),
        Some(locator_index),
    ));
    continue;
}
```

This skips the PDF extraction entirely and produces the correct error code. The `continue` skips the rest of the locator's PDF validation — there's no point extracting if we already know the backend is disallowed.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --locked --test validate ocr_disabled`
Expected: PASS

- [ ] **Step 5: Write the failing test for `ocr_disabled` in `locate`**

Add to `tests/cli.rs`:

```rust
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
            "does not exist natively",
            "--page",
            "1",
        ],
    );

    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(
        stderr.contains("OCR is disabled"),
        "should refuse OCR fallback: {stderr}"
    );
}
```

- [ ] **Step 6: Run test to verify it fails**

Run: `cargo test --locked --test cli locate_refuses_ocr_fallback`
Expected: FAIL — locate currently attempts OCR regardless.

- [ ] **Step 7: Add `ocr_disabled` check in `locate_pdf`**

In `src/cli.rs`, in the `locate_pdf` function, right before the OCR fallback block (after the `let Some(page) = page else { ... }` guard around line 369):

```rust
if !corpus.ocr_config().enabled {
    bail!("exact text was not found natively and OCR is disabled in configuration");
}
```

This requires `locate_pdf` to receive the corpus. Currently it doesn't — it takes `tools`, `pdf`, `pdf_sha256`, `page`, `exact`. Update the signature to add `ocr_enabled: bool`:

```rust
fn locate_pdf(
    tools: &PdfTools,
    pdf: &Path,
    pdf_sha256: &str,
    page: Option<usize>,
    exact: &str,
    ocr_enabled: bool,
) -> Result<(usize, PdfBackend, Option<PdfBbox>, Option<f64>)> {
```

Add the check right before the OCR fallback:

```rust
let Some(page) = page else {
    bail!("exact text was not found natively; pass --page to permit OCR fallback");
};
if !ocr_enabled {
    bail!("exact text was not found natively and OCR is disabled in configuration");
}
let ocr = tools.ocr_page(pdf, pdf_sha256, page)?;
```

Update the call site in `locate()` to pass `corpus.ocr_config().enabled`.

- [ ] **Step 8: Run test to verify it passes**

Run: `cargo test --locked --test cli locate_refuses_ocr_fallback`
Expected: PASS

- [ ] **Step 9: Run full suite**

Run: `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: all clean, all 60+ tests pass.

- [ ] **Step 10: Commit**

```
feat(ocr): honor enabled: false with ocr_disabled error
```

---

### Task 6: Final verification and housekeeping commit

**Files:**
- Modify: `receipts.yaml` (if needed — ensure example config mentions `page_segmentation_mode`)

- [ ] **Step 1: Run full CI suite**

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo run --locked -- doctor
```

Expected: all green. Doctor output shows `OCR profile: ok (tesseract-eng-300dpi-v1)` (default config).

- [ ] **Step 2: Verify config-driven behavior manually**

Temporarily edit `receipts.yaml` to set `dpi: 600`:

```sh
cargo run --locked -- --format text doctor
```

Expected: `OCR profile: ok (tesseract-eng-600dpi-v1)`

Revert the change.

- [ ] **Step 3: Commit the spec move and any housekeeping**

```
chore: move specs from docs/ to record/superpowers/specs/
```

This covers the directory restructure done at the start of this session.

use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};

use super::{
    Executable, ExtractedPage, NumericVersion, PdfBbox, TextSpan,
    cache::{CacheManifest, OcrCache, OcrProfile},
    mutool::Mutool,
    parse_numeric_version, run,
};
use crate::{coordinate::Page, normalize::normalize};

const ORIENTATION_COMMAND: &str = "tesseract IMAGE stdout -l osd --psm 0";
pub const SUPPORTED_VERSION_RANGE: &str = ">=5.5.0, <5.6.0";
const MIN_SUPPORTED_VERSION: NumericVersion = NumericVersion::new(5, 5, 0);
const MAX_SUPPORTED_VERSION: NumericVersion = NumericVersion::new(5, 6, 0);

pub fn validate_version(output: &str) -> Result<()> {
    let version = parse_numeric_version(output)?;
    if !(MIN_SUPPORTED_VERSION..MAX_SUPPORTED_VERSION).contains(&version) {
        bail!("Tesseract version {version} is unsupported; supported {SUPPORTED_VERSION_RANGE}");
    }
    Ok(())
}

/// Cache profile name for a language and render resolution.
#[must_use]
pub fn profile_name(lang: &str, dpi: u16) -> String {
    format!("tesseract-{lang}-{dpi}dpi-v2")
}

fn render_command(dpi: u16) -> String {
    format!("mutool draw -q -r {dpi} [-R ROTATION] -o OUTPUT PDF PAGE")
}

fn recognition_command(lang: &str, psm: u8) -> String {
    format!("tesseract IMAGE stdout -l {lang} --psm {psm} tsv")
}

/// Parsed word text and diagnostic confidence from a Tesseract TSV.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTsv {
    pub text: String,
    pub mean_confidence: Option<f64>,
    pub spans: Vec<TextSpan>,
}

impl ParsedTsv {
    #[must_use]
    pub fn into_extracted_page(self, page: Page) -> ExtractedPage {
        ExtractedPage {
            page,
            text: self.text,
            spans: self.spans,
            mean_confidence: self.mean_confidence,
        }
    }
}

/// Tesseract command adapter.
#[derive(Debug, Clone)]
pub struct Tesseract {
    executable: Executable,
    lang: String,
    dpi: u16,
    psm: u8,
}

/// `MuPDF` behavior required by the page-level OCR pipeline.
pub trait PageRenderer {
    fn executable(&self) -> Result<&Path>;
    fn version(&self) -> Result<String>;
    fn render_page(
        &self,
        pdf: &Path,
        page: Page,
        dpi: u16,
        rotation: i16,
        output: &Path,
    ) -> Result<()>;
}

/// Tesseract behavior required by the page-level OCR pipeline.
pub trait OcrEngine {
    fn executable(&self) -> Result<&Path>;
    fn version(&self) -> Result<String>;
    fn rotation(&self, image: &Path) -> Result<i16>;
    fn tsv(&self, image: &Path) -> Result<String>;
    fn lang(&self) -> &str;
    fn dpi(&self) -> u16;
    fn psm(&self) -> u8;
}

impl Default for Tesseract {
    fn default() -> Self {
        Self {
            executable: Executable::resolve("tesseract", None),
            lang: "eng".to_owned(),
            dpi: 300,
            psm: 3,
        }
    }
}

impl Tesseract {
    #[must_use]
    pub fn new(lang: String, dpi: u16, psm: u8) -> Self {
        Self::with_executable(None, lang, dpi, psm)
    }

    #[must_use]
    pub fn with_executable(executable: Option<&Path>, lang: String, dpi: u16, psm: u8) -> Self {
        Self {
            executable: Executable::resolve("tesseract", executable),
            lang,
            dpi,
            psm,
        }
    }

    pub fn executable(&self) -> Result<&Path> {
        self.executable.path()
    }

    pub fn version(&self) -> Result<String> {
        let output = run(
            Command::new(self.executable()?).arg("--version"),
            "Tesseract version probe",
        )?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned())
    }

    fn rotation(&self, image: &Path) -> Result<i16> {
        let output = run(
            Command::new(self.executable()?)
                .arg(image)
                .args(["stdout", "-l", "osd", "--psm", "0"]),
            "Tesseract orientation detection",
        )?;
        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(parse_orientation(&combined))
    }

    fn tsv(&self, image: &Path) -> Result<String> {
        let output = run(
            Command::new(self.executable()?).arg(image).args([
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
}

impl PageRenderer for Mutool {
    fn executable(&self) -> Result<&Path> {
        Mutool::executable(self)
    }

    fn version(&self) -> Result<String> {
        Mutool::version(self)
    }

    fn render_page(
        &self,
        pdf: &Path,
        page: Page,
        dpi: u16,
        rotation: i16,
        output: &Path,
    ) -> Result<()> {
        Mutool::render_page(self, pdf, page, dpi, rotation, output)
    }
}

impl OcrEngine for Tesseract {
    fn executable(&self) -> Result<&Path> {
        Tesseract::executable(self)
    }

    fn version(&self) -> Result<String> {
        Tesseract::version(self)
    }

    fn rotation(&self, image: &Path) -> Result<i16> {
        Tesseract::rotation(self, image)
    }

    fn tsv(&self, image: &Path) -> Result<String> {
        Tesseract::tsv(self, image)
    }

    fn lang(&self) -> &str {
        &self.lang
    }

    fn dpi(&self) -> u16 {
        self.dpi
    }

    fn psm(&self) -> u8 {
        self.psm
    }
}

/// Build the immutable OCR profile from the current tool versions and settings.
pub fn resolve_profile(
    mutool: &impl PageRenderer,
    tesseract: &impl OcrEngine,
) -> Result<OcrProfile> {
    let lang = tesseract.lang();
    let dpi = tesseract.dpi();
    let psm = tesseract.psm();
    Ok(OcrProfile {
        name: profile_name(lang, dpi),
        language: lang.to_owned(),
        dpi,
        page_segmentation_mode: psm,
        mutool_executable: mutool.executable()?.display().to_string(),
        mutool_version: mutool.version()?,
        tesseract_executable: tesseract.executable()?.display().to_string(),
        tesseract_version: tesseract.version()?,
        render_command: render_command(dpi),
        orientation_command: ORIENTATION_COMMAND.to_owned(),
        recognition_command: recognition_command(lang, psm),
    })
}

/// Extract a page with the engine's configured OCR profile, reusing a matching cache entry.
pub fn ocr_page(
    mutool: &impl PageRenderer,
    tesseract: &impl OcrEngine,
    cache: &OcrCache,
    pdf: &Path,
    pdf_sha256: &str,
    page: Page,
) -> Result<ExtractedPage> {
    let profile = resolve_profile(mutool, tesseract)?;
    ocr_page_with_profile(mutool, tesseract, cache, &profile, pdf, pdf_sha256, page)
}

/// Extract a page using a pre-resolved OCR profile, avoiding repeated version probes.
pub fn ocr_page_with_profile(
    mutool: &impl PageRenderer,
    tesseract: &impl OcrEngine,
    cache: &OcrCache,
    profile: &OcrProfile,
    pdf: &Path,
    pdf_sha256: &str,
    page: Page,
) -> Result<ExtractedPage> {
    let manifest = CacheManifest::new(pdf_sha256.to_owned(), page, profile.clone())?;
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
        mutool.render_page(pdf, page, profile.dpi, 0, &initial)?;
        let rotation = tesseract.rotation(&initial)?;
        let ocr_image = if rotation == 0 {
            &initial
        } else {
            mutool.render_page(pdf, page, profile.dpi, rotation, &rotated)?;
            &rotated
        };
        let tsv = tesseract.tsv(ocr_image)?;
        let stored_manifest = manifest.clone().with_render_rotation(rotation)?;
        cache.store(&stored_manifest, &tsv)?;
        Ok::<_, anyhow::Error>(tsv)
    })();
    if let Err(error) = fs::remove_file(&initial)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("warning: failed to remove {}: {error}", initial.display());
    }
    if let Err(error) = fs::remove_file(&rotated)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("warning: failed to remove {}: {error}", rotated.display());
    }
    let tsv = result?;
    extracted_page(page, &tsv)
}

/// Reconstruct Tesseract words in TSV row order.
pub fn parse_tsv(source: &str) -> Result<ParsedTsv> {
    let mut lines = source.lines();
    let header = lines.next().context("Tesseract TSV is empty")?;
    let columns: Vec<_> = header.split('\t').collect();
    let text_index = columns
        .iter()
        .position(|column| *column == "text")
        .context("Tesseract TSV has no text column")?;
    let confidence_index = columns
        .iter()
        .position(|column| *column == "conf")
        .context("Tesseract TSV has no conf column")?;
    let geometry_indexes = ["left", "top", "width", "height"]
        .map(|name| columns.iter().position(|column| *column == name));
    // Tesseract numbers lines within paragraphs within blocks; a change in
    // that triple is a visual line break, kept as `\n` for dehyphenation.
    let line_key_indexes = ["block_num", "par_num", "line_num"]
        .map(|name| columns.iter().position(|column| *column == name));
    let mut text_buffer = String::new();
    let mut previous_line_key: Option<[&str; 3]> = None;
    let mut confidences = Vec::new();
    let mut spans = Vec::new();

    let geo_indexes: Option<[usize; 4]> = match geometry_indexes {
        [Some(a), Some(b), Some(c), Some(d)] => Some([a, b, c, d]),
        _ => None,
    };
    let line_indexes: Option<[usize; 3]> = match line_key_indexes {
        [Some(a), Some(b), Some(c)] => Some([a, b, c]),
        _ => None,
    };
    for line in lines {
        let fields: Vec<_> = line.split('\t').collect();
        let Some(text) = fields.get(text_index).map(|value| value.trim()) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let line_key = line_indexes
            .and_then(|[bi, pi, li]| Some([*fields.get(bi)?, *fields.get(pi)?, *fields.get(li)?]));
        if !text_buffer.is_empty() {
            let same_line = line_key.is_some() && line_key == previous_line_key;
            text_buffer.push(if same_line { ' ' } else { '\n' });
        }
        previous_line_key = line_key;
        text_buffer.push_str(text);
        let bbox = geo_indexes.and_then(|[li, ti, wi, hi]| {
            Some(PdfBbox {
                x: fields.get(li)?.parse::<f64>().ok()?,
                y: fields.get(ti)?.parse::<f64>().ok()?,
                width: fields.get(wi)?.parse::<f64>().ok()?,
                height: fields.get(hi)?.parse::<f64>().ok()?,
            })
        });
        spans.push(TextSpan {
            text: text.to_owned(),
            bbox,
        });
        if let Some(confidence) = fields
            .get(confidence_index)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| *value >= 0.0)
        {
            confidences.push(confidence);
        }
    }
    let mean_confidence = (!confidences.is_empty()).then(|| {
        let count = u32::try_from(confidences.len()).unwrap_or(u32::MAX);
        confidences.iter().sum::<f64>() / f64::from(count)
    });
    Ok(ParsedTsv {
        text: normalize(&text_buffer),
        mean_confidence,
        spans,
    })
}

/// Read the rerender rotation requested by Tesseract OSD.
#[must_use]
pub fn parse_orientation(output: &str) -> i16 {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Rotate:"))
        .and_then(|value| value.trim().parse::<i16>().ok())
        .filter(|value| matches!(value, 0 | 90 | 180 | 270))
        .unwrap_or(0)
}

fn extracted_page(page: Page, tsv: &str) -> Result<ExtractedPage> {
    Ok(parse_tsv(tsv)?.into_extracted_page(page))
}

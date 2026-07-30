use std::{
    fs,
    path::Path,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};

use super::{
    ExtractedPage,
    cache::{CacheManifest, OcrCache, OcrProfile},
    mutool::Mutool,
};
use crate::normalize::normalize;

const PROFILE_NAME: &str = "tesseract-eng-300dpi-v1";
const OCR_DPI: u16 = 300;
const OCR_PSM: u8 = 3;

/// Parsed word text and diagnostic confidence from a Tesseract TSV.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTsv {
    pub text: String,
    pub mean_confidence: Option<f64>,
}

/// Tesseract command adapter.
#[derive(Debug, Clone)]
pub struct Tesseract {
    executable: String,
}

impl Default for Tesseract {
    fn default() -> Self {
        Self {
            executable: "tesseract".to_owned(),
        }
    }
}

impl Tesseract {
    pub fn version(&self) -> Result<String> {
        let output = run(
            Command::new(&self.executable).arg("--version"),
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
            Command::new(&self.executable)
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
            Command::new(&self.executable)
                .arg(image)
                .args(["stdout", "-l", "eng", "--psm", "3", "tsv"]),
            "Tesseract OCR",
        )?;
        String::from_utf8(output.stdout).context("Tesseract TSV was not UTF-8")
    }
}

/// Extract a page with the fixed OCR profile, reusing a matching cache entry.
pub fn ocr_page(
    mutool: &Mutool,
    tesseract: &Tesseract,
    cache: &OcrCache,
    pdf: &Path,
    pdf_sha256: &str,
    page: usize,
) -> Result<ExtractedPage> {
    if page == 0 {
        bail!("PDF pages are one-based");
    }
    let profile = OcrProfile {
        name: PROFILE_NAME.to_owned(),
        language: "eng".to_owned(),
        dpi: OCR_DPI,
        page_segmentation_mode: OCR_PSM,
        mutool_version: mutool.version()?,
        tesseract_version: tesseract.version()?,
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
    mutool.render_page(pdf, page, OCR_DPI, 0, &initial)?;
    let rotation = tesseract.rotation(&initial)?;
    let ocr_image = if rotation == 0 {
        &initial
    } else {
        mutool.render_page(pdf, page, OCR_DPI, rotation, &rotated)?;
        &rotated
    };
    let tsv = tesseract.tsv(ocr_image)?;
    cache.store(&manifest, &tsv)?;
    let _ = fs::remove_file(&initial);
    let _ = fs::remove_file(&rotated);
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
    let mut words = Vec::new();
    let mut confidences = Vec::new();

    for line in lines {
        let fields: Vec<_> = line.split('\t').collect();
        let Some(text) = fields.get(text_index).map(|value| value.trim()) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        words.push(text);
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
        text: normalize(&words.join(" ")),
        mean_confidence,
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

fn extracted_page(page: usize, tsv: &str) -> Result<ExtractedPage> {
    Ok(ExtractedPage {
        page,
        text: parse_tsv(tsv)?.text,
    })
}

fn run(command: &mut Command, label: &str) -> Result<Output> {
    let output = command
        .output()
        .with_context(|| format!("failed to execute {label}"))?;
    if !output.status.success() {
        bail!(
            "{label} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output)
}

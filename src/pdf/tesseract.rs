use std::{
    fs,
    path::Path,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};

use super::{
    ExtractedPage, PdfBbox, TextSpan,
    cache::{CacheManifest, OcrCache, OcrProfile},
    mutool::Mutool,
};
use crate::normalize::normalize;

const ORIENTATION_COMMAND: &str = "tesseract IMAGE stdout -l osd --psm 0";

/// Cache profile name for a language and render resolution.
#[must_use]
pub fn profile_name(lang: &str, dpi: u16) -> String {
    format!("tesseract-{lang}-{dpi}dpi-v1")
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
    pub fn into_extracted_page(self, page: usize) -> ExtractedPage {
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
    executable: String,
    lang: String,
    dpi: u16,
    psm: u8,
}

/// `MuPDF` behavior required by the page-level OCR pipeline.
pub trait PageRenderer {
    fn version(&self) -> Result<String>;
    fn render_page(
        &self,
        pdf: &Path,
        page: usize,
        dpi: u16,
        rotation: i16,
        output: &Path,
    ) -> Result<()>;
}

/// Tesseract behavior required by the page-level OCR pipeline.
pub trait OcrEngine {
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
            Command::new(&self.executable).arg(image).args([
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
    fn version(&self) -> Result<String> {
        Mutool::version(self)
    }

    fn render_page(
        &self,
        pdf: &Path,
        page: usize,
        dpi: u16,
        rotation: i16,
        output: &Path,
    ) -> Result<()> {
        Mutool::render_page(self, pdf, page, dpi, rotation, output)
    }
}

impl OcrEngine for Tesseract {
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

/// Extract a page with the engine's configured OCR profile, reusing a matching cache entry.
pub fn ocr_page(
    mutool: &impl PageRenderer,
    tesseract: &impl OcrEngine,
    cache: &OcrCache,
    pdf: &Path,
    pdf_sha256: &str,
    page: usize,
) -> Result<ExtractedPage> {
    if page == 0 {
        bail!("PDF pages are one-based");
    }
    let lang = tesseract.lang();
    let dpi = tesseract.dpi();
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
    let mut words = Vec::new();
    let mut confidences = Vec::new();
    let mut spans = Vec::new();

    for line in lines {
        let fields: Vec<_> = line.split('\t').collect();
        let Some(text) = fields.get(text_index).map(|value| value.trim()) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        words.push(text);
        let bbox = geometry_indexes
            .iter()
            .copied()
            .collect::<Option<Vec<_>>>()
            .and_then(|indexes| {
                let values = indexes
                    .into_iter()
                    .map(|index| fields.get(index)?.parse::<f64>().ok())
                    .collect::<Option<Vec<_>>>()?;
                Some(PdfBbox {
                    x: values[0],
                    y: values[1],
                    width: values[2],
                    height: values[3],
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
        text: normalize(&words.join(" ")),
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

fn extracted_page(page: usize, tsv: &str) -> Result<ExtractedPage> {
    Ok(parse_tsv(tsv)?.into_extracted_page(page))
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

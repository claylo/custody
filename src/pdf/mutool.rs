use std::{
    path::Path,
    process::{Command, Output},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use super::{Executable, NumericVersion, parse_numeric_version};
use super::{ExtractedPage, PdfBbox, TextSpan};
use crate::{coordinate::Page, normalize::normalize};

pub const PROFILE_NAME: &str = "mutool-native";
pub const SUPPORTED_VERSION_RANGE: &str = ">=1.28.0, <1.29.0";
const MIN_SUPPORTED_VERSION: NumericVersion = NumericVersion::new(1, 28, 0);
const MAX_SUPPORTED_VERSION: NumericVersion = NumericVersion::new(1, 29, 0);

pub fn validate_version(output: &str) -> Result<()> {
    let version = parse_numeric_version(output)?;
    if !(MIN_SUPPORTED_VERSION..MAX_SUPPORTED_VERSION).contains(&version) {
        bail!("MuPDF version {version} is unsupported; supported {SUPPORTED_VERSION_RANGE}");
    }
    Ok(())
}

/// `MuPDF` command adapter.
#[derive(Debug, Clone)]
pub struct Mutool {
    executable: Executable,
}

impl Default for Mutool {
    fn default() -> Self {
        Self {
            executable: Executable::resolve("mutool", None),
        }
    }
}

impl Mutool {
    #[must_use]
    pub fn with_executable(executable: Option<&Path>) -> Self {
        Self {
            executable: Executable::resolve("mutool", executable),
        }
    }

    pub fn executable(&self) -> Result<&Path> {
        self.executable.path()
    }

    /// Extract native structured text, optionally from a single physical page.
    pub fn native_pages(&self, pdf: &Path, page: Option<Page>) -> Result<Vec<ExtractedPage>> {
        let mut command = Command::new(self.executable()?);
        command.args(["draw", "-q", "-F", "stext.json", "-o", "-"]);
        command.arg(pdf);
        if let Some(page) = page {
            command.arg(page.to_string());
        }
        let output = run(&mut command, "MuPDF structured-text extraction")?;
        let mut pages = parse_stext_json(
            std::str::from_utf8(&output.stdout).context("MuPDF output was not UTF-8")?,
        )?;
        if let Some(page) = page {
            if pages.len() != 1 {
                bail!(
                    "MuPDF returned {} pages when physical page {page} was requested",
                    pages.len()
                );
            }
            pages[0].page = page;
        }
        Ok(pages)
    }

    /// Render one page to a PNG suitable for deterministic OCR.
    pub fn render_page(
        &self,
        pdf: &Path,
        page: Page,
        dpi: u16,
        rotation: i16,
        output: &Path,
    ) -> Result<()> {
        let mut command = Command::new(self.executable()?);
        command.args(["draw", "-q", "-r", &dpi.to_string()]);
        if rotation != 0 {
            command.args(["-R", &rotation.to_string()]);
        }
        command.args(["-o", output.as_os_str().to_string_lossy().as_ref()]);
        command.arg(pdf).arg(page.to_string());
        run(&mut command, "MuPDF page rendering")?;
        Ok(())
    }

    /// Return the installed `MuPDF` version text.
    pub fn version(&self) -> Result<String> {
        let output = run(
            Command::new(self.executable()?).arg("-v"),
            "MuPDF version probe",
        )?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(format!("{stdout} {stderr}").trim().to_owned())
    }
}

/// Parse `MuPDF`'s `stext.json` output without repairing token fragmentation.
pub fn parse_stext_json(source: &str) -> Result<Vec<ExtractedPage>> {
    let document: StextDocument = serde_json::from_str(source)
        .map_err(|error| anyhow!("failed to parse MuPDF structured-text JSON: {error}"))?;
    if document.pages.is_empty() {
        bail!("MuPDF structured-text JSON contains no pages");
    }

    document
        .pages
        .into_iter()
        .enumerate()
        .map(|(index, page)| -> Result<ExtractedPage> {
            let mut lines = Vec::new();
            for block in page.blocks {
                if block.kind == "text" {
                    lines.extend(block.lines.context("MuPDF text block is missing lines")?);
                }
            }
            let text = lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            Ok(ExtractedPage {
                page: Page::new(index + 1).expect("enumerated pages are one-based"),
                text: normalize(&text),
                spans: lines
                    .into_iter()
                    .map(|line| TextSpan {
                        text: line.text,
                        bbox: line.bbox.map(Into::into),
                    })
                    .collect(),
                mean_confidence: None,
            })
        })
        .collect()
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

#[derive(Debug, Deserialize)]
struct StextDocument {
    pages: Vec<StextPage>,
}

#[derive(Debug, Deserialize)]
struct StextPage {
    blocks: Vec<StextBlock>,
}

#[derive(Debug, Deserialize)]
struct StextBlock {
    #[serde(rename = "type")]
    kind: String,
    lines: Option<Vec<StextLine>>,
}

#[derive(Debug, Deserialize)]
struct StextLine {
    #[serde(default)]
    bbox: Option<StextBbox>,
    text: String,
}

#[derive(Debug, Deserialize)]
struct StextBbox {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl From<StextBbox> for PdfBbox {
    fn from(value: StextBbox) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.w,
            height: value.h,
        }
    }
}

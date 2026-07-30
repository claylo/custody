use std::{
    path::Path,
    process::{Command, Output},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::{ExtractedPage, PdfBbox, TextSpan};
use crate::normalize::normalize;

pub const PROFILE_NAME: &str = "mutool-native";

/// `MuPDF` command adapter.
#[derive(Debug, Clone)]
pub struct Mutool {
    executable: String,
}

impl Default for Mutool {
    fn default() -> Self {
        Self {
            executable: "mutool".to_owned(),
        }
    }
}

impl Mutool {
    /// Extract native structured text, optionally from a single physical page.
    pub fn native_pages(&self, pdf: &Path, page: Option<usize>) -> Result<Vec<ExtractedPage>> {
        if page == Some(0) {
            bail!("PDF pages are one-based");
        }

        let mut command = Command::new(&self.executable);
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
        page: usize,
        dpi: u16,
        rotation: i16,
        output: &Path,
    ) -> Result<()> {
        if page == 0 {
            bail!("PDF pages are one-based");
        }
        let mut command = Command::new(&self.executable);
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
            Command::new(&self.executable).arg("-v"),
            "MuPDF version probe",
        )?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(format!("{stdout} {stderr}").trim().to_owned())
    }
}

/// Parse `MuPDF`'s `stext.json` output without repairing token fragmentation.
pub fn parse_stext_json(source: &str) -> Result<Vec<ExtractedPage>> {
    let document: StextDocument =
        serde_json::from_str(source).context("failed to parse MuPDF structured-text JSON")?;
    if document.pages.is_empty() {
        bail!("MuPDF structured-text JSON contains no pages");
    }

    Ok(document
        .pages
        .into_iter()
        .enumerate()
        .map(|(index, page)| {
            let lines = page
                .blocks
                .into_iter()
                .flat_map(|block| block.lines)
                .collect::<Vec<_>>();
            let text = lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            ExtractedPage {
                page: index + 1,
                text: normalize(&text),
                spans: lines
                    .into_iter()
                    .map(|line| TextSpan {
                        text: line.text,
                        bbox: line.bbox.map(Into::into),
                    })
                    .collect(),
                mean_confidence: None,
            }
        })
        .collect())
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
    #[serde(default)]
    pages: Vec<StextPage>,
}

#[derive(Debug, Deserialize)]
struct StextPage {
    #[serde(default)]
    blocks: Vec<StextBlock>,
}

#[derive(Debug, Deserialize)]
struct StextBlock {
    #[serde(default)]
    lines: Vec<StextLine>,
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

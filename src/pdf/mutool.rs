use std::{path::Path, process::Command};

use anyhow::{Context, Result, anyhow, bail};
use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};

use super::{Executable, NumericVersion, parse_numeric_version, run};
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
        let source = self.stext(pdf, page)?;
        let mut pages = parse_stext(&source)?;
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

    /// Extract native text as per-page paragraphs, for `custody extract`.
    pub fn native_blocks(&self, pdf: &Path) -> Result<Vec<PageBlocks>> {
        let source = self.stext(pdf, None)?;
        parse_stext_blocks(&source)
    }

    /// Run `mutool draw -F stext`, the XML structured-text writer.
    ///
    /// The XML writer is used rather than `stext.json` because the JSON writer
    /// emits one "line" object per font run, so a typeset line whose glyphs
    /// come from two publisher font subsets (`attachme` + `nt`, `Bowlby` +
    /// `’` + `s`) arrives as several objects and any join re-creates the seam
    /// as whitespace. The XML `<line text="…">` attribute is the whole line.
    fn stext(&self, pdf: &Path, page: Option<Page>) -> Result<String> {
        let mut command = Command::new(self.executable()?);
        command.args(["draw", "-q", "-F", "stext", "-o", "-"]);
        command.arg(pdf);
        if let Some(page) = page {
            command.arg(page.to_string());
        }
        let output = run(&mut command, "MuPDF structured-text extraction")?;
        String::from_utf8(output.stdout).context("MuPDF output was not UTF-8")
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

/// Parse `MuPDF`'s `stext` XML output without repairing token fragmentation.
///
/// Lines are joined with `\n` before normalization so that a word hyphenated
/// across a line break can be rejoined by the dehyphenation rule.
pub fn parse_stext(source: &str) -> Result<Vec<ExtractedPage>> {
    parse_document(source)?
        .into_iter()
        .enumerate()
        .map(|(index, page)| -> Result<ExtractedPage> {
            let lines: Vec<StextLine> = page.into_iter().flatten().collect();
            let text = lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            Ok(ExtractedPage {
                page: Page::new(index + 1).expect("enumerated pages are one-based"),
                text: normalize(&text),
                spans: lines
                    .into_iter()
                    .map(|line| TextSpan {
                        text: line.text,
                        bbox: line.bbox,
                    })
                    .collect(),
                mean_confidence: None,
            })
        })
        .collect()
}

/// One page of native text as a list of paragraphs, each already normalized.
///
/// A paragraph is one `MuPDF` text block with its lines joined through the
/// normalizer, so dehyphenation and quote folding have been applied. Empty
/// blocks are dropped; a page with no text yields an empty list.
pub type PageBlocks = Vec<String>;

/// Parse `MuPDF`'s `stext` XML output into per-page, per-block text.
pub fn parse_stext_blocks(source: &str) -> Result<Vec<PageBlocks>> {
    Ok(parse_document(source)?
        .into_iter()
        .map(|page| {
            page.into_iter()
                .map(|block| {
                    let joined = block
                        .iter()
                        .map(|line| line.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    normalize(&joined)
                })
                .filter(|text| !text.is_empty())
                .collect()
        })
        .collect())
}

/// One `<line>` element: its decoded `text` attribute and optional geometry.
struct StextLine {
    text: String,
    bbox: Option<PdfBbox>,
}

type StextBlock = Vec<StextLine>;
type StextPage = Vec<StextBlock>;

/// Stream the XML and keep only `<page>`, `<block>`, and `<line>` structure.
///
/// The XML carries a `<char>` element for every glyph, so a long book is
/// hundreds of megabytes of markup; the reader never materializes a tree.
/// Elements other than those three (`<font>`, `<char>`, `<image>`) are
/// skipped, and a `<line>` outside a `<block>` is a `MuPDF` format error.
fn parse_document(source: &str) -> Result<Vec<StextPage>> {
    let mut reader = Reader::from_str(source);
    let mut pages: Vec<StextPage> = Vec::new();
    let mut in_document = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| anyhow!("failed to parse MuPDF structured-text XML: {error}"))?;
        match event {
            Event::Start(element) | Event::Empty(element) => match element.name().as_ref() {
                "document" => in_document = true,
                "page" => {
                    if !in_document {
                        bail!("MuPDF structured-text XML has a page outside <document>");
                    }
                    pages.push(Vec::new());
                }
                "block" => pages
                    .last_mut()
                    .context("MuPDF structured-text XML has a block outside <page>")?
                    .push(Vec::new()),
                "line" => pages
                    .last_mut()
                    .and_then(|page| page.last_mut())
                    .context("MuPDF structured-text XML has a line outside <block>")?
                    .push(parse_line(&element)?),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    if !in_document {
        bail!("MuPDF structured-text XML has no <document> root");
    }
    if pages.is_empty() {
        bail!("MuPDF structured-text XML contains no pages");
    }
    Ok(pages)
}

fn parse_line(element: &BytesStart<'_>) -> Result<StextLine> {
    let mut text = None;
    let mut bbox = None;
    for attribute in element.attributes() {
        let attribute = attribute
            .map_err(|error| anyhow!("malformed MuPDF structured-text attribute: {error}"))?;
        match attribute.key.as_ref() {
            "text" => {
                text = Some(
                    attribute
                        .normalized_value(XmlVersion::Explicit1_0)
                        .context("MuPDF line text is not valid XML")?
                        .into_owned(),
                );
            }
            "bbox" => {
                let value = attribute
                    .normalized_value(XmlVersion::Explicit1_0)
                    .context("MuPDF line bbox is not valid XML")?;
                bbox = Some(parse_bbox(&value)?);
            }
            _ => {}
        }
    }
    Ok(StextLine {
        text: text.context("MuPDF text line is missing its text attribute")?,
        bbox,
    })
}

/// Parse the `x0 y0 x1 y1` attribute form into an origin-plus-size box.
fn parse_bbox(value: &str) -> Result<PdfBbox> {
    let corners: Vec<f64> = value
        .split_ascii_whitespace()
        .map(|number| {
            number
                .parse::<f64>()
                .with_context(|| format!("MuPDF bbox coordinate {number:?} is not a number"))
        })
        .collect::<Result<_>>()?;
    let [x0, y0, x1, y1] = corners[..] else {
        bail!("MuPDF bbox {value:?} does not have four coordinates");
    };
    Ok(PdfBbox {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

//! PDF text extraction backends.

use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::coordinate::Page;

pub mod cache;
pub mod mutool;
pub mod tesseract;

pub(crate) fn run(command: &mut Command, label: &str) -> Result<Output> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NumericVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl NumericVersion {
    pub(crate) const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for NumericVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

pub(crate) fn parse_numeric_version(output: &str) -> Result<NumericVersion> {
    let token = output
        .split_whitespace()
        .find(|token| token.as_bytes().first().is_some_and(u8::is_ascii_digit))
        .context("version probe output contains no numeric version")?;
    let mut components = token.split('.');
    let major = parse_version_component(components.next(), output)?;
    let minor = parse_version_component(components.next(), output)?;
    let patch = parse_version_component(components.next(), output)?;
    if components.next().is_some() {
        bail!("version probe output contains an unsupported version shape: {output:?}");
    }
    Ok(NumericVersion::new(major, minor, patch))
}

fn parse_version_component(component: Option<&str>, output: &str) -> Result<u32> {
    let component = component
        .context("version probe output must contain major, minor, and patch components")?;
    let digits = component
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    if digits.is_empty() {
        bail!("version probe output contains an invalid component: {output:?}");
    }
    digits
        .parse()
        .with_context(|| format!("version component is out of range: {digits}"))
}

#[derive(Debug, Clone)]
pub(crate) struct Executable {
    resolved: std::result::Result<PathBuf, String>,
}

impl Executable {
    pub(crate) fn resolve(name: &str, configured: Option<&Path>) -> Self {
        let resolved = resolve_executable(name, configured).map_err(|error| format!("{error:#}"));
        Self { resolved }
    }

    pub(crate) fn path(&self) -> Result<&Path> {
        self.resolved
            .as_deref()
            .map_err(|error| anyhow!(error.clone()))
    }
}

fn resolve_executable(name: &str, configured: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = configured {
        if !path.is_absolute() {
            bail!("pdf.tools.{name} must be an absolute path");
        }
        return canonical_executable(path)
            .with_context(|| format!("failed to resolve configured {name} executable"));
    }

    let search_path = env::var_os("PATH").context("PATH is not set")?;
    let file_name = format!("{name}{}", env::consts::EXE_SUFFIX);
    for directory in env::split_paths(&search_path) {
        let candidate = directory.join(&file_name);
        if let Ok(path) = canonical_executable(&candidate) {
            return Ok(path);
        }
    }
    bail!("{name} was not found as an executable file on PATH")
}

fn canonical_executable(path: &Path) -> Result<PathBuf> {
    let path = fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize {}", path.display()))?;
    let metadata =
        fs::metadata(&path).with_context(|| format!("failed to inspect {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            bail!("{} is not executable", path.display());
        }
    }
    Ok(path)
}

/// Normalized text extracted from one physical PDF page.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedPage {
    pub page: Page,
    pub text: String,
    pub spans: Vec<TextSpan>,
    pub mean_confidence: Option<f64>,
}

/// One extracted text fragment and its page-space bounding box.
#[derive(Debug, Clone, PartialEq)]
pub struct TextSpan {
    pub text: String,
    pub bbox: Option<PdfBbox>,
}

/// Bounding rectangle in backend page coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfBbox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Return the union of fragment boxes touched by one unique exact match.
///
/// Spans are joined with line breaks and normalized cumulatively, so the
/// searchable text is exactly the page text the match was found in, including
/// any dehyphenation across a span boundary. Each span's byte range is the
/// growth of the normalized prefix when that span is appended; when the
/// previous span's trailing hyphen was dropped, the range is widened by one
/// byte to cover it.
#[must_use]
pub fn matching_bbox(page: &ExtractedPage, exact: &str) -> Option<PdfBbox> {
    let mut raw = String::new();
    let mut searchable = String::new();
    let mut ranges = Vec::new();
    for span in &page.spans {
        if crate::normalize::normalize(&span.text).is_empty() {
            continue;
        }
        if !raw.is_empty() {
            raw.push('\n');
        }
        raw.push_str(&span.text);
        let grown = crate::normalize::normalize(&raw);
        let mut start = searchable.len();
        if !grown.starts_with(&searchable) {
            // Dehyphenation removed the previous span's trailing hyphen.
            start = start.saturating_sub(1);
        }
        searchable = grown;
        ranges.push((start..searchable.len(), span.bbox));
    }
    let mut matches = searchable.match_indices(exact);
    let (start, _) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let match_range = start..start + exact.len();
    ranges
        .into_iter()
        .filter(|(range, _)| range.start < match_range.end && match_range.start < range.end)
        .filter_map(|(_, bbox)| bbox)
        .reduce(PdfBbox::union)
}

impl PdfBbox {
    fn union(self, other: Self) -> Self {
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }
}

/// Backend boundary used by evidence validation and test doubles.
pub trait PdfTextProvider {
    fn native_pages(&self, pdf: &Path, page: Option<Page>) -> Result<Vec<ExtractedPage>>;

    fn ocr_page(&self, pdf: &Path, pdf_sha256: &str, page: Page) -> Result<ExtractedPage>;
}

/// Concrete `MuPDF` + Tesseract provider with a corpus-local OCR cache.
#[derive(Debug)]
pub struct PdfTools {
    pub mutool: mutool::Mutool,
    pub tesseract: tesseract::Tesseract,
    pub cache: cache::OcrCache,
    ocr_enabled: bool,
    ocr_profile: std::cell::OnceCell<cache::OcrProfile>,
}

impl PdfTools {
    pub fn new(cache_root: std::path::PathBuf, ocr: &crate::config::OcrConfig) -> Result<Self> {
        Self::with_cache_read_policy(cache_root, ocr, cache::CacheReadPolicy::Trusted)
    }

    pub fn with_cache_read_policy(
        cache_root: std::path::PathBuf,
        ocr: &crate::config::OcrConfig,
        read_policy: cache::CacheReadPolicy,
    ) -> Result<Self> {
        Self::build(
            cache_root,
            ocr,
            &crate::config::PdfToolConfig::default(),
            read_policy,
        )
    }

    pub fn from_config(
        cache_root: std::path::PathBuf,
        pdf: &crate::config::PdfConfig,
        read_policy: cache::CacheReadPolicy,
    ) -> Result<Self> {
        Self::build(cache_root, &pdf.ocr, &pdf.tools, read_policy)
    }

    fn build(
        cache_root: std::path::PathBuf,
        ocr: &crate::config::OcrConfig,
        tools: &crate::config::PdfToolConfig,
        read_policy: cache::CacheReadPolicy,
    ) -> Result<Self> {
        let dpi = crate::config::validated_ocr_dpi(ocr.dpi)?;
        Ok(Self {
            mutool: mutool::Mutool::with_executable(tools.mutool.as_deref()),
            tesseract: tesseract::Tesseract::with_executable(
                tools.tesseract.as_deref(),
                ocr.lang.clone(),
                dpi,
                ocr.page_segmentation_mode,
            ),
            cache: cache::OcrCache::with_read_policy(cache_root, read_policy),
            ocr_enabled: ocr.enabled,
            ocr_profile: std::cell::OnceCell::new(),
        })
    }

    fn ocr_profile(&self) -> Result<&cache::OcrProfile> {
        if let Some(profile) = self.ocr_profile.get() {
            return Ok(profile);
        }
        let profile = tesseract::resolve_profile(&self.mutool, &self.tesseract)?;
        Ok(self.ocr_profile.get_or_init(|| profile))
    }

    pub fn validate_toolchain(&self) -> Result<()> {
        let profile = self.ocr_profile()?;
        mutool::validate_version(&profile.mutool_version)?;
        tesseract::validate_version(&profile.tesseract_version)?;
        Ok(())
    }
}

impl PdfTextProvider for PdfTools {
    fn native_pages(&self, pdf: &Path, page: Option<Page>) -> Result<Vec<ExtractedPage>> {
        self.mutool.native_pages(pdf, page)
    }

    fn ocr_page(&self, pdf: &Path, pdf_sha256: &str, page: Page) -> Result<ExtractedPage> {
        if !self.ocr_enabled {
            anyhow::bail!("OCR is disabled in configuration");
        }
        let profile = self.ocr_profile()?;
        tesseract::ocr_page_with_profile(
            &self.mutool,
            &self.tesseract,
            &self.cache,
            profile,
            pdf,
            pdf_sha256,
            page,
        )
    }
}

//! PDF text extraction backends.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;

pub mod cache;
pub mod mutool;
pub mod tesseract;

/// Normalized text extracted from one physical PDF page.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedPage {
    pub page: usize,
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PdfBbox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Return the union of fragment boxes touched by one unique exact match.
#[must_use]
pub fn matching_bbox(page: &ExtractedPage, exact: &str) -> Option<PdfBbox> {
    let mut searchable = String::new();
    let mut ranges = Vec::new();
    for span in &page.spans {
        let text = crate::normalize::normalize(&span.text);
        if text.is_empty() {
            continue;
        }
        if !searchable.is_empty() {
            searchable.push(' ');
        }
        let start = searchable.len();
        searchable.push_str(&text);
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
    fn native_pages(&self, pdf: &Path, page: Option<usize>) -> Result<Vec<ExtractedPage>>;

    fn ocr_page(&self, pdf: &Path, pdf_sha256: &str, page: usize) -> Result<ExtractedPage>;
}

/// Concrete `MuPDF` + Tesseract provider with a corpus-local OCR cache.
#[derive(Debug, Clone)]
pub struct PdfTools {
    pub mutool: mutool::Mutool,
    pub tesseract: tesseract::Tesseract,
    pub cache: cache::OcrCache,
}

impl PdfTools {
    #[must_use]
    pub fn new(cache_root: std::path::PathBuf) -> Self {
        Self {
            mutool: mutool::Mutool::default(),
            tesseract: tesseract::Tesseract::default(),
            cache: cache::OcrCache::new(cache_root),
        }
    }
}

impl PdfTextProvider for PdfTools {
    fn native_pages(&self, pdf: &Path, page: Option<usize>) -> Result<Vec<ExtractedPage>> {
        self.mutool.native_pages(pdf, page)
    }

    fn ocr_page(&self, pdf: &Path, pdf_sha256: &str, page: usize) -> Result<ExtractedPage> {
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

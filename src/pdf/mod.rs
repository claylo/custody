//! PDF text extraction backends.

use std::path::Path;

use anyhow::Result;

pub mod mutool;

/// Normalized text extracted from one physical PDF page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedPage {
    pub page: usize,
    pub text: String,
}

/// Backend boundary used by evidence validation and test doubles.
pub trait PdfTextProvider {
    fn native_pages(&self, pdf: &Path, page: Option<usize>) -> Result<Vec<ExtractedPage>>;

    fn ocr_page(&self, pdf: &Path, pdf_sha256: &str, page: usize) -> Result<ExtractedPage>;
}

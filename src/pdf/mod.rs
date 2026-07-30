//! PDF text extraction backends.

use std::path::Path;

use anyhow::Result;

pub mod cache;
pub mod mutool;
pub mod tesseract;

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

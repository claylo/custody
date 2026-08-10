//! Ranked candidate locators for claims that carry no evidence yet.
//!
//! Nothing here consults a model and nothing here writes. Candidates are spans
//! lifted straight out of the corpus, verified against both the Markdown unit
//! that holds them and the native PDF text, then ordered by a total comparison
//! so the same corpus always yields the same suggestions.

use serde::Serialize;

use crate::evidence::PdfBackend;
use crate::markdown::UnitKind;
use crate::normalize::normalize;

/// Every claim considered for one summary, with its candidates.
#[derive(Debug, Clone, Serialize)]
pub struct ProposalReport {
    pub id: String,
    pub claims: Vec<ClaimProposal>,
}

/// Candidates offered for one claim, and the tokens none of them reach.
#[derive(Debug, Clone, Serialize)]
pub struct ClaimProposal {
    pub claim: usize,
    pub required_tokens: Vec<String>,
    pub uncovered_tokens: Vec<String>,
    pub candidates: Vec<Candidate>,
}

/// One verified span, ready to be written as a locator.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub source: String,
    pub exact: String,
    pub coverage: CoverageScore,
    pub markdown: MarkdownMatch,
    pub pdf: Option<PdfMatch>,
}

/// How many of a claim's required tokens one candidate reaches.
#[derive(Debug, Clone, Serialize)]
pub struct CoverageScore {
    pub matched: usize,
    pub required: usize,
}

/// Where a candidate sits in the converted Markdown.
#[derive(Debug, Clone, Serialize)]
pub struct MarkdownMatch {
    pub line: usize,
    pub column: usize,
    pub unit: UnitKind,
    pub section: Vec<String>,
}

/// The single PDF page a candidate was found on.
#[derive(Debug, Clone, Serialize)]
pub struct PdfMatch {
    pub page: usize,
    pub backend: PdfBackend,
}

/// Split text on terminal punctuation followed by whitespace.
///
/// Deliberately naive: `et al.` and `Fig. 3` split where a reader would not.
/// That is acceptable because every candidate is verified against both sources
/// before it is offered, so a bad boundary costs a candidate, never a false one.
#[must_use]
pub fn split_sentences(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut sentences = Vec::new();
    let mut start = 0;
    let mut index = 0;

    while index < bytes.len() {
        let boundary = matches!(bytes[index], b'.' | b'!' | b'?')
            && bytes.get(index + 1).is_some_and(u8::is_ascii_whitespace);
        index += 1;
        if !boundary {
            continue;
        }
        push_normalized(&text[start..index], &mut sentences);
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        start = index;
    }

    push_normalized(&text[start..], &mut sentences);
    sentences
}

fn push_normalized(segment: &str, sentences: &mut Vec<String>) {
    let normalized = normalize(segment);
    if !normalized.is_empty() {
        sentences.push(normalized);
    }
}

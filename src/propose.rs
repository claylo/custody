//! Ranked candidate locators for claims that carry no evidence yet.
//!
//! Nothing here consults a model and nothing here writes. Candidates are spans
//! lifted straight out of the corpus, verified against both the Markdown unit
//! that holds them and the native PDF text, then ordered by a total comparison
//! so the same corpus always yields the same suggestions.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::Result;
use serde::Serialize;

use crate::coordinate::{ClaimIndex, Column, Line, Page};
use crate::corpus::Corpus;
use crate::evidence::{MarkdownLocator, PdfBackend, PdfLocator, SummaryDocument};
use crate::markdown::{MarkdownUnit, UnitKind, exact_count, parse_units};
use crate::normalize::normalize;
use crate::pdf::PdfTextProvider;
use crate::tokens::{self, ExtractedTokens};

/// Every claim considered for one summary, with its candidates.
#[derive(Debug, Clone, Serialize)]
pub struct ProposalReport {
    pub id: String,
    pub claims: Vec<ClaimProposal>,
}

/// Candidates offered for one claim, and the tokens none of them reach.
#[derive(Debug, Clone, Serialize)]
pub struct ClaimProposal {
    pub claim: ClaimIndex,
    pub required_tokens: Vec<String>,
    pub uncovered_tokens: Vec<String>,
    pub advisory_tokens: Vec<String>,
    pub candidates: Vec<Candidate>,
}

/// One verified span, ready to be written as a locator.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub source: String,
    pub exact: String,
    pub coverage: CoverageScore,
    pub markdown: MarkdownLocator,
    pub pdf: Option<PdfLocator>,
}

/// How many of a claim's required tokens one candidate reaches.
#[derive(Debug, Clone, Serialize)]
pub struct CoverageScore {
    pub matched: usize,
    pub required: usize,
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

/// Offer ranked candidate locators for the claims of one summary.
///
/// Claims that already carry locators are skipped unless `all_claims` is set,
/// and each claim keeps at most `max_candidates` candidates, best first. Every
/// source is read once; nothing is written.
pub fn propose_document(
    corpus: &Corpus,
    summary: &SummaryDocument,
    provider: &impl PdfTextProvider,
    max_candidates: usize,
    all_claims: bool,
) -> Result<ProposalReport> {
    let settled: BTreeSet<ClaimIndex> = summary
        .evidence
        .as_ref()
        .map(|evidence| {
            evidence
                .claims
                .iter()
                .filter(|entry| !entry.locators.is_empty())
                .map(|entry| entry.claim)
                .collect()
        })
        .unwrap_or_default();

    let mut source_units: BTreeMap<String, Vec<MarkdownUnit>> = BTreeMap::new();
    let mut source_pages: BTreeMap<String, HashMap<Page, String>> = BTreeMap::new();

    for source_name in corpus.source_names() {
        let markdown_candidates = corpus.markdown_candidates_for(&summary.id, &source_name)?;
        if let Some(path) = markdown_candidates.iter().find(|path| path.is_file())
            && let Ok(markdown) = corpus.read_contained_text(path)
        {
            source_units.insert(source_name.clone(), parse_units(&markdown));
        }

        let pdf_path = corpus.pdf_path_for(&summary.id, &source_name)?;
        if pdf_path.is_file()
            && let Ok(pages) = provider.native_pages(&pdf_path, None)
        {
            source_pages.insert(
                source_name,
                pages
                    .into_iter()
                    .map(|page| (page.page, normalize(&page.text)))
                    .collect(),
            );
        }
    }

    let mut claims = Vec::new();
    for (index, claim_text) in summary.claims.iter().enumerate() {
        let index = ClaimIndex::new(index);
        if !all_claims && settled.contains(&index) {
            continue;
        }

        let claim_tokens = tokens::extract(claim_text);
        let mut raw_candidates = Vec::new();
        for (source_name, units) in &source_units {
            raw_candidates.extend(generate_candidates(source_name, units, &claim_tokens));
        }
        rank_raw_candidates(&mut raw_candidates);

        let mut candidates = Vec::new();
        for raw in raw_candidates {
            if candidates.len() >= max_candidates {
                break;
            }
            let pages = source_pages.get(&raw.source);
            let Some(page) = pages.and_then(|pages| verify_pdf(pages, &raw.exact)) else {
                continue;
            };
            candidates.push(Candidate {
                source: raw.source,
                exact: raw.exact,
                coverage: CoverageScore {
                    matched: raw.matched,
                    required: claim_tokens.required.len(),
                },
                markdown: MarkdownLocator {
                    line: raw.line,
                    column: raw.column,
                    unit: raw.unit,
                    section: raw.section,
                },
                pdf: Some(PdfLocator {
                    page,
                    backend: PdfBackend::MutoolNative,
                }),
            });
        }

        let uncovered_tokens = claim_tokens
            .required
            .iter()
            .filter(|token| {
                !candidates
                    .iter()
                    .any(|candidate| covers(&candidate.exact, token))
            })
            .cloned()
            .collect();

        claims.push(ClaimProposal {
            claim: index,
            required_tokens: claim_tokens.required,
            uncovered_tokens,
            advisory_tokens: claim_tokens.advisory,
            candidates,
        });
    }

    Ok(ProposalReport {
        id: summary.id.clone(),
        claims,
    })
}

/// A scored span before it has been checked against the PDF.
struct RawCandidate {
    source: String,
    exact: String,
    matched: usize,
    line: Line,
    column: Column,
    unit: UnitKind,
    section: Vec<String>,
}

fn generate_candidates(
    source: &str,
    units: &[MarkdownUnit],
    claim_tokens: &ExtractedTokens,
) -> Vec<RawCandidate> {
    let mut candidates = Vec::new();

    for unit in units {
        for span in candidate_spans(&unit.text) {
            // A span that repeats inside its own unit can never be pinned to
            // one place in it, which is exactly what validation demands.
            if exact_count(&unit.text, &span) != 1 {
                continue;
            }
            let matched = claim_tokens
                .required
                .iter()
                .filter(|token| covers(&span, token))
                .count();
            if matched == 0 {
                continue;
            }
            candidates.push(RawCandidate {
                source: source.to_owned(),
                exact: span,
                matched,
                line: unit.line,
                column: unit.column,
                unit: unit.kind,
                section: unit.section.clone(),
            });
        }
    }

    candidates
}

/// Every sentence, plus the whole unit when it holds more than one.
fn candidate_spans(text: &str) -> Vec<String> {
    let mut spans = split_sentences(text);
    if spans.len() > 1 {
        spans.push(text.to_owned());
    }
    spans
}

/// Number words drift in case between a summary and its source.
fn covers(text: &str, token: &str) -> bool {
    if tokens::is_number_word(token) {
        tokens::is_covered_case_insensitive(text, token)
    } else {
        tokens::is_covered(text, token)
    }
}

/// The one page holding a span, or `None` when zero or several hold it.
fn verify_pdf(pages: &HashMap<Page, String>, exact: &str) -> Option<Page> {
    let mut matched = pages
        .iter()
        .filter(|(_, text)| exact_count(text, exact) > 0)
        .map(|(page, _)| *page);
    let page = matched.next()?;
    matched.next().is_none().then_some(page)
}

/// Order raw candidates before PDF verification so only top-ranked spans are checked.
fn rank_raw_candidates(candidates: &mut [RawCandidate]) {
    candidates.sort_by(|left, right| {
        right
            .matched
            .cmp(&left.matched)
            .then_with(|| left.exact.len().cmp(&right.exact.len()))
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.column.cmp(&right.column))
    });
}

//! Ranked candidate locators for claims that carry no evidence yet.
//!
//! Nothing here consults a model and nothing here writes. Candidates are spans
//! lifted straight out of the corpus, verified against both the Markdown unit
//! that holds them and the native PDF text, then ordered by a total comparison
//! so the same corpus always yields the same suggestions.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::coordinate::{ClaimIndex, Column, Line, Page};
use crate::corpus::Corpus;
use crate::evidence::{MarkdownLocator, PdfBackend, PdfLocator, SummaryDocument};
use crate::markdown::{MarkdownUnit, UnitKind, exact_count, parse_units};
use crate::normalize::normalize;
use crate::pdf::PdfTextProvider;
use crate::tokens::{self, ExtractedTokens};

/// Every claim considered for one summary, with its candidates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalReport {
    pub id: String,
    pub claims: Vec<ClaimProposal>,
}

/// Candidates offered for one claim, and the tokens none of them reach.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimProposal {
    pub claim: ClaimIndex,
    pub required_tokens: Vec<String>,
    pub uncovered_tokens: Vec<String>,
    pub advisory_tokens: Vec<String>,
    pub candidates: Vec<Candidate>,
}

/// One verified span, ready to be written as a locator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub source: String,
    pub exact: String,
    pub coverage: CoverageScore,
    pub markdown: MarkdownLocator,
    pub pdf: PdfLocator,
}

/// How many of a claim's required tokens one candidate reaches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoverageScore {
    pub matched: usize,
    pub required: usize,
}

/// Maximum unique PDF scans spent for each requested accepted candidate.
const PDF_VERIFICATION_ATTEMPTS_PER_CANDIDATE: usize = 8;

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

    let mut prepared_spans = Vec::new();
    let mut source_pages: BTreeMap<&str, HashMap<Page, String>> = BTreeMap::new();

    for source_name in corpus.source_names() {
        let markdown_candidates = corpus.markdown_candidates_for(&summary.id, source_name)?;
        if let Some(path) = markdown_candidates.iter().find(|path| path.is_file())
            && let Ok(markdown) = corpus.read_contained_text(path)
        {
            prepared_spans.extend(prepare_spans(source_name, &parse_units(&markdown)));
        }

        let pdf_path = corpus.pdf_path_for(&summary.id, source_name)?;
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
    let mut pdf_verification: HashMap<String, HashMap<String, Option<Page>>> = HashMap::new();
    for (index, claim_text) in summary.claims.iter().enumerate() {
        let index = ClaimIndex::new(index);
        if !all_claims && settled.contains(&index) {
            continue;
        }

        let claim_tokens = tokens::extract(claim_text);
        let mut scored_candidates = score_spans(&prepared_spans, &claim_tokens);
        rank_scored_candidates(&mut scored_candidates);

        let mut candidates = Vec::new();
        let mut verification_attempts = 0_usize;
        let verification_budget =
            max_candidates.saturating_mul(PDF_VERIFICATION_ATTEMPTS_PER_CANDIDATE);
        for scored in scored_candidates {
            if candidates.len() >= max_candidates {
                break;
            }
            let span = scored.span;
            let cached = pdf_verification
                .get(&span.source)
                .and_then(|source| source.get(&span.exact))
                .copied();
            let page = if let Some(cached) = cached {
                cached
            } else {
                if verification_attempts >= verification_budget {
                    break;
                }
                verification_attempts += 1;
                let verified = source_pages
                    .get(span.source.as_str())
                    .and_then(|pages| verify_pdf(pages, &span.exact));
                pdf_verification
                    .entry(span.source.clone())
                    .or_default()
                    .insert(span.exact.clone(), verified);
                verified
            };
            let Some(page) = page else {
                continue;
            };
            candidates.push(Candidate {
                source: span.source.clone(),
                exact: span.exact.clone(),
                coverage: CoverageScore {
                    matched: scored.matched,
                    required: claim_tokens.required.len(),
                },
                markdown: MarkdownLocator {
                    line: span.line,
                    column: span.column,
                    unit: span.unit,
                    section: span.section.as_ref().to_vec(),
                },
                pdf: PdfLocator {
                    page,
                    backend: PdfBackend::MutoolNative,
                },
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

/// One claim-independent span prepared from the Markdown corpus.
struct PreparedSpan {
    source: String,
    exact: String,
    line: Line,
    column: Column,
    unit: UnitKind,
    section: Rc<[String]>,
}

/// A prepared span scored for one claim without copying its corpus metadata.
struct ScoredCandidate<'a> {
    span: &'a PreparedSpan,
    matched: usize,
}

fn prepare_spans(source: &str, units: &[MarkdownUnit]) -> Vec<PreparedSpan> {
    let mut prepared = Vec::new();

    for unit in units {
        let section: Rc<[String]> = Rc::from(unit.section.clone());
        for span in candidate_spans(&unit.text) {
            // A span that repeats inside its own unit can never be pinned to
            // one place in it, which is exactly what validation demands.
            if exact_count(&unit.text, &span) != 1 {
                continue;
            }
            prepared.push(PreparedSpan {
                source: source.to_owned(),
                exact: span,
                line: unit.line,
                column: unit.column,
                unit: unit.kind,
                section: Rc::clone(&section),
            });
        }
    }

    prepared
}

fn score_spans<'a>(
    spans: &'a [PreparedSpan],
    claim_tokens: &ExtractedTokens,
) -> Vec<ScoredCandidate<'a>> {
    spans
        .iter()
        .filter_map(|span| {
            let matched = claim_tokens
                .required
                .iter()
                .filter(|token| covers(&span.exact, token))
                .count();
            (matched > 0).then_some(ScoredCandidate { span, matched })
        })
        .collect()
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

/// Order scored candidates before PDF verification so only top-ranked spans are checked.
fn rank_scored_candidates(candidates: &mut [ScoredCandidate<'_>]) {
    candidates.sort_by(|left, right| {
        right
            .matched
            .cmp(&left.matched)
            .then_with(|| left.span.exact.len().cmp(&right.span.exact.len()))
            .then_with(|| left.span.source.cmp(&right.span.source))
            .then_with(|| left.span.line.cmp(&right.span.line))
            .then_with(|| left.span.column.cmp(&right.span.column))
    });
}

//! Extraction of the material tokens a claim commits to.
//!
//! Required tokens — numbers, number words, quoted phrases — are the parts of a
//! claim a reader can check against the source, so every one of them must show
//! up in at least one locator. Advisory tokens are reported but never enforced,
//! because proper nouns drift between a summary and its source too often to
//! carry a failure.

use std::collections::BTreeSet;

use crate::normalize::normalize;

/// The tokens a claim commits to, split by whether coverage is enforced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedTokens {
    /// Tokens that must each appear in at least one locator.
    pub required: Vec<String>,
    /// Tokens worth reporting on, never enforced.
    pub advisory: Vec<String>,
}

/// Pull the required and advisory tokens out of claim text.
#[must_use]
pub fn extract(claim: &str) -> ExtractedTokens {
    let mut required = BTreeSet::new();
    required.extend(numeric_literals(claim));
    required.extend(number_words(claim));
    required.extend(quoted_phrases(claim));

    ExtractedTokens {
        required: required.into_iter().collect(),
        advisory: capitalized_multiword_terms(claim),
    }
}

/// Whether `token` appears in `text` delimited by non-alphanumerics or edges.
///
/// The boundary check is what keeps `3` from being covered by `13`.
#[must_use]
pub fn is_covered(text: &str, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    for (index, _) in text.match_indices(token) {
        let before_ok = index == 0 || !text.as_bytes()[index - 1].is_ascii_alphanumeric();
        let after = index + token.len();
        let after_ok = after >= text.len() || !text.as_bytes()[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Case-insensitive [`is_covered`], for number words.
///
/// Avoids allocating lowercase copies by scanning ASCII bytes directly.
#[must_use]
pub fn is_covered_case_insensitive(text: &str, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let text_bytes = text.as_bytes();
    let token_bytes = token.as_bytes();
    let token_len = token_bytes.len();
    if token_len > text_bytes.len() {
        return false;
    }
    for start in 0..=(text_bytes.len() - token_len) {
        if text_bytes[start..(start + token_len)].eq_ignore_ascii_case(token_bytes) {
            let before_ok = start == 0 || !text_bytes[start - 1].is_ascii_alphanumeric();
            let after = start + token_len;
            let after_ok = after >= text_bytes.len() || !text_bytes[after].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
        }
    }
    false
}

/// Whether `token` is one of the recognized number words.
#[must_use]
pub fn is_number_word(token: &str) -> bool {
    NUMBER_WORDS.contains(&token)
}

const NUMBER_WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "million",
    "billion",
];

/// Maximal digit-led runs of digits, `.`, and `,`, plus any trailing `%`.
fn numeric_literals(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut results = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len()
            && (bytes[index].is_ascii_digit() || bytes[index] == b'.' || bytes[index] == b',')
        {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'%' {
            index += 1;
        }
        // A separator that ends the run is sentence punctuation, not part of
        // the number: "42." is forty-two, "1,228" keeps its comma.
        let token = text[start..index].trim_end_matches(['.', ',']);
        if !token.is_empty() {
            results.push(token.to_owned());
        }
    }

    results
}

/// Number words present in the claim, stored lowercase.
fn number_words(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    NUMBER_WORDS
        .iter()
        .filter(|word| is_covered(&lowered, word))
        .map(|word| (*word).to_owned())
        .collect()
}

/// Normalized contents of paired `"` or `'` runs.
fn quoted_phrases(text: &str) -> Vec<String> {
    let mut results = Vec::new();

    for delimiter in ['"', '\''] {
        let mut characters = text.char_indices();
        while let Some((open, character)) = characters.next() {
            if character != delimiter || !opens_quote(text, open) {
                continue;
            }
            let content_start = open + character.len_utf8();
            let close = characters
                .by_ref()
                .find(|(index, inner)| {
                    *inner == delimiter && closes_quote(text, index + inner.len_utf8())
                })
                .map(|(index, _)| index);
            if let Some(close) = close {
                let phrase = normalize(&text[content_start..close]);
                if !phrase.is_empty() {
                    results.push(phrase);
                }
            }
        }
    }

    results
}

/// A delimiter hugged by a letter on its left is an apostrophe, not an opener.
///
/// Without this, the apostrophe in `didn't` opens a phrase that runs to the
/// next apostrophe, producing a required token no locator can ever cover.
fn opens_quote(text: &str, index: usize) -> bool {
    text[..index]
        .chars()
        .next_back()
        .is_none_or(|character| !character.is_alphanumeric())
}

/// The mirror of [`opens_quote`], so `'don't stop'` closes at the right quote.
fn closes_quote(text: &str, after: usize) -> bool {
    text[after..]
        .chars()
        .next()
        .is_none_or(|character| !character.is_alphanumeric())
}

/// Runs of two or more capitalized words that do not open a sentence.
fn capitalized_multiword_terms(text: &str) -> Vec<String> {
    let mut results = Vec::new();

    for sentence in text.split(['.', '!', '?']) {
        let mut run: Vec<&str> = Vec::new();
        for (position, word) in sentence.split_whitespace().enumerate() {
            // The first word of a sentence is capitalized by grammar, not by
            // being a proper noun, so it can never open a run.
            if position > 0 && is_capitalized(word) {
                run.push(word);
            } else {
                flush_run(&mut run, &mut results);
            }
        }
        flush_run(&mut run, &mut results);
    }

    results
}

fn flush_run(run: &mut Vec<&str>, results: &mut Vec<String>) {
    if run.len() >= 2 {
        results.push(run.join(" "));
    }
    run.clear();
}

fn is_capitalized(word: &str) -> bool {
    word.starts_with(char::is_uppercase) && word.chars().skip(1).any(char::is_lowercase)
}

/// Grammar words that `coverage.words` never requires a locator to carry.
///
/// Kept to function words: determiners, pronouns, prepositions,
/// conjunctions, auxiliaries, and comparatives. A corpus adds its own
/// framing vocabulary through `coverage.allowed_words`.
const STOP_WORDS: &[&str] = &[
    "about", "above", "across", "after", "again", "against", "along", "also", "among", "another",
    "any", "are", "around", "because", "been", "before", "being", "below", "between", "beyond",
    "both", "but", "can", "cannot", "could", "did", "does", "doing", "down", "during", "each",
    "either", "else", "even", "ever", "every", "few", "for", "from", "further", "had", "has",
    "have", "having", "her", "here", "hers", "him", "his", "how", "however", "into", "its",
    "itself", "just", "less", "many", "may", "might", "more", "most", "much", "must", "neither",
    "nor", "not", "off", "once", "one", "only", "onto", "other", "others", "our", "ours", "out",
    "over", "own", "per", "same", "several", "shall", "she", "should", "since", "some", "still",
    "such", "than", "that", "the", "their", "theirs", "them", "then", "there", "these", "they",
    "this", "those", "though", "through", "thus", "too", "toward", "towards", "under", "until",
    "upon", "very", "was", "were", "what", "when", "where", "whereas", "whether", "which", "while",
    "who", "whom", "whose", "why", "will", "with", "within", "without", "would", "yet",
];

/// The content words a claim commits to when `coverage.words` is on.
///
/// Lowercase alphabetic runs of at least `min_len` letters, split on
/// hyphens and apostrophes, minus grammar words and the corpus allowlist.
/// Digits are the material-token rule's business and are skipped here.
#[must_use]
pub fn content_words(claim: &str, min_len: usize, allowed: &[String]) -> Vec<String> {
    let normalized = normalize(claim).to_ascii_lowercase();
    let mut words = BTreeSet::new();
    for raw in normalized.split(|c: char| !c.is_ascii_alphabetic()) {
        if raw.len() < min_len
            || STOP_WORDS.contains(&raw)
            || allowed.iter().any(|word| word.eq_ignore_ascii_case(raw))
        {
            continue;
        }
        words.insert(raw.to_owned());
    }
    words.into_iter().collect()
}

#[cfg(test)]
mod content_word_tests {
    use super::*;

    #[test]
    fn keeps_content_words_and_drops_grammar_and_short_ones() {
        let words = content_words(
            "The coastal survey transect was characterized by an absence of loose sediments.",
            4,
            &[],
        );
        assert_eq!(
            words,
            vec![
                "absence",
                "characterized",
                "coastal",
                "loose",
                "sediments",
                "survey",
                "transect"
            ]
        );
    }

    #[test]
    fn splits_hyphens_and_apostrophes_and_honours_the_allowlist() {
        let words = content_words(
            "Self-levelling instrument's temperature-controlled readings; the authors argue.",
            4,
            &["authors".to_owned(), "argue".to_owned()],
        );
        assert_eq!(
            words,
            vec![
                "controlled",
                "instrument",
                "levelling",
                "readings",
                "self",
                "temperature"
            ]
        );
    }

    #[test]
    fn digits_and_quoted_phrases_are_left_to_the_token_rule() {
        let words = content_words("Recovered 4.18 years after \"the washout\".", 4, &[]);
        assert_eq!(words, vec!["recovered", "washout", "years"]);
    }
}

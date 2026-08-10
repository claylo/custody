use receipts::tokens::{extract, is_covered, is_covered_case_insensitive, is_number_word};

#[test]
fn extracts_decimal_with_percent() {
    let tokens = extract("The rate was 67.5% in the control group.");
    assert!(tokens.required.contains(&"67.5%".to_owned()));
}

#[test]
fn extracts_thousands_separator() {
    let tokens = extract("There were 1,228 participants.");
    assert!(tokens.required.contains(&"1,228".to_owned()));
}

#[test]
fn extracts_plain_integer() {
    let tokens = extract("Over 3 trials the result held.");
    assert!(tokens.required.contains(&"3".to_owned()));
}

#[test]
fn extracts_decimal_without_percent() {
    let tokens = extract("p-value was 0.05.");
    assert!(tokens.required.contains(&"0.05".to_owned()));
}

#[test]
fn trims_trailing_punctuation_from_numbers() {
    let tokens = extract("There were 42.");
    assert!(tokens.required.contains(&"42".to_owned()));
    assert!(!tokens.required.iter().any(|t| t.ends_with('.')));
}

#[test]
fn extracts_number_words_case_insensitively() {
    let tokens = extract("Three cohorts were tested against Two baselines.");
    assert!(tokens.required.contains(&"three".to_owned()));
    assert!(tokens.required.contains(&"two".to_owned()));
}

#[test]
fn extracts_scale_words() {
    let tokens = extract("Over one hundred thousand samples.");
    assert!(tokens.required.contains(&"one".to_owned()));
    assert!(tokens.required.contains(&"hundred".to_owned()));
    assert!(tokens.required.contains(&"thousand".to_owned()));
}

#[test]
fn thirteen_does_not_match_three() {
    let tokens = extract("Thirteen subjects participated.");
    assert!(tokens.required.contains(&"thirteen".to_owned()));
    assert!(!tokens.required.contains(&"three".to_owned()));
}

#[test]
fn extracts_quoted_phrases() {
    let tokens = extract("The study found \"no significant effect\" in the data.");
    assert!(
        tokens
            .required
            .contains(&"no significant effect".to_owned())
    );
}

#[test]
fn extracts_single_quoted_phrases() {
    let tokens = extract("The method is called 'gradient descent' here.");
    assert!(tokens.required.contains(&"gradient descent".to_owned()));
}

#[test]
fn quoted_phrase_is_normalized() {
    let tokens = extract("Found \"excess   whitespace\" in output.");
    assert!(tokens.required.contains(&"excess whitespace".to_owned()));
}

#[test]
fn empty_quoted_phrase_is_skipped() {
    let tokens = extract("The field was \"\" throughout.");
    assert!(tokens.required.is_empty());
}

#[test]
fn contraction_apostrophes_do_not_open_a_phrase() {
    let tokens = extract("It didn't work and wouldn't scale.");
    assert!(tokens.required.is_empty());
}

#[test]
fn quoted_phrase_survives_a_contraction_beside_it() {
    let tokens = extract("The team didn't reach the 'target' threshold.");
    assert_eq!(tokens.required, ["target"]);
}

#[test]
fn quoted_phrase_may_contain_a_contraction() {
    let tokens = extract("The sign read 'don't stop' in red.");
    assert_eq!(tokens.required, ["don't stop"]);
}

#[test]
fn repeated_tokens_are_deduplicated() {
    let tokens = extract("Of 12 cases, 12 resolved.");
    let twelves = tokens.required.iter().filter(|t| *t == "12").count();
    assert_eq!(twelves, 1);
}

#[test]
fn required_tokens_are_sorted() {
    let tokens = extract("Counts of 9, 100, and 30 were recorded.");
    let mut sorted = tokens.required.clone();
    sorted.sort();
    assert_eq!(tokens.required, sorted);
}

#[test]
fn advisory_captures_capitalized_multiword_not_at_sentence_start() {
    let tokens = extract("The Global Carbon Budget increased sharply.");
    assert!(!tokens.advisory.is_empty());
}

#[test]
fn sentence_start_capitalization_is_not_advisory() {
    let tokens = extract("Results were clear.");
    assert!(tokens.advisory.is_empty());
}

#[test]
fn boundary_check_prevents_substring_match() {
    assert!(!is_covered("13 regimes", "3"));
    assert!(is_covered("3 regimes", "3"));
    assert!(is_covered("value=3", "3"));
    assert!(!is_covered("x13y", "3"));
}

#[test]
fn boundary_at_string_edges() {
    assert!(is_covered("3", "3"));
    assert!(is_covered("hello 3", "3"));
    assert!(is_covered("3 hello", "3"));
}

#[test]
fn case_insensitive_coverage_for_number_words() {
    assert!(is_covered_case_insensitive(
        "Three regimes were tested",
        "three"
    ));
    assert!(is_covered_case_insensitive("THREE REGIMES", "three"));
}

#[test]
fn number_words_are_recognized() {
    assert!(is_number_word("seventeen"));
    assert!(is_number_word("billion"));
    assert!(!is_number_word("Seventeen"));
    assert!(!is_number_word("seventeenth"));
}

#[test]
fn empty_claim_yields_no_tokens() {
    let tokens = extract("");
    assert!(tokens.required.is_empty());
    assert!(tokens.advisory.is_empty());
}
